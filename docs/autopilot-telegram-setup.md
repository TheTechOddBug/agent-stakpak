# Autopilot Telegram Channel Setup Guide

## Prerequisites

- Stakpak CLI installed (`brew install stakpak` or built from source)
- A Stakpak API key (from your team or personal account)
- A Telegram account

---

## Step 1: Create a Telegram Bot

1. Open Telegram and message [@BotFather](https://t.me/BotFather)
2. Send `/newbot`
3. Choose a display name (e.g., "My Stakpak Bot")
4. Choose a username (must end in `bot`, e.g., `my_stakpak_bot`)
5. Copy the bot token (format: `123456789:ABCdef...`)

## Step 2: Get Your Telegram Chat ID

1. Open Telegram and send any message to your new bot
2. Run the following to fetch your chat ID from the bot's updates:

```bash
curl -s "https://api.telegram.org/bot<YOUR_BOT_TOKEN>/getUpdates" | python3 -m json.tool
```

3. Look for `"chat": { "id": 123456789 }` in the response — that number is your chat ID

## Step 3: Authenticate with Stakpak

```bash
stakpak auth login --api-key <YOUR_STAKPAK_API_KEY>
```

Or use a specific profile:

```bash
stakpak --profile team auth login --api-key <YOUR_STAKPAK_API_KEY>
```

## Step 4: Add the Telegram Channel

```bash
stakpak autopilot channel add telegram --token "<YOUR_BOT_TOKEN>"
```

## Step 5: Configure `~/.stakpak/autopilot.toml`

After adding the channel, verify and edit the autopilot config. A complete working config looks like:

```toml
[server]
url = "https://apiv2.stakpak.dev"
token = "<YOUR_STAKPAK_API_KEY>"

[gateway]
store = "~/.stakpak/autopilot/gateway.db"
title_template = "{channel} / {peer}"
prune_after_hours = 168
delivery_context_ttl_hours = 4
approval_mode = "allow_all"
approval_allowlist = []

[routing]
dm_scope = "per_channel_peer"
bindings = []

[channels.telegram]
type = "telegram"
token = "<YOUR_BOT_TOKEN>"
target = "<YOUR_CHAT_ID>"
```

### Required Fields for `[channels.telegram]`

| Field    | Description                                    | Example                                        |
| -------- | ---------------------------------------------- | ---------------------------------------------- |
| `type`   | Channel type identifier                        | `"telegram"`                                   |
| `token`  | Bot token from BotFather                       | `"123456789:ABCdef..."`                        |
| `target` | Telegram chat ID where notifications are sent  | `"6408789164"`                                 |

> **Note:** The `stakpak autopilot channel add` command (v0.3.44) writes `token` and `require_mention` but omits `type` and `target`. You must add these manually or the config will fail to parse.

## Step 6: Add Schedules (Optional)

Add a cron-based schedule that runs an agent and notifies you on Telegram:

```bash
stakpak autopilot schedule add health-check \
  --cron "*/5 * * * *" \
  --prompt "Check system health"
```

This adds to `autopilot.toml`:

```toml
[[schedules]]
name = "health-check"
cron = "*/5 * * * *"
prompt = "Check system health"
enabled = true
max_steps = 50
trigger_on = "failure"
pause_on_approval = false
```

## Step 7: Start Autopilot

```bash
stakpak up
```

Or with a specific profile:

```bash
stakpak --profile team up
```

Expected output:

```
  Autopilot is running.

  Server      http://127.0.0.1:4096
  Schedules   1 active
  Channels    telegram

  View logs   stakpak autopilot logs
  Status      stakpak autopilot status
  Stop        stakpak down
```

## Step 8: Verify

1. Send a message to your bot on Telegram
2. Check the logs for activity:

```bash
stakpak autopilot logs
```

---

## Useful Commands

| Command                                | Description                         |
| -------------------------------------- | ----------------------------------- |
| `stakpak up`                           | Start autopilot                     |
| `stakpak down`                         | Stop autopilot                      |
| `stakpak autopilot status`             | Check autopilot status              |
| `stakpak autopilot logs`               | View autopilot logs                 |
| `stakpak autopilot restart`            | Restart autopilot                   |
| `stakpak autopilot channel add telegram --token <TOKEN>` | Add Telegram channel |
| `stakpak autopilot channel list`       | List configured channels            |
| `stakpak autopilot channel test`       | Test channel connectivity           |
| `stakpak autopilot schedule add <name> --cron <expr> --prompt <text>` | Add a schedule |

---

## Known Issue: `unsupported parse_mode` Error

### Symptom

Gateway logs show:

```
WARN stakpak_gateway::channels::telegram: telegram send chunk failed
  error=telegram sendMessage error 400: Bad Request: unsupported parse_mode
```

The bot receives messages but cannot reply.

### Cause

In `libs/gateway/src/channels/telegram.rs`, the `SendMessageParams` struct serializes `Option<String>` fields as `null` in JSON when they are `None`. Telegram's API rejects `"parse_mode": null` — it expects either a valid value (`"HTML"`, `"Markdown"`, `"MarkdownV2"`) or the field to be omitted entirely.

### File

`libs/gateway/src/channels/telegram.rs` — `SendMessageParams` struct

### Before (broken)

```rust
// Line 315-322
#[derive(Debug, Serialize)]
struct SendMessageParams {
    chat_id: i64,
    text: String,
    parse_mode: Option<String>,
    reply_to_message_id: Option<i64>,
    message_thread_id: Option<i64>,
}
```

This serializes to:

```json
{
  "chat_id": 123456789,
  "text": "Hello",
  "parse_mode": null,
  "reply_to_message_id": null,
  "message_thread_id": null
}
```

Telegram rejects `"parse_mode": null` with HTTP 400.

### After (fixed)

```rust
// Line 315-325
#[derive(Debug, Serialize)]
struct SendMessageParams {
    chat_id: i64,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    parse_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reply_to_message_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message_thread_id: Option<i64>,
}
```

This serializes to:

```json
{
  "chat_id": 123456789,
  "text": "Hello"
}
```

The `None` fields are omitted entirely, which Telegram accepts.

### Resolution

- Fixed in source at commit containing this change
- Requires building from source (`cargo build --release -p stakpak`) or upgrading to the release that includes this fix
- Affects all versions up to and including v0.3.44

---

## Known Issue: `autopilot channel add` Missing Fields

### Symptom

After running `stakpak autopilot channel add telegram --token <TOKEN>`, the next `stakpak up` fails with:

```
TOML parse error at line XX, column 1
   |
XX | [channels.telegram]
   | ^^^^^^^^^^^^^^^^^^^
missing field `type`
```

Or after adding `type`:

```
missing field `target`
```

### Cause

The `channel add` command writes the channel config in the gateway format (`token` + `require_mention`) but `stakpak up` parses it using the autopilot format which additionally requires `type` and `target` fields.

### Fix

Manually add the missing fields to `~/.stakpak/autopilot.toml`:

```toml
[channels.telegram]
type = "telegram"
token = "<YOUR_BOT_TOKEN>"
target = "<YOUR_CHAT_ID>"
```

---

## Comparison with OpenClaw / ZeroClaw / NanoClaw

Stakpak's Telegram integration was compared against other open-source agent platforms to identify gaps and improvements.

### OpenClaw

OpenClaw has the most mature Telegram integration. Key differences from Stakpak:

| Feature | OpenClaw | Stakpak |
|---------|----------|---------|
| Parse mode | Sends `parse_mode: "HTML"`, retries as plain text on failure | Sends `parse_mode: null` (bug), no fallback retry |
| Text chunk limit | 4000 chars | 4096 chars |
| Privacy mode docs | Documented: must `/setprivacy` via BotFather or grant admin | Not documented |
| DM access control | `pairing`, `allowlist`, `open`, `disabled` | `require_mention` only |
| Group access control | Per-group allowlist, sender filtering, mention requirements | No group-level access control |
| Streaming/live preview | `streamMode: "partial"` or `"block"` (edits messages during generation) | No streaming to Telegram |
| Webhook support | Supports both long polling and webhook mode | Long polling only |
| Link preview control | `linkPreview: false` config option | No link preview control |
| Media support | Audio, video, stickers, voice notes | Text only |
| Inline buttons | Configurable per scope (`dm`, `group`, `all`) | Not supported |
| Native commands | Registers `/command` menus with Telegram at startup | Not supported |
| Forum/topic support | Full forum topic support with per-topic config | Basic `message_thread_id` forwarding |

**Reference:** [OpenClaw Telegram Docs](https://docs.openclaw.ai/channels/telegram)

### ZeroClaw

ZeroClaw is a lightweight Rust-based alternative to OpenClaw. Key differences:

| Feature | ZeroClaw | Stakpak |
|---------|----------|---------|
| Allowlist model | Empty = deny all, `"*"` = allow all, supports usernames and numeric IDs | `require_mention` flag only |
| Onboarding | `zeroclaw onboard --channels-only` auto-discovers sender identity from logs | Manual chat ID lookup via `getUpdates` API |
| Migration | `zeroclaw migrate openclaw` imports existing data | No migration tooling |

**Reference:** [ZeroClaw GitHub](https://github.com/zeroclaw-labs/zeroclaw)

### NanoClaw

NanoClaw does **not** have Telegram support yet. It currently only supports WhatsApp via Baileys. Telegram is listed as a "Request for Skills" (`/add-telegram`).

**Reference:** [NanoClaw GitHub](https://github.com/qwibitai/nanoclaw)

---

## Additional Gaps Identified

Based on comparison with other platforms, the following improvements are recommended for Stakpak's Telegram integration:

### 1. No Privacy Mode Documentation

**Impact:** Bots in Telegram groups default to Privacy Mode, which means they only receive messages that mention the bot or are commands. Users may not know they need to disable this via BotFather (`/setprivacy`) or grant admin status.

**Recommendation:** Document this in the setup guide and add a warning in `stakpak autopilot channel test`.

### 2. No `GetUpdatesParams` Skip Serialization

**File:** `libs/gateway/src/channels/telegram.rs:308-313`

The `GetUpdatesParams` struct also has an `Option` field (`offset`) that serializes as `null`:

```rust
#[derive(Debug, Serialize)]
struct GetUpdatesParams {
    offset: Option<i64>,    // serializes as "offset": null
    timeout: i64,
    allowed_updates: Vec<String>,
}
```

While Telegram currently accepts `"offset": null`, for consistency and safety it should also use `skip_serializing_if`:

```rust
#[derive(Debug, Serialize)]
struct GetUpdatesParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    offset: Option<i64>,
    timeout: i64,
    allowed_updates: Vec<String>,
}
```

### 3. No Graceful Fallback on Send Failure

**File:** `libs/gateway/src/channels/telegram.rs:107-153`

When `send_chunk` fails (e.g., due to message formatting), the error is returned immediately with no retry. OpenClaw retries with plain text if HTML parse fails.

**Recommendation:** If a send fails with a 400 error related to formatting, retry without `parse_mode`.

### 4. No Media Support

**File:** `libs/gateway/src/channels/telegram.rs:155-210`

Inbound messages only extract `text` (line 157). Photos, documents, audio, video, and stickers are silently dropped.

```rust
let text = message.text?;  // Returns None for media-only messages
```

### 5. No `disable_notification` Option

Telegram supports `disable_notification` to send messages silently. This would be useful for schedule notifications that shouldn't buzz the user's phone at 3 AM.

---

## Building from Source (Required for Fixes)

If you're running v0.3.44 (the latest Homebrew release), the `parse_mode` bug and missing config fields issue are present. To get the fixes, build from source.

### Prerequisites

- Rust toolchain (`rustup` + `cargo`)
- macOS or Linux

### Build Steps

```bash
# Clone the repo
git clone https://github.com/stakpak/agent.git
cd agent

# Build release binary
cargo build --release -p stakpak
```

The binary will be at `./target/release/stakpak`.

### Running from Source

**Important:** When running from source, use `--foreground` to run the fixed binary directly. If you use `stakpak up` without `--foreground`, it installs a system service that uses the system-installed binary (v0.3.44), not your built binary.

```bash
# Stop any running autopilot first
./target/release/stakpak down

# Kill any lingering process on port 4096
lsof -ti :4096 | xargs kill 2>/dev/null

# Run from source in foreground
./target/release/stakpak --profile team up --foreground
```

Expected output:

```
+-------------------------------------+
|   Stakpak Autopilot                 |
|   Autonomous Agent Scheduler        |
+-------------------------------------+

Configuration:
  PID:        54951
  Database:   ~/.stakpak/autopilot/autopilot.db
  Profile:    team
  Timeout:    30m

Registered Schedules (1):
  NAME                     CRON               NEXT RUN
  ------------------------------------------------------------------
  health-check             * * * * *          in 15s

Autopilot running. Press Ctrl+C to stop.

  Server      http://127.0.0.1:4096
  Auth        enabled
  Gateway     enabled
```

### Replacing the System Binary (Optional)

To replace the Homebrew-installed binary with your built one:

```bash
# Back up the old binary
cp /opt/homebrew/bin/stakpak /opt/homebrew/bin/stakpak.bak

# Copy the new binary
cp ./target/release/stakpak /opt/homebrew/bin/stakpak
```

Then you can use `stakpak up` normally (as a service) with the fixes applied.

---

## Config Format Differences: v0.3.44 vs v0.3.48

The autopilot config format changed between versions. If you switch between the Homebrew binary (v0.3.44) and the source-built binary (v0.3.48), you need to adjust `~/.stakpak/autopilot.toml`.

### v0.3.44 (Homebrew) — Autopilot Format

```toml
[channels.telegram]
type = "telegram"
token = "<YOUR_BOT_TOKEN>"
target = "<YOUR_CHAT_ID>"
```

Required fields: `type`, `token`, `target`

### v0.3.48 (Source) — Gateway Format

```toml
[channels.telegram]
token = "<YOUR_BOT_TOKEN>"
require_mention = false
```

Required fields: `token` only. The `type` and `target` fields are not needed.

### Full Working Config (v0.3.48)

```toml
[server]
url = "https://apiv2.stakpak.dev"
token = "<YOUR_STAKPAK_API_KEY>"
listen = "127.0.0.1:4096"

[gateway]
store = "~/.stakpak/autopilot/gateway.db"
title_template = "{channel} / {peer}"
prune_after_hours = 168
delivery_context_ttl_hours = 4
approval_mode = "allow_all"
approval_allowlist = []

[routing]
dm_scope = "per_channel_peer"
bindings = []

[channels.telegram]
token = "<YOUR_BOT_TOKEN>"
require_mention = false

[[schedules]]
name = "health-check"
cron = "* * * * *"
prompt = "Check system health"
enabled = true
max_steps = 50
trigger_on = "failure"
pause_on_approval = false
```

---

## Schedule Cron Syntax

Schedules use standard 5-field cron syntax. The minimum interval is **1 minute** (cron does not support seconds).

| Expression      | Meaning              |
|-----------------|----------------------|
| `* * * * *`     | Every minute         |
| `*/5 * * * *`   | Every 5 minutes      |
| `*/15 * * * *`  | Every 15 minutes     |
| `0 * * * *`     | Every hour           |
| `0 */6 * * *`   | Every 6 hours        |
| `0 9 * * *`     | Daily at 9 AM        |
| `0 9 * * 1-5`   | Weekdays at 9 AM     |

### Adding a Schedule via CLI

```bash
stakpak autopilot schedule add <NAME> \
  --cron "<CRON_EXPRESSION>" \
  --prompt "<WHAT_THE_AGENT_SHOULD_DO>"
```

Example:

```bash
stakpak autopilot schedule add health-check \
  --cron "* * * * *" \
  --prompt "Check system health"
```

### Schedule Fields Reference

| Field              | Required | Default     | Description                                          |
|--------------------|----------|-------------|------------------------------------------------------|
| `name`             | Yes      | —           | Unique schedule identifier                           |
| `cron`             | Yes      | —           | 5-field cron expression                              |
| `prompt`           | Yes      | —           | Agent prompt to execute on each trigger              |
| `enabled`          | No       | `true`      | Enable/disable the schedule                          |
| `max_steps`        | No       | `50`        | Max agent steps per run                              |
| `trigger_on`       | No       | `"failure"` | When to trigger: `"failure"`, `"success"`, `"any"`   |
| `check`            | No       | —           | Shell command to run before agent (exit code decides trigger) |
| `workdir`          | No       | —           | Working directory for the agent                      |
| `channel`          | No       | —           | Override notification channel for this schedule      |
| `pause_on_approval`| No       | `false`     | Pause when agent requests tool approval              |

### `trigger_on` Values

- `"failure"` — Only run the agent if the `check` command fails (non-zero exit)
- `"success"` — Only run the agent if the `check` command succeeds (exit 0)
- `"any"` — Always run the agent regardless of `check` exit code

> **Known bug (v0.3.44):** The CLI writes `trigger_on = "always"` but the runtime expects `"any"`. If you see `unknown variant 'always'`, change it to `"any"` in `autopilot.toml`.

---

## Telegram Privacy Mode (Group Bots)

If you add the bot to a Telegram group and it doesn't receive messages:

1. Bots default to **Privacy Mode** — they only see messages that mention the bot or are `/commands`
2. To fix, message @BotFather:
   ```
   /setprivacy
   → Select your bot
   → Disable
   ```
3. **Remove and re-add** the bot to the group (privacy mode change requires re-adding)
4. Alternatively, grant the bot **admin status** in group settings
