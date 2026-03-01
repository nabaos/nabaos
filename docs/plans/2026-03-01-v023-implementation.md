# v0.2.3 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix 7 runtime bugs/UX issues: chat ID config, BERT model on HF, graceful shutdown + firewall hints, embedded web frontend, TUI redesign + log suppression, watcher wizard step, and systemd daemon mode.

**Architecture:** Each issue is a self-contained task. We add `rust-embed` for SPA embedding, a custom tracing layer for TUI log capture, signal handling with `tokio::signal`, and systemd service generation. The wizard gains 2 new steps (Watcher) and 2 new fields (chat IDs, watcher config).

**Tech Stack:** Rust 2024, ratatui 0.29, axum 0.8, rust-embed, tracing-subscriber custom layer, tokio signals, systemd unit generation, HuggingFace Hub upload.

---

## Task 1: Add Telegram Chat IDs to Wizard

**Files:**
- Modify: `src/tui/wizard.rs:165-191` (WizardResult), `src/tui/wizard.rs:347-437` (WizardState), `src/tui/wizard.rs:866-930` (into_result), `src/tui/wizard.rs:2344-2439` (draw_channels)
- Modify: `src/main.rs:3697` (.env writing for telegram)

**Step 1: Add fields to WizardResult and WizardState**

In `src/tui/wizard.rs`, add to WizardResult after `telegram_token` (line 175):

```rust
pub telegram_chat_ids: String,
```

Add to WizardState after `telegram_editing` (line 421):

```rust
telegram_chat_ids: String,
telegram_chat_ids_editing: bool,
```

Initialize in WizardState::new() (around line 728):

```rust
telegram_chat_ids: String::new(),
telegram_chat_ids_editing: false,
```

Add to into_result() (around line 914):

```rust
telegram_chat_ids: self.telegram_chat_ids,
```

**Step 2: Add chat IDs sub-field UI in draw_channels**

In `draw_channels()` after the Telegram token line block (after line 2375, the closing `}` of the Telegram section), add a sub-field when telegram is enabled:

```rust
if state.telegram_enabled {
    let ids_focused = state.channel_focus == 0 && !state.telegram_editing;
    let ids_editing = state.telegram_chat_ids_editing;
    let ids_marker = if ids_editing || ids_focused { "▸" } else { " " };
    let ids_bg = if ids_editing || ids_focused { HIGHLIGHT_BG } else { BG };

    let mut id_spans = vec![
        Span::styled(format!("      {} ", ids_marker), Style::default().fg(ACCENT).bg(ids_bg)),
        Span::styled("Chat IDs      ", Style::default().fg(HEADING).bg(ids_bg)),
    ];
    if ids_editing {
        id_spans.push(Span::styled(format!("{}_", state.telegram_chat_ids), Style::default().fg(ACCENT).bg(ids_bg)));
    } else if !state.telegram_chat_ids.is_empty() {
        id_spans.push(Span::styled(&state.telegram_chat_ids, Style::default().fg(FG).bg(ids_bg)));
    } else {
        id_spans.push(Span::styled("not set", Style::default().fg(DIM).bg(ids_bg)));
    }
    lines.push(Line::from(id_spans));

    if ids_editing || (ids_focused && state.telegram_chat_ids.is_empty()) {
        lines.push(Line::from(vec![
            Span::styled("      Your Telegram numeric ID (comma-separated for multiple)", Style::default().fg(DIM).bg(BG)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("      Tip: Message @userinfobot on Telegram to find your ID", Style::default().fg(DIM).bg(BG)),
        ]));
    }
}
```

**Step 3: Add key handling for chat IDs editing**

In the Channels key handler (around line 1496-1600), add handling for `telegram_chat_ids_editing` — accept digits, commas, and backspace. Toggle editing on Enter when Telegram sub-field is focused. Model it on the existing `telegram_editing` handler.

**Step 4: Write NABA_ALLOWED_CHAT_IDS in .env**

In `src/main.rs`, after the telegram token line (around line 3700), add:

```rust
if !result.telegram_chat_ids.is_empty() {
    env_lines.push(format!("NABA_ALLOWED_CHAT_IDS={}", result.telegram_chat_ids));
}
```

**Step 5: Run tests**

Run: `cargo test --lib tui::wizard`
Expected: All existing tests pass (new fields have defaults)

**Step 6: Commit**

```bash
git add src/tui/wizard.rs src/main.rs
git commit -m "feat: add Telegram chat IDs to setup wizard"
```

---

## Task 2: Upload BERT Model to HuggingFace + Fix Download

**Files:**
- Modify: `scripts/install.sh:270-297` (add download_bert_model function)
- Modify: `src/security/bert_classifier.rs:197-216` (demote log level)

