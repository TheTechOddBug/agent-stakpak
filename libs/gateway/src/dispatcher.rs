use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use chrono::Utc;
use stakai::{Message, Role};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{error, warn};

use crate::{
    channels::Channel,
    client::{
        MessageType, SendMessageOptions, StakpakClient, ToolCallsProposedPayload,
        ToolDecisionAction, ToolDecisionInput,
    },
    config::ApprovalMode,
    router::{RouterConfig, resolve_routing_key},
    store::{GatewayStore, SessionMapping},
    targeting::{render_title_template, target_key_from_inbound},
    types::{DeliveryContext, InboundMessage, OutboundReply},
};

pub struct Dispatcher {
    client: StakpakClient,
    channels: HashMap<String, Arc<dyn Channel>>,
    store: Arc<GatewayStore>,
    router_config: RouterConfig,
    // TODO: persist dispatcher state (active_runs, pending_queues, event_cursors) to store
    // for crash recovery. Current behavior relies on watch-side reconciler for eventual
    // consistency after gateway restart.
    active_runs: Mutex<HashMap<String, ActiveRun>>,
    pending_queues: Mutex<HashMap<String, Vec<QueuedMessage>>>,
    event_cursors: Mutex<HashMap<String, u64>>,
    default_model: Option<String>,
    approval_mode: ApprovalMode,
    approval_allowlist: HashSet<String>,
    title_template: String,
}

#[derive(Debug, Clone)]
struct ActiveRun {
    run_id: String,
    cancel: CancellationToken,
}

#[derive(Debug, Clone, Default)]
struct RunStartOptions {
    model: Option<String>,
    sandbox: Option<bool>,
    timeout_seconds: Option<u64>,
}

#[derive(Debug, Clone)]
struct QueuedMessage {
    inbound: InboundMessage,
    text: String,
    run_options: RunStartOptions,
}

#[derive(Debug)]
struct RunTaskResult {
    session_id: String,
    run_id: String,
    outcome: RunOutcome,
}

#[derive(Clone)]
struct RunContext {
    channels: HashMap<String, Arc<dyn Channel>>,
    delivery: DeliveryContext,
    session_id: String,
    run_id: String,
    timeout_seconds: Option<u64>,
}

#[derive(Debug)]
enum RunOutcome {
    Completed { cursor: Option<u64> },
    Error { cursor: Option<u64> },
    Cancelled { cursor: Option<u64> },
    StreamEnded { cursor: Option<u64> },
}

