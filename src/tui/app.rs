//! Main TUI application — split-pane dashboard with tabbed interface.
//!
//! Features:
//! - Split-pane layout (60% main content / 40% context panel)
//! - Persistent orchestrator (Arc<Mutex<Orchestrator>>)
//! - Conversation history loaded from MemoryStore on startup
//! - Context-aware status bar with per-tab hints
//! - 7 tabs: Chat, Agents, Workflows, Resources, PEA, Settings, History
//! - Background query processing (non-blocking UI)
//! - Animated loading spinner with elapsed timer

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
use super::tabs::agents::{AgentAction, AgentEntry, AgentsTab, DisplayState};
use super::tabs::chat::ChatTab;
use super::tabs::history::{HistoryEntry, HistoryTab};
use super::tabs::resources::ResourcesTab;
use super::tabs::settings::{ConfigEntry, SettingsTab};
use super::tabs::tasks::{ObjectiveSummary, TasksTab};
use super::tabs::workflows::WorkflowsTab;
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

/// What the right context panel displays.
#[derive(Clone)]
enum ContextPanel {
    Welcome,
    AgentDetail(usize),
    ObjectiveDetail(usize),
    WorkflowDetail(usize),
    ResourceDetail(usize),
    HistoryDetail(usize),
}

/// The main TUI application state.
pub struct App {
    pub active_tab: TabId,
    pub chat: ChatTab,
    pub tasks: TasksTab,
    pub agents: AgentsTab,
    pub settings: SettingsTab,
    pub history: HistoryTab,
    pub workflows: WorkflowsTab,
    pub resources: ResourcesTab,
    pub should_quit: bool,
    pub show_help: bool,
    pub stats_queries: u64,
    pub stats_saved: f64,
    pub stats_spent: f64,
    pub stats_cache_pct: f64,
    pub show_logs: bool,
    context_panel: ContextPanel,
    log_buffer: Arc<Mutex<VecDeque<String>>>,
    start_time: Instant,
    config: NyayaConfig,
    orchestrator: Option<Arc<Mutex<Orchestrator>>>,
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

        // Create persistent orchestrator
        let orchestrator = match Orchestrator::new(config.clone()) {
            Ok(orch) => Some(Arc::new(Mutex::new(orch))),
            Err(e) => {
                tracing::warn!("Failed to create orchestrator: {}", e);
                None
            }
        };

        let mut app = Self {
            active_tab: TabId::Chat,
            chat: ChatTab::new(),
            tasks: TasksTab::new(),
            agents: AgentsTab::new(),
            settings,
            history: HistoryTab::new(),
            workflows: WorkflowsTab::new(),
            resources: ResourcesTab::new(),
            should_quit: false,
            show_help: false,
            show_logs: true,
            context_panel: ContextPanel::Welcome,
            log_buffer,
            start_time: Instant::now(),
            stats_queries: 0,
            stats_saved: 0.0,
            stats_spent: 0.0,
            stats_cache_pct: 0.0,
            config,
            orchestrator,
            rx,
            tx,
        };

        // Load initial data
        app.load_conversation_history();
        app.load_agents();
        app.load_objectives();
        app.load_workflows();
        app.load_resources();
        app.refresh_stats();