**Step 1: Create HF repo and upload BERT model files**

First, check if BERT model files exist locally:

```bash
find ~/.nabaos/models -name "bert_*" -o -name "bert_*" 2>/dev/null
ls -la ~/nabaos/models/ 2>/dev/null
```

If files don't exist locally, we need to generate/find them. The BERT classifier expects:
- `bert_model.onnx`
- `bert_tokenizer.json`
- `bert_classes.json`

Upload to HuggingFace using the provided token:

```bash
# Create repo
curl -X POST "https://huggingface.co/api/repos/create" \
  -H "Authorization: Bearer HF_TOKEN_REDACTED" \
  -H "Content-Type: application/json" \
  -d '{"name":"bert-security-classifier","type":"model","private":false}'

# Upload files with hf CLI
hf upload nabaos/bert-security-classifier ./bert_model.onnx bert_model.onnx --token HF_TOKEN_REDACTED
hf upload nabaos/bert-security-classifier ./bert_tokenizer.json bert_tokenizer.json --token HF_TOKEN_REDACTED
hf upload nabaos/bert-security-classifier ./bert_classes.json bert_classes.json --token HF_TOKEN_REDACTED
```

**Step 2: Add download_bert_model() to install.sh**

In `scripts/install.sh`, after `download_models()` function (line 297), add:

```bash
download_bert_model() {
    local bert_dir="${DATA_DIR}/models/setfit-w5h2"
    if [ -f "${bert_dir}/bert_model.onnx" ]; then
        ok "BERT security classifier already present"
        return
    fi

    info "Downloading BERT security classifier from HuggingFace..."
    mkdir -p "$bert_dir"
    local hf_base="https://huggingface.co/nabaos/bert-security-classifier/resolve/main"
    local files=("bert_model.onnx" "bert_tokenizer.json" "bert_classes.json")
    for f in "${files[@]}"; do
        if ! download "${hf_base}/${f}" "${bert_dir}/${f}" 2>/dev/null; then
            warn "Could not download ${f} — BERT Tier 1 classification disabled"
            return
        fi
    done
    ok "BERT security classifier installed"
}
```

Call it in `main()` after `download_models`:

```bash
download_bert_model
```

**Step 3: Demote "BERT model not found" from INFO to DEBUG**

In `src/security/bert_classifier.rs:200`, change:

```rust
tracing::info!(
```

to:

```rust
tracing::debug!(
```

This prevents the log spam on every classify call when BERT isn't installed.

**Step 4: Run tests**

Run: `cargo test --lib security`
Expected: PASS

**Step 5: Commit**

```bash
git add scripts/install.sh src/security/bert_classifier.rs
git commit -m "feat: upload BERT model to HuggingFace, fix install + demote log"
```

---

## Task 3: Add rust-embed and Embed Web Frontend

**Files:**
- Modify: `Cargo.toml:127-129` (add rust-embed)
- Modify: `src/channels/web.rs:2558-2574` (fallback HTML), `src/channels/web.rs:2860-2980` (create_router, static serving)

**Step 1: Add rust-embed dependency**

In `Cargo.toml` after `tower-http` (line 129), add:

```toml
rust-embed = "8"
```

**Step 2: Build the web frontend**

```bash
cd nabaos-web && npm run build
```

Verify: `ls nabaos-web/dist/index.html nabaos-web/dist/assets/`

**Step 3: Add embed struct in web.rs**

At the top of `src/channels/web.rs` (near other imports), add:

```rust
#[derive(rust_embed::Embed)]
#[folder = "nabaos-web/dist/"]
struct WebAssets;
```

**Step 4: Replace filesystem static serving with embedded assets**

Replace the static dir serving block (around lines 2966-2980) with:

```rust
// Serve embedded SPA assets, or fallback HTML if not embedded
if WebAssets::get("index.html").is_some() {
    app = app
        .fallback(|uri: axum::http::Uri| async move {
            let path = uri.path().trim_start_matches('/');
            if let Some(file) = WebAssets::get(path) {
                let mime = mime_guess::from_path(path).first_or_octet_stream();
                (
                    [(axum::http::header::CONTENT_TYPE, mime.as_ref().to_string())],
                    file.data.to_vec(),
                ).into_response()
            } else if let Some(index) = WebAssets::get("index.html") {
                // SPA fallback — serve index.html for client-side routing
                (
                    [(axum::http::header::CONTENT_TYPE, "text/html".to_string())],
                    index.data.to_vec(),
                ).into_response()
            } else {
                axum::http::StatusCode::NOT_FOUND.into_response()
            }
        });
} else {
    app = app.fallback(|| async { fallback_html().await });
}
```

**Step 5: Add mime_guess dependency**