impl Dispatcher {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        client: StakpakClient,
        channels: HashMap<String, Arc<dyn Channel>>,
        store: Arc<GatewayStore>,
        router_config: RouterConfig,
        default_model: Option<String>,
        approval_mode: ApprovalMode,
        approval_allowlist: Vec<String>,
        title_template: String,
    ) -> Self {
        Self {
            client,
            channels,
            store,
            router_config,
            active_runs: Mutex::new(HashMap::new()),
            pending_queues: Mutex::new(HashMap::new()),
            event_cursors: Mutex::new(HashMap::new()),
            default_model,
            approval_mode,
            approval_allowlist: approval_allowlist.into_iter().collect(),
            title_template,
        }
    }

    pub async fn run(
        self: Arc<Self>,
        mut inbound_rx: mpsc::Receiver<InboundMessage>,
        cancel: CancellationToken,
    ) {
        let (run_tx, mut run_rx) = mpsc::channel::<RunTaskResult>(128);

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    self.cancel_all_runs();
                    break;
                }
                maybe_inbound = inbound_rx.recv() => {
                    let Some(inbound) = maybe_inbound else {
                        break;
                    };
                    if let Err(error) = self.handle_inbound(inbound, run_tx.clone()).await {
                        error!(error = %error, "failed to handle inbound message");
                    }
                }
                maybe_result = run_rx.recv() => {
                    let Some(result) = maybe_result else {
                        continue;
                    };
                    if let Err(error) = self.handle_run_result(result, run_tx.clone()).await {
                        error!(error = %error, "failed to handle run result");
                    }
                }
            }
        }
    }

    async fn handle_inbound(
        self: &Arc<Self>,
        inbound: InboundMessage,
        run_tx: mpsc::Sender<RunTaskResult>,
    ) -> Result<(), String> {
        let routing_key = resolve_routing_key(
            &self.router_config,
            &inbound.channel,
            &inbound.peer_id,
            &inbound.chat_type,
        );

        let target_key = target_key_from_inbound(&inbound);
        let enriched_text = match self
            .store
            .pop_delivery_context(&inbound.channel.0, &target_key)
            .await
        {
            Ok(Some(context)) => enrich_with_context(&context, &inbound.text),
            Ok(None) => inbound.text.clone(),
            Err(error) => {
                warn!(error = %error, "failed to pop delivery context");
                inbound.text.clone()
            }
        };

        let maybe_mapping = self
            .store
            .get(&routing_key)
            .await
            .map_err(|error| format!("failed to get mapping: {error}"))?;

        let mapping = if let Some(mapping) = maybe_mapping {
            let delivery = self.delivery_context_from_inbound(&inbound);
            if let Err(error) = self.store.update_delivery(&routing_key, &delivery).await {
                warn!(error = %error, "failed to update delivery context");
            }
            mapping
        } else {
            let title = self.render_title(&inbound);
            let created = self
                .client
                .create_session(&title)
                .await
                .map_err(|error| format!("create session failed: {error}"))?;

            let now = Utc::now().timestamp_millis();
            let mapping = SessionMapping {
                session_id: created.id.to_string(),
                title,
                delivery: self.delivery_context_from_inbound(&inbound),
                created_at: now,
            };

            self.store
                .set(&routing_key, &mapping)
                .await
                .map_err(|error| format!("failed to persist mapping: {error}"))?;

            mapping
        };

        let run_options = extract_run_options(&inbound.metadata);
        let queued = QueuedMessage {
            inbound,
            text: enriched_text,
            run_options,
        };

        if self.is_run_active(&mapping.session_id) {
            self.enqueue_message(mapping.session_id.clone(), queued)?;
            return Ok(());
        }

        self.start_run(mapping.session_id, queued, run_tx).await
    }

    async fn handle_run_result(
        self: &Arc<Self>,
        result: RunTaskResult,
        run_tx: mpsc::Sender<RunTaskResult>,
    ) -> Result<(), String> {
        self.remove_active_run(&result.session_id, &result.run_id);

        let cursor = match &result.outcome {
            RunOutcome::Completed { cursor }
            | RunOutcome::Error { cursor }
            | RunOutcome::Cancelled { cursor }
            | RunOutcome::StreamEnded { cursor } => *cursor,
        };

        if let Some(cursor) = cursor {
            self.set_cursor(&result.session_id, cursor)?;
        }

        match result.outcome {
            RunOutcome::Completed { .. }
            | RunOutcome::Error { .. }
            | RunOutcome::Cancelled { .. }
            | RunOutcome::StreamEnded { .. } => {}
        }

        self.drain_queue(&result.session_id, run_tx).await
    }

    async fn start_run(
        self: &Arc<Self>,
        session_id: String,
        queued: QueuedMessage,
        run_tx: mpsc::Sender<RunTaskResult>,
    ) -> Result<(), String> {
        let message = Message::new(Role::User, queued.text.clone());
        let response = self
            .client
            .send_messages(
                &session_id,
                vec![message],
                SendMessageOptions {
                    model: queued
                        .run_options
                        .model
                        .clone()
                        .or_else(|| self.default_model.clone()),
                    message_type: MessageType::Message,
                    run_id: None,
                    sandbox: queued.run_options.sandbox,
                },
            )
            .await;

        let response = match response {
            Ok(response) => response,
            Err(crate::client::ClientError::Conflict) => {
                self.enqueue_message(session_id, queued)?;
                return Ok(());
            }
            Err(error) => return Err(format!("send message failed: {error}")),
        };

        let run_id = response.run_id.to_string();
        let cancel = CancellationToken::new();

        {
            let mut guard = self
                .active_runs
                .lock()
                .map_err(|_| "failed to lock active_runs".to_string())?;
            guard.insert(
                session_id.clone(),
                ActiveRun {
                    run_id: run_id.clone(),
                    cancel: cancel.clone(),
                },
            );
        }

        let client = self.client.clone();
        let run_context = RunContext {
            channels: self.channels.clone(),
            delivery: self.delivery_context_from_inbound(&queued.inbound),
            session_id: session_id.clone(),
            run_id: run_id.clone(),
            timeout_seconds: queued.run_options.timeout_seconds,
        };
        let session_id_for_task = session_id.clone();
        let run_id_for_task = run_id.clone();
        let approval_mode = self.approval_mode.clone();
        let approval_allowlist = self.approval_allowlist.clone();
        let last_event_id = self.get_cursor(&session_id)?;

        tokio::spawn(async move {
            let outcome = consume_run_events(
                client,
                run_context,
                last_event_id,
                approval_mode,
                approval_allowlist,
                cancel,
            )
            .await;

            if let Err(error) = run_tx
                .send(RunTaskResult {
                    session_id: session_id_for_task,
                    run_id: run_id_for_task,
                    outcome,
                })
                .await
            {
                error!(error = %error, "failed to send run outcome");
            }
        });

        Ok(())
    }

    async fn drain_queue(
        self: &Arc<Self>,
        session_id: &str,
        run_tx: mpsc::Sender<RunTaskResult>,
    ) -> Result<(), String> {
        let queue = {
            let mut guard = self
                .pending_queues
                .lock()
                .map_err(|_| "failed to lock pending_queues".to_string())?;
            guard.remove(session_id).unwrap_or_default()
        };

        if queue.is_empty() {
            return Ok(());
        }

        let combined_text = format_batched_queue_messages(&queue);

        if let Some(latest) = queue.last() {
            let routing_key = resolve_routing_key(
                &self.router_config,
                &latest.inbound.channel,
                &latest.inbound.peer_id,
                &latest.inbound.chat_type,
            );
            let delivery = self.delivery_context_from_inbound(&latest.inbound);
            if let Err(error) = self.store.update_delivery(&routing_key, &delivery).await {
                warn!(error = %error, "failed to refresh delivery context from queue");
            }
        }

        let Some(latest) = queue.last() else {
            return Ok(());
        };

        let queued = QueuedMessage {
            inbound: latest.inbound.clone(),
            text: combined_text,
            run_options: latest.run_options.clone(),
        };

        if let Err(error) = self.start_run(session_id.to_string(), queued, run_tx).await {
            self.restore_queue(session_id.to_string(), queue)?;
            return Err(error);
        }

        Ok(())
    }

    fn render_title(&self, inbound: &InboundMessage) -> String {
        render_title_template(
            &self.title_template,
            &inbound.channel.0,
            &inbound.peer_id.0,
            &inbound.chat_type,
        )
    }

    fn delivery_context_from_inbound(&self, inbound: &InboundMessage) -> DeliveryContext {
        DeliveryContext {
            channel: inbound.channel.clone(),
            peer_id: inbound.peer_id.clone(),
            chat_type: inbound.chat_type.clone(),
            channel_meta: inbound.metadata.clone(),
            updated_at: Utc::now().timestamp_millis(),
        }
    }

    pub fn is_run_active(&self, session_id: &str) -> bool {
        self.active_runs
            .lock()
            .ok()
            .and_then(|guard| guard.get(session_id).cloned())
            .is_some()
    }

    fn enqueue_message(&self, session_id: String, message: QueuedMessage) -> Result<(), String> {
        let mut guard = self
            .pending_queues
            .lock()
            .map_err(|_| "failed to lock pending_queues".to_string())?;

        guard.entry(session_id).or_default().push(message);
        Ok(())
    }

    fn restore_queue(&self, session_id: String, drained: Vec<QueuedMessage>) -> Result<(), String> {
        let mut guard = self
            .pending_queues
            .lock()
            .map_err(|_| "failed to lock pending_queues".to_string())?;

        let entry = guard.entry(session_id).or_default();
        let existing = std::mem::take(entry);
        *entry = merge_drained_queue(drained, existing);

        Ok(())
    }

    fn remove_active_run(&self, session_id: &str, run_id: &str) {
        if let Ok(mut guard) = self.active_runs.lock()
            && let Some(active) = guard.get(session_id)
            && active.run_id == run_id
        {
            guard.remove(session_id);
        }
    }

    fn cancel_all_runs(&self) {
        if let Ok(guard) = self.active_runs.lock() {
            for active in guard.values() {
                active.cancel.cancel();
            }
        }
    }

    fn get_cursor(&self, session_id: &str) -> Result<Option<u64>, String> {
        let guard = self
            .event_cursors
            .lock()
            .map_err(|_| "failed to lock event_cursors".to_string())?;
        Ok(guard.get(session_id).copied())
    }

    fn set_cursor(&self, session_id: &str, cursor: u64) -> Result<(), String> {
        let mut guard = self
            .event_cursors
            .lock()
            .map_err(|_| "failed to lock event_cursors".to_string())?;
        let current = guard.get(session_id).copied();
        let next = current.map_or(cursor, |value| value.max(cursor));
        guard.insert(session_id.to_string(), next);
        Ok(())
    }
}