        app
    }

    /// Load conversation history from MemoryStore.
    fn load_conversation_history(&mut self) {
        use crate::memory::memory_store::{MemoryStore, TurnRole};

        let db_path = self.config.data_dir.join("memory.db");
        if let Ok(store) = MemoryStore::open(&db_path) {
            if let Ok(turns) = store.recent_turns("default", 50) {
                // Remove the initial welcome message if we have history
                if !turns.is_empty() {
                    self.chat.messages.clear();
                }
                for turn in turns {
                    match turn.role {
                        TurnRole::User => {
                            self.chat.push_user_silent(turn.content, turn.created_at);
                        }
                        TurnRole::Assistant => {
                            self.chat.push_agent_silent(turn.content, turn.created_at);
                        }
                        TurnRole::System => {}
                    }
                }
            }
        }
    }

    /// Load agents from catalog, merging installed state from AgentStore.
    fn load_agents(&mut self) {
        use crate::agent_os::catalog::AgentCatalog;
        use crate::agent_os::store::AgentStore;

        let catalog_dir = self.config.data_dir.join("catalog");
        let catalog = AgentCatalog::new(&catalog_dir);

        // Load installed agents from store
        let store_db = self.config.data_dir.join("agents.db");
        let agents_dir = self.config.data_dir.join("agents");
        let installed = AgentStore::open(&store_db, &agents_dir)
            .and_then(|s| s.list())
            .unwrap_or_default();

        if let Ok(entries) = catalog.list() {
            let agents: Vec<_> = entries
                .into_iter()
                .map(|e| {
                    // Check if this agent is installed
                    let inst = installed.iter().find(|i| i.id == e.name);
                    let (state, is_installed) = match inst {
                        Some(i) => {
                            let ds = match i.state {
                                crate::agent_os::types::AgentState::Running => DisplayState::Running,
                                crate::agent_os::types::AgentState::Paused => DisplayState::Paused,
                                crate::agent_os::types::AgentState::Stopped => DisplayState::Stopped,
                                crate::agent_os::types::AgentState::Disabled => DisplayState::Disabled,
                            };
                            (ds, true)
                        }
                        None => (DisplayState::NotInstalled, false),
                    };

                    AgentEntry {
                        name: e.name,
                        version: e.version,
                        category: e.category,
                        description: e.description,
                        author: e.author,
                        permissions: e.permissions,
                        state,
                        installed: is_installed,
                    }
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

    /// Load workflow definitions.
    fn load_workflows(&mut self) {
        use crate::chain::workflow_store::WorkflowStore;

        let db_path = self.config.data_dir.join("chains.db");
        if let Ok(store) = WorkflowStore::open(&db_path) {
            if let Ok(defs) = store.list_defs() {
                let summaries: Vec<_> = defs
                    .into_iter()
                    .map(|(id, name)| {
                        let instance_count = store.count_active_instances(&id).unwrap_or(0);
                        super::tabs::workflows::WorkflowSummary {
                            id,
                            name,
                            instance_count,
                            last_status: if instance_count > 0 {
                                "running".to_string()
                            } else {
                                "idle".to_string()
                            },
                        }
                    })
                    .collect();
                self.workflows.set_workflows(summaries);
            }
        }
    }

    /// Load registered resources.
    fn load_resources(&mut self) {
        use crate::resource::registry::ResourceRegistry;

        let db_path = self.config.data_dir.join("resources.db");
        if let Ok(registry) = ResourceRegistry::open(&db_path) {
            let active_leases = registry.list_active_leases().unwrap_or_default();
            if let Ok(records) = registry.list_resources() {
                let summaries: Vec<_> = records
                    .into_iter()
                    .map(|r| {
                        let leases_for = active_leases
                            .iter()
                            .filter(|l| l.resource_id == r.id)
                            .count();
                        super::tabs::resources::ResourceSummary {
                            id: r.id,
                            name: r.name,
                            resource_type: format!("{}", r.resource_type),
                            status: format!("{}", r.status),
                            active_leases: leases_for,
                        }
                    })
                    .collect();
                self.resources.set_resources(summaries);
            }
        }
    }

    /// Refresh stats from the orchestrator.
    fn refresh_stats(&mut self) {
        if let Some(ref orch) = self.orchestrator {
            if let Ok(orch) = orch.lock() {
                if let Ok(summary) = orch.cost_summary(None) {
                    self.stats_queries = summary.total_llm_calls + summary.total_cache_hits;
                    self.stats_saved = summary.total_saved_usd;
                    self.stats_spent = summary.total_spent_usd;
                    self.stats_cache_pct = summary.savings_percent;
                }
            }
        } else {
            // Fallback: create a temporary orchestrator for stats
            if let Ok(orch) = Orchestrator::new(self.config.clone()) {
                if let Ok(summary) = orch.cost_summary(None) {
                    self.stats_queries = summary.total_llm_calls + summary.total_cache_hits;
                    self.stats_saved = summary.total_saved_usd;
                    self.stats_spent = summary.total_spent_usd;
                    self.stats_cache_pct = summary.savings_percent;
                }
            }
        }
    }

    /// Submit a query for background processing.
    fn submit_query(&mut self, query: String) {
        self.chat.push_user(query.clone());
        self.chat.set_loading(true);

        let tx = self.tx.clone();
        let config = self.config.clone();
        let orch_arc = self.orchestrator.clone();
        let q = query;

        std::thread::spawn(move || {
            let start = Instant::now();

            // Try using the shared orchestrator, fall back to creating a new one
            let result = if let Some(ref orch_arc) = orch_arc {
                if let Ok(mut orch) = orch_arc.lock() {
                    Some(orch.process_query(&q, None))
                } else {
                    None
                }
            } else {
                None
            };

            let result = match result {
                Some(r) => r,
                None => {
                    // Fallback: create new orchestrator
                    match Orchestrator::new(config) {
                        Ok(mut orch) => orch.process_query(&q, None),
                        Err(e) => {
                            tx.send(AppMessage::QueryError(e.to_string())).ok();
                            return;
                        }
                    }
                }
            };

            match result {
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

    /// Process pending agent actions from the agents tab.
    fn process_agent_actions(&mut self) {
        if let Some(action) = self.agents.take_action() {
            match action {
                AgentAction::Install(name) => {
                    self.do_agent_install(&name);
                }
                AgentAction::Uninstall(name) => {
                    self.do_agent_uninstall(&name);
                }
                AgentAction::Start(name) => {
                    self.do_agent_start(&name);
                }
                AgentAction::Stop(name) => {
                    self.do_agent_stop(&name);
                }
            }
        }
    }

    fn do_agent_install(&mut self, name: &str) {
        use crate::agent_os::store::AgentStore;

        let store_db = self.config.data_dir.join("agents.db");
        let agents_dir = self.config.data_dir.join("agents");
        let catalog_dir = self.config.data_dir.join("catalog");
        let agent_src = catalog_dir.join(name);

        match AgentStore::open(&store_db, &agents_dir) {
            Ok(store) => match store.install_from_dir(&agent_src) {
                Ok(_installed) => {
                    self.agents
                        .show_status(format!("Installed {}", name), false);
                    self.load_agents();
                }
                Err(e) => {
                    self.agents
                        .show_status(format!("Install failed: {}", e), true);
                }
            },
            Err(e) => {
                self.agents
                    .show_status(format!("Store error: {}", e), true);
            }
        }
    }

    fn do_agent_uninstall(&mut self, name: &str) {
        use crate::agent_os::store::AgentStore;

        let store_db = self.config.data_dir.join("agents.db");
        let agents_dir = self.config.data_dir.join("agents");

        match AgentStore::open(&store_db, &agents_dir) {
            Ok(store) => match store.uninstall(name) {
                Ok(()) => {
                    self.agents
                        .show_status(format!("Uninstalled {}", name), false);
                    self.load_agents();
                }
                Err(e) => {
                    self.agents
                        .show_status(format!("Uninstall failed: {}", e), true);
                }
            },
            Err(e) => {
                self.agents
                    .show_status(format!("Store error: {}", e), true);
            }
        }
    }

    fn do_agent_start(&mut self, name: &str) {
        use crate::agent_os::store::AgentStore;
        use crate::agent_os::types::AgentState;

        let store_db = self.config.data_dir.join("agents.db");
        let agents_dir = self.config.data_dir.join("agents");

        match AgentStore::open(&store_db, &agents_dir) {
            Ok(store) => match store.set_state(name, AgentState::Running) {
                Ok(()) => {
                    self.agents
                        .show_status(format!("Started {}", name), false);
                    self.load_agents();
                }
                Err(e) => {
                    self.agents
                        .show_status(format!("Start failed: {}", e), true);
                }
            },
            Err(e) => {
                self.agents
                    .show_status(format!("Store error: {}", e), true);
            }
        }
    }

    fn do_agent_stop(&mut self, name: &str) {
        use crate::agent_os::store::AgentStore;
        use crate::agent_os::types::AgentState;

        let store_db = self.config.data_dir.join("agents.db");
        let agents_dir = self.config.data_dir.join("agents");

        match AgentStore::open(&store_db, &agents_dir) {
            Ok(store) => match store.set_state(name, AgentState::Stopped) {
                Ok(()) => {
                    self.agents
                        .show_status(format!("Stopped {}", name), false);
                    self.load_agents();
                }
                Err(e) => {
                    self.agents
                        .show_status(format!("Stop failed: {}", e), true);
                }
            },
            Err(e) => {
                self.agents
                    .show_status(format!("Store error: {}", e), true);
            }
        }
    }

    /// Update context panel based on active tab and selection.
    fn update_context_panel(&mut self) {
        self.context_panel = match self.active_tab {
            TabId::Agents => {
                if let Some(i) = self.agents.state.selected() {
                    ContextPanel::AgentDetail(i)
                } else {
                    ContextPanel::Welcome
                }
            }
            TabId::Pea => {
                if let Some(i) = self.tasks.state.selected() {
                    ContextPanel::ObjectiveDetail(i)
                } else {
                    ContextPanel::Welcome
                }
            }
            TabId::Workflows => {
                if let Some(i) = self.workflows.state.selected() {
                    ContextPanel::WorkflowDetail(i)
                } else {
                    ContextPanel::Welcome
                }
            }
            TabId::Resources => {
                if let Some(i) = self.resources.state.selected() {
                    ContextPanel::ResourceDetail(i)
                } else {
                    ContextPanel::Welcome
                }
            }
            TabId::History => {
                if let Some(i) = self.history.state.selected() {
                    ContextPanel::HistoryDetail(i)
                } else {
                    ContextPanel::Welcome
                }
            }
            _ => ContextPanel::Welcome,
        };
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

        // Process agent actions
        app.process_agent_actions();

        // Update context panel
        app.update_context_panel();

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
                                && app.active_tab != TabId::Agents
                                && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            app.show_logs = !app.show_logs;
                        }
                        KeyCode::Tab => {
                            app.active_tab = app.active_tab.next();
                        }
                        KeyCode::BackTab => {
                            app.active_tab = app.active_tab.prev();
                        }
                        KeyCode::Char(n @ '1'..='7')
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
                                TabId::Pea => {
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
                                TabId::Workflows => {
                                    app.workflows.handle_key(key);
                                }
                                TabId::Resources => {
                                    app.resources.handle_key(key);
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
        Constraint::Min(5),             // content (split-pane)
        Constraint::Length(log_height), // logs panel
        Constraint::Length(1),          // status bar
    ])
    .split(inner);

    // Tab bar
    draw_tab_bar(frame, chunks[0], app);

    // Content area — split pane (60% main / 40% context)
    let terminal_width = size.width;
    if terminal_width >= 100 {
        // Wide enough for split pane
        let content_chunks = Layout::horizontal([
            Constraint::Percentage(60),
            Constraint::Percentage(40),
        ])
        .split(chunks[1]);

        // Left: active tab content
        render_active_tab(frame, content_chunks[0], app);

        // Right: context panel
        draw_context_panel(frame, content_chunks[1], app);
    } else {
        // Narrow terminal: full-width content, no context panel
        render_active_tab(frame, chunks[1], app);
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

    // Status bar — context-aware hints
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
            app.active_tab.hints(),
            Style::default().fg(DIM).bg(BG),
        ),
        Span::styled("  ", Style::default().bg(BG)),
        Span::styled("\u{2502} ", Style::default().fg(BORDER).bg(BG)),
        Span::styled(
            "[?] help  [L] logs  [Ctrl+C] quit ",
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

/// Render the active tab into the given area.
fn render_active_tab(frame: &mut ratatui::Frame, area: ratatui::layout::Rect, app: &App) {
    match app.active_tab {
        TabId::Chat => app.chat.render(frame, area),
        TabId::Pea => app.tasks.render(frame, area),
        TabId::Agents => app.agents.render(frame, area),
        TabId::Settings => app.settings.render(frame, area),
        TabId::History => app.history.render(frame, area),
        TabId::Workflows => app.workflows.render(frame, area),
        TabId::Resources => app.resources.render(frame, area),
    }
}

/// Draw the right-side context panel.
fn draw_context_panel(frame: &mut ratatui::Frame, area: ratatui::layout::Rect, app: &App) {
    match &app.context_panel {
        ContextPanel::Welcome => draw_context_welcome(frame, area, app),
        ContextPanel::AgentDetail(i) => draw_context_agent(frame, area, app, *i),
        ContextPanel::ObjectiveDetail(i) => draw_context_objective(frame, area, app, *i),
        ContextPanel::WorkflowDetail(i) => draw_context_workflow(frame, area, app, *i),
        ContextPanel::ResourceDetail(i) => draw_context_resource(frame, area, app, *i),
        ContextPanel::HistoryDetail(i) => draw_context_history(frame, area, app, *i),
    }
}

/// Welcome / system info context panel.
fn draw_context_welcome(frame: &mut ratatui::Frame, area: ratatui::layout::Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER))
        .title(Line::from(vec![Span::styled(
            " Context ",
            Style::default().fg(HEADING).bg(BG),
        )]));

    let mut lines = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            "  System Overview",
            Style::default()
                .fg(ACCENT)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        context_row("Version", &format!("v{}", env!("CARGO_PKG_VERSION"))),
        context_row("Agents", &format!("{}", app.agents.agents.len())),
        context_row("Objectives", &format!("{}", app.tasks.objectives.len())),
        context_row("Workflows", &format!("{}", app.workflows.workflows.len())),
        context_row("Resources", &format!("{}", app.resources.resources.len())),
        Line::from(""),
        Line::from(vec![Span::styled(
            "  Stats",
            Style::default()
                .fg(ACCENT)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        context_row("Queries", &format!("{}", app.stats_queries)),
        context_row("Cache", &format!("{:.0}%", app.stats_cache_pct)),
        context_row("Saved", &format_money(app.stats_saved)),
        context_row("Spent", &format_money(app.stats_spent)),
        Line::from(""),
        Line::from(vec![Span::styled(
            "  Provider",
            Style::default()
                .fg(ACCENT)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        context_row(
            "LLM",
            app.config
                .llm_provider
                .as_deref()
                .unwrap_or("not set"),
        ),
        context_row(
            "Model",
            app.config
                .llm_model
                .as_deref()
                .unwrap_or("default"),
        ),
    ];

    if let Some(ref path) = app.config.constitution_path {
        lines.push(context_row(
            "Constitution",
            &path
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| "custom".to_string()),
        ));
    }

    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .style(Style::default().bg(BG)),
        area,
    );
}

/// Agent detail context panel — full lifecycle info.
fn draw_context_agent(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    app: &App,
    idx: usize,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER))
        .title(Line::from(vec![Span::styled(
            " Agent Detail ",
            Style::default().fg(HEADING).bg(BG),
        )]));

    let filtered = app.agents.filtered();
    if let Some(agent) = filtered.get(idx) {
        let mut lines = vec![
            Line::from(""),
            Line::from(vec![Span::styled(
                format!("  {}", agent.name),
                Style::default()
                    .fg(ACCENT)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            context_row("Version", &agent.version),
            context_row("Author", &agent.author),
            context_row("Category", &agent.category),
            Line::from(vec![
                Span::styled("  Status        ", Style::default().fg(DIM)),
                Span::styled(
                    format!("{} {}", agent.state.symbol(), agent.state.label()),
                    Style::default().fg(agent.state.color()),
                ),
            ]),
        ];

        // Permissions section
        if !agent.permissions.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![Span::styled(
                "  Permissions",
                Style::default()
                    .fg(ACCENT)
                    .add_modifier(Modifier::BOLD),
            )]));
            lines.push(Line::from(""));
            for perm in &agent.permissions {
                let (icon, color) = if agent.installed {
                    ("✓", GREEN)
                } else {
                    ("◦", DIM)
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("   {} ", icon), Style::default().fg(color)),
                    Span::styled(perm.to_string(), Style::default().fg(FG)),
                ]));
            }
        }

        // Description
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            "  Description",
            Style::default()
                .fg(ACCENT)
                .add_modifier(Modifier::BOLD),
        )]));
        lines.push(Line::from(""));
        for line in super::tabs::chat::wrap_text(
            &agent.description,
            area.width.saturating_sub(6) as usize,
        ) {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(line, Style::default().fg(FG)),
            ]));
        }

        // Action hints
        lines.push(Line::from(""));
        if !agent.installed {
            lines.push(Line::from(vec![Span::styled(
                "  [i] Install",
                Style::default().fg(DIM),
            )]));
        } else {
            let action_hint = match agent.state {
                DisplayState::Running => "[s] Stop  [u] Uninstall",
                DisplayState::Stopped | DisplayState::Paused => "[s] Start  [u] Uninstall",
                _ => "[u] Uninstall",
            };
            lines.push(Line::from(vec![Span::styled(
                format!("  {}", action_hint),
                Style::default().fg(DIM),
            )]));
        }

        frame.render_widget(
            Paragraph::new(lines)
                .block(block)
                .style(Style::default().bg(BG)),
            area,
        );
    } else {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(vec![Span::styled(
                    "  No agent selected",
                    Style::default().fg(DIM),
                )]),
            ])
            .block(block)
            .style(Style::default().bg(BG)),
            area,
        );
    }
}

