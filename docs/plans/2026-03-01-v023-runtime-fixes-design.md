# v0.2.3 Design: Runtime Fixes + Daemon Mode + Watcher Setup

**Date:** 2026-03-01
**Status:** Approved
**Scope:** 7 issues across 4 categories — wizard config, model distribution, daemon lifecycle, TUI polish

---

## Issue 1: NABA_ALLOWED_CHAT_IDS Not Set

**Problem:** Wizard collects Telegram bot token but never asks for chat IDs. Bot denies all messages at runtime.

**Solution:** Add `telegram_chat_ids` field to WizardState and WizardResult. Show as a sub-field when Telegram is enabled:

```
  ▸ ◆ Telegram Bot      tok:1234...XYZ
      Chat IDs          123456789_
      Your Telegram numeric ID (comma-separated for multiple)
      Tip: Message @userinfobot on Telegram to find your ID
```

**Files:** `src/tui/wizard.rs`, `src/main.rs`

**Env output:** `NABA_ALLOWED_CHAT_IDS=123456789,987654321`

First ID in the list automatically gets admin privileges (existing behavior in `is_admin()`).

---

## Issue 2: BERT Model Not Found

**Problem:** `bert_model.onnx` not present at `$NABA_DATA_DIR/models/setfit-w5h2/`. Install script only downloads W5H2 SetFit models from GitHub releases.

**Solution:**

1. Upload BERT files to HuggingFace repo `nabaos/bert-security-classifier`:
   - `bert_model.onnx`
   - `bert_tokenizer.json`
   - `bert_classes.json`
   - Use token: `HF_TOKEN_REDACTED`

2. Update `scripts/install.sh` — add `download_bert_model()` function:
   - Download from HF using `curl` (no `hf` CLI dependency for install)
   - Store at `$NABA_DATA_DIR/models/setfit-w5h2/`
   - Called after `download_models()`

3. Update `cmd_setup()` WebBERT download to also fetch BERT classifier files.

4. Demote the "BERT model not found" log from INFO to DEBUG (it fires on every classify call).

**Files:** `scripts/install.sh`, `src/main.rs`, `src/security/bert_classifier.rs`

---

## Issue 3: Firewall + Graceful Shutdown

### Graceful Shutdown

**Problem:** Ctrl+C kills the process but web server keeps the port bound (no signal handling).

**Solution:** Add signal handling to the daemon loop and web server.

**Daemon (main.rs cmd_daemon):**
- Replace `std::thread::sleep(60s)` loop with a tokio runtime
- Use `tokio::signal::ctrl_c()` as shutdown trigger
- On signal: set shutdown flag, join spawned threads, exit cleanly

**Web server (channels/web.rs):**
- Use `axum::serve(...).with_graceful_shutdown(shutdown_signal())`
- `shutdown_signal()` awaits a tokio oneshot receiver triggered by the daemon's shutdown

**Telegram (channels/telegram.rs):**
- Check shutdown flag in polling loop, break when set

### Firewall (Advisory Only)

On `nabaos start`, if web is public (`0.0.0.0:port`) and `ufw` is installed+active:
- Print: `Hint: sudo ufw allow <port>/tcp`

On `nabaos stop`, same check:
- Print: `Hint: sudo ufw delete allow <port>/tcp`

We do NOT auto-run firewall commands (requires root, too risky).

**Files:** `src/main.rs`, `src/channels/web.rs`, `src/channels/telegram.rs`

---

## Issue 4: Embed Web Frontend in Binary

**Problem:** Binary looks for `nabaos-web/dist/` on filesystem. Release binary never has it.

**Solution:** Use `rust-embed` crate to embed `nabaos-web/dist/` at compile time.

```rust
#[derive(rust_embed::Embed)]
#[folder = "nabaos-web/dist/"]
struct WebAssets;
```

In `web.rs`, serve from embedded assets:
- `GET /` → serve `index.html` from embedded
- `GET /assets/*` → serve JS/CSS from embedded
- SPA fallback: any non-API route → `index.html`
- If `nabaos-web/dist/` didn't exist at compile time (dev build without npm), the embed is empty → show fallback HTML

**Build sequence:** `cd nabaos-web && npm run build` before `cargo build --release`.

**Files:** `Cargo.toml` (add rust-embed), `src/channels/web.rs`

---

## Issue 5: TUI Redesign + Log Suppression

### Log Suppression

**Problem:** Tracing logs (`INFO BERT model not found...`) flood stdout and overwrite the TUI.

**Solution:** When TUI is active, install a custom tracing layer that writes to an in-memory ring buffer (`VecDeque<String>`, capacity 500 lines). The TUI reads from this buffer to display a "Logs" panel.

Implementation:
- Create `src/tui/log_layer.rs` — custom `tracing_subscriber::Layer` that formats events and pushes to `Arc<Mutex<VecDeque<String>>>`
- In `run_tui()`, build subscriber with both the ring-buffer layer and a file appender (for `$NABA_DATA_DIR/logs/nabaos.log`)
- No tracing output goes to stdout/stderr while TUI is active

### TUI Redesign

Apply wizard color palette and polish:

- **Colors:** BG(22,22,30), FG(200,200,210), ACCENT(255,175,95), GREEN(130,200,130)
- **Title bar:** NabaOS banner with version, uptime, active channels indicators
- **Tab bar:** Styled tabs matching wizard aesthetic (rounded borders, accent highlights)
- **Status bar:** Show memory usage, cache hit %, active agents count
- **Logs panel:** Bottom 25% of screen shows live log tail from ring buffer (toggleable with `L` key)
- **6th tab: Logs** — full-screen scrollable log view

**Files:** `src/tui/app.rs`, `src/tui/log_layer.rs` (new), `src/tui/mod.rs`

---