fn merge_drained_queue(
    mut drained: Vec<QueuedMessage>,
    mut existing: Vec<QueuedMessage>,
) -> Vec<QueuedMessage> {
    drained.append(&mut existing);
    drained
}

async fn consume_run_events(
    client: StakpakClient,
    run_context: RunContext,
    last_event_id: Option<u64>,
    approval_mode: ApprovalMode,
    approval_allowlist: HashSet<String>,
    cancel: CancellationToken,
) -> RunOutcome {
    let mut stream = match client
        .subscribe_events(&run_context.session_id, last_event_id)
        .await
    {
        Ok(stream) => stream,
        Err(error) => {
            warn!(error = %error, "failed to subscribe to run event stream");
            return RunOutcome::Error {
                cursor: last_event_id,
            };
        }
    };

    let mut streamed_buffer = String::new();
    let mut last_stream_at = Instant::now();
    let mut cursor = last_event_id;
    let timeout_deadline = run_context
        .timeout_seconds
        .map(|seconds| tokio::time::Instant::now() + Duration::from_secs(seconds));
    let timeout_future = async {
        if let Some(deadline) = timeout_deadline {
            tokio::time::sleep_until(deadline).await;
        } else {
            std::future::pending::<()>().await;
        }
    };
    tokio::pin!(timeout_future);

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                return RunOutcome::Cancelled { cursor };
            }
            _ = &mut timeout_future => {
                flush_stream_buffer(&run_context.channels, &run_context.delivery, &mut streamed_buffer, true).await;
                deliver_channel_text(&run_context.channels, &run_context.delivery, "⏱️ Interactive run timed out.").await;
                return RunOutcome::Error { cursor };
            }
            next = stream.next_event() => {
                let event = match next {
                    Ok(Some(event)) => event,
                    Ok(None) => {
                        flush_stream_buffer(&run_context.channels, &run_context.delivery, &mut streamed_buffer, true).await;
                        return RunOutcome::StreamEnded { cursor };
                    }
                    Err(error) => {
                        flush_stream_buffer(&run_context.channels, &run_context.delivery, &mut streamed_buffer, true).await;
                        warn!(error = %error, "run event stream read failed");
                        return RunOutcome::Error { cursor };
                    }
                };

                if let Some(id) = event.event_id_u64 {
                    cursor = Some(cursor.map_or(id, |value| value.max(id)));
                }

                if event.run_id().as_deref() != Some(run_context.run_id.as_str()) {
                    continue;
                }

                match event.event_type.as_str() {
                    "text_delta" => {
                        if let Some(delta) = event.as_text_delta() {
                            streamed_buffer.push_str(&delta);

                            if should_flush_stream_buffer(&streamed_buffer, last_stream_at.elapsed()) {
                                flush_stream_buffer(&run_context.channels, &run_context.delivery, &mut streamed_buffer, false).await;
                                last_stream_at = Instant::now();
                            }
                        }
                    }
                    "tool_calls_proposed" => {
                        if let Some(proposed) = event.as_tool_calls_proposed() {
                            flush_stream_buffer(&run_context.channels, &run_context.delivery, &mut streamed_buffer, true).await;

                            let tool_names = proposed
                                .tool_calls
                                .iter()
                                .map(|tool_call| tool_call.name.as_str())
                                .collect::<Vec<_>>()
                                .join(", ");
                            if !tool_names.is_empty() {
                                let text = format!("🔧 Running: {tool_names}");
                                deliver_channel_text(&run_context.channels, &run_context.delivery, text).await;
                            }

                            let decisions = build_tool_decisions(
                                proposed,
                                &approval_mode,
                                &approval_allowlist,
                            );
                            if let Err(error) = client
                                .resolve_tools(&run_context.session_id, &run_context.run_id, decisions)
                                .await
                            {
                                warn!(error = %error, "resolve_tools failed");
                                return RunOutcome::Error { cursor };
                            }
                            last_stream_at = Instant::now();
                        }
                    }
                    "run_completed" => {
                        flush_stream_buffer(&run_context.channels, &run_context.delivery, &mut streamed_buffer, true).await;
                        return RunOutcome::Completed { cursor };
                    }
                    "run_error" => {
                        flush_stream_buffer(&run_context.channels, &run_context.delivery, &mut streamed_buffer, true).await;
                        let error_text = event
                            .as_run_error()
                            .and_then(|payload| payload.error)
                            .unwrap_or_else(|| "unknown error".to_string());
                        warn!(
                            session_id = %run_context.session_id,
                            run_id = %run_context.run_id,
                            error = %error_text,
                            "interactive run failed"
                        );
                        deliver_channel_text(
                            &run_context.channels,
                            &run_context.delivery,
                            format!("⚠️ Agent run failed (session: {})", run_context.session_id),
                        )
                        .await;

                        return RunOutcome::Error { cursor };
                    }
                    _ => {}
                }
            }
        }
    }
}