/// Objective detail context panel.
fn draw_context_objective(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    app: &App,
    idx: usize,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER))
        .title(Line::from(vec![Span::styled(
            " Objective Detail ",
            Style::default().fg(HEADING).bg(BG),
        )]));

    if let Some(obj) = app.tasks.objectives.get(idx) {
        let (status_symbol, status_color) = match obj.status.as_str() {
            "active" => ("● Active", Color::Cyan),
            "completed" => ("✓ Completed", Color::Green),
            "failed" => ("✗ Failed", Color::Red),
            "paused" => ("◌ Paused", Color::Yellow),
            _ => ("○ Unknown", Color::DarkGray),
        };

        let frac = if obj.budget > 0.0 {
            (obj.spent / obj.budget).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let bar_w: usize = 15;
        let filled = (frac * bar_w as f64).round() as usize;
        let empty = bar_w.saturating_sub(filled);
        let bar = format!("{}{}", "█".repeat(filled), "░".repeat(empty));

        let lines = vec![
            Line::from(""),
            Line::from(vec![Span::styled(
                format!("  {}", &obj.id[..8.min(obj.id.len())]),
                Style::default()
                    .fg(ACCENT)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  Status      ", Style::default().fg(DIM)),
                Span::styled(status_symbol, Style::default().fg(status_color)),
            ]),
            Line::from(""),
            context_row("Budget", &format!("${:.2}", obj.budget)),
            context_row("Spent", &format!("${:.2}", obj.spent)),
            Line::from(vec![
                Span::styled("  Progress    ", Style::default().fg(DIM)),
                Span::styled(
                    bar,
                    Style::default().fg(if frac > 0.8 { Color::Yellow } else { GREEN }),
                ),
                Span::styled(
                    format!(" {:.0}%", frac * 100.0),
                    Style::default().fg(DIM),
                ),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "  Description",
                Style::default().fg(DIM),
            )]),
            Line::from(""),
        ];

        let mut all_lines = lines;
        for line in super::tabs::chat::wrap_text(&obj.description, area.width.saturating_sub(6) as usize) {
            all_lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(line, Style::default().fg(FG)),
            ]));
        }

        frame.render_widget(
            Paragraph::new(all_lines)
                .block(block)
                .style(Style::default().bg(BG)),
            area,
        );
    } else {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(vec![Span::styled(
                    "  No objective selected",
                    Style::default().fg(DIM),
                )]),
            ])
            .block(block)
            .style(Style::default().bg(BG)),
            area,
        );
    }
}