In `Cargo.toml`:

```toml
mime_guess = "2"
```

Also add the `IntoResponse` import at the top of web.rs:

```rust
use axum::response::IntoResponse;
```

**Step 6: Verify build**

Run: `cargo check`
Expected: zero errors

**Step 7: Commit**

```bash
git add Cargo.toml src/channels/web.rs
git commit -m "feat: embed web SPA in binary with rust-embed"
```

---

## Task 4: Graceful Shutdown + Firewall Hints

**Files:**
- Modify: `src/main.rs:59-76` (Start command args), `src/main.rs:874-886` (Start dispatch), `src/main.rs:2668-3008` (cmd_daemon)
- Modify: `src/channels/web.rs:2990-3040` (run_server_with_engine)

**Step 1: Add --foreground flag to Start command**

In `src/main.rs:66-76`, add:

```rust
/// Run in foreground (used by systemd ExecStart)
#[arg(long)]
foreground: bool,
```

**Step 2: Add shutdown signal to web server**

In `src/channels/web.rs`, modify `run_server_with_engine` to accept a shutdown receiver:

```rust
pub async fn run_server_with_engine(
    config: NyayaConfig,
    orch: Orchestrator,
    two_fa: TwoFactorAuth,
    bind_addr: &str,
    workflow_engine: Option<Arc<Mutex<WorkflowEngine>>>,
    shutdown_rx: Option<tokio::sync::watch::Receiver<bool>>,
) -> Result<()> {
```

Before `axum::serve(listener, app).await?;`, replace with:

```rust
if let Some(mut rx) = shutdown_rx {
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = rx.wait_for(|&v| v).await;
            tracing::info!("Web server shutting down gracefully...");
        })
        .await?;
} else {
    axum::serve(listener, app).await?;
}
```

**Step 3: Add shutdown coordination to cmd_daemon**

In `src/main.rs`, in `cmd_daemon()`:

a) Create shutdown channel at the top (after line 2668):

```rust
let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);
```

b) Pass shutdown receiver to web server spawn (replace the web thread block around line 2800-2835):

```rust
let web_shutdown_rx = shutdown_tx.subscribe();
// ... pass web_shutdown_rx to run_server_with_engine
```

c) Replace the `loop { ... sleep(60) }` (lines 2876-3008) with a tokio runtime that handles signals:

```rust
// Create a flag for the main loop
let running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
let r = running.clone();
ctrlc::set_handler(move || {
    eprintln!("\n[daemon] Received shutdown signal, stopping...");
    r.store(false, std::sync::atomic::Ordering::SeqCst);
}).ok();

while running.load(std::sync::atomic::Ordering::SeqCst) {
    // ... existing loop body ...
    std::thread::sleep(std::time::Duration::from_secs(1)); // check more often
}

// Trigger web server shutdown
let _ = shutdown_tx.send(true);
println!("[daemon] Shutdown complete.");
```

Add `ctrlc = "3.4"` to Cargo.toml dependencies.

**Step 4: Add firewall hints**

In `cmd_daemon()`, after web server spawn, add:

```rust
// Advisory: firewall hint for public web access
if bind.starts_with("0.0.0.0") {
    let port = bind.split(':').last().unwrap_or("8919");
    if std::process::Command::new("ufw").arg("status").output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("active"))
        .unwrap_or(false)
    {
        println!("[daemon] Hint: sudo ufw allow {}/tcp  (web is public)", port);
    }
}
```

**Step 5: Run cargo check**

Run: `cargo check`
Expected: zero errors

**Step 6: Commit**

```bash
git add Cargo.toml src/main.rs src/channels/web.rs
git commit -m "feat: graceful shutdown on Ctrl+C, firewall hints for public web"
```

---

## Task 5: TUI Log Suppression + Redesign

**Files:**
- Create: `src/tui/log_layer.rs`
- Modify: `src/tui/mod.rs` (add module)
- Modify: `src/tui/app.rs:1-603` (redesign)

**Step 1: Create ring-buffer tracing layer**

Create `src/tui/log_layer.rs`:

```rust
//! Custom tracing layer that captures log events into a ring buffer
//! instead of writing to stdout, for TUI display.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tracing_subscriber::Layer;

pub struct RingBufferLayer {
    buffer: Arc<Mutex<VecDeque<String>>>,
    capacity: usize,
}

impl RingBufferLayer {
    pub fn new(capacity: usize) -> (Self, Arc<Mutex<VecDeque<String>>>) {
        let buffer = Arc::new(Mutex::new(VecDeque::with_capacity(capacity)));
        let layer = Self {
            buffer: Arc::clone(&buffer),
            capacity,
        };
        (layer, buffer)
    }
}

impl<S> Layer<S> for RingBufferLayer
where
    S: tracing::Subscriber,
{
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: tracing_subscriber::layer::Context<'_, S>) {
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);
        let level = event.metadata().level();
        let target = event.metadata().target();
        let line = format!("{} {:>5} {} {}", chrono::Local::now().format("%H:%M:%S"), level, target, visitor.message);
        if let Ok(mut buf) = self.buffer.lock() {
            if buf.len() >= self.capacity {
                buf.pop_front();
            }
            buf.push_back(line);
        }
    }
}

#[derive(Default)]
struct MessageVisitor {
    message: String,
}

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{:?}", value);
        }
    }
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        }
    }
}
```

