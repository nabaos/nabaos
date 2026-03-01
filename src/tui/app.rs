//! Main TUI application — full-screen dashboard with tabbed interface.
//!
//! Features:
//! - Background query processing (non-blocking UI)
//! - Animated loading spinner
//! - Live stats in title bar
//! - Help overlay
//! - Agents loaded from catalog, objectives from PEA engine

use std::collections::VecDeque;
use std::io;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Tabs};
use ratatui::Terminal;

use crate::core::config::NyayaConfig;
use crate::core::error::Result;
use crate::core::orchestrator::Orchestrator;
use super::tabs::agents::AgentsTab;
use super::tabs::chat::ChatTab;
use super::tabs::history::{HistoryEntry, HistoryTab};
use super::tabs::settings::{ConfigEntry, SettingsTab};
use super::tabs::tasks::{ObjectiveSummary, TasksTab};
use super::tabs::{Tab, TabId};

// ── Wizard-matching color palette ───────────────────────────────────────────
const BG: Color = Color::Rgb(22, 22, 30);
const FG: Color = Color::Rgb(200, 200, 210);
const ACCENT: Color = Color::Rgb(255, 175, 95);
const GREEN: Color = Color::Rgb(130, 200, 130);
const DIM: Color = Color::Rgb(100, 100, 120);
const HEADING: Color = Color::Rgb(170, 170, 190);
const BORDER: Color = Color::Rgb(60, 60, 80);
const HIGHLIGHT_BG: Color = Color::Rgb(35, 35, 50);

/// Messages from background processing threads.
enum AppMessage {
    QueryResult {
        text: String,
        cost_label: String,
        tier: String,
        latency_ms: f64,
        cost: f64,
        query: String,
    },
    QueryError(String),
}

/// The main TUI application state.
pub struct App {
    pub active_tab: TabId,
    pub chat: ChatTab,
    pub tasks: TasksTab,
    pub agents: AgentsTab,
    pub settings: SettingsTab,
    pub history: HistoryTab,
    pub should_quit: bool,
    pub show_help: bool,
    pub stats_queries: u64,
    pub stats_saved: f64,
    pub stats_spent: f64,
    pub stats_cache_pct: f64,
    pub show_logs: bool,
    log_buffer: Arc<Mutex<VecDeque<String>>>,
    start_time: Instant,
    config: NyayaConfig,
    rx: mpsc::Receiver<AppMessage>,
    tx: mpsc::Sender<AppMessage>,
}

impl App {
    pub fn new(config: NyayaConfig, log_buffer: Arc<Mutex<VecDeque<String>>>) -> Self {
        let (tx, rx) = mpsc::channel();

        let mut settings = SettingsTab::new();

        // Populate settings from config
        let mut entries = vec![
            ConfigEntry {
                key: "LLM Provider".into(),
                value: config
                    .llm_provider
                    .as_deref()
                    .unwrap_or("not set")
                    .to_string(),
            },
            ConfigEntry {
                key: "LLM Model".into(),
                value: config
                    .llm_model
                    .as_deref()
                    .unwrap_or("default")
                    .to_string(),
            },
            ConfigEntry {
                key: "Data directory".into(),
                value: config.data_dir.display().to_string(),
            },
        ];
        if let Some(ref path) = config.constitution_path {
            entries.push(ConfigEntry {
                key: "Constitution".into(),
                value: path.display().to_string(),
            });
        }
        entries.push(ConfigEntry {
            key: "Version".into(),
            value: env!("CARGO_PKG_VERSION").to_string(),
        });
        settings.set_entries(entries);

        let mut app = Self {
            active_tab: TabId::Chat,
            chat: ChatTab::new(),
            tasks: TasksTab::new(),
            agents: AgentsTab::new(),
            settings,
            history: HistoryTab::new(),
            should_quit: false,
            show_help: false,
            show_logs: true,
            log_buffer,
            start_time: Instant::now(),
            stats_queries: 0,
            stats_saved: 0.0,
            stats_spent: 0.0,
            stats_cache_pct: 0.0,
            config,
            rx,
            tx,
        };

        // Load initial data
        app.load_agents();
        app.load_objectives();
        app.refresh_stats();

        app
    }

    /// Load agents from the catalog directory.
    fn load_agents(&mut self) {
        use crate::agent_os::catalog::AgentCatalog;

        let catalog_dir = self.config.data_dir.join("catalog");
        let catalog = AgentCatalog::new(&catalog_dir);

        if let Ok(entries) = catalog.list() {
            let agents: Vec<_> = entries
                .into_iter()
                .map(|e| super::tabs::agents::AgentEntry {
                    name: e.name,
                    category: e.category,
                    description: e.description,
                    installed: false,
                })
                .collect();
            self.agents.set_agents(agents);
        }
    }