/// Workflow detail context panel.
fn draw_context_workflow(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    app: &App,
    idx: usize,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER))
        .title(Line::from(vec![Span::styled(
            " Workflow Detail ",
            Style::default().fg(HEADING).bg(BG),
        )]));

    if let Some(wf) = app.workflows.workflows.get(idx) {
        let status_color = match wf.last_status.as_str() {
            "running" => Color::Cyan,
            "completed" => Color::Green,
            "failed" => Color::Red,
            "cancelled" => Color::Yellow,
            _ => Color::DarkGray,
        };

        let lines = vec![
            Line::from(""),
            Line::from(vec![Span::styled(
                format!("  {}", wf.name),
                Style::default()
                    .fg(ACCENT)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            context_row("ID", &wf.id),
            Line::from(vec![
                Span::styled("  Status      ", Style::default().fg(DIM)),
                Span::styled(&wf.last_status, Style::default().fg(status_color)),
            ]),
            context_row("Instances", &format!("{}", wf.instance_count)),
        ];

        frame.render_widget(
            Paragraph::new(lines)
                .block(block)
                .style(Style::default().bg(BG)),
            area,
        );
    } else {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(vec![Span::styled(
                    "  No workflow selected",
                    Style::default().fg(DIM),
                )]),
            ])
            .block(block)
            .style(Style::default().bg(BG)),
            area,
        );
    }
}