## Issue 6: Watcher Config in Wizard

**Problem:** Watcher feature exists but wizard doesn't expose it. Users must manually edit .env.

**Solution:** Add Step 12 (Watcher) to the wizard, between PEA and Channels.

### Wizard UI

```
  ╭─ Runtime Watcher ──────────────────────────────╮
  │                                                 │
  │  ▸ ◆ Enable Watcher                            │
  │                                                 │
  │      Alert Threshold    ◄ 0.7 (default) ►      │
  │      Pause Threshold    ◄ 0.9 (default) ►      │
  │      Alert Channel      ◄ Telegram ►           │
  │                                                 │
  │  Monitors anomalies, auto-pauses risky          │
  │  components, and sends alerts.                  │
  │                                                 │
  ╰─────────────────────────────────────────────────╯
```

### Fields

| Field | Type | Options | Default |
|-------|------|---------|---------|
| Enable | toggle | on/off | off |
| Alert Threshold | cycle | 0.5 / 0.7 / 0.9 | 0.7 |
| Pause Threshold | cycle | 0.8 / 0.9 / 0.95 | 0.9 |
| Alert Channel | cycle | Telegram / Web / Both | Telegram (only enabled channels shown) |

### WizardState/Result additions

```rust
// WizardState
watcher_enabled: bool,
watcher_alert_threshold_idx: usize,  // indexes into [0.5, 0.7, 0.9]
watcher_pause_threshold_idx: usize,  // indexes into [0.8, 0.9, 0.95]
watcher_alert_channel_idx: usize,    // indexes into filtered channel list
watcher_focus: usize,                // 0=toggle, 1=alert, 2=pause, 3=channel

// WizardResult
pub enable_watcher: bool,
pub watcher_alert_threshold: f64,
pub watcher_pause_threshold: f64,
pub watcher_alert_channels: String,
```

### Env output

```
NABA_WATCHER_ENABLED=true
NABA_WATCHER_ALERT_THRESHOLD=0.7
NABA_WATCHER_PAUSE_THRESHOLD=0.9
NABA_WATCHER_ALERT_CHANNELS=telegram
```

**Files:** `src/tui/wizard.rs`, `src/main.rs`

---

## Issue 7: Daemon Mode with systemd

**Problem:** `nabaos start` runs in foreground indefinitely. No `stop` command. No auto-restart on reboot.

### New CLI structure

```
nabaos start              # Pre-flight checks → install+start systemd service → exit
nabaos start --foreground # Run daemon directly (what systemd ExecStart calls)
nabaos stop               # Stop systemd service (or send SIGTERM to PID)
nabaos status             # Show service status, active channels, uptime
```

### `nabaos start` flow

```
1. Pre-flight checks:
   ✓ Config file exists ($NABA_DATA_DIR/.env)
   ✓ LLM provider configured
   ✓ Telegram token format valid (if enabled)
   ✓ Web port available (if enabled)
   ✓ Model files present (if BERT enabled)
   ✗ Any failure → print error, exit 1

2. Check systemd availability (systemctl --version):

   YES (systemd available):
     a. Generate unit file
     b. Install to:
        - /etc/systemd/system/nabaos.service (if root)
        - ~/.config/systemd/user/nabaos.service (if non-root)
     c. systemctl [--user] daemon-reload
     d. systemctl [--user] enable --now nabaos
     e. Print success + "journalctl [--user] -u nabaos -f" hint
     f. Print firewall hint if web public + ufw active
     g. Exit 0

   NO (Docker/WSL/no systemd):
     a. Print "systemd not available, running in foreground"
     b. Write PID to $NABA_DATA_DIR/nabaos.pid
     c. Run cmd_daemon() with signal handlers
     d. On exit: remove PID file
```

### Generated systemd unit

```ini
[Unit]
Description=NabaOS - Autonomous agent runtime
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart={binary_path} start --foreground
EnvironmentFile={data_dir}/.env
Environment=NABA_DATA_DIR={data_dir}
WorkingDirectory={data_dir}
Restart=on-failure
RestartSec=5
TimeoutStopSec=30

[Install]
WantedBy=multi-user.target  # (or default.target for user service)
```

### `nabaos stop`

```
1. Check for systemd service:
   - systemctl [--user] is-active nabaos → stop it
2. Fallback: check $NABA_DATA_DIR/nabaos.pid
   - Read PID, send SIGTERM, wait up to 10s, then SIGKILL
3. Print firewall cleanup hint if applicable
```

### `nabaos status`

```
NabaOS v0.2.3
  Status:    ● running (systemd, PID 12345)
  Uptime:    2h 15m
  Channels:  Telegram ✓  Web ✓ (0.0.0.0:8919)
  Watcher:   enabled (0 alerts, 0 paused)
  Cache:     87% hit rate (1,234 queries)
```

If not running: show last exit reason from journald.

**Files:** `src/main.rs` (new subcommands, pre-flight, systemd generation)

---

## Build Sequence

1. `cd nabaos-web && npm run build` (generates dist/)
2. `cargo build --release` (embeds dist/ via rust-embed)
3. Upload BERT model to HuggingFace
4. Package + release

## Execution Priority

| Priority | Issue | Risk | Effort |
|----------|-------|------|--------|
| P0 | #7 Daemon mode + signals | High (users can't stop cleanly) | Large |
| P0 | #3 Graceful shutdown | High (port stays bound) | Medium |
| P0 | #4 Embed web frontend | High (web never works) | Medium |
| P1 | #1 Chat IDs in wizard | Medium (telegram unusable) | Small |
| P1 | #2 BERT model on HF | Medium (Tier-1 broken) | Small |
| P1 | #5 TUI redesign + logs | Medium (UX broken) | Large |
| P2 | #6 Watcher in wizard | Low (power users only) | Small |
