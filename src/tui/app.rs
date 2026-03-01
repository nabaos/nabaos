//! Main TUI application — full-screen dashboard with tabs.

use std::io;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Terminal;

use crate::core::config::NyayaConfig;
use crate::core::error::Result;
use crate::core::orchestrator::Orchestrator;
use super::tabs::agents::AgentsTab;
use super::tabs::chat::ChatTab;
use super::tabs::history::{HistoryEntry, HistoryTab};
use super::tabs::settings::{ConfigEntry, SettingsTab};
use super::tabs::tasks::TasksTab;
use super::tabs::{Tab, TabId};

/// The main TUI application state.
pub struct App {
    pub active_tab: TabId,
    pub chat: ChatTab,
    pub tasks: TasksTab,
    pub agents: AgentsTab,
    pub settings: SettingsTab,
    pub history: HistoryTab,
    pub should_quit: bool,
    pub stats_queries: u64,
    pub stats_saved: f64,
    pub stats_cache_pct: f64,
    config: NyayaConfig,
}

impl App {
    pub fn new(config: NyayaConfig) -> Self {
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

        Self {
            active_tab: TabId::Chat,
            chat: ChatTab::new(),
            tasks: TasksTab::new(),
            agents: AgentsTab::new(),
            settings,
            history: HistoryTab::new(),
            should_quit: false,
            stats_queries: 0,
            stats_saved: 0.0,
            stats_cache_pct: 0.0,
            config,
        }
    }

    /// Refresh stats from the database.
    fn refresh_stats(&mut self) {
        if let Ok(orch) = Orchestrator::new(self.config.clone()) {
            if let Ok(summary) = orch.cost_summary(None) {
                self.stats_queries = summary.total_llm_calls + summary.total_cache_hits;
                self.stats_saved = summary.total_saved_usd;
                self.stats_cache_pct = summary.savings_percent;
            }
        }
    }

    /// Process a chat query through the orchestrator.
    fn process_query(&mut self, query: String) {
        self.chat.push_user(query.clone());

        match Orchestrator::new(self.config.clone()) {
            Ok(mut orch) => match orch.process_query(&query, None) {
                Ok(result) => {
                    let text = result
                        .response_text
                        .unwrap_or_else(|| result.description.clone());
                    let tier_str = format!("{}", result.tier);
                    let cost_label = if tier_str.contains("cache") || tier_str.contains("Cache") {
                        format!("[cached · $0.00]")
                    } else {
                        format!("[llm · ${:.4}]", 0.003) // approximate
                    };
                    self.chat.push_agent(text, cost_label.clone());

                    // Add to history
                    self.history.push(HistoryEntry {
                        timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                        query,
                        tier: tier_str,
                        cost: result.confidence * 0.001, // rough estimate
                        latency_ms: result.latency_ms,
                    });

                    self.refresh_stats();
                }
                Err(e) => {
                    self.chat
                        .push_agent(format!("Error: {}", e), "[error]".into());
                }
            },
            Err(e) => {
                self.chat
                    .push_agent(format!("Error: {}", e), "[error]".into());
            }
        }
    }
}

