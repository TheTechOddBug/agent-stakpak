# PR: Fix Telegram Channel Integration

## Summary

Fixes critical Telegram send failures (`unsupported parse_mode`) and hardens the channel against formatting errors with a plain text retry fallback. Also adds `disable_notification` support for silent schedule notifications.

## Problem

The Telegram gateway was unable to send any replies. Every outbound message failed with:

```
WARN stakpak_gateway::channels::telegram: telegram send chunk failed
  error=telegram sendMessage error 400: Bad Request: unsupported parse_mode
```

**Root cause:** `SendMessageParams` serialized `Option` fields as `null` in JSON (e.g., `"parse_mode": null`). Telegram's Bot API rejects `null` — it expects either a valid value (`"HTML"`, `"MarkdownV2"`) or the field to be **omitted entirely**.

## Changes

### File: `libs/gateway/src/channels/telegram.rs`

| Change | Lines | Description |
|--------|-------|-------------|
| Fix `SendMessageParams` serialization | 316-328 | Add `#[serde(skip_serializing_if = "Option::is_none")]` to all `Option` fields |
| Fix `GetUpdatesParams` serialization | 308-314 | Add `skip_serializing_if` to `offset` field for consistency |
| Add plain text retry fallback | 149-160 | On 400 with parse-related error, clear `parse_mode` and retry |
| Add `disable_notification` field | 326-327 | Supports silent notifications for schedule-triggered messages |
| Add `Clone` derive | 316 | Required for the retry fallback logic |

### Before (broken)

```rust
// Line 315-322
#[derive(Debug, Serialize)]
struct SendMessageParams {
    chat_id: i64,
    text: String,
    parse_mode: Option<String>,          // serializes as "parse_mode": null
    reply_to_message_id: Option<i64>,    // serializes as "reply_to_message_id": null
    message_thread_id: Option<i64>,      // serializes as "message_thread_id": null
}
```

Produced JSON:
```json
{
  "chat_id": 123456789,
  "text": "Hello",
  "parse_mode": null,
  "reply_to_message_id": null,
  "message_thread_id": null
}
```

Telegram rejected with HTTP 400: `unsupported parse_mode`.

### After (fixed)

```rust
// Line 316-328
#[derive(Debug, Clone, Serialize)]
struct SendMessageParams {
    chat_id: i64,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    parse_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reply_to_message_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message_thread_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    disable_notification: Option<bool>,
}
```

Produces clean JSON:
```json
{
  "chat_id": 123456789,
  "text": "Hello"
}
```

### Retry Fallback Logic

```rust
// Line 149-160 — inside send_chunk loop
// If Telegram returns 400 with a parse-related error description,
// clear parse_mode and retry as plain text instead of failing
if status == reqwest::StatusCode::BAD_REQUEST
    && params.parse_mode.is_some()
    && description.to_lowercase().contains("parse")
{
    warn!(
        parse_mode = ?params.parse_mode,
        "telegram parse_mode rejected, retrying as plain text"
    );
    params.parse_mode = None;
    continue;
}
```

This matches OpenClaw's behavior of falling back to plain text when HTML/Markdown parsing fails.

## Additional Fix: `GetUpdatesParams`

```rust
// Before
struct GetUpdatesParams {
    offset: Option<i64>,    // "offset": null
    ...
}

// After
struct GetUpdatesParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    offset: Option<i64>,    // omitted when None
    ...
}
```

While Telegram currently accepts `"offset": null`, this aligns with the same fix pattern for safety.

## Related: Config Issues (v0.3.44)

Also documented in `docs/autopilot-telegram-setup.md`:

- `stakpak autopilot channel add telegram` writes gateway format (`token` + `require_mention`) but `stakpak up` expects autopilot format (`type` + `target` + `token`). Users must manually add `type` and `target` fields.
- `ScheduleTriggerOn::Always` vs `CheckTriggerOn::Any` enum mismatch causes restart failures (pre-existing bug).

## Testing

```bash
cargo test -p stakpak-gateway -- telegram
```

Manual verification:
1. `stakpak autopilot channel add telegram --token <TOKEN>`
2. Manually add `type = "telegram"` and `target = "<CHAT_ID>"` to `~/.stakpak/autopilot.toml`
3. `stakpak up`
4. Send message to bot on Telegram
5. Verify reply received (no `unsupported parse_mode` in logs)

## Docs

- `docs/autopilot-telegram-setup.md` — Full setup guide, known issues, comparison with OpenClaw/ZeroClaw/NanoClaw

## Files Changed

- `libs/gateway/src/channels/telegram.rs` — serialization fix, retry fallback, `disable_notification`
- `docs/autopilot-telegram-setup.md` — new setup guide and known issues documentation