**Step 2: Register module**

In `src/tui/mod.rs`, add:

```rust
pub mod log_layer;
```

**Step 3: Wire into run_tui**

In `src/tui/app.rs:275` (`run_tui`), before entering the event loop, replace the global tracing subscriber with the ring-buffer layer:

```rust
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

let (layer, log_buffer) = super::log_layer::RingBufferLayer::new(500);
let file_appender = tracing_appender::rolling::daily(&config.data_dir.join("logs"), "nabaos.log");
let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

// Replace global subscriber for TUI mode
let subscriber = tracing_subscriber::registry()
    .with(layer)
    .with(tracing_subscriber::fmt::layer().with_writer(non_blocking).with_target(false));
tracing::subscriber::set_global_default(subscriber).ok();
```

Add `tracing-appender = "0.2"` to Cargo.toml.

Pass `log_buffer` to `App::new()` and store it. Use it in a "Logs" panel at the bottom of the TUI.

**Step 4: Redesign TUI colors and layout**

In `src/tui/app.rs`, replace hardcoded colors with the wizard palette:

```rust
const BG: Color = Color::Rgb(22, 22, 30);
const FG: Color = Color::Rgb(200, 200, 210);
const ACCENT: Color = Color::Rgb(255, 175, 95);
const GREEN: Color = Color::Rgb(130, 200, 130);
const DIM: Color = Color::Rgb(100, 100, 120);
const HEADING: Color = Color::Rgb(170, 170, 190);
const BORDER: Color = Color::Rgb(60, 60, 80);
const HIGHLIGHT_BG: Color = Color::Rgb(35, 35, 50);
```

Replace title bar rendering (around line 402-407) with NabaOS banner:

```rust
let title = Line::from(vec![
    Span::styled(" NabaOS ", Style::default().fg(ACCENT).bg(BG).add_modifier(Modifier::BOLD)),
    Span::styled(format!("v{} ", env!("CARGO_PKG_VERSION")), Style::default().fg(DIM).bg(BG)),
    Span::styled("│ ", Style::default().fg(BORDER).bg(BG)),
    Span::styled(format!("▲{} ", self.stats_queries), Style::default().fg(GREEN).bg(BG)),
    Span::styled(format!("$-{:.2} ", self.stats_saved), Style::default().fg(GREEN).bg(BG)),
    Span::styled(format!("cache {}% ", self.stats_cache_pct as u32), Style::default().fg(ACCENT).bg(BG)),
]);
```

Add bottom Logs panel (25% height, toggleable with `L`):

```rust
// In the main layout, split vertically:
let main_chunks = Layout::vertical([
    Constraint::Length(1),  // title
    Constraint::Length(1),  // tabs
    Constraint::Min(10),    // content
    Constraint::Length(if self.show_logs { 8 } else { 0 }), // logs
    Constraint::Length(1),  // status bar
]).split(area);
```

Render log lines from `log_buffer`:

```rust
if self.show_logs {
    let logs = self.log_buffer.lock().unwrap();
    let log_lines: Vec<Line> = logs.iter().rev().take(8).rev().map(|l| {
        Line::from(Span::styled(l.as_str(), Style::default().fg(DIM).bg(BG)))
    }).collect();
    let log_block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(BORDER))
        .title(Span::styled(" Logs ", Style::default().fg(HEADING).bg(BG)));
    frame.render_widget(Paragraph::new(log_lines).block(log_block).style(Style::default().bg(BG)), main_chunks[3]);
}
```

Add status bar with uptime, memory, active channels.

**Step 5: Add show_logs field and L keybinding**

In `App` struct, add:

```rust
pub show_logs: bool,
log_buffer: Arc<Mutex<VecDeque<String>>>,
```

In key handler, add:

```rust
KeyCode::Char('l') | KeyCode::Char('L') => {
    self.show_logs = !self.show_logs;
}
```

**Step 6: Run tests**

Run: `cargo test --lib tui::`
Expected: All tests pass

**Step 7: Commit**