    /// Load objectives from PEA engine.
    fn load_objectives(&mut self) {
        use crate::pea::engine::PeaEngine;

        if let Ok(engine) = PeaEngine::open(&self.config.data_dir) {
            if let Ok(objectives) = engine.list_objectives() {
                let summaries: Vec<_> = objectives
                    .into_iter()
                    .map(|obj| ObjectiveSummary {
                        id: obj.id,
                        description: obj.description,
                        status: format!("{}", obj.status),
                        spent: obj.spent_usd,
                        budget: obj.budget_usd,
                    })
                    .collect();
                self.tasks.set_objectives(summaries);
            }
        }
    }

    /// Refresh stats from the database.
    fn refresh_stats(&mut self) {
        if let Ok(orch) = Orchestrator::new(self.config.clone()) {
            if let Ok(summary) = orch.cost_summary(None) {
                self.stats_queries = summary.total_llm_calls + summary.total_cache_hits;
                self.stats_saved = summary.total_saved_usd;
                self.stats_spent = summary.total_spent_usd;
                self.stats_cache_pct = summary.savings_percent;
            }
        }
    }

    /// Submit a query for background processing.
    fn submit_query(&mut self, query: String) {
        self.chat.push_user(query.clone());
        self.chat.is_loading = true;

        let tx = self.tx.clone();
        let config = self.config.clone();
        let q = query;

        std::thread::spawn(move || {
            let start = Instant::now();
            match Orchestrator::new(config) {
                Ok(mut orch) => match orch.process_query(&q, None) {
                    Ok(result) => {
                        let text = result
                            .response_text
                            .unwrap_or_else(|| result.description.clone());
                        let tier_str = format!("{}", result.tier);
                        let is_cached = tier_str.contains("Cache")
                            || tier_str.contains("cache")
                            || tier_str.contains("Fingerprint")
                            || tier_str.contains("Bert");
                        let cost = if is_cached { 0.0 } else { result.confidence * 0.001 };
                        let cost_label = if is_cached {
                            "cached · $0.00".to_string()
                        } else {
                            format!("llm · ${:.4}", cost)
                        };
                        let elapsed = start.elapsed().as_millis() as f64;
                        tx.send(AppMessage::QueryResult {
                            text,
                            cost_label,
                            tier: tier_str,
                            latency_ms: if result.latency_ms > 0.0 {
                                result.latency_ms
                            } else {
                                elapsed
                            },
                            cost,
                            query: q,
                        })
                        .ok();
                    }
                    Err(e) => {
                        tx.send(AppMessage::QueryError(e.to_string())).ok();
                    }
                },
                Err(e) => {
                    tx.send(AppMessage::QueryError(e.to_string())).ok();
                }
            }
        });
    }

    /// Poll for background results (non-blocking).
    fn poll_messages(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                AppMessage::QueryResult {
                    text,
                    cost_label,
                    tier,
                    latency_ms,
                    cost,
                    query,
                } => {
                    self.chat.push_agent(text, cost_label);
                    self.history.push(HistoryEntry {
                        timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                        query,
                        tier,
                        cost,
                        latency_ms,
                    });
                    self.refresh_stats();
                    self.load_objectives();
                }
                AppMessage::QueryError(e) => {
                    self.chat
                        .push_agent(format!("Error: {}", e), "error".into());
                }
            }
        }
    }
}