/// Resource detail context panel.
fn draw_context_resource(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    app: &App,
    idx: usize,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER))
        .title(Line::from(vec![Span::styled(
            " Resource Detail ",
            Style::default().fg(HEADING).bg(BG),
        )]));

    if let Some(res) = app.resources.resources.get(idx) {
        let status_color = match res.status.as_str() {
            "Available" => Color::Green,
            s if s.starts_with("InUse") => Color::Cyan,
            "Provisioning" => Color::Yellow,
            "Degraded" => Color::Yellow,
            "Offline" => Color::Red,
            "Terminated" => Color::DarkGray,
            _ => Color::DarkGray,
        };

        let lines = vec![
            Line::from(""),
            Line::from(vec![Span::styled(
                format!("  {}", res.name),
                Style::default()
                    .fg(ACCENT)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            context_row("ID", &res.id),
            context_row("Type", &res.resource_type),
            Line::from(vec![
                Span::styled("  Status      ", Style::default().fg(DIM)),
                Span::styled(&res.status, Style::default().fg(status_color)),
            ]),
            context_row(
                "Leases",
                &format!(
                    "{}",
                    if res.active_leases > 0 {
                        format!("{} active", res.active_leases)
                    } else {
                        "none".to_string()
                    }
                ),
            ),
        ];

        frame.render_widget(
            Paragraph::new(lines)
                .block(block)
                .style(Style::default().bg(BG)),
            area,
        );
    } else {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(vec![Span::styled(
                    "  No resource selected",
                    Style::default().fg(DIM),
                )]),
            ])
            .block(block)
            .style(Style::default().bg(BG)),
            area,
        );
    }
}