/// Launch the interactive TUI.
pub fn run_tui(config: NyayaConfig) -> Result<()> {
    // Setup terminal
    enable_raw_mode().map_err(|e| crate::core::error::NyayaError::Config(e.to_string()))?;
    io::stdout()
        .execute(EnterAlternateScreen)
        .map_err(|e| crate::core::error::NyayaError::Config(e.to_string()))?;

    let backend = ratatui::backend::CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)
        .map_err(|e| crate::core::error::NyayaError::Config(e.to_string()))?;

    let mut app = App::new(config);
    app.refresh_stats();

    let mut last_refresh = Instant::now();
    let refresh_interval = Duration::from_secs(5);

    // Main loop
    let result = loop {
        // Draw
        terminal
            .draw(|frame| draw_ui(frame, &app))
            .map_err(|e| crate::core::error::NyayaError::Config(e.to_string()))?;

        // Poll events
        if event::poll(Duration::from_millis(250))
            .map_err(|e| crate::core::error::NyayaError::Config(e.to_string()))?
        {
            if let Event::Key(key) = event::read()
                .map_err(|e| crate::core::error::NyayaError::Config(e.to_string()))?
            {
                // Global keys
                match key.code {
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.should_quit = true;
                    }
                    KeyCode::Char('q')
                        if app.active_tab != TabId::Chat && app.active_tab != TabId::Agents =>
                    {
                        app.should_quit = true;
                    }
                    KeyCode::Tab => {
                        app.active_tab = app.active_tab.next();
                    }
                    KeyCode::Char('1') if key.modifiers.is_empty() && app.active_tab != TabId::Chat && app.active_tab != TabId::Agents => {
                        app.active_tab = TabId::Chat;
                    }
                    KeyCode::Char('2') if key.modifiers.is_empty() && app.active_tab != TabId::Chat && app.active_tab != TabId::Agents => {
                        app.active_tab = TabId::Tasks;
                    }
                    KeyCode::Char('3') if key.modifiers.is_empty() && app.active_tab != TabId::Chat && app.active_tab != TabId::Agents => {
                        app.active_tab = TabId::Agents;
                    }
                    KeyCode::Char('4') if key.modifiers.is_empty() && app.active_tab != TabId::Chat && app.active_tab != TabId::Agents => {
                        app.active_tab = TabId::Settings;
                    }
                    KeyCode::Char('5') if key.modifiers.is_empty() && app.active_tab != TabId::Chat && app.active_tab != TabId::Agents => {
                        app.active_tab = TabId::History;
                    }
                    KeyCode::Enter if app.active_tab == TabId::Chat => {
                        let input = app.chat.take_input();
                        if !input.is_empty() {
                            app.process_query(input);
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

    // Outer border
    let outer = Block::default()
        .borders(Borders::ALL)
        .title(format!(" NabaOS v{} ", env!("CARGO_PKG_VERSION")))
        .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));
    let inner = outer.inner(size);
    frame.render_widget(outer, size);

    // Main layout: sidebar + content + status bar
    let main_chunks = Layout::vertical([
        Constraint::Min(10),   // nav + content
        Constraint::Length(1), // status bar
    ])
    .split(inner);

    // Sidebar + content
    let content_chunks = Layout::horizontal([
        Constraint::Length(16), // sidebar
        Constraint::Min(30),   // content
    ])
    .split(main_chunks[0]);

    // Sidebar: nav + stats
    let sidebar_chunks = Layout::vertical([
        Constraint::Min(8),    // nav tabs
        Constraint::Length(6), // stats
    ])
    .split(content_chunks[0]);

    draw_nav(frame, sidebar_chunks[0], app);
    draw_stats(frame, sidebar_chunks[1], app);

    // Content area: active tab
    let content_area = content_chunks[1];
    match app.active_tab {
        TabId::Chat => app.chat.render(frame, content_area),
        TabId::Tasks => app.tasks.render(frame, content_area),
        TabId::Agents => app.agents.render(frame, content_area),
        TabId::Settings => app.settings.render(frame, content_area),
        TabId::History => app.history.render(frame, content_area),
    }

    // Status bar
    let status = Paragraph::new(Line::from(vec![
        Span::styled(" Tab", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(": switch  "),
        Span::styled("Enter", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(": send  "),
        Span::styled("Ctrl+C", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(": quit"),
    ]))
    .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(status, main_chunks[1]);
}

/// Draw the navigation sidebar.
fn draw_nav(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let block = Block::default().borders(Borders::ALL).title(" Nav ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let tabs = TabId::all();
    for (i, tab_id) in tabs.iter().enumerate() {
        if i as u16 >= inner.height {
            break;
        }
        let is_active = *tab_id == app.active_tab;
        let (symbol, style) = if is_active {
            (
                "\u{25cf} ", // ●
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            (
                "\u{25cb} ", // ○
                Style::default().fg(Color::White),
            )
        };
        let line = Paragraph::new(format!("{}{}", symbol, tab_id.label())).style(style);
        let tab_area = Rect {
            x: inner.x,
            y: inner.y + i as u16,
            width: inner.width,
            height: 1,
        };
        frame.render_widget(line, tab_area);
    }
}

/// Draw the stats section in the sidebar.
fn draw_stats(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let block = Block::default().borders(Borders::ALL).title(" Stats ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines = vec![
        format!("Saved ${:.2}", app.stats_saved),
        format!("Cache {:.0}%", app.stats_cache_pct),
        format!("Queries {}", app.stats_queries),
    ];

    for (i, line) in lines.iter().enumerate() {
        if i as u16 >= inner.height {
            break;
        }
        let para = Paragraph::new(line.as_str()).style(Style::default().fg(Color::DarkGray));
        let line_area = Rect {
            x: inner.x,
            y: inner.y + i as u16,
            width: inner.width,
            height: 1,
        };
        frame.render_widget(para, line_area);
    }
}