```bash
git add src/tui/log_layer.rs src/tui/mod.rs src/tui/app.rs Cargo.toml
git commit -m "feat: TUI log capture + dashboard redesign with wizard palette"
```

---

## Task 6: Watcher Config in Wizard

**Files:**
- Modify: `src/tui/wizard.rs:165-206` (WizardResult, Step enum), `src/tui/wizard.rs:347-437` (WizardState), `src/tui/wizard.rs:866-930` (into_result)
- Modify: `src/main.rs` (.env writing)

**Step 1: Add Watcher step to Step enum**

In `src/tui/wizard.rs:194-206`, insert `Watcher` after `Pea`:

```rust
enum Step {
    Welcome,
    Provider,
    ApiKeyModel,
    Constitution,
    Persona,
    Plugins,
    Studio,
    Pea,
    Watcher,   // NEW
    Channels,
    Agents,
    Summary,
}
```

Update `Step::index()`, `Step::count()`, `Step::label()`, and the `next()`/`prev()` navigation accordingly. The step count goes from 11 to 12.

**Step 2: Add WizardResult fields**

After `studio_api_keys` (line 190):

```rust
pub enable_watcher: bool,
pub watcher_alert_threshold: f64,
pub watcher_pause_threshold: f64,
pub watcher_alert_channels: String,
```

**Step 3: Add WizardState fields**

After the PEA fields (around line 432):

```rust
// Watcher
watcher_enabled: bool,
watcher_alert_idx: usize,    // 0=0.5, 1=0.7, 2=0.9
watcher_pause_idx: usize,    // 0=0.8, 1=0.9, 2=0.95
watcher_channel_idx: usize,  // 0=telegram, 1=web, 2=both
watcher_focus: usize,        // 0=toggle, 1=alert, 2=pause, 3=channel
```

Initialize all to defaults (disabled, idx=1 for 0.7, idx=1 for 0.9, idx=0 for telegram, focus=0).

**Step 4: Add into_result() mapping**

In `into_result()` (around line 925):

```rust
const ALERT_THRESHOLDS: [f64; 3] = [0.5, 0.7, 0.9];
const PAUSE_THRESHOLDS: [f64; 3] = [0.8, 0.9, 0.95];
let watcher_channels = match self.watcher_channel_idx {
    0 => "telegram",
    1 => "web",
    _ => "telegram,web",
};

// In the WizardResult struct literal:
enable_watcher: self.watcher_enabled,
watcher_alert_threshold: ALERT_THRESHOLDS[self.watcher_alert_idx],
watcher_pause_threshold: PAUSE_THRESHOLDS[self.watcher_pause_idx],
watcher_alert_channels: watcher_channels.to_string(),
```

**Step 5: Add draw_watcher() function**

Add new function after `draw_pea()`:

```rust
fn draw_watcher(frame: &mut ratatui::Frame, area: Rect, state: &WizardState) {
    let chunks = Layout::vertical([
        Constraint::Length(1), Constraint::Length(1), Constraint::Min(8), Constraint::Length(1),
    ]).split(area);
    draw_step_indicator(frame, chunks[0], Step::Watcher);

    let content_area = centered_rect(55, 65, chunks[2]);
    let block = Block::default()
        .borders(Borders::ALL).border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .title(Line::from(vec![
            Span::styled(" ", Style::default().bg(BG)),
            Span::styled("Runtime Watcher", Style::default().fg(ACCENT).bg(BG).add_modifier(Modifier::BOLD)),
            Span::styled(" ", Style::default().bg(BG)),
        ])).style(Style::default().bg(BG));

    let block_inner = block.inner(content_area);
    frame.render_widget(block, content_area);

    let alert_vals = ["0.5", "0.7 (default)", "0.9"];
    let pause_vals = ["0.8", "0.9 (default)", "0.95"];
    let channel_vals = ["Telegram", "Web", "Both"];

    let mut lines = Vec::new();
    lines.push(Line::from(""));

    // Toggle
    let focused = state.watcher_focus == 0;
    let marker = if focused { "▸" } else { " " };
    let check = if state.watcher_enabled { "◆" } else { "◇" };
    let check_color = if state.watcher_enabled { GREEN } else { DIM };
    let bg = if focused { HIGHLIGHT_BG } else { BG };
    lines.push(Line::from(vec![
        Span::styled(format!("  {} ", marker), Style::default().fg(ACCENT).bg(bg)),
        Span::styled(format!("{} ", check), Style::default().fg(check_color).bg(bg)),
        Span::styled("Enable Watcher", Style::default().fg(if focused { ACCENT } else { FG }).bg(bg).add_modifier(if focused { Modifier::BOLD } else { Modifier::empty() })),
    ]));

    if state.watcher_enabled {
        lines.push(Line::from(""));
        // Alert threshold
        let f = state.watcher_focus == 1;
        let m = if f { "▸" } else { " " };
        lines.push(Line::from(vec![
            Span::styled(format!("      {} ", m), Style::default().fg(ACCENT).bg(if f { HIGHLIGHT_BG } else { BG })),
            Span::styled("Alert Threshold ", Style::default().fg(HEADING).bg(if f { HIGHLIGHT_BG } else { BG })),
            Span::styled(format!("◄ {} ►", alert_vals[state.watcher_alert_idx]), Style::default().fg(if f { ACCENT } else { FG }).bg(if f { HIGHLIGHT_BG } else { BG })),
        ]));

        // Pause threshold
        let f = state.watcher_focus == 2;
        let m = if f { "▸" } else { " " };
        lines.push(Line::from(vec![
            Span::styled(format!("      {} ", m), Style::default().fg(ACCENT).bg(if f { HIGHLIGHT_BG } else { BG })),
            Span::styled("Pause Threshold ", Style::default().fg(HEADING).bg(if f { HIGHLIGHT_BG } else { BG })),
            Span::styled(format!("◄ {} ►", pause_vals[state.watcher_pause_idx]), Style::default().fg(if f { ACCENT } else { FG }).bg(if f { HIGHLIGHT_BG } else { BG })),
        ]));

        // Alert channel
        let f = state.watcher_focus == 3;
        let m = if f { "▸" } else { " " };
        lines.push(Line::from(vec![
            Span::styled(format!("      {} ", m), Style::default().fg(ACCENT).bg(if f { HIGHLIGHT_BG } else { BG })),
            Span::styled("Alert Channel   ", Style::default().fg(HEADING).bg(if f { HIGHLIGHT_BG } else { BG })),
            Span::styled(format!("◄ {} ►", channel_vals[state.watcher_channel_idx]), Style::default().fg(if f { ACCENT } else { FG }).bg(if f { HIGHLIGHT_BG } else { BG })),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  Monitors anomalies, auto-pauses risky components,", Style::default().fg(DIM).bg(BG)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  and sends alerts.", Style::default().fg(DIM).bg(BG)),
    ]));

    frame.render_widget(Paragraph::new(lines).style(Style::default().bg(BG)), block_inner);
    draw_hint_bar(frame, chunks[3], &[("↑↓", "navigate"), ("Space", "toggle"), ("←→", "change"), ("Enter", "next"), ("Esc", "back")]);
}
```

**Step 6: Add key handler and step routing**

In the step dispatch (around line 1637), add:

```rust
Step::Watcher => draw_watcher(frame, inner, state),
```

Add key handler for Watcher step (model on Pea key handling):
- Up/Down: move watcher_focus (0..=3, clamped based on watcher_enabled)
- Space: toggle watcher_enabled when focus==0
- Left/Right: cycle idx when focus 1-3

**Step 7: Write watcher env vars in cmd_setup**

In `src/main.rs`, after the web env block, add:

```rust
if result.enable_watcher {
    env_lines.push("NABA_WATCHER_ENABLED=true".to_string());
    env_lines.push(format!("NABA_WATCHER_ALERT_THRESHOLD={}", result.watcher_alert_threshold));
    env_lines.push(format!("NABA_WATCHER_PAUSE_THRESHOLD={}", result.watcher_pause_threshold));
    env_lines.push(format!("NABA_WATCHER_ALERT_CHANNELS={}", result.watcher_alert_channels));
}
```

**Step 8: Run tests**

Run: `cargo test --lib tui::wizard`
Expected: All tests pass

**Step 9: Commit**

```bash
git add src/tui/wizard.rs src/main.rs
git commit -m "feat: add watcher config step to setup wizard"
```

---

## Task 7: Systemd Daemon Mode (nabaos start / stop / status)

**Files:**
- Modify: `src/main.rs:38-76` (Commands enum), `src/main.rs:874-886` (dispatch), `src/main.rs:2668-3009` (cmd_daemon)

**Step 1: Add Stop and new Start args to Commands enum**

In `src/main.rs:38-76`:

```rust
/// Start the agent runtime
Start {
    /// Only start the Telegram bot
    #[arg(long)]
    telegram_only: bool,
    /// Only start the web dashboard
    #[arg(long)]
    web_only: bool,
    /// Bind address for web dashboard (host:port)
    #[arg(long, default_value = "127.0.0.1:8919")]
    bind: String,
    /// Run in foreground (used by systemd ExecStart)
    #[arg(long)]
    foreground: bool,
},

/// Stop the running agent
Stop,
```

**Step 2: Add pre-flight checks function**