/// History entry detail context panel.
fn draw_context_history(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    app: &App,
    idx: usize,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER))
        .title(Line::from(vec![Span::styled(
            " Query Detail ",
            Style::default().fg(HEADING).bg(BG),
        )]));

    if let Some(entry) = app.history.entries.get(idx) {
        let cost_color = if entry.cost == 0.0 {
            Color::Green
        } else {
            Color::Yellow
        };
        let latency = if entry.latency_ms < 1000.0 {
            format!("{:.0}ms", entry.latency_ms)
        } else {
            format!("{:.1}s", entry.latency_ms / 1000.0)
        };

        let lines = vec![
            Line::from(""),
            Line::from(vec![Span::styled(
                "  Query",
                Style::default()
                    .fg(ACCENT)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
        ];

        let mut all_lines = lines;
        for line in super::tabs::chat::wrap_text(&entry.query, area.width.saturating_sub(6) as usize) {
            all_lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(line, Style::default().fg(FG)),
            ]));
        }

        all_lines.push(Line::from(""));
        all_lines.push(context_row("Time", &entry.timestamp));
        all_lines.push(context_row("Tier", &entry.tier));
        all_lines.push(Line::from(vec![
            Span::styled("  Cost        ", Style::default().fg(DIM)),
            Span::styled(format_money(entry.cost), Style::default().fg(cost_color)),
        ]));
        all_lines.push(context_row("Latency", &latency));

        frame.render_widget(
            Paragraph::new(all_lines)
                .block(block)
                .style(Style::default().bg(BG)),
            area,
        );
    } else {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(vec![Span::styled(
                    "  No entry selected",
                    Style::default().fg(DIM),
                )]),
            ])
            .block(block)
            .style(Style::default().bg(BG)),
            area,
        );
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
                TabId::Pea if !app.tasks.objectives.is_empty() => {
                    format!("{} ({})", tab.label(), app.tasks.objectives.len())
                }
                TabId::History if !app.history.entries.is_empty() => {
                    format!("{} ({})", tab.label(), app.history.entries.len())
                }
                TabId::Workflows if !app.workflows.workflows.is_empty() => {
                    format!("{} ({})", tab.label(), app.workflows.workflows.len())
                }
                TabId::Resources if !app.resources.resources.is_empty() => {
                    format!("{} ({})", tab.label(), app.resources.resources.len())
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
    let w = 55.min(area.width.saturating_sub(4));
    let h = 21.min(area.height.saturating_sub(4));
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
        help_row("1-7", "Jump to tab"),
        help_row("↑ ↓ / j k", "Navigate lists"),
        help_row("Enter", "Send query (Chat) / Detail"),
        help_row("PgUp / PgDn", "Scroll messages"),
        help_row("Ctrl+L", "Clear chat"),
        help_row("Ctrl+A / Ctrl+E", "Cursor start/end"),
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

/// Build a context panel key-value row (owned strings to avoid lifetime issues).
fn context_row(key: &str, val: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("  {:<12}  ", key),
            Style::default().fg(DIM),
        ),
        Span::styled(val.to_string(), Style::default().fg(FG)),
    ])
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