fn should_flush_stream_buffer(buffer: &str, elapsed_since_last_stream: Duration) -> bool {
    const STREAM_MIN_INTERVAL: Duration = Duration::from_secs(3);
    const STREAM_MAX_BUFFER_LEN: usize = 500;

    if buffer.trim().is_empty() {
        return false;
    }

    if buffer.contains("\n\n") {
        return true;
    }

    let has_complete_line = buffer.contains('\n');
    has_complete_line
        && (buffer.chars().count() >= STREAM_MAX_BUFFER_LEN
            || elapsed_since_last_stream >= STREAM_MIN_INTERVAL)
}

fn take_completed_line_chunk(buffer: &mut String) -> Option<String> {
    let split_index = buffer.rfind('\n')?;
    let split_after = split_index + '\n'.len_utf8();

    let remainder = buffer.split_off(split_after);
    let chunk = std::mem::replace(buffer, remainder);

    Some(chunk)
}

async fn flush_stream_buffer(
    channels: &HashMap<String, Arc<dyn Channel>>,
    delivery: &DeliveryContext,
    buffer: &mut String,
    force: bool,
) {
    if buffer.trim().is_empty() {
        buffer.clear();
        return;
    }

    let text = if force {
        std::mem::take(buffer)
    } else {
        let Some(chunk) = take_completed_line_chunk(buffer) else {
            return;
        };
        chunk
    };

    if text.trim().is_empty() {
        return;
    }

    deliver_channel_text(channels, delivery, text.trim()).await;
}