/// Launch the interactive TUI.
pub fn run_tui(config: NyayaConfig) -> Result<()> {
    use tracing_subscriber::layer::SubscriberExt;

    // Set up ring-buffer log capture + file appender BEFORE entering the TUI
    let (layer, log_buffer) = super::log_layer::RingBufferLayer::new(500);
    let log_dir = config.data_dir.join("logs");
    std::fs::create_dir_all(&log_dir).ok();
    let file_appender = tracing_appender::rolling::daily(&log_dir, "nabaos.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    let subscriber = tracing_subscriber::registry()
        .with(layer)
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(non_blocking)
                .with_target(false),
        );
    // set_global_default may fail if already set — that's OK in TUI mode
    let _ = tracing::subscriber::set_global_default(subscriber);

    // Setup terminal
    enable_raw_mode().map_err(|e| crate::core::error::NyayaError::Config(e.to_string()))?;
    io::stdout()
        .execute(EnterAlternateScreen)
        .map_err(|e| crate::core::error::NyayaError::Config(e.to_string()))?;

    let backend = ratatui::backend::CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)
        .map_err(|e| crate::core::error::NyayaError::Config(e.to_string()))?;

    let mut app = App::new(config, log_buffer);
    let mut last_refresh = Instant::now();
    let refresh_interval = Duration::from_secs(3);

    // Main loop
    let result = loop {
        // Tick animations
        app.chat.tick();

        // Poll background results
        app.poll_messages();

        // Draw
        terminal
            .draw(|frame| draw_ui(frame, &app))
            .map_err(|e| crate::core::error::NyayaError::Config(e.to_string()))?;

        // Poll events (100ms for smooth spinner animation)
        if event::poll(Duration::from_millis(100))
            .map_err(|e| crate::core::error::NyayaError::Config(e.to_string()))?
        {
            if let Event::Key(key) = event::read()
                .map_err(|e| crate::core::error::NyayaError::Config(e.to_string()))?
            {
                // Help overlay intercepts all keys
                if app.show_help {
                    app.show_help = false;
                    // Don't process the key further
                } else {
                    // Global keys
                    match key.code {
                        KeyCode::Char('c')
                            if key.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            app.should_quit = true;
                        }
                        KeyCode::Char('q')
                            if app.active_tab != TabId::Chat
                                && app.active_tab != TabId::Agents =>
                        {
                            app.should_quit = true;
                        }
                        KeyCode::Char('?')
                            if app.active_tab != TabId::Chat
                                && app.active_tab != TabId::Agents =>
                        {
                            app.show_help = !app.show_help;
                        }
                        KeyCode::Char('l') | KeyCode::Char('L')
                            if app.active_tab != TabId::Chat
                                && app.active_tab != TabId::Agents =>
                        {
                            app.show_logs = !app.show_logs;
                        }
                        KeyCode::Tab => {
                            app.active_tab = app.active_tab.next();
                        }
                        KeyCode::BackTab => {
                            app.active_tab = app.active_tab.prev();
                        }
                        KeyCode::Char(n @ '1'..='5')
                            if key.modifiers.is_empty()
                                && app.active_tab != TabId::Chat
                                && app.active_tab != TabId::Agents =>
                        {
                            app.active_tab =
                                TabId::from_index((n as usize) - ('1' as usize));
                        }
                        KeyCode::Enter if app.active_tab == TabId::Chat => {
                            let input = app.chat.take_input();
                            if !input.is_empty() {
                                app.submit_query(input);
                            }
                        }
                        _ => {
                            // Delegate to active tab
                            match app.active_tab {
                                TabId::Chat => {
                                    app.chat.handle_key(key);
                                }
                                TabId::Tasks => {
                                    app.tasks.handle_key(key);
                                }
                                TabId::Agents => {
                                    app.agents.handle_key(key);
                                }
                                TabId::Settings => {
                                    app.settings.handle_key(key);
                                }
                                TabId::History => {
                                    app.history.handle_key(key);
                                }
                            }
                        }
                    }
                }
            }
        }

        if app.should_quit {
            break Ok(());
        }

        // Periodic refresh
        if last_refresh.elapsed() >= refresh_interval {
            app.refresh_stats();
            last_refresh = Instant::now();
        }
    };

    // Restore terminal
    disable_raw_mode().ok();
    io::stdout().execute(LeaveAlternateScreen).ok();

    result
}

/// Draw the full TUI layout.
fn draw_ui(frame: &mut ratatui::Frame, app: &App) {
    let size = frame.area();

    // Fill entire background
    frame.render_widget(Block::default().style(Style::default().bg(BG)), size);

    // Build live stats for title bar
    let stats_text = format!(
        " saved {} · cache {:.0}% · {} queries ",
        format_money(app.stats_saved),
        app.stats_cache_pct,
        app.stats_queries,
    );

    // Outer block with stats in bottom border
    let outer = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(BG))
        .title(Line::from(vec![Span::styled(
            format!(" NabaOS v{} ", env!("CARGO_PKG_VERSION")),
            Style::default()
                .fg(ACCENT)
                .bg(BG)
                .add_modifier(Modifier::BOLD),
        )]))
        .title_bottom(Line::from(vec![Span::styled(
            stats_text,
            Style::default().fg(GREEN).bg(BG),
        )]));

    let inner = outer.inner(size);
    frame.render_widget(outer, size);

    // Layout: tab bar + content + logs (optional) + status bar
    let log_height = if app.show_logs { 8 } else { 0 };
    let chunks = Layout::vertical([
        Constraint::Length(2),          // tab bar
        Constraint::Min(5),             // content
        Constraint::Length(log_height), // logs panel
        Constraint::Length(1),          // status bar
    ])
    .split(inner);

    // Tab bar
    draw_tab_bar(frame, chunks[0], app);

    // Content area — active tab
    match app.active_tab {
        TabId::Chat => app.chat.render(frame, chunks[1]),
        TabId::Tasks => app.tasks.render(frame, chunks[1]),
        TabId::Agents => app.agents.render(frame, chunks[1]),
        TabId::Settings => app.settings.render(frame, chunks[1]),
        TabId::History => app.history.render(frame, chunks[1]),
    }

    // Logs panel
    if app.show_logs {
        if let Ok(logs) = app.log_buffer.lock() {
            let log_lines: Vec<Line> = logs
                .iter()
                .rev()
                .take(7)
                .rev()
                .map(|l| Line::from(Span::styled(l.as_str(), Style::default().fg(DIM).bg(BG))))
                .collect();
            let log_block = Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(BORDER))
                .title(Span::styled(
                    " Logs (L to toggle) ",
                    Style::default().fg(HEADING).bg(BG),
                ));
            frame.render_widget(
                Paragraph::new(log_lines)
                    .block(log_block)
                    .style(Style::default().bg(BG)),
                chunks[2],
            );
        }
    }

    // Status bar
    let uptime = app.start_time.elapsed();
    let uptime_str = if uptime.as_secs() >= 3600 {
        format!(
            "{}h {}m",
            uptime.as_secs() / 3600,
            (uptime.as_secs() % 3600) / 60
        )
    } else {
        format!("{}m {}s", uptime.as_secs() / 60, uptime.as_secs() % 60)
    };
    let status = Line::from(vec![
        Span::styled(
            format!(" Up: {} ", uptime_str),
            Style::default().fg(DIM).bg(BG),
        ),
        Span::styled("\u{2502} ", Style::default().fg(BORDER).bg(BG)),
        Span::styled(
            "?: help  L: logs  Ctrl+C: quit ",
            Style::default().fg(DIM).bg(BG),
        ),
    ]);
    frame.render_widget(
        Paragraph::new(vec![status]).style(Style::default().bg(BG)),
        chunks[3],
    );

    // Help overlay
    if app.show_help {
        draw_help_overlay(frame, size);
    }
}