```rust
fn preflight_checks(config: &NyayaConfig) -> Result<()> {
    let env_path = config.data_dir.join(".env");
    let mut ok_count = 0u32;
    let mut fail_count = 0u32;

    // Check .env
    if env_path.exists() {
        println!("  ✓ Config file exists");
        ok_count += 1;
    } else {
        println!("  ✗ Config file missing (run: nabaos setup)");
        fail_count += 1;
    }

    // Check LLM provider
    if std::env::var("NABA_LLM_PROVIDER").is_ok() {
        println!("  ✓ LLM provider configured");
        ok_count += 1;
    } else {
        println!("  ✗ LLM provider not configured");
        fail_count += 1;
    }

    // Check Telegram (if enabled)
    if std::env::var("NABA_TELEGRAM_ENABLED").unwrap_or_default() == "true" {
        if std::env::var("NABA_TELEGRAM_BOT_TOKEN").is_ok() {
            println!("  ✓ Telegram bot token set");
            ok_count += 1;
        } else {
            println!("  ✗ Telegram enabled but token missing");
            fail_count += 1;
        }
        if std::env::var("NABA_ALLOWED_CHAT_IDS").is_ok() {
            println!("  ✓ Telegram chat IDs configured");
            ok_count += 1;
        } else {
            println!("  ⚠ NABA_ALLOWED_CHAT_IDS not set — bot will deny all messages");
        }
    }

    // Check web port (if enabled)
    if std::env::var("NABA_WEB_ENABLED").unwrap_or_default() == "true" {
        let bind = std::env::var("NABA_WEB_BIND").unwrap_or_else(|_| "127.0.0.1:8919".to_string());
        match std::net::TcpListener::bind(&bind) {
            Ok(_) => { println!("  ✓ Web port {} available", bind); ok_count += 1; }
            Err(_) => { println!("  ✗ Web port {} already in use", bind); fail_count += 1; }
        }
    }

    println!("\n  {} checks passed, {} failed", ok_count, fail_count);
    if fail_count > 0 {
        anyhow::bail!("Pre-flight checks failed");
    }
    Ok(())
}
```

**Step 3: Add systemd service generation**

```rust
fn generate_systemd_unit(config: &NyayaConfig) -> String {
    let binary = std::env::current_exe().unwrap_or_default();
    let data_dir = &config.data_dir;
    let env_file = data_dir.join(".env");

    format!(
        r#"[Unit]
Description=NabaOS - Autonomous agent runtime
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart={binary} start --foreground
EnvironmentFile={env_file}
Environment=NABA_DATA_DIR={data_dir}
WorkingDirectory={data_dir}
Restart=on-failure
RestartSec=5
TimeoutStopSec=30

[Install]
WantedBy=default.target
"#,
        binary = binary.display(),
        env_file = env_file.display(),
        data_dir = data_dir.display(),
    )
}

fn install_systemd_service(config: &NyayaConfig) -> Result<bool> {
    // Check if systemd is available
    if std::process::Command::new("systemctl").arg("--version").output().is_err() {
        return Ok(false);
    }

    let unit = generate_systemd_unit(config);
    let is_root = unsafe { libc::getuid() } == 0;

    let (service_path, user_flag) = if is_root {
        ("/etc/systemd/system/nabaos.service".to_string(), "")
    } else {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let dir = format!("{}/.config/systemd/user", home);
        std::fs::create_dir_all(&dir)?;
        (format!("{}/nabaos.service", dir), "--user")
    };

    std::fs::write(&service_path, &unit)?;
    println!("  ✓ Service file written to {}", service_path);

    // daemon-reload
    let mut cmd = std::process::Command::new("systemctl");
    if !user_flag.is_empty() { cmd.arg(user_flag); }
    cmd.arg("daemon-reload").status()?;

    // enable --now
    let mut cmd = std::process::Command::new("systemctl");
    if !user_flag.is_empty() { cmd.arg(user_flag); }
    cmd.args(["enable", "--now", "nabaos"]).status()?;

    println!("  ✓ NabaOS started as systemd service");
    let jcmd = if user_flag.is_empty() {
        "journalctl -u nabaos -f"
    } else {
        "journalctl --user -u nabaos -f"
    };
    println!("  View logs: {}", jcmd);

    Ok(true)
}
```

Add `libc = "0.2"` to Cargo.toml dependencies.

**Step 4: Rewrite Start command dispatch**

Replace `src/main.rs:874-886`:

```rust
Commands::Start {
    telegram_only,
    web_only,
    bind,
    foreground,
} => {
    if telegram_only {
        cmd_telegram(&config)
    } else if web_only {
        cmd_web(&config, &bind)
    } else if foreground {
        // Called by systemd ExecStart — run daemon directly
        cmd_daemon(&config)
    } else {
        // Pre-flight checks → systemd install → exit
        println!("NabaOS v{}", env!("CARGO_PKG_VERSION"));
        println!("\nPre-flight checks:");
        preflight_checks(&config)?;
        println!();

        match install_systemd_service(&config) {
            Ok(true) => Ok(()), // systemd started
            Ok(false) => {
                println!("systemd not available, running in foreground...");
                cmd_daemon(&config)
            }
            Err(e) => {
                println!("systemd install failed: {}", e);
                println!("Falling back to foreground mode...");
                cmd_daemon(&config)
            }
        }
    }
}
```

**Step 5: Add Stop command**

```rust
Commands::Stop => {
    let user_flag = if unsafe { libc::getuid() } == 0 { "" } else { "--user" };

    // Try systemd first
    let mut cmd = std::process::Command::new("systemctl");
    if !user_flag.is_empty() { cmd.arg(user_flag); }
    match cmd.args(["stop", "nabaos"]).status() {
        Ok(s) if s.success() => {
            println!("NabaOS stopped.");
            // Firewall hint
            if let Ok(bind) = std::env::var("NABA_WEB_BIND") {
                if bind.starts_with("0.0.0.0") {
                    let port = bind.split(':').last().unwrap_or("8919");
                    if std::process::Command::new("ufw").arg("status").output()
                        .map(|o| String::from_utf8_lossy(&o.stdout).contains("active"))
                        .unwrap_or(false)
                    {
                        println!("Hint: sudo ufw delete allow {}/tcp", port);
                    }
                }
            }
            Ok(())
        }
        _ => {
            // Fallback: PID file
            let pid_path = config.data_dir.join("nabaos.pid");
            if pid_path.exists() {
                let pid = std::fs::read_to_string(&pid_path)?;
                let pid: i32 = pid.trim().parse()?;
                unsafe { libc::kill(pid, libc::SIGTERM); }
                println!("Sent SIGTERM to PID {}", pid);
                std::fs::remove_file(&pid_path).ok();
                Ok(())
            } else {
                anyhow::bail!("NabaOS is not running (no systemd service or PID file found)")
            }
        }
    }
}
```

**Step 6: Write PID file in cmd_daemon for non-systemd fallback**

At the start of `cmd_daemon()`, after creating data_dir:

```rust
// Write PID file for non-systemd stop
let pid_path = config.data_dir.join("nabaos.pid");
std::fs::write(&pid_path, std::process::id().to_string())?;
```

At the end (after the loop exits):

```rust
std::fs::remove_file(&pid_path).ok();
```

**Step 7: Run cargo check and tests**

Run: `cargo check && cargo test --lib`
Expected: zero errors, all tests pass

**Step 8: Commit**

```bash
git add Cargo.toml src/main.rs
git commit -m "feat: systemd daemon mode with nabaos start/stop/preflight checks"
```

---

## Task 8: Final Integration + Version Bump

**Files:**
- Modify: `Cargo.toml:3` (version)

**Step 1: Bump version**

Change `version = "0.2.2"` to `version = "0.2.3"` in `Cargo.toml`.

**Step 2: Build web frontend**

```bash
cd nabaos-web && npm run build
```

**Step 3: Full build + test**

```bash
cargo build --release 2>&1
cargo test --lib 2>&1
```

Expected: zero errors, all tests pass.

**Step 4: Verify embedded SPA**

```bash
ls -la target/release/nabaos  # Should be larger now (SPA embedded)
```

**Step 5: Commit + tag + push**

```bash
git add -A
git commit -m "chore: bump to v0.2.3"
git tag -a v0.2.3 -m "v0.2.3: daemon mode, embedded web, TUI redesign, watcher wizard"
git push origin main --tags
```

**Step 6: Build release + upload**

```bash
mkdir -p /tmp/nabaos-release
cp target/release/nabaos /tmp/nabaos-release/
cd /tmp/nabaos-release
tar czf nabaos-linux-amd64.tar.gz nabaos
sha256sum nabaos-linux-amd64.tar.gz > SHA256SUMS

gh release create v0.2.3 \
  nabaos-linux-amd64.tar.gz SHA256SUMS \
  --title "v0.2.3" \
  --notes "## Changes
- Systemd daemon mode: \`nabaos start\` installs service, \`nabaos stop\` stops it
- Pre-flight config validation on start
- Graceful shutdown on Ctrl+C (web server + telegram)
- Firewall hints for public web access (ufw advisory)
- Web SPA embedded in binary (no external files needed)
- Telegram chat IDs in setup wizard
- BERT model downloadable from HuggingFace
- TUI dashboard redesigned with wizard color palette
- Log ring buffer: no more stdout spam in TUI mode
- Watcher config step in setup wizard
- BERT 'model not found' demoted to debug level"
```