async fn deliver_channel_text(
    channels: &HashMap<String, Arc<dyn Channel>>,
    delivery: &DeliveryContext,
    text: impl Into<String>,
) {
    let Some(channel) = channels.get(&delivery.channel.0) else {
        warn!(channel = %delivery.channel.0, "channel not connected");
        return;
    };

    let reply = OutboundReply {
        channel: delivery.channel.clone(),
        peer_id: delivery.peer_id.clone(),
        chat_type: delivery.chat_type.clone(),
        text: text.into(),
        metadata: delivery.channel_meta.clone(),
    };

    if let Err(error) = channel.send(reply).await {
        warn!(error = %error, "failed to send channel reply");
    }
}

fn format_batched_queue_messages(queue: &[QueuedMessage]) -> String {
    if queue.len() <= 1 {
        return queue
            .first()
            .map(|item| item.text.clone())
            .unwrap_or_default();
    }

    queue
        .iter()
        .map(|item| {
            let sender = sender_name(&item.inbound.metadata)
                .unwrap_or_else(|| item.inbound.peer_id.0.clone());
            format!("{sender}: {}", item.text.trim())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn sender_name(metadata: &serde_json::Value) -> Option<String> {
    metadata
        .get("display_name")
        .and_then(|value| value.as_str())
        .or_else(|| metadata.get("username").and_then(|value| value.as_str()))
        .map(ToOwned::to_owned)
}

fn extract_run_options(metadata: &serde_json::Value) -> RunStartOptions {
    let options = metadata
        .get("gateway_run_options")
        .and_then(|value| value.as_object());

    let model = options
        .and_then(|value| value.get("model"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

    let sandbox = options
        .and_then(|value| value.get("sandbox"))
        .and_then(|value| value.as_bool());

    let timeout_seconds = options
        .and_then(|value| value.get("timeout"))
        .and_then(|value| value.as_u64())
        .filter(|value| *value > 0);

    RunStartOptions {
        model,
        sandbox,
        timeout_seconds,
    }
}

fn build_tool_decisions(
    proposed: ToolCallsProposedPayload,
    approval_mode: &ApprovalMode,
    approval_allowlist: &HashSet<String>,
) -> HashMap<String, ToolDecisionInput> {
    proposed
        .tool_calls
        .into_iter()
        .map(|tool_call| {
            let action = match approval_mode {
                ApprovalMode::AllowAll => ToolDecisionAction::Accept,
                ApprovalMode::DenyAll => ToolDecisionAction::Reject,
                ApprovalMode::Allowlist => {
                    if approval_allowlist.contains(&tool_call.name) {
                        ToolDecisionAction::Accept
                    } else {
                        ToolDecisionAction::Reject
                    }
                }
            };

            (
                tool_call.id,
                ToolDecisionInput {
                    action,
                    content: None,
                },
            )
        })
        .collect()
}

fn enrich_with_context(context: &serde_json::Value, user_text: &str) -> String {
    let mut enriched =
        String::from("The user is replying to a previous notification.\n\n--- Watch Context ---\n");

    if let Some(trigger) = context.get("trigger").and_then(|value| value.as_str()) {
        enriched.push_str(&format!("Trigger: {trigger}\n"));
    }

    if let Some(status) = context.get("status").and_then(|value| value.as_str()) {
        enriched.push_str(&format!("Status: {status}\n"));
    }

    if let Some(summary) = context.get("summary").and_then(|value| value.as_str()) {
        enriched.push_str(&format!("Summary: {summary}\n"));
    }

    if let Some(check_output) = context.get("check_output").and_then(|value| value.as_str()) {
        enriched.push_str(&format!("Check output: {check_output}\n"));
    }

    enriched.push_str("---\n\n");
    enriched.push_str(&format!("User message: {user_text}"));
    enriched
}

#[cfg(test)]
mod tests {
    use super::{
        QueuedMessage, extract_run_options, format_batched_queue_messages, merge_drained_queue,
        sender_name, should_flush_stream_buffer, take_completed_line_chunk,
    };
    use crate::types::{ChannelId, ChatType, InboundMessage, PeerId};
    use chrono::Utc;
    use std::time::Duration;

    fn queued(text: &str, display_name: Option<&str>, peer: &str) -> QueuedMessage {
        let metadata = match display_name {
            Some(name) => serde_json::json!({"display_name": name}),
            None => serde_json::json!({}),
        };

        QueuedMessage {
            inbound: InboundMessage {
                channel: ChannelId("slack".to_string()),
                peer_id: PeerId(peer.to_string()),
                chat_type: ChatType::Direct,
                text: text.to_string(),
                media: Vec::new(),
                metadata,
                timestamp: Utc::now(),
            },
            text: text.to_string(),
            run_options: super::RunStartOptions::default(),
        }
    }

    #[test]
    fn stream_buffer_flush_rules() {
        assert!(should_flush_stream_buffer(
            "hello\n\nworld",
            Duration::from_millis(100)
        ));
        assert!(!should_flush_stream_buffer(
            &"x".repeat(501),
            Duration::from_millis(100)
        ));
        assert!(should_flush_stream_buffer(
            "hello\nworld",
            Duration::from_secs(3)
        ));
        assert!(!should_flush_stream_buffer("hello", Duration::from_secs(3)));
    }

    #[test]
    fn take_completed_line_chunk_keeps_remainder() {
        let mut buffer = String::from("line1\nline2\npartial");
        let chunk = take_completed_line_chunk(&mut buffer).expect("chunk should exist");

        assert_eq!(chunk, "line1\nline2\n");
        assert_eq!(buffer, "partial");
    }

    #[test]
    fn queue_batching_keeps_sender_attribution() {
        let batch = vec![
            queued("Can you check logs?", Some("alice"), "u1"),
            queued("Also include disk usage", Some("bob"), "u2"),
        ];

        let combined = format_batched_queue_messages(&batch);
        assert!(combined.contains("alice: Can you check logs?"));
        assert!(combined.contains("bob: Also include disk usage"));
    }

    #[test]
    fn sender_name_falls_back_to_username() {
        let metadata = serde_json::json!({"username": "carol"});
        assert_eq!(sender_name(&metadata).as_deref(), Some("carol"));
    }

    #[test]
    fn merge_drained_queue_keeps_drained_messages_first() {
        let drained = vec![queued("drained-1", Some("alice"), "u1")];
        let existing = vec![queued("existing-1", Some("bob"), "u2")];

        let merged = merge_drained_queue(drained, existing);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].text, "drained-1");
        assert_eq!(merged[1].text, "existing-1");
    }

    #[test]
    fn extract_run_options_reads_timeout_model_and_sandbox() {
        let metadata = serde_json::json!({
            "gateway_run_options": {
                "model": "claude-sonnet",
                "sandbox": true,
                "timeout": 60
            }
        });

        let options = extract_run_options(&metadata);
        assert_eq!(options.model.as_deref(), Some("claude-sonnet"));
        assert_eq!(options.sandbox, Some(true));
        assert_eq!(options.timeout_seconds, Some(60));
    }
}