/// Draw the top tab bar using ratatui Tabs widget.
fn draw_tab_bar(frame: &mut ratatui::Frame, area: ratatui::layout::Rect, app: &App) {
    let titles: Vec<Line> = TabId::all()
        .iter()
        .enumerate()
        .map(|(i, tab)| {
            let label = match tab {
                TabId::Agents if !app.agents.agents.is_empty() => {
                    format!("{} ({})", tab.label(), app.agents.agents.len())
                }
                TabId::Tasks if !app.tasks.objectives.is_empty() => {
                    format!("{} ({})", tab.label(), app.tasks.objectives.len())
                }
                TabId::History if !app.history.entries.is_empty() => {
                    format!("{} ({})", tab.label(), app.history.entries.len())
                }
                _ => tab.label().to_string(),
            };
            Line::from(format!(" {} {} ", i + 1, label))
        })
        .collect();

    let tabs = Tabs::new(titles)
        .select(app.active_tab.index())
        .style(Style::default().fg(DIM).bg(BG))
        .highlight_style(
            Style::default()
                .fg(ACCENT)
                .bg(HIGHLIGHT_BG)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )
        .divider("\u{2502}");

    frame.render_widget(tabs, area);
}

/// Draw the help overlay centered on screen.
fn draw_help_overlay(frame: &mut ratatui::Frame, area: ratatui::layout::Rect) {
    let w = 50.min(area.width.saturating_sub(4));
    let h = 17.min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(w)) / 2 + area.x;
    let y = (area.height.saturating_sub(h)) / 2 + area.y;
    let help_area = ratatui::layout::Rect::new(x, y, w, h);

    frame.render_widget(Clear, help_area);

    let help_lines = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            "  Keyboard Shortcuts",
            Style::default()
                .fg(ACCENT)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        help_row("Tab / Shift+Tab", "Switch tabs"),
        help_row("1-5", "Jump to tab"),
        help_row("↑ ↓ / j k", "Navigate lists"),
        help_row("Enter", "Send query (Chat)"),
        help_row("PgUp / PgDn", "Scroll messages"),
        help_row("L", "Toggle logs"),
        help_row("q", "Quit"),
        help_row("Ctrl+C", "Force quit"),
        help_row("?", "Toggle this help"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "  Press any key to close",
            Style::default().fg(DIM),
        )]),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT).bg(BG))
        .style(Style::default().bg(BG))
        .title(Line::from(vec![Span::styled(
            " Help ",
            Style::default()
                .fg(ACCENT)
                .bg(BG)
                .add_modifier(Modifier::BOLD),
        )]));

    frame.render_widget(
        Paragraph::new(help_lines).block(block).style(Style::default().bg(BG)),
        help_area,
    );
}

fn help_row<'a>(key: &'a str, desc: &'a str) -> Line<'a> {
    Line::from(vec![
        Span::styled(
            format!("  {:<19}", key),
            Style::default()
                .fg(FG)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(desc, Style::default().fg(DIM)),
    ])
}

fn format_money(usd: f64) -> String {
    if usd < 0.01 && usd > 0.0 {
        format!("${:.4}", usd)
    } else {
        format!("${:.2}", usd)
    }
}
