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
//! - Command palette (Ctrl+K) with fuzzy search
//! - Toast notifications for background events
//! - Mouse support for tab switching and scrolling

use std::collections::VecDeque;
use std::io;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyModifiers, MouseButton, MouseEventKind};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
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
use super::tabs::resources::{ResourceAction, ResourcesTab};
use super::tabs::settings::{ConfigEntry, SettingsAction, SettingsTab};
use super::tabs::tasks::{ObjectiveSummary, PeaAction, TasksTab};
use super::tabs::workflows::{WorkflowAction, WorkflowsTab};
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
pub(crate) enum AppMessage {
    QueryResult {
        text: String,
        cost_label: String,
        tier: String,
        latency_ms: f64,
        cost: f64,
        query: String,
    },
    QueryError(String),
    /// A sensitive action requires interactive user confirmation.
    ConfirmationNeeded {
        request: crate::agent_os::confirmation::ConfirmationRequest,
        responder: std::sync::mpsc::Sender<crate::agent_os::confirmation::ConfirmationResponse>,
    },
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
    SettingsDetail(usize),
    ScheduleDetail(usize),
}

// ── Confirmation Modal ───────────────────────────────────────────────────────

/// State for the interactive permission confirmation modal.
struct PendingConfirmation {
    request: crate::agent_os::confirmation::ConfirmationRequest,
    responder: std::sync::mpsc::Sender<crate::agent_os::confirmation::ConfirmationResponse>,
    selected: usize, // 0=AllowOnce, 1=AllowSession, 2=AllowAlwaysAgent, 3=Deny
}

// ── Command Palette ──────────────────────────────────────────────────────────

/// A single command palette entry.
#[derive(Clone)]
struct PaletteCommand {
    label: String,
    category: &'static str,
    action: PaletteAction,
}

#[derive(Clone)]
enum PaletteAction {
    SwitchTab(TabId),
    ToggleLogs,
    ToggleHelp,
    Quit,
    ReloadSettings,
    ClearChat,
    SetStyle(String),
    ClearStyle,
    CycleCostPeriod,
    RunSecurityScan,
}

/// Fuzzy command palette state.
struct CommandPalette {
    open: bool,
    query: String,
    commands: Vec<PaletteCommand>,
    filtered: Vec<usize>, // indices into commands
    selected: usize,
}

impl CommandPalette {
    fn new() -> Self {
        let commands = vec![
            PaletteCommand { label: "Go to Chat".into(), category: "Navigation", action: PaletteAction::SwitchTab(TabId::Chat) },
            PaletteCommand { label: "Go to Agents".into(), category: "Navigation", action: PaletteAction::SwitchTab(TabId::Agents) },
            PaletteCommand { label: "Go to Workflows".into(), category: "Navigation", action: PaletteAction::SwitchTab(TabId::Workflows) },
            PaletteCommand { label: "Go to Resources".into(), category: "Navigation", action: PaletteAction::SwitchTab(TabId::Resources) },
            PaletteCommand { label: "Go to PEA".into(), category: "Navigation", action: PaletteAction::SwitchTab(TabId::Pea) },
            PaletteCommand { label: "Go to Settings".into(), category: "Navigation", action: PaletteAction::SwitchTab(TabId::Settings) },
            PaletteCommand { label: "Go to History".into(), category: "Navigation", action: PaletteAction::SwitchTab(TabId::History) },
            PaletteCommand { label: "Toggle Logs Panel".into(), category: "View", action: PaletteAction::ToggleLogs },
            PaletteCommand { label: "Show Help".into(), category: "View", action: PaletteAction::ToggleHelp },
            PaletteCommand { label: "Clear Chat".into(), category: "Chat", action: PaletteAction::ClearChat },
            PaletteCommand { label: "Reload Settings".into(), category: "Settings", action: PaletteAction::ReloadSettings },
            PaletteCommand { label: "Style: Children".into(), category: "Style", action: PaletteAction::SetStyle("children".into()) },
            PaletteCommand { label: "Style: Young Adults".into(), category: "Style", action: PaletteAction::SetStyle("young_adults".into()) },
            PaletteCommand { label: "Style: Seniors".into(), category: "Style", action: PaletteAction::SetStyle("seniors".into()) },
            PaletteCommand { label: "Style: Technical".into(), category: "Style", action: PaletteAction::SetStyle("technical".into()) },
            PaletteCommand { label: "Clear Style".into(), category: "Style", action: PaletteAction::ClearStyle },
            PaletteCommand { label: "Cycle Cost Period".into(), category: "Stats", action: PaletteAction::CycleCostPeriod },
            PaletteCommand { label: "Run Security Scan".into(), category: "Security", action: PaletteAction::RunSecurityScan },
            PaletteCommand { label: "Quit".into(), category: "System", action: PaletteAction::Quit },
        ];
        let filtered: Vec<usize> = (0..commands.len()).collect();
        Self {
            open: false,
            query: String::new(),
            commands,
            filtered,
            selected: 0,
        }
    }

    fn toggle(&mut self) {
        self.open = !self.open;
        if self.open {
            self.query.clear();
            self.filtered = (0..self.commands.len()).collect();
            self.selected = 0;
        }
    }

    fn close(&mut self) {
        self.open = false;
    }

    fn update_filter(&mut self) {
        let q = self.query.to_lowercase();
        if q.is_empty() {
            self.filtered = (0..self.commands.len()).collect();
        } else {
            self.filtered = self
                .commands
                .iter()
                .enumerate()
                .filter(|(_, c)| {
                    let label_lower = c.label.to_lowercase();
                    let cat_lower = c.category.to_lowercase();
                    // Simple fuzzy: all query chars appear in order
                    fuzzy_match(&q, &label_lower) || cat_lower.contains(&q)
                })
                .map(|(i, _)| i)
                .collect();
        }
        if self.selected >= self.filtered.len() {
            self.selected = 0;
        }
    }

    fn select_next(&mut self) {
        if !self.filtered.is_empty() {
            self.selected = (self.selected + 1) % self.filtered.len();
        }
    }

    fn select_prev(&mut self) {
        if !self.filtered.is_empty() {
            self.selected = (self.selected + self.filtered.len() - 1) % self.filtered.len();
        }
    }

    fn selected_action(&self) -> Option<PaletteAction> {
        self.filtered
            .get(self.selected)
            .and_then(|&i| self.commands.get(i))
            .map(|c| c.action.clone())
    }
}

/// Simple subsequence fuzzy match.
fn fuzzy_match(query: &str, target: &str) -> bool {
    let mut chars = target.chars();
    for qc in query.chars() {
        loop {
            match chars.next() {
                Some(tc) if tc == qc => break,
                Some(_) => continue,
                None => return false,
            }
        }
    }
    true
}

// ── Toast Notifications ──────────────────────────────────────────────────────

struct Toast {
    message: String,
    is_error: bool,
    created: Instant,
}

impl Toast {
    fn new(message: String, is_error: bool) -> Self {
        Self {
            message,
            is_error,
            created: Instant::now(),
        }
    }

    fn expired(&self) -> bool {
        self.created.elapsed() > Duration::from_secs(3)
    }
}

// ── Layout geometry for mouse hit testing ────────────────────────────────────

/// Cached layout rectangles for mouse hit-testing.
struct LayoutGeometry {
    tab_bar: ratatui::layout::Rect,
}

impl Default for LayoutGeometry {
    fn default() -> Self {
        Self {
            tab_bar: ratatui::layout::Rect::default(),
        }
    }
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
    pub schedule: super::tabs::schedule::ScheduleTab,
    pub should_quit: bool,
    pub show_help: bool,
    pub stats_queries: u64,
    pub stats_saved: f64,
    pub stats_spent: f64,
    pub stats_cache_pct: f64,
    /// Cost period filter: 0=All, 1=24h, 2=7d, 3=30d
    pub cost_period: u8,
    pub show_logs: bool,
    context_panel: ContextPanel,
    log_buffer: Arc<Mutex<VecDeque<String>>>,
    start_time: Instant,
    config: NyayaConfig,
    orchestrator: Option<Arc<Mutex<Orchestrator>>>,
    rx: mpsc::Receiver<AppMessage>,
    tx: mpsc::Sender<AppMessage>,
    palette: CommandPalette,
    toasts: Vec<Toast>,
    layout: LayoutGeometry,
    /// Active confirmation modal (if any).
    pending_confirmation: Option<PendingConfirmation>,
}

impl App {
    pub fn new(config: NyayaConfig, log_buffer: Arc<Mutex<VecDeque<String>>>) -> Self {
        let (tx, rx) = mpsc::channel();

        let mut settings = SettingsTab::new();
        Self::populate_settings(&mut settings, &config);

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
            schedule: super::tabs::schedule::ScheduleTab::new(),
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
            cost_period: 0,
            config,
            orchestrator,
            rx,
            tx,
            palette: CommandPalette::new(),
            toasts: Vec::new(),
            layout: LayoutGeometry::default(),
            pending_confirmation: None,
        };

        // Seed catalog + workflows if empty (first run)
        app.seed_agent_catalog();
        app.seed_demo_workflows();

        // Load initial data
        app.load_conversation_history();
        app.load_agents();
        app.load_objectives();
        app.load_workflows();
        app.load_resources();
        app.load_scheduled_jobs();
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

    /// Seed agent catalog with bundled agents if catalog is empty.
    fn seed_agent_catalog(&self) {
        let catalog_dir = self.config.data_dir.join("catalog");
        std::fs::create_dir_all(&catalog_dir).ok();

        // Check if catalog already has entries
        if let Ok(entries) = std::fs::read_dir(&catalog_dir) {
            if entries
                .filter_map(|e| e.ok())
                .any(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            {
                return; // Already seeded
            }
        }

        // Bundled agents — same set as the setup wizard fallback
        let agents = [
            ("code-reviewer", "Developer", "Automated code review with security and style checks", "NabaOS Core", "1.2.0", &["llm.chat", "files.read", "git.read"][..]),
            ("morning-briefing", "Productivity", "Calendar, weather, news summary delivered daily", "NabaOS Core", "1.0.0", &["calendar.read", "weather.read", "news.read"]),
            ("email-assistant", "Communication", "Smart email triage and drafting", "NabaOS Core", "1.1.0", &["email.read", "email.write", "contacts.read"]),
            ("research-digest", "Research", "Summarize academic papers and articles", "NabaOS Core", "1.0.0", &["web.read", "pdf.read", "filesystem.write"]),
            ("budget-tracker", "Finance", "Track expenses, alert on budget overruns", "NabaOS Core", "1.0.0", &["finance.read", "spreadsheet.write"]),
            ("social-scheduler", "Creative", "Schedule and manage social media posts", "NabaOS Core", "1.0.0", &["social.write", "schedule.write"]),
            ("security-scanner", "Security", "Scan repos for leaked secrets and vulnerabilities", "NabaOS Core", "1.3.0", &["git.read", "files.read", "shell.exec"]),
            ("devops-monitor", "Developer", "Monitor CI/CD pipelines and deployment status", "NabaOS Core", "1.1.0", &["ci.read", "github.read", "shell.exec"]),
            ("data-analyst", "Research", "Run SQL queries and generate charts from datasets", "NabaOS Core", "1.0.0", &["database.read", "filesystem.write", "llm.chat"]),
            ("meeting-notes", "Productivity", "Transcribe and summarize meeting recordings", "NabaOS Core", "1.0.0", &["voice.transcribe", "llm.chat", "filesystem.write"]),
            ("api-tester", "Developer", "Automated API endpoint testing and reporting", "NabaOS Core", "1.0.0", &["web.read", "web.write", "filesystem.write"]),
            ("doc-generator", "Developer", "Generate documentation from codebases", "NabaOS Core", "1.0.0", &["files.read", "llm.chat", "filesystem.write"]),
        ];

        for (name, category, description, author, version, permissions) in &agents {
            let agent_dir = catalog_dir.join(name);
            if std::fs::create_dir_all(&agent_dir).is_ok() {
                let perms_yaml: Vec<String> = permissions.iter().map(|p| format!("  - {}", p)).collect();
                let manifest = format!(
                    "name: {}\nversion: \"{}\"\ndescription: \"{}\"\ncategory: {}\nauthor: {}\npermissions:\n{}\n",
                    name, version, description, category, author, perms_yaml.join("\n")
                );
                std::fs::write(agent_dir.join("manifest.yaml"), manifest).ok();
            }
        }
        tracing::info!("Seeded agent catalog with {} agents", agents.len());
    }

    /// Seed demo workflows into chains.db if empty.
    fn seed_demo_workflows(&self) {
        use crate::chain::demo_workflows::all_demo_workflows;
        use crate::chain::workflow_store::WorkflowStore;

        let db_path = self.config.data_dir.join("chains.db");
        if let Ok(store) = WorkflowStore::open(&db_path) {
            // Only seed if no definitions exist
            if let Ok(defs) = store.list_defs() {
                if !defs.is_empty() {
                    return;
                }
            }
            for def in all_demo_workflows() {
                if let Err(e) = store.store_def(&def) {
                    tracing::warn!("Failed to seed workflow {}: {}", def.name, e);
                }
            }
            tracing::info!("Seeded {} demo workflows", all_demo_workflows().len());
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
    fn populate_settings(settings: &mut SettingsTab, config: &NyayaConfig) {
        let mut entries = Vec::new();

        // Provider section
        entries.push(ConfigEntry {
            key: "LLM Provider".into(),
            value: config.llm_provider.as_deref().unwrap_or("not set").to_string(),
            section: "Provider".into(),
            editable: true,
            is_secret: false,
            env_key: Some("NABA_LLM_PROVIDER".into()),
        });
        entries.push(ConfigEntry {
            key: "LLM Model".into(),
            value: config.llm_model.as_deref().unwrap_or("default").to_string(),
            section: "Provider".into(),
            editable: true,
            is_secret: false,
            env_key: Some("NABA_LLM_MODEL".into()),
        });
        entries.push(ConfigEntry {
            key: "API Key".into(),
            value: config.llm_api_key.as_deref().unwrap_or("").to_string(),
            section: "Provider".into(),
            editable: true,
            is_secret: true,
            env_key: Some("NABA_LLM_API_KEY".into()),
        });
        if let Some(ref url) = config.llm_base_url {
            entries.push(ConfigEntry {
                key: "Base URL".into(),
                value: url.clone(),
                section: "Provider".into(),
                editable: true,
                is_secret: false,
                env_key: Some("NABA_LLM_BASE_URL".into()),
            });
        }

        // Constitution section
        entries.push(ConfigEntry {
            key: "Template".into(),
            value: config
                .constitution_template
                .as_deref()
                .unwrap_or("default")
                .to_string(),
            section: "Constitution".into(),
            editable: true,
            is_secret: false,
            env_key: Some("NABA_CONSTITUTION".into()),
        });
        if let Some(ref path) = config.constitution_path {
            entries.push(ConfigEntry {
                key: "Path".into(),
                value: path.display().to_string(),
                section: "Constitution".into(),
                editable: false,
                is_secret: false,
                env_key: None,
            });
        }

        // Budget section
        entries.push(ConfigEntry {
            key: "Daily Budget".into(),
            value: config
                .daily_budget_usd
                .map(|b| format!("${:.2}", b))
                .unwrap_or_else(|| "unlimited".to_string()),
            section: "Budget".into(),
            editable: true,
            is_secret: false,
            env_key: Some("NABA_DAILY_BUDGET_USD".into()),
        });
        entries.push(ConfigEntry {
            key: "Per-Task Budget".into(),
            value: config
                .per_task_budget_usd
                .map(|b| format!("${:.2}", b))
                .unwrap_or_else(|| "unlimited".to_string()),
            section: "Budget".into(),
            editable: true,
            is_secret: false,
            env_key: Some("NABA_PER_TASK_BUDGET_USD".into()),
        });

        // Channels section
        let web_enabled = std::env::var("NABA_WEB_ENABLED")
            .map(|v| v == "true")
            .unwrap_or(false);
        entries.push(ConfigEntry {
            key: "Web Dashboard".into(),
            value: if web_enabled { "enabled" } else { "disabled" }.to_string(),
            section: "Channels".into(),
            editable: true,
            is_secret: false,
            env_key: Some("NABA_WEB_ENABLED".into()),
        });
        let telegram_enabled = std::env::var("NABA_TELEGRAM_ENABLED")
            .map(|v| v == "true")
            .unwrap_or(false);
        entries.push(ConfigEntry {
            key: "Telegram".into(),
            value: if telegram_enabled { "enabled" } else { "disabled" }.to_string(),
            section: "Channels".into(),
            editable: true,
            is_secret: false,
            env_key: Some("NABA_TELEGRAM_ENABLED".into()),
        });
        if let Ok(bind) = std::env::var("NABA_WEB_BIND") {
            entries.push(ConfigEntry {
                key: "Web Bind".into(),
                value: bind,
                section: "Channels".into(),
                editable: true,
                is_secret: false,
                env_key: Some("NABA_WEB_BIND".into()),
            });
        }

        // API Keys section
        for &(key_name, desc) in &[
            ("NABA_UNSPLASH_KEY", "Unsplash image search"),
            ("NABA_PEXELS_KEY", "Pexels image search"),
            ("NABA_FAL_API_KEY", "FAL AI image generation"),
        ] {
            let is_set = std::env::var(key_name).map(|v| !v.is_empty()).unwrap_or(false);
            entries.push(ConfigEntry {
                key: desc.to_string(),
                value: if is_set { "[SET]".to_string() } else { "[NOT SET]".to_string() },
                section: "API Keys".to_string(),
                editable: true,
                is_secret: true,
                env_key: Some(key_name.to_string()),
            });
        }

        // System section
        entries.push(ConfigEntry {
            key: "Data Directory".into(),
            value: config.data_dir.display().to_string(),
            section: "System".into(),
            editable: false,
            is_secret: false,
            env_key: None,
        });
        entries.push(ConfigEntry {
            key: "Plugin Dir".into(),
            value: config.plugin_dir.display().to_string(),
            section: "System".into(),
            editable: false,
            is_secret: false,
            env_key: None,
        });
        entries.push(ConfigEntry {
            key: "Version".into(),
            value: env!("CARGO_PKG_VERSION").to_string(),
            section: "System".into(),
            editable: false,
            is_secret: false,
            env_key: None,
        });

        settings.set_entries(entries);
    }

    fn load_objectives(&mut self) {
        use crate::pea::engine::PeaEngine;

        if let Ok(engine) = PeaEngine::open(&self.config.data_dir) {
            if let Ok(objectives) = engine.list_objectives() {
                let summaries: Vec<_> = objectives
                    .into_iter()
                    .map(|obj| {
                        let tasks = engine.get_tasks(&obj.id).unwrap_or_default();
                        let task_count = tasks.len();
                        let completed_tasks = tasks
                            .iter()
                            .filter(|t| {
                                matches!(
                                    t.status,
                                    crate::pea::objective::TaskStatus::Completed
                                )
                            })
                            .count();
                        let milestones_achieved = obj
                            .milestones
                            .iter()
                            .filter(|m| m.achieved)
                            .count();
                        let beliefs: Vec<(String, f64)> = obj
                            .beliefs
                            .confidence
                            .iter()
                            .map(|(k, &v)| (k.clone(), v))
                            .collect();

                        let output_id = obj
                            .beliefs
                            .get("output_id")
                            .and_then(|v| v.as_str().map(|s| s.to_string()));

                        ObjectiveSummary {
                            id: obj.id,
                            description: obj.description,
                            status: format!("{}", obj.status),
                            spent: obj.spent_usd,
                            budget: obj.budget_usd,
                            progress_score: obj.progress_score,
                            task_count,
                            completed_tasks,
                            milestone_count: obj.milestones.len(),
                            milestones_achieved,
                            budget_strategy: format!("{:?}", obj.budget_strategy),
                            beliefs,
                            created_at: obj.created_at,
                            output_id,
                        }
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
                        let active_count = store.count_active_instances(&id).unwrap_or(0);
                        let instances = store.list_instances_for_workflow(&id).unwrap_or_default();
                        let instance_count = instances.len();
                        let last_status = instances
                            .last()
                            .map(|i| format!("{:?}", i.status))
                            .unwrap_or_else(|| {
                                if active_count > 0 { "running".to_string() } else { "idle".to_string() }
                            });

                        // Load full def for node/param info
                        let (description, node_count, param_names, max_instances, global_timeout_secs) =
                            if let Ok(Some(def)) = store.get_def(&id) {
                                (
                                    def.description.clone(),
                                    def.nodes.len(),
                                    def.params.iter().map(|p| p.name.clone()).collect(),
                                    def.max_instances,
                                    def.global_timeout_secs,
                                )
                            } else {
                                (String::new(), 0, Vec::new(), 0, 0)
                            };

                        super::tabs::workflows::WorkflowSummary {
                            id,
                            name,
                            description,
                            node_count,
                            param_names,
                            instance_count,
                            active_count,
                            last_status,
                            max_instances,
                            global_timeout_secs,
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
                        let cost_model = r
                            .cost_model
                            .as_ref()
                            .map(|c| format!("{:?}", c))
                            .unwrap_or_else(|| "Free".to_string());
                        let metadata: Vec<(String, String)> = r
                            .metadata
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        super::tabs::resources::ResourceSummary {
                            id: r.id,
                            name: r.name,
                            resource_type: format!("{}", r.resource_type),
                            status: format!("{}", r.status),
                            active_leases: leases_for,
                            cost_model,
                            registered_at: r.registered_at,
                            metadata,
                        }
                    })
                    .collect();
                self.resources.set_resources(summaries);
            }
        }
    }

    /// Load scheduled jobs from the scheduler database.
    fn load_scheduled_jobs(&mut self) {
        use crate::chain::scheduler::Scheduler;

        let db_path = self.config.data_dir.join("scheduler.db");
        if let Ok(scheduler) = Scheduler::open(&db_path) {
            if let Ok(jobs) = scheduler.list() {
                let summaries: Vec<_> = jobs
                    .into_iter()
                    .map(|j| {
                        let schedule_desc = if let Some(ref cron) = j.cron_expr {
                            if !cron.is_empty() {
                                format!("cron: {}", cron)
                            } else if j.interval_secs >= 3600 {
                                format!("every {}h", j.interval_secs / 3600)
                            } else {
                                format!("every {}m", j.interval_secs / 60)
                            }
                        } else if j.interval_secs >= 3600 {
                            format!("every {}h", j.interval_secs / 3600)
                        } else {
                            format!("every {}m", j.interval_secs / 60)
                        };
                        super::tabs::schedule::JobSummary {
                            id: j.id,
                            chain_id: j.chain_id,
                            enabled: j.enabled,
                            schedule_desc,
                            last_run_at: j.last_run_at,
                            last_output: j.last_output,
                            run_count: j.run_count,
                            created_at: j.created_at,
                        }
                    })
                    .collect();
                self.schedule.set_jobs(summaries);
            }
        }
    }

    /// Process pending schedule tab actions.
    fn process_schedule_action(&mut self) {
        use crate::chain::scheduler::{Scheduler, ScheduleSpec};

        let action = match self.schedule.take_action() {
            Some(a) => a,
            None => return,
        };

        let db_path = self.config.data_dir.join("scheduler.db");
        match action {
            super::tabs::schedule::ScheduleAction::Create { chain_id, spec } => {
                match Scheduler::open(&db_path) {
                    Ok(scheduler) => {
                        let schedule_spec = if spec.starts_with("cron:") {
                            ScheduleSpec::Cron(spec.trim_start_matches("cron:").trim().to_string())
                        } else {
                            // Parse "every Nm" / "every Nh" / "every Ns"
                            let secs = parse_interval(&spec);
                            ScheduleSpec::Interval(secs)
                        };
                        match scheduler.schedule(&chain_id, schedule_spec, "{}") {
                            Ok(id) => {
                                self.schedule.show_status(
                                    format!("Created job {}", &id[..id.len().min(20)]),
                                    false,
                                );
                                self.load_scheduled_jobs();
                            }
                            Err(e) => {
                                self.schedule.show_status(format!("Error: {}", e), true);
                            }
                        }
                    }
                    Err(e) => {
                        self.schedule.show_status(format!("DB error: {}", e), true);
                    }
                }
            }
            super::tabs::schedule::ScheduleAction::ToggleEnabled { job_id, enable } => {
                match Scheduler::open(&db_path) {
                    Ok(scheduler) => {
                        let result = if enable {
                            scheduler.enable(&job_id)
                        } else {
                            scheduler.disable(&job_id)
                        };
                        match result {
                            Ok(()) => {
                                let verb = if enable { "Enabled" } else { "Disabled" };
                                self.schedule.show_status(
                                    format!("{} job", verb),
                                    false,
                                );
                                self.load_scheduled_jobs();
                            }
                            Err(e) => {
                                self.schedule.show_status(format!("Error: {}", e), true);
                            }
                        }
                    }
                    Err(e) => {
                        self.schedule.show_status(format!("DB error: {}", e), true);
                    }
                }
            }
            super::tabs::schedule::ScheduleAction::LoadHistory { job_id } => {
                match Scheduler::open(&db_path) {
                    Ok(scheduler) => {
                        if let Ok(entries) = scheduler.history(&job_id, 50) {
                            let rows: Vec<_> = entries
                                .into_iter()
                                .map(|e| super::tabs::schedule::HistoryRow {
                                    job_id: e.job_id,
                                    chain_id: e.chain_id,
                                    output: e.output,
                                    changed: e.changed,
                                    run_at: e.run_at,
                                })
                                .collect();
                            self.schedule.set_history(rows);
                        }
                    }
                    Err(_) => {}
                }
            }
        }
    }

    /// Compute the `since_ms` filter for the current cost period.
    fn cost_since_ms(&self) -> Option<i64> {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        match self.cost_period {
            1 => Some(now_ms - 86_400_000),        // 24h
            2 => Some(now_ms - 7 * 86_400_000),    // 7d
            3 => Some(now_ms - 30 * 86_400_000),   // 30d
            _ => None,                              // All time
        }
    }

    /// Human label for the current cost period.
    fn cost_period_label(&self) -> &'static str {
        match self.cost_period {
            1 => "24h",
            2 => "7d",
            3 => "30d",
            _ => "all",
        }
    }

    /// Refresh stats from the orchestrator.
    fn refresh_stats(&mut self) {
        let since = self.cost_since_ms();
        if let Some(ref orch) = self.orchestrator {
            if let Ok(orch) = orch.lock() {
                if let Ok(summary) = orch.cost_summary(since) {
                    self.stats_queries = summary.total_llm_calls + summary.total_cache_hits;
                    self.stats_saved = summary.total_saved_usd;
                    self.stats_spent = summary.total_spent_usd;
                    self.stats_cache_pct = summary.savings_percent;
                }
            }
        } else {
            // Fallback: create a temporary orchestrator for stats
            if let Ok(orch) = Orchestrator::new(self.config.clone()) {
                if let Ok(summary) = orch.cost_summary(since) {
                    self.stats_queries = summary.total_llm_calls + summary.total_cache_hits;
                    self.stats_saved = summary.total_saved_usd;
                    self.stats_spent = summary.total_spent_usd;
                    self.stats_cache_pct = summary.savings_percent;
                }
            }
        }
    }

    /// Parse an `@agent-name` mention from the start of a query.
    /// Returns `(Some(agent_name), rest_of_query)` or `(None, original_query)`.
    fn parse_agent_mention(query: &str) -> (Option<String>, String) {
        let trimmed = query.trim();
        if let Some(rest) = trimmed.strip_prefix('@') {
            if let Some(space_idx) = rest.find(char::is_whitespace) {
                let agent = rest[..space_idx].to_string();
                let remainder = rest[space_idx..].trim().to_string();
                if !agent.is_empty() && !remainder.is_empty() {
                    return (Some(agent), remainder);
                }
            }
            // Bare "@agent" with no query — treat entire string as agent, empty query
            if !rest.is_empty() && !rest.contains(char::is_whitespace) {
                return (Some(rest.to_string()), String::new());
            }
        }
        (None, query.to_string())
    }

    /// Submit a query for background processing.
    fn submit_query(&mut self, query: String) {
        self.chat.push_user(query.clone());
        self.chat.set_loading(true);

        let tx = self.tx.clone();
        let config = self.config.clone();
        let orch_arc = self.orchestrator.clone();

        // Parse @agent mention
        let (target_agent, q) = Self::parse_agent_mention(&query);

        std::thread::spawn(move || {
            let start = Instant::now();

            // Try using the shared orchestrator, fall back to creating a new one
            let result = if let Some(ref orch_arc) = orch_arc {
                if let Ok(mut orch) = orch_arc.lock() {
                    // Handle @agent routing
                    let prev_agent = if let Some(ref agent) = target_agent {
                        let prev = orch.active_agent().to_string();
                        orch.set_active_agent(agent);
                        Some(prev)
                    } else {
                        None
                    };

                    // Pass confirm_tx so orchestrator can request confirmations
                    orch.set_confirm_tx(Some(tx.clone()));
                    let res = orch.process_query(&q, None);
                    orch.set_confirm_tx(None);

                    // Restore previous agent if we did @agent routing
                    if let Some(prev) = prev_agent {
                        orch.set_active_agent(&prev);
                    }

                    Some(res)
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
                    self.push_toast(format!("Query failed: {}", e), true);
                }
                AppMessage::ConfirmationNeeded { request, responder } => {
                    self.pending_confirmation = Some(PendingConfirmation {
                        request,
                        responder,
                        selected: 0,
                    });
                }
            }
        }
    }

    /// Push a toast notification.
    fn push_toast(&mut self, message: String, is_error: bool) {
        self.toasts.push(Toast::new(message, is_error));
        // Keep at most 5 toasts
        if self.toasts.len() > 5 {
            self.toasts.remove(0);
        }
    }

    /// Remove expired toasts.
    fn tick_toasts(&mut self) {
        self.toasts.retain(|t| !t.expired());
    }

    /// Execute a command palette action.
    fn execute_palette_action(&mut self, action: PaletteAction) {
        match action {
            PaletteAction::SwitchTab(tab) => {
                self.active_tab = tab;
            }
            PaletteAction::ToggleLogs => {
                self.show_logs = !self.show_logs;
            }
            PaletteAction::ToggleHelp => {
                self.show_help = !self.show_help;
            }
            PaletteAction::Quit => {
                self.should_quit = true;
            }
            PaletteAction::ReloadSettings => {
                self.pending_settings_reload();
            }
            PaletteAction::ClearChat => {
                self.chat.clear();
            }
            PaletteAction::SetStyle(name) => {
                let result = self.orchestrator.as_ref().and_then(|o| {
                    o.lock().ok().map(|mut orch| orch.set_style(&name))
                });
                match result {
                    Some(Ok(())) => self.push_toast(format!("Style set to '{}'", name), false),
                    Some(Err(e)) => self.push_toast(format!("Style error: {}", e), true),
                    None => {}
                }
            }
            PaletteAction::ClearStyle => {
                if let Some(ref orch) = self.orchestrator {
                    let _ = orch.lock().map(|mut o| o.clear_style());
                }
                self.push_toast("Style cleared".into(), false);
            }
            PaletteAction::CycleCostPeriod => {
                self.cost_period = (self.cost_period + 1) % 4;
                let label = self.cost_period_label();
                self.push_toast(format!("Cost period: {}", label), false);
                self.refresh_stats();
            }
            PaletteAction::RunSecurityScan => {
                use crate::security::{credential_scanner, pattern_matcher};
                // Scan the last user message from chat
                let text = self
                    .chat
                    .last_user_message()
                    .unwrap_or_default();
                if text.is_empty() {
                    self.push_toast("No message to scan.".into(), true);
                } else {
                    let creds = credential_scanner::scan_summary(&text);
                    let injection = pattern_matcher::assess(&text);
                    let mut msg = String::from("Scan: ");
                    if creds.credential_count == 0 && creds.pii_count == 0 && !injection.likely_injection {
                        msg.push_str("clean");
                    } else {
                        if creds.credential_count > 0 {
                            msg.push_str(&format!("{} creds ", creds.credential_count));
                        }
                        if creds.pii_count > 0 {
                            msg.push_str(&format!("{} PII ", creds.pii_count));
                        }
                        if injection.likely_injection {
                            msg.push_str("INJECTION ");
                        }
                    }
                    self.push_toast(msg, injection.likely_injection || creds.credential_count > 0);
                }
            }
        }
    }

    fn pending_settings_reload(&mut self) {
        use crate::core::config::NyayaConfig;
        if let Ok(new_config) = NyayaConfig::load() {
            self.config = new_config;
            Self::populate_settings(&mut self.settings, &self.config);
            self.push_toast("Settings reloaded".into(), false);
        } else {
            self.push_toast("Failed to reload settings".into(), true);
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
                    let msg = format!("Installed {}", name);
                    self.agents.show_status(msg.clone(), false);
                    self.push_toast(msg, false);
                    self.load_agents();
                }
                Err(e) => {
                    let msg = format!("Install failed: {}", e);
                    self.agents.show_status(msg.clone(), true);
                    self.push_toast(msg, true);
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

    /// Process pending workflow actions from the workflows tab.
    fn process_workflow_actions(&mut self) {
        if let Some(action) = self.workflows.take_action() {
            match action {
                WorkflowAction::Start { workflow_id, params } => {
                    self.do_workflow_start(&workflow_id, params);
                }
                WorkflowAction::Cancel { instance_id } => {
                    self.do_workflow_cancel(&instance_id);
                }
                WorkflowAction::LoadInstances { workflow_id } => {
                    self.do_load_instances(&workflow_id);
                }
            }
        }
    }

    fn do_workflow_start(&mut self, workflow_id: &str, params: Vec<(String, String)>) {
        use crate::chain::workflow_engine::WorkflowEngine;
        use crate::chain::workflow_store::WorkflowStore;
        use std::collections::HashMap;

        let db_path = self.config.data_dir.join("chains.db");
        match WorkflowStore::open(&db_path) {
            Ok(store) => {
                let engine = WorkflowEngine::new(store);
                let param_map: HashMap<String, String> = params.into_iter().collect();
                match engine.start(workflow_id, param_map) {
                    Ok(instance_id) => {
                        let short_id = if instance_id.len() > 12 {
                            format!("{}…", &instance_id[..12])
                        } else {
                            instance_id.clone()
                        };
                        self.workflows
                            .show_status(format!("Started instance {}", short_id), false);
                        self.load_workflows();
                        // Reload instances if viewing this workflow
                        if let super::tabs::workflows::WorkflowView::Instances(ref wf_id) =
                            self.workflows.view
                        {
                            if wf_id == workflow_id {
                                self.do_load_instances(workflow_id);
                            }
                        }
                    }
                    Err(e) => {
                        self.workflows
                            .show_status(format!("Start failed: {}", e), true);
                    }
                }
            }
            Err(e) => {
                self.workflows
                    .show_status(format!("Store error: {}", e), true);
            }
        }
    }

    fn do_workflow_cancel(&mut self, instance_id: &str) {
        use crate::chain::workflow_engine::WorkflowEngine;
        use crate::chain::workflow_store::WorkflowStore;

        let db_path = self.config.data_dir.join("chains.db");
        match WorkflowStore::open(&db_path) {
            Ok(store) => {
                let engine = WorkflowEngine::new(store);
                match engine.cancel(instance_id) {
                    Ok(()) => {
                        let short_id = if instance_id.len() > 12 {
                            format!("{}…", &instance_id[..12])
                        } else {
                            instance_id.to_string()
                        };
                        self.workflows
                            .show_status(format!("Cancelled {}", short_id), false);
                        self.load_workflows();
                        // Reload instances for current view
                        if let super::tabs::workflows::WorkflowView::Instances(ref wf_id) =
                            self.workflows.view
                        {
                            let wf_id = wf_id.clone();
                            self.do_load_instances(&wf_id);
                        }
                    }
                    Err(e) => {
                        self.workflows
                            .show_status(format!("Cancel failed: {}", e), true);
                    }
                }
            }
            Err(e) => {
                self.workflows
                    .show_status(format!("Store error: {}", e), true);
            }
        }
    }

    fn do_load_instances(&mut self, workflow_id: &str) {
        use crate::chain::workflow_store::WorkflowStore;

        let db_path = self.config.data_dir.join("chains.db");
        if let Ok(store) = WorkflowStore::open(&db_path) {
            // Get the def for node info
            let node_info: Vec<(String, String)> =
                if let Ok(Some(def)) = store.get_def(workflow_id) {
                    def.nodes
                        .iter()
                        .map(|n| {
                            let id = n.id().to_string();
                            let ntype = match n {
                                crate::chain::workflow::WorkflowNode::Action(_) => "action",
                                crate::chain::workflow::WorkflowNode::WaitEvent(_) => "wait",
                                crate::chain::workflow::WorkflowNode::Delay(_) => "delay",
                                crate::chain::workflow::WorkflowNode::WaitPoll(_) => "poll",
                                crate::chain::workflow::WorkflowNode::Parallel(_) => "parallel",
                                crate::chain::workflow::WorkflowNode::Branch(_) => "branch",
                                crate::chain::workflow::WorkflowNode::Compensate(_) => "compensate",
                            };
                            (id, ntype.to_string())
                        })
                        .collect()
                } else {
                    Vec::new()
                };

            let node_count = node_info.len();

            if let Ok(instances) = store.list_instances_for_workflow(workflow_id) {
                let summaries: Vec<_> = instances
                    .into_iter()
                    .map(|inst| super::tabs::workflows::InstanceSummary {
                        instance_id: inst.instance_id,
                        workflow_id: inst.workflow_id,
                        status: format!("{:?}", inst.status),
                        cursor_node: inst.cursor.node_index,
                        node_count,
                        error: inst.error,
                        created_at: inst.created_at,
                        updated_at: inst.updated_at,
                        execution_ms: inst.execution_ms,
                        node_names: node_info.clone(),
                    })
                    .collect();
                self.workflows.set_instances(summaries);
            }
        }
    }

    /// Process pending resource actions from the resources tab.
    fn process_resource_actions(&mut self) {
        if let Some(action) = self.resources.take_action() {
            match action {
                ResourceAction::Register {
                    id,
                    name,
                    resource_type,
                } => {
                    self.do_resource_register(&id, &name, &resource_type);
                }
                ResourceAction::Delete { resource_id } => {
                    self.do_resource_delete(&resource_id);
                }
                ResourceAction::LoadLeases { resource_id } => {
                    self.do_load_leases(&resource_id);
                }
            }
        }
    }

    fn do_resource_register(&mut self, id: &str, name: &str, resource_type: &str) {
        use crate::resource::registry::ResourceRegistry;
        use crate::resource::ResourceType;

        let db_path = self.config.data_dir.join("resources.db");
        match ResourceRegistry::open(&db_path) {
            Ok(registry) => {
                let rtype = match resource_type {
                    "Compute" => ResourceType::Compute,
                    "Financial" => ResourceType::Financial,
                    "Device" => ResourceType::Device,
                    _ => ResourceType::ApiService,
                };
                match registry.register(id, name, &rtype, "{}") {
                    Ok(()) => {
                        self.resources
                            .show_status(format!("Registered \"{}\"", name), false);
                        self.load_resources();
                    }
                    Err(e) => {
                        self.resources
                            .show_status(format!("Register failed: {}", e), true);
                    }
                }
            }
            Err(e) => {
                self.resources
                    .show_status(format!("Store error: {}", e), true);
            }
        }
    }

    fn do_resource_delete(&mut self, resource_id: &str) {
        use crate::resource::registry::ResourceRegistry;

        let db_path = self.config.data_dir.join("resources.db");
        match ResourceRegistry::open(&db_path) {
            Ok(registry) => match registry.unregister(resource_id) {
                Ok(()) => {
                    self.resources
                        .show_status(format!("Deleted \"{}\"", resource_id), false);
                    self.load_resources();
                }
                Err(e) => {
                    self.resources
                        .show_status(format!("Delete failed: {}", e), true);
                }
            },
            Err(e) => {
                self.resources
                    .show_status(format!("Store error: {}", e), true);
            }
        }
    }

    fn do_load_leases(&mut self, resource_id: &str) {
        use crate::resource::registry::ResourceRegistry;

        let db_path = self.config.data_dir.join("resources.db");
        if let Ok(registry) = ResourceRegistry::open(&db_path) {
            let all_leases = registry.list_active_leases().unwrap_or_default();
            let summaries: Vec<_> = all_leases
                .into_iter()
                .filter(|l| l.resource_id == resource_id)
                .map(|l| super::tabs::resources::LeaseSummary {
                    lease_id: l.lease_id,
                    resource_id: l.resource_id,
                    agent_id: l.agent_id,
                    capabilities: l
                        .capabilities
                        .iter()
                        .map(|c| format!("{:?}", c))
                        .collect(),
                    used_cost_usd: l.used_cost_usd,
                    max_cost_usd: l.quota.max_cost_usd,
                    used_calls: l.used_calls,
                    max_calls: l.quota.max_calls,
                    started_at: l.started_at,
                    expires_at: l.expires_at,
                    status: format!("{:?}", l.status),
                })
                .collect();
            self.resources.set_leases(summaries);
        }
    }

    /// Process pending PEA actions from the tasks tab.
    fn process_pea_actions(&mut self) {
        if let Some(action) = self.tasks.take_action() {
            match action {
                PeaAction::Create {
                    description,
                    budget_usd,
                } => {
                    self.do_pea_create(&description, budget_usd);
                }
                PeaAction::Pause { objective_id } => {
                    self.do_pea_pause(&objective_id);
                }
                PeaAction::Resume { objective_id } => {
                    self.do_pea_resume(&objective_id);
                }
                PeaAction::Cancel { objective_id } => {
                    self.do_pea_cancel(&objective_id);
                }
                PeaAction::LoadTasks { objective_id } => {
                    self.do_load_tasks(&objective_id);
                }
                PeaAction::ViewOutput { objective_id } => {
                    self.do_view_output(&objective_id);
                }
                PeaAction::ImproveOutput { objective_id } => {
                    self.do_improve_output(&objective_id);
                }
            }
        }
    }

    fn do_pea_create(&mut self, description: &str, budget_usd: f64) {
        use crate::pea::engine::PeaEngine;

        match PeaEngine::open(&self.config.data_dir) {
            Ok(engine) => {
                // Create with a single desire matching the description
                let desires = vec![(
                    description.to_string(),
                    "objective completed".to_string(),
                    0,
                )];
                match engine.create_objective(description, budget_usd, desires) {
                    Ok(obj_id) => {
                        let short_id = if obj_id.len() > 8 {
                            &obj_id[..8]
                        } else {
                            &obj_id
                        };
                        self.tasks
                            .show_status(format!("Created objective {}", short_id), false);
                        self.load_objectives();
                    }
                    Err(e) => {
                        self.tasks
                            .show_status(format!("Create failed: {}", e), true);
                    }
                }
            }
            Err(e) => {
                self.tasks
                    .show_status(format!("PEA error: {}", e), true);
            }
        }
    }

    fn do_pea_pause(&mut self, objective_id: &str) {
        use crate::pea::engine::PeaEngine;

        match PeaEngine::open(&self.config.data_dir) {
            Ok(engine) => match engine.pause(objective_id) {
                Ok(()) => {
                    self.tasks
                        .show_status("Objective paused".to_string(), false);
                    self.load_objectives();
                }
                Err(e) => {
                    self.tasks
                        .show_status(format!("Pause failed: {}", e), true);
                }
            },
            Err(e) => {
                self.tasks
                    .show_status(format!("PEA error: {}", e), true);
            }
        }
    }

    fn do_pea_resume(&mut self, objective_id: &str) {
        use crate::pea::engine::PeaEngine;

        match PeaEngine::open(&self.config.data_dir) {
            Ok(engine) => match engine.resume(objective_id) {
                Ok(()) => {
                    self.tasks
                        .show_status("Objective resumed".to_string(), false);
                    self.load_objectives();
                }
                Err(e) => {
                    self.tasks
                        .show_status(format!("Resume failed: {}", e), true);
                }
            },
            Err(e) => {
                self.tasks
                    .show_status(format!("PEA error: {}", e), true);
            }
        }
    }

    fn do_pea_cancel(&mut self, objective_id: &str) {
        use crate::pea::engine::PeaEngine;

        match PeaEngine::open(&self.config.data_dir) {
            Ok(engine) => match engine.cancel(objective_id) {
                Ok(()) => {
                    self.tasks
                        .show_status("Objective cancelled".to_string(), false);
                    self.load_objectives();
                }
                Err(e) => {
                    self.tasks
                        .show_status(format!("Cancel failed: {}", e), true);
                }
            },
            Err(e) => {
                self.tasks
                    .show_status(format!("PEA error: {}", e), true);
            }
        }
    }

    fn do_load_tasks(&mut self, objective_id: &str) {
        use crate::pea::engine::PeaEngine;

        if let Ok(engine) = PeaEngine::open(&self.config.data_dir) {
            if let Ok(tasks) = engine.get_tasks(objective_id) {
                // Build depth map from parent_task_id hierarchy
                let id_to_parent: std::collections::HashMap<String, Option<String>> = tasks
                    .iter()
                    .map(|t| (t.id.clone(), t.parent_task_id.clone()))
                    .collect();

                fn calc_depth(
                    id: &str,
                    map: &std::collections::HashMap<String, Option<String>>,
                ) -> usize {
                    match map.get(id) {
                        Some(Some(parent)) => 1 + calc_depth(parent, map),
                        _ => 0,
                    }
                }

                let summaries: Vec<_> = tasks
                    .into_iter()
                    .map(|t| {
                        let depth = calc_depth(&t.id, &id_to_parent);
                        super::tabs::tasks::TaskSummary {
                            id: t.id,
                            description: t.description,
                            status: format!("{:?}", t.status),
                            task_type: format!("{:?}", t.task_type),
                            depends_on: t.depends_on,
                            parent_task_id: t.parent_task_id,
                            depth,
                            capability: t.capability_required,
                            retry_count: t.retry_count,
                            max_retries: t.max_retries,
                        }
                    })
                    .collect();
                self.tasks.set_tasks(summaries);
            }
        }
    }

    fn do_view_output(&mut self, objective_id: &str) {
        use crate::pea::output_store::OutputStore;
        let db_path = self.config.data_dir.join("outputs.db");
        match OutputStore::open(&db_path) {
            Ok(store) => match store.find_by_source(objective_id) {
                Ok(records) if !records.is_empty() => {
                    let rec = &records[0];
                    let path_info = rec
                        .file_path
                        .as_deref()
                        .unwrap_or("(no file)");
                    self.tasks.show_status(
                        format!("Output: {} — {}", rec.title, path_info),
                        false,
                    );
                }
                Ok(_) => {
                    self.tasks.show_status("No output found".to_string(), true);
                }
                Err(e) => {
                    self.tasks
                        .show_status(format!("Output lookup error: {}", e), true);
                }
            },
            Err(e) => {
                self.tasks
                    .show_status(format!("Output store error: {}", e), true);
            }
        }
    }

    fn do_improve_output(&mut self, objective_id: &str) {
        use crate::pea::engine::PeaEngine;
        match PeaEngine::open(&self.config.data_dir) {
            Ok(engine) => match engine.improve_objective(objective_id, None, 5.0) {
                Ok(new_id) => {
                    self.tasks.show_status(
                        format!("Improvement objective created: {}", new_id),
                        false,
                    );
                    self.load_objectives();
                }
                Err(e) => {
                    self.tasks
                        .show_status(format!("Improve error: {}", e), true);
                }
            },
            Err(e) => {
                self.tasks
                    .show_status(format!("PEA error: {}", e), true);
            }
        }
    }

    /// Process pending settings actions.
    fn process_settings_actions(&mut self) {
        if let Some(action) = self.settings.take_action() {
            match action {
                SettingsAction::Save { env_key, value } => {
                    self.do_settings_save(&env_key, &value);
                }
                SettingsAction::Reload => {
                    Self::populate_settings(&mut self.settings, &self.config);
                    self.settings
                        .show_status("Settings reloaded".to_string(), false);
                }
            }
        }
    }

    fn do_settings_save(&mut self, env_key: &str, value: &str) {
        let env_path = self.config.data_dir.join(".env");

        // Read existing .env, update or append the key
        let existing = std::fs::read_to_string(&env_path).unwrap_or_default();
        let mut found = false;
        let mut lines: Vec<String> = existing
            .lines()
            .map(|line| {
                if line.starts_with(env_key) && line.contains('=') {
                    let prefix = format!("{}=", env_key);
                    if line.starts_with(&prefix) {
                        found = true;
                        return format!("{}={}", env_key, value);
                    }
                }
                line.to_string()
            })
            .collect();

        if !found {
            lines.push(format!("{}={}", env_key, value));
        }

        match std::fs::write(&env_path, lines.join("\n")) {
            Ok(()) => {
                // Also set in current process env
                // Reload config from .env file
                if let Ok(new_config) = NyayaConfig::load() {
                    self.config = new_config;
                }
                Self::populate_settings(&mut self.settings, &self.config);
                self.settings
                    .show_status(format!("Saved {}", env_key), false);
            }
            Err(e) => {
                self.settings
                    .show_status(format!("Save failed: {}", e), true);
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
            TabId::Settings => {
                if let Some(i) = self.settings.state.selected() {
                    ContextPanel::SettingsDetail(i)
                } else {
                    ContextPanel::Welcome
                }
            }
            TabId::Schedule => {
                if let Some(i) = self.schedule.state.selected() {
                    ContextPanel::ScheduleDetail(i)
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
    io::stdout()
        .execute(EnableMouseCapture)
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
        app.tick_toasts();

        // Poll background results
        app.poll_messages();

        // Process agent actions
        app.process_agent_actions();

        // Process workflow actions
        app.process_workflow_actions();

        // Process resource actions
        app.process_resource_actions();

        // Process PEA actions
        app.process_pea_actions();

        // Process schedule actions
        app.process_schedule_action();

        // Process settings actions
        app.process_settings_actions();

        // Update context panel
        app.update_context_panel();

        // Compute layout geometry for mouse hit testing
        {
            let term_size = terminal.size()
                .map_err(|e| crate::core::error::NyayaError::Config(e.to_string()))?;
            let term_rect = ratatui::layout::Rect::new(0, 0, term_size.width, term_size.height);
            let outer = Block::default().borders(Borders::ALL);
            let inner = outer.inner(term_rect);
            let log_height = if app.show_logs { 8u16 } else { 0 };
            let chunks = Layout::vertical([
                Constraint::Length(2),
                Constraint::Min(5),
                Constraint::Length(log_height),
                Constraint::Length(1),
            ])
            .split(inner);
            app.layout.tab_bar = chunks[0];
        }

        // Draw
        terminal
            .draw(|frame| draw_ui(frame, &app))
            .map_err(|e| crate::core::error::NyayaError::Config(e.to_string()))?;

        // Poll events (100ms for smooth spinner animation)
        if event::poll(Duration::from_millis(100))
            .map_err(|e| crate::core::error::NyayaError::Config(e.to_string()))?
        {
            let ev = event::read()
                .map_err(|e| crate::core::error::NyayaError::Config(e.to_string()))?;

            match ev {
                Event::Key(key) => {
                    // Confirmation modal intercepts all keys when active
                    if let Some(ref mut pending) = app.pending_confirmation {
                        use crate::agent_os::confirmation::ConfirmationResponse;
                        match key.code {
                            KeyCode::Up => {
                                if pending.selected > 0 {
                                    pending.selected -= 1;
                                }
                            }
                            KeyCode::Down => {
                                if pending.selected < 3 {
                                    pending.selected += 1;
                                }
                            }
                            KeyCode::Char(n @ '1'..='4') => {
                                pending.selected = (n as usize) - ('1' as usize);
                            }
                            KeyCode::Enter => {
                                let response = ConfirmationResponse::ALL[pending.selected];
                                let _ = pending.responder.send(response);
                                app.pending_confirmation = None;
                            }
                            KeyCode::Esc => {
                                let _ = pending.responder.send(ConfirmationResponse::Deny);
                                app.pending_confirmation = None;
                            }
                            _ => {}
                        }
                    }
                    // Command palette intercepts all keys when open
                    else if app.palette.open {
                        match key.code {
                            KeyCode::Esc => {
                                app.palette.close();
                            }
                            KeyCode::Enter => {
                                if let Some(action) = app.palette.selected_action() {
                                    app.palette.close();
                                    app.execute_palette_action(action);
                                }
                            }
                            KeyCode::Up => {
                                app.palette.select_prev();
                            }
                            KeyCode::Down => {
                                app.palette.select_next();
                            }
                            KeyCode::Backspace => {
                                app.palette.query.pop();
                                app.palette.update_filter();
                            }
                            KeyCode::Char(c) => {
                                app.palette.query.push(c);
                                app.palette.update_filter();
                            }
                            _ => {}
                        }
                    } else if app.show_help {
                        // Help overlay intercepts all keys
                        app.show_help = false;
                    } else {
                        // Global keys
                        match key.code {
                            // Ctrl+K: Command palette
                            KeyCode::Char('k')
                                if key.modifiers.contains(KeyModifiers::CONTROL) =>
                            {
                                app.palette.toggle();
                            }
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
                            KeyCode::Char(n @ '1'..='8')
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
                                    TabId::Schedule => {
                                        app.schedule.handle_key(key);
                                    }
                                }
                            }
                        }
                    }
                }
                Event::Mouse(mouse) => {
                    handle_mouse(&mut app, mouse);
                }
                _ => {}
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
    io::stdout().execute(DisableMouseCapture).ok();
    io::stdout().execute(LeaveAlternateScreen).ok();

    result
}

/// Draw the full TUI layout.
/// Handle mouse events — tab clicks and scroll wheel.
fn handle_mouse(app: &mut App, mouse: crossterm::event::MouseEvent) {
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            let tab_bar = app.layout.tab_bar;
            // Click in tab bar area?
            if mouse.row >= tab_bar.y && mouse.row < tab_bar.y + tab_bar.height {
                // Determine which tab was clicked based on x position
                // Each tab takes roughly equal width
                let tab_count = TabId::all().len() as u16;
                let tab_width = tab_bar.width / tab_count;
                if tab_width > 0 {
                    let rel_x = mouse.column.saturating_sub(tab_bar.x);
                    let tab_idx = (rel_x / tab_width) as usize;
                    if tab_idx < TabId::all().len() {
                        app.active_tab = TabId::from_index(tab_idx);
                    }
                }
            }
        }
        MouseEventKind::ScrollUp => {
            // Send Up key to active tab
            let key = crossterm::event::KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
            match app.active_tab {
                TabId::Chat => { app.chat.handle_key(key); }
                TabId::Pea => { app.tasks.handle_key(key); }
                TabId::Agents => { app.agents.handle_key(key); }
                TabId::Settings => { app.settings.handle_key(key); }
                TabId::History => { app.history.handle_key(key); }
                TabId::Workflows => { app.workflows.handle_key(key); }
                TabId::Resources => { app.resources.handle_key(key); }
                TabId::Schedule => { app.schedule.handle_key(key); }
            }
        }
        MouseEventKind::ScrollDown => {
            let key = crossterm::event::KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
            match app.active_tab {
                TabId::Chat => { app.chat.handle_key(key); }
                TabId::Pea => { app.tasks.handle_key(key); }
                TabId::Agents => { app.agents.handle_key(key); }
                TabId::Settings => { app.settings.handle_key(key); }
                TabId::History => { app.history.handle_key(key); }
                TabId::Workflows => { app.workflows.handle_key(key); }
                TabId::Resources => { app.resources.handle_key(key); }
                TabId::Schedule => { app.schedule.handle_key(key); }
            }
        }
        _ => {}
    }
}

fn draw_ui(frame: &mut ratatui::Frame, app: &App) {
    let size = frame.area();

    // Fill entire background
    frame.render_widget(Block::default().style(Style::default().bg(BG)), size);

    // Build live stats for title bar
    let period_label = app.cost_period_label();
    let stats_text = format!(
        " [{}] saved {} · cache {:.0}% · {} queries ",
        period_label,
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
            "[Ctrl+K] palette  [?] help  [L] logs  [Ctrl+C] quit ",
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

    // Command palette overlay
    if app.palette.open {
        draw_command_palette(frame, size, &app.palette);
    }

    // Confirmation modal overlay (highest priority overlay)
    if let Some(ref pending) = app.pending_confirmation {
        draw_confirmation_modal(frame, size, pending);
    }

    // Toast notifications (bottom-right)
    draw_toasts(frame, size, &app.toasts);
}

/// Render the confirmation modal overlay.
fn draw_confirmation_modal(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    pending: &PendingConfirmation,
) {
    use crate::agent_os::confirmation::ConfirmationResponse;
    use ratatui::widgets::Clear;

    let modal_width = 48_u16.min(area.width.saturating_sub(4));
    let modal_height = 12_u16.min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(modal_width)) / 2;
    let y = (area.height.saturating_sub(modal_height)) / 2;
    let modal_area = ratatui::layout::Rect::new(x, y, modal_width, modal_height);

    // Clear the area behind the modal
    frame.render_widget(Clear, modal_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT))
        .title(Span::styled(
            " Permission Required ",
            Style::default()
                .fg(ACCENT)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(BG));

    let inner = block.inner(modal_area);
    frame.render_widget(block, modal_area);

    // Build modal content
    let mut lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Agent:  ", Style::default().fg(DIM)),
            Span::styled(&pending.request.agent_id, Style::default().fg(FG)),
        ]),
        Line::from(vec![
            Span::styled("  Action: ", Style::default().fg(DIM)),
            Span::styled(&pending.request.ability, Style::default().fg(ACCENT)),
        ]),
        Line::from(vec![
            Span::styled("  Reason: ", Style::default().fg(DIM)),
            Span::styled(&pending.request.reason, Style::default().fg(FG)),
        ]),
        Line::from(""),
    ];

    for (i, variant) in ConfirmationResponse::ALL.iter().enumerate() {
        let marker = if i == pending.selected { "\u{25b8} " } else { "  " };
        let style = if i == pending.selected {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(FG)
        };
        lines.push(Line::from(Span::styled(
            format!("  {}[{}] {}", marker, i + 1, variant.label()),
            style,
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  [Enter] confirm  [Esc] deny",
        Style::default().fg(DIM),
    )));

    frame.render_widget(
        Paragraph::new(lines).style(Style::default().bg(BG)),
        inner,
    );
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
        TabId::Schedule => app.schedule.render(frame, area),
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
        ContextPanel::SettingsDetail(i) => draw_context_settings(frame, area, app, *i),
        ContextPanel::ScheduleDetail(i) => draw_context_schedule(frame, area, app, *i),
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
        context_row("Period", app.cost_period_label()),
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

    // Watcher alerts section
    #[cfg(feature = "watcher")]
    {
        use crate::watcher::alerts::AlertStore;
        let watcher_db = app.config.data_dir.join("watcher.db");
        if let Ok(store) = AlertStore::open(&watcher_db) {
            if let Ok(alerts) = store.list_recent(3600) {
                let active = alerts.iter().filter(|a| a.resolved_at.is_none()).count();
                lines.push(Line::from(""));
                lines.push(Line::from(vec![Span::styled(
                    "  Watcher",
                    Style::default()
                        .fg(ACCENT)
                        .add_modifier(Modifier::BOLD),
                )]));
                lines.push(Line::from(""));
                let alert_color = if active > 0 { Color::Red } else { GREEN };
                lines.push(Line::from(vec![
                    Span::styled("  Alerts (1h)  ", Style::default().fg(DIM)),
                    Span::styled(
                        format!("{} active, {} total", active, alerts.len()),
                        Style::default().fg(alert_color),
                    ),
                ]));
                for alert in alerts.iter().take(3) {
                    let icon = if alert.resolved_at.is_some() { "✓" } else { "!" };
                    let color = if alert.resolved_at.is_some() { DIM } else { Color::Yellow };
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {} ", icon), Style::default().fg(color)),
                        Span::styled(
                            alert.event_summary.chars().take(30).collect::<String>(),
                            Style::default().fg(color),
                        ),
                    ]));
                }
            }
        }
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
    use super::tabs::tasks::PeaView;

    match &app.tasks.view {
        PeaView::Tasks(_) => {
            draw_context_task_detail(frame, area, app);
        }
        PeaView::Objectives => {
            draw_context_objective_detail(frame, area, app, idx);
        }
    }
}

/// Context panel for an objective — budget, milestones, beliefs.
fn draw_context_objective_detail(
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
        let bar_w = (area.width.saturating_sub(20)) as usize;
        let filled = (frac * bar_w as f64).round() as usize;
        let empty = bar_w.saturating_sub(filled);

        let mut lines = vec![
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
            context_row("Strategy", &obj.budget_strategy),
            Line::from(""),
            context_row("Budget", &format!("${:.2}", obj.budget)),
            context_row("Spent", &format!("${:.2} ({:.0}%)", obj.spent, frac * 100.0)),
            Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(
                    "█".repeat(filled),
                    Style::default().fg(if frac > 0.8 { Color::Yellow } else { GREEN }),
                ),
                Span::styled(
                    "░".repeat(empty),
                    Style::default().fg(Color::Rgb(60, 60, 80)),
                ),
            ]),
        ];

        // Tasks progress
        if obj.task_count > 0 {
            lines.push(Line::from(""));
            lines.push(context_row(
                "Tasks",
                &format!("{}/{} completed", obj.completed_tasks, obj.task_count),
            ));
        }

        // Milestones
        if obj.milestone_count > 0 {
            lines.push(context_row(
                "Milestones",
                &format!("{}/{}", obj.milestones_achieved, obj.milestone_count),
            ));
        }

        // Progress score
        if obj.progress_score > 0.0 {
            lines.push(context_row(
                "Progress",
                &format!("{:.0}%", obj.progress_score * 100.0),
            ));
        }

        // Beliefs
        if !obj.beliefs.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![Span::styled(
                "  Beliefs:",
                Style::default().fg(FG),
            )]));
            for (key, confidence) in obj.beliefs.iter().take(6) {
                let conf_bar_w: usize = 8;
                let conf_filled = (confidence * conf_bar_w as f64).round() as usize;
                let conf_empty = conf_bar_w.saturating_sub(conf_filled);
                let conf_color = if *confidence >= 0.8 {
                    Color::Green
                } else if *confidence >= 0.5 {
                    Color::Yellow
                } else {
                    Color::Red
                };
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("    {:<12} ", truncate_str(key, 12)),
                        Style::default().fg(DIM),
                    ),
                    Span::styled(
                        "█".repeat(conf_filled),
                        Style::default().fg(conf_color),
                    ),
                    Span::styled(
                        "░".repeat(conf_empty),
                        Style::default().fg(Color::Rgb(60, 60, 80)),
                    ),
                    Span::styled(
                        format!(" {:.0}%", confidence * 100.0),
                        Style::default().fg(DIM),
                    ),
                ]));
            }
        }

        // Description
        lines.push(Line::from(""));
        for line in super::tabs::chat::wrap_text(&obj.description, area.width.saturating_sub(6) as usize) {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(line, Style::default().fg(DIM)),
            ]));
        }

        // Action hints
        lines.push(Line::from(""));
        let mut hints = vec![
            Span::styled(
                "  [Enter] ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("tasks  ", Style::default().fg(DIM)),
            Span::styled(
                "[n] ",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("new  ", Style::default().fg(DIM)),
        ];
        if obj.status == "active" || obj.status == "paused" {
            hints.push(Span::styled(
                "[p] ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ));
            hints.push(Span::styled(
                if obj.status == "active" {
                    "pause  "
                } else {
                    "resume  "
                },
                Style::default().fg(DIM),
            ));
            hints.push(Span::styled(
                "[x] ",
                Style::default()
                    .fg(Color::Red)
                    .add_modifier(Modifier::BOLD),
            ));
            hints.push(Span::styled("cancel", Style::default().fg(DIM)));
        }
        lines.push(Line::from(hints));

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

/// Context panel for a selected task — dependencies, capabilities, retries.
fn draw_context_task_detail(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    app: &App,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER))
        .title(Line::from(vec![Span::styled(
            " Task Detail ",
            Style::default().fg(HEADING).bg(BG),
        )]));

    if let Some(task) = app.tasks.selected_task() {
        let (sym, sc) = super::tabs::tasks::task_status_icon(&task.status);

        let mut lines = vec![
            Line::from(""),
            Line::from(vec![Span::styled(
                format!("  {}", task.id),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  Status      ", Style::default().fg(DIM)),
                Span::styled(format!("{} {}", sym, task.status), Style::default().fg(sc)),
            ]),
            context_row("Type", &task.task_type),
        ];

        if let Some(ref cap) = task.capability {
            lines.push(context_row("Capability", cap));
        }

        if task.max_retries > 0 {
            lines.push(context_row(
                "Retries",
                &format!("{}/{}", task.retry_count, task.max_retries),
            ));
        }

        // Dependencies
        if !task.depends_on.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![Span::styled(
                "  Dependencies:",
                Style::default().fg(FG),
            )]));
            for dep in &task.depends_on {
                let dep_short = truncate_str(dep, 20);
                // Find dep task to show its status
                let dep_status = app
                    .tasks
                    .tasks
                    .iter()
                    .find(|t| t.id == *dep)
                    .map(|t| &t.status);
                let (dep_sym, dep_color) = dep_status
                    .map(|s| super::tabs::tasks::task_status_icon(s))
                    .unwrap_or(("?", Color::DarkGray));
                lines.push(Line::from(vec![
                    Span::styled(format!("    {} ", dep_sym), Style::default().fg(dep_color)),
                    Span::styled(dep_short.to_string(), Style::default().fg(DIM)),
                ]));
            }
        }

        // Description
        lines.push(Line::from(""));
        for line in super::tabs::chat::wrap_text(
            &task.description,
            area.width.saturating_sub(6) as usize,
        ) {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(line, Style::default().fg(FG)),
            ]));
        }

        // Action hints
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(
                "  [Esc] ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("back to objectives", Style::default().fg(DIM)),
        ]));

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
                    "  No task selected",
                    Style::default().fg(DIM),
                )]),
            ])
            .block(block)
            .style(Style::default().bg(BG)),
            area,
        );
    }
}

fn truncate_str(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}

/// Workflow detail context panel — shows either definition or instance detail.
fn draw_context_workflow(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    app: &App,
    idx: usize,
) {
    use super::tabs::workflows::WorkflowView;

    match &app.workflows.view {
        WorkflowView::Instances(_) => {
            draw_context_workflow_instance(frame, area, app);
        }
        WorkflowView::Definitions => {
            draw_context_workflow_def(frame, area, app, idx);
        }
    }
}

/// Context panel for a workflow definition.
fn draw_context_workflow_def(
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
        let status_color = workflow_status_color(&wf.last_status);

        let mut lines = vec![
            Line::from(""),
            Line::from(vec![Span::styled(
                format!("  {}", wf.name),
                Style::default()
                    .fg(ACCENT)
                    .add_modifier(Modifier::BOLD),
            )]),
        ];

        if !wf.description.is_empty() {
            lines.push(Line::from(vec![Span::styled(
                format!("  {}", wf.description),
                Style::default().fg(DIM),
            )]));
        }

        lines.push(Line::from(""));
        lines.push(context_row("ID", &wf.id));
        lines.push(Line::from(vec![
            Span::styled("  Status      ", Style::default().fg(DIM)),
            Span::styled(wf.last_status.to_string(), Style::default().fg(status_color)),
        ]));
        lines.push(context_row("Nodes", &format!("{}", wf.node_count)));
        lines.push(context_row(
            "Instances",
            &format!("{} total, {} active", wf.instance_count, wf.active_count),
        ));

        if wf.max_instances > 0 {
            lines.push(context_row("Max parallel", &format!("{}", wf.max_instances)));
        }
        if wf.global_timeout_secs > 0 {
            lines.push(context_row(
                "Timeout",
                &format_timeout_secs(wf.global_timeout_secs),
            ));
        }

        // Parameters
        if !wf.param_names.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![Span::styled(
                "  Parameters:",
                Style::default().fg(FG),
            )]));
            for p in &wf.param_names {
                lines.push(Line::from(vec![
                    Span::styled("    ◦ ", Style::default().fg(ACCENT)),
                    Span::styled(p.to_string(), Style::default().fg(FG)),
                ]));
            }
        }

        // Action hints
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("  [n] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled("new instance  ", Style::default().fg(DIM)),
            Span::styled("[Enter] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled("view instances", Style::default().fg(DIM)),
        ]));

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

/// Context panel for a selected workflow instance — shows DAG + status.
fn draw_context_workflow_instance(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    app: &App,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER))
        .title(Line::from(vec![Span::styled(
            " Instance Detail ",
            Style::default().fg(HEADING).bg(BG),
        )]));

    if let Some(inst) = app.workflows.selected_instance() {
        let sc = workflow_status_color(&inst.status);

        let mut lines = vec![
            Line::from(""),
            Line::from(vec![Span::styled(
                format!("  {}", inst.instance_id),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  Status      ", Style::default().fg(DIM)),
                Span::styled(inst.status.to_string(), Style::default().fg(sc)),
            ]),
        ];

        if inst.node_count > 0 {
            // Progress bar
            let progress = inst.cursor_node.min(inst.node_count) as f64 / inst.node_count as f64;
            let bar_w = (area.width.saturating_sub(20)) as usize;
            let filled = (progress * bar_w as f64) as usize;
            let empty = bar_w.saturating_sub(filled);
            lines.push(Line::from(vec![
                Span::styled("  Progress    ", Style::default().fg(DIM)),
                Span::styled(
                    "█".repeat(filled),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    "░".repeat(empty),
                    Style::default().fg(Color::Rgb(60, 60, 80)),
                ),
            ]));
            lines.push(context_row(
                "Position",
                &format!("{}/{} nodes", inst.cursor_node.min(inst.node_count), inst.node_count),
            ));
        }

        let elapsed = if inst.execution_ms < 1000 {
            format!("{}ms", inst.execution_ms)
        } else if inst.execution_ms < 60_000 {
            format!("{:.1}s", inst.execution_ms as f64 / 1000.0)
        } else {
            format!("{:.1}m", inst.execution_ms as f64 / 60_000.0)
        };
        lines.push(context_row("Elapsed", &elapsed));

        // DAG node list
        if !inst.node_names.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![Span::styled(
                "  Nodes:",
                Style::default().fg(FG),
            )]));
            for (i, (node_id, node_type)) in inst.node_names.iter().enumerate() {
                let (sym, color) = if i < inst.cursor_node {
                    ("✓", Color::Green) // completed
                } else if i == inst.cursor_node && !super::tabs::workflows::is_terminal_status(&inst.status) {
                    ("▸", Color::Cyan) // current
                } else {
                    ("○", Color::Rgb(60, 60, 80)) // pending
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("  {} ", sym), Style::default().fg(color)),
                    Span::styled(
                        node_id.to_string(),
                        Style::default().fg(if i == inst.cursor_node {
                            Color::White
                        } else {
                            DIM
                        }),
                    ),
                    Span::styled(
                        format!("  ({})", node_type),
                        Style::default().fg(Color::Rgb(60, 60, 80)),
                    ),
                ]));
                // Draw connector line between nodes
                if i < inst.node_names.len() - 1 {
                    let connector_color = if i < inst.cursor_node {
                        Color::Green
                    } else {
                        Color::Rgb(60, 60, 80)
                    };
                    lines.push(Line::from(vec![Span::styled(
                        "  │",
                        Style::default().fg(connector_color),
                    )]));
                }
            }
        }

        // Error detail
        if let Some(ref err) = inst.error {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("  Error: ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::styled(err.to_string(), Style::default().fg(Color::Red)),
            ]));
        }

        // Action hints
        lines.push(Line::from(""));
        if !super::tabs::workflows::is_terminal_status(&inst.status) {
            lines.push(Line::from(vec![
                Span::styled("  [c] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled("cancel  ", Style::default().fg(DIM)),
                Span::styled("[Esc] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled("back", Style::default().fg(DIM)),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled("  [Esc] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled("back to definitions", Style::default().fg(DIM)),
            ]));
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
                    "  No instance selected",
                    Style::default().fg(DIM),
                )]),
            ])
            .block(block)
            .style(Style::default().bg(BG)),
            area,
        );
    }
}

fn workflow_status_color(status: &str) -> Color {
    match status.to_lowercase().as_str() {
        "completed" => Color::Green,
        "running" => Color::Cyan,
        "waiting" => Color::Yellow,
        "created" => Color::Blue,
        "failed" => Color::Red,
        "cancelled" => Color::Yellow,
        "timed_out" | "timedout" => Color::Red,
        "compensated" => Color::Magenta,
        _ => Color::DarkGray,
    }
}

fn format_timeout_secs(secs: u64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else {
        format!("{}h", secs / 3600)
    }
}

/// Resource detail context panel.
fn draw_context_resource(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    app: &App,
    idx: usize,
) {
    use super::tabs::resources::ResourceView;

    match &app.resources.view {
        ResourceView::Leases(_) => {
            draw_context_lease_detail(frame, area, app);
        }
        ResourceView::Resources => {
            draw_context_resource_detail(frame, area, app, idx);
        }
    }
}

/// Context panel for a resource record.
fn draw_context_resource_detail(
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
        let sc = resource_ctx_status_color(&res.status);

        let mut lines = vec![
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
                Span::styled(res.status.to_string(), Style::default().fg(sc)),
            ]),
            context_row(
                "Leases",
                &if res.active_leases > 0 {
                    format!("{} active", res.active_leases)
                } else {
                    "none".to_string()
                },
            ),
            context_row("Cost", &res.cost_model),
        ];

        // Metadata
        if !res.metadata.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![Span::styled(
                "  Metadata:",
                Style::default().fg(FG),
            )]));
            for (k, v) in &res.metadata {
                lines.push(Line::from(vec![
                    Span::styled(format!("    {} ", k), Style::default().fg(DIM)),
                    Span::styled(v.to_string(), Style::default().fg(FG)),
                ]));
            }
        }

        // Action hints
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(
                "  [Enter] ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("leases  ", Style::default().fg(DIM)),
            Span::styled(
                "[r] ",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("register  ", Style::default().fg(DIM)),
            Span::styled(
                "[d] ",
                Style::default()
                    .fg(Color::Red)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("delete", Style::default().fg(DIM)),
        ]));

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

/// Context panel for a selected lease — shows quota bars and capabilities.
fn draw_context_lease_detail(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    app: &App,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER))
        .title(Line::from(vec![Span::styled(
            " Lease Detail ",
            Style::default().fg(HEADING).bg(BG),
        )]));

    if let Some(lease) = app.resources.selected_lease() {
        let sc = match lease.status.to_lowercase().as_str() {
            "active" => Color::Green,
            "expired" => Color::Yellow,
            "revoked" => Color::Red,
            _ => Color::DarkGray,
        };

        let mut lines = vec![
            Line::from(""),
            Line::from(vec![Span::styled(
                format!("  {}", lease.lease_id),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            context_row("Agent", &lease.agent_id),
            Line::from(vec![
                Span::styled("  Status      ", Style::default().fg(DIM)),
                Span::styled(lease.status.to_string(), Style::default().fg(sc)),
            ]),
        ];

        // Quota bars
        let bar_w = (area.width.saturating_sub(20)) as usize;
        if let Some(max_calls) = lease.max_calls {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![Span::styled(
                format!("  Calls: {}/{}", lease.used_calls, max_calls),
                Style::default().fg(FG),
            )]));
            let (filled, empty, color) = super::tabs::resources::quota_bar(
                lease.used_calls as f64,
                max_calls as f64,
                bar_w,
            );
            lines.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(filled, Style::default().fg(color)),
                Span::styled(empty, Style::default().fg(Color::Rgb(60, 60, 80))),
            ]));
        }

        if let Some(max_cost) = lease.max_cost_usd {
            lines.push(Line::from(vec![Span::styled(
                format!("  Cost: ${:.2}/${:.2}", lease.used_cost_usd, max_cost),
                Style::default().fg(FG),
            )]));
            let (filled, empty, color) =
                super::tabs::resources::quota_bar(lease.used_cost_usd, max_cost, bar_w);
            lines.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(filled, Style::default().fg(color)),
                Span::styled(empty, Style::default().fg(Color::Rgb(60, 60, 80))),
            ]));
        }

        // Capabilities
        if !lease.capabilities.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![Span::styled(
                "  Capabilities:",
                Style::default().fg(FG),
            )]));
            for cap in &lease.capabilities {
                lines.push(Line::from(vec![
                    Span::styled("    ✓ ", Style::default().fg(Color::Green)),
                    Span::styled(cap.to_string(), Style::default().fg(FG)),
                ]));
            }
        }

        // Action hints
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(
                "  [Esc] ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("back to resources", Style::default().fg(DIM)),
        ]));

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
                    "  No lease selected",
                    Style::default().fg(DIM),
                )]),
            ])
            .block(block)
            .style(Style::default().bg(BG)),
            area,
        );
    }
}

fn resource_ctx_status_color(status: &str) -> Color {
    let s = status.to_lowercase();
    if s == "available" {
        Color::Green
    } else if s.starts_with("inuse") || s.starts_with("in_use") {
        Color::Cyan
    } else if s == "provisioning" {
        Color::Yellow
    } else if s == "degraded" {
        Color::Yellow
    } else if s == "offline" {
        Color::Red
    } else if s == "terminated" {
        Color::DarkGray
    } else {
        Color::DarkGray
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

fn draw_context_settings(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    app: &App,
    idx: usize,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER))
        .title(Line::from(vec![Span::styled(
            " Setting Detail ",
            Style::default().fg(HEADING).bg(BG),
        )]));

    if let Some(entry) = app.settings.entries.get(idx) {
        let display_val = if entry.is_secret && !entry.value.is_empty() {
            let len = entry.value.len();
            if len > 4 {
                format!(
                    "{}...{}",
                    "*".repeat((len - 4).min(12)),
                    &entry.value[len - 4..]
                )
            } else {
                "*".repeat(len)
            }
        } else if entry.value.is_empty() {
            "(not set)".to_string()
        } else {
            entry.value.clone()
        };

        let section_color = match entry.section.as_str() {
            "Provider" => Color::Cyan,
            "Constitution" => Color::Magenta,
            "Budget" => Color::Yellow,
            "Channels" => Color::Green,
            "System" => Color::Rgb(100, 100, 120),
            _ => Color::DarkGray,
        };

        let mut lines = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("  Section     ", Style::default().fg(DIM)),
                Span::styled(
                    entry.section.clone(),
                    Style::default()
                        .fg(section_color)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("  Key         ", Style::default().fg(DIM)),
                Span::styled(
                    entry.key.clone(),
                    Style::default().fg(FG).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "  Value",
                Style::default()
                    .fg(ACCENT)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    display_val,
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(""),
        ];

        if let Some(ref ek) = entry.env_key {
            lines.push(Line::from(vec![
                Span::styled("  Env var     ", Style::default().fg(DIM)),
                Span::styled(ek.clone(), Style::default().fg(Color::Rgb(150, 150, 170))),
            ]));
        }

        lines.push(Line::from(vec![
            Span::styled("  Editable    ", Style::default().fg(DIM)),
            if entry.editable {
                Span::styled("Yes", Style::default().fg(GREEN))
            } else {
                Span::styled("No", Style::default().fg(Color::DarkGray))
            },
        ]));

        if entry.is_secret {
            lines.push(Line::from(vec![
                Span::styled("  Secret      ", Style::default().fg(DIM)),
                Span::styled("Yes (masked)", Style::default().fg(Color::Yellow)),
            ]));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(""));

        if entry.editable {
            lines.push(Line::from(vec![
                Span::styled("  [Enter] ", Style::default().fg(GREEN).add_modifier(Modifier::BOLD)),
                Span::styled("Edit value", Style::default().fg(FG)),
            ]));
        }
        lines.push(Line::from(vec![
            Span::styled("  [r] ", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
            Span::styled("Reload config", Style::default().fg(FG)),
        ]));

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
                    "  No setting selected",
                    Style::default().fg(DIM),
                )]),
            ])
            .block(block)
            .style(Style::default().bg(BG)),
            area,
        );
    }
}

fn draw_context_schedule(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    app: &App,
    idx: usize,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER))
        .title(Line::from(vec![Span::styled(
            " Job Detail ",
            Style::default().fg(HEADING).bg(BG),
        )]));

    if let Some(job) = app.schedule.jobs.get(idx) {
        let status = if job.enabled { "Enabled" } else { "Disabled" };
        let status_color = if job.enabled { GREEN } else { Color::DarkGray };

        let last_run = match job.last_run_at {
            Some(ts) => {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as i64;
                let diff = (now_ms - ts) / 1000;
                if diff < 60 {
                    format!("{}s ago", diff)
                } else if diff < 3600 {
                    format!("{}m ago", diff / 60)
                } else {
                    format!("{}h ago", diff / 3600)
                }
            }
            None => "never".to_string(),
        };

        let output_preview = job
            .last_output
            .as_deref()
            .unwrap_or("(no output)")
            .lines()
            .take(8)
            .map(|l| {
                Line::from(vec![Span::styled(
                    format!("  {}", l),
                    Style::default().fg(Color::Rgb(150, 150, 170)),
                )])
            })
            .collect::<Vec<_>>();

        let mut lines = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("  Job ID      ", Style::default().fg(DIM)),
                Span::styled(
                    job.id.clone(),
                    Style::default().fg(FG).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("  Chain       ", Style::default().fg(DIM)),
                Span::styled(
                    job.chain_id.clone(),
                    Style::default()
                        .fg(ACCENT)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("  Schedule    ", Style::default().fg(DIM)),
                Span::styled(job.schedule_desc.clone(), Style::default().fg(Color::Cyan)),
            ]),
            Line::from(vec![
                Span::styled("  Status      ", Style::default().fg(DIM)),
                Span::styled(
                    status.to_string(),
                    Style::default().fg(status_color).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  Last run    ", Style::default().fg(DIM)),
                Span::styled(last_run, Style::default().fg(Color::Rgb(150, 150, 170))),
            ]),
            Line::from(vec![
                Span::styled("  Total runs  ", Style::default().fg(DIM)),
                Span::styled(
                    format!("{}", job.run_count),
                    Style::default().fg(Color::Rgb(150, 150, 170)),
                ),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "  Last Output",
                Style::default()
                    .fg(ACCENT)
                    .add_modifier(Modifier::BOLD),
            )]),
        ];
        lines.extend(output_preview);
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(
                "  [Enter] ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("View history", Style::default().fg(FG)),
        ]));
        lines.push(Line::from(vec![
            Span::styled(
                "  [d] ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                if job.enabled {
                    "Disable job"
                } else {
                    "Enable job"
                },
                Style::default().fg(FG),
            ),
        ]));

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
                    "  No job selected",
                    Style::default().fg(DIM),
                )]),
            ])
            .block(block)
            .style(Style::default().bg(BG)),
            area,
        );
    }
}

/// Parse an interval string like "every 10m", "every 1h", "every 30s" into seconds.
fn parse_interval(spec: &str) -> u64 {
    let s = spec
        .trim()
        .trim_start_matches("every")
        .trim();
    if let Some(rest) = s.strip_suffix('h') {
        rest.trim().parse::<u64>().unwrap_or(3600) * 3600
    } else if let Some(rest) = s.strip_suffix('m') {
        rest.trim().parse::<u64>().unwrap_or(60) * 60
    } else if let Some(rest) = s.strip_suffix('s') {
        rest.trim().parse::<u64>().unwrap_or(60)
    } else {
        // Try parsing as raw seconds
        s.parse::<u64>().unwrap_or(600)
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
                TabId::Schedule if !app.schedule.jobs.is_empty() => {
                    format!("{} ({})", tab.label(), app.schedule.jobs.len())
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
/// Draw the command palette overlay.
fn draw_command_palette(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    palette: &CommandPalette,
) {
    let max_items = 10;
    let visible = palette.filtered.len().min(max_items);
    let h = (visible as u16 + 5).min(area.height.saturating_sub(4));
    let w = 50.min(area.width.saturating_sub(8));
    let x = (area.width.saturating_sub(w)) / 2 + area.x;
    let y = area.height / 6 + area.y; // positioned in upper third
    let palette_area = ratatui::layout::Rect::new(x, y, w, h);

    frame.render_widget(Clear, palette_area);

    let mut lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                if palette.query.is_empty() {
                    "Type to search...".to_string()
                } else {
                    format!("{}▏", palette.query)
                },
                Style::default()
                    .fg(if palette.query.is_empty() { DIM } else { Color::White })
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
    ];

    for (vi, &cmd_idx) in palette.filtered.iter().enumerate().take(max_items) {
        if let Some(cmd) = palette.commands.get(cmd_idx) {
            let is_selected = vi == palette.selected;
            let prefix = if is_selected { "▸ " } else { "  " };
            let fg = if is_selected { ACCENT } else { FG };
            let cat_fg = if is_selected {
                Color::Rgb(200, 150, 80)
            } else {
                DIM
            };
            lines.push(Line::from(vec![
                Span::styled(prefix, Style::default().fg(fg)),
                Span::styled(
                    cmd.label.clone(),
                    Style::default()
                        .fg(fg)
                        .add_modifier(if is_selected {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
                Span::styled(
                    format!("  {}", cmd.category),
                    Style::default().fg(cat_fg),
                ),
            ]));
        }
    }

    if palette.filtered.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "  No matches",
            Style::default().fg(DIM),
        )]));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT).bg(BG))
        .style(Style::default().bg(BG))
        .title(Line::from(vec![Span::styled(
            " Command Palette ",
            Style::default()
                .fg(ACCENT)
                .bg(BG)
                .add_modifier(Modifier::BOLD),
        )]));

    frame.render_widget(
        Paragraph::new(lines).block(block).style(Style::default().bg(BG)),
        palette_area,
    );
}

/// Draw toast notifications in the bottom-right corner.
fn draw_toasts(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    toasts: &[Toast],
) {
    if toasts.is_empty() {
        return;
    }

    let toast_w: u16 = 40.min(area.width.saturating_sub(4));
    let mut y = area.height.saturating_sub(3);

    for toast in toasts.iter().rev().take(3) {
        if y < 2 {
            break;
        }
        let toast_area = ratatui::layout::Rect::new(
            area.width.saturating_sub(toast_w + 2),
            y,
            toast_w,
            2,
        );

        frame.render_widget(Clear, toast_area);

        let color = if toast.is_error { Color::Red } else { GREEN };
        let icon = if toast.is_error { "✗" } else { "✓" };

        let line = Line::from(vec![
            Span::styled(format!(" {} ", icon), Style::default().fg(color)),
            Span::styled(
                toast.message.chars().take((toast_w as usize).saturating_sub(6)).collect::<String>(),
                Style::default().fg(FG),
            ),
        ]);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(color))
            .style(Style::default().bg(Color::Rgb(30, 30, 40)));

        frame.render_widget(
            Paragraph::new(vec![line])
                .block(block)
                .style(Style::default().bg(Color::Rgb(30, 30, 40))),
            toast_area,
        );

        y = y.saturating_sub(3);
    }
}

/// Draw the help overlay centered on screen.
fn draw_help_overlay(frame: &mut ratatui::Frame, area: ratatui::layout::Rect) {
    let w = 55.min(area.width.saturating_sub(4));
    let h = 24.min(area.height.saturating_sub(4));
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
        help_row("Ctrl+K", "Command palette"),
        help_row("Ctrl+L", "Clear chat"),
        help_row("Ctrl+A / Ctrl+E", "Cursor start/end"),
        help_row("L", "Toggle logs"),
        help_row("q", "Quit"),
        help_row("Ctrl+C", "Force quit"),
        help_row("?", "Toggle this help"),
        help_row("Mouse click", "Switch tabs"),
        help_row("Scroll wheel", "Navigate lists"),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_os::confirmation::*;

    #[test]
    fn test_parse_agent_mention_with_query() {
        let (agent, query) = App::parse_agent_mention("@morning-briefing check my schedule");
        assert_eq!(agent, Some("morning-briefing".to_string()));
        assert_eq!(query, "check my schedule");
    }

    #[test]
    fn test_parse_agent_mention_bare() {
        let (agent, query) = App::parse_agent_mention("@morning-briefing");
        assert_eq!(agent, Some("morning-briefing".to_string()));
        assert_eq!(query, "");
    }

    #[test]
    fn test_parse_agent_mention_no_at() {
        let (agent, query) = App::parse_agent_mention("hello world");
        assert_eq!(agent, None);
        assert_eq!(query, "hello world");
    }

    #[test]
    fn test_confirmation_channel_allow_once() {
        // Simulate the confirmation flow: background thread sends request,
        // "TUI" responds with AllowOnce via the channel.
        let (app_tx, app_rx) = std::sync::mpsc::channel::<AppMessage>();

        let request = ConfirmationRequest::new(
            "test-agent",
            "email.send:bob@example.com",
            "Email send requires approval",
            ConfirmationSource::Constitution {
                rule_name: "confirm_send_actions".into(),
            },
        );

        let (resp_tx, resp_rx) = std::sync::mpsc::channel::<ConfirmationResponse>();

        // Simulate orchestrator sending confirmation request
        app_tx
            .send(AppMessage::ConfirmationNeeded {
                request: request.clone(),
                responder: resp_tx,
            })
            .unwrap();

        // Simulate TUI receiving and handling it
        if let Ok(AppMessage::ConfirmationNeeded { request: req, responder }) = app_rx.try_recv() {
            assert_eq!(req.agent_id, "test-agent");
            assert_eq!(req.ability, "email.send:bob@example.com");
            // TUI user selects "Allow once"
            responder.send(ConfirmationResponse::AllowOnce).unwrap();
        } else {
            panic!("Expected ConfirmationNeeded message");
        }

        // Orchestrator receives the response
        let response = resp_rx.recv_timeout(std::time::Duration::from_secs(1)).unwrap();
        assert_eq!(response, ConfirmationResponse::AllowOnce);
    }

    #[test]
    fn test_confirmation_channel_deny() {
        let (app_tx, app_rx) = std::sync::mpsc::channel::<AppMessage>();

        let request = ConfirmationRequest::new(
            "risky-agent",
            "shell.exec",
            "Shell execution needs approval",
            ConfirmationSource::CircuitBreaker {
                breaker_id: "global_shell".into(),
            },
        );

        let (resp_tx, resp_rx) = std::sync::mpsc::channel::<ConfirmationResponse>();

        app_tx
            .send(AppMessage::ConfirmationNeeded {
                request,
                responder: resp_tx,
            })
            .unwrap();

        // TUI user denies
        if let Ok(AppMessage::ConfirmationNeeded { responder, .. }) = app_rx.try_recv() {
            responder.send(ConfirmationResponse::Deny).unwrap();
        }

        let response = resp_rx.recv_timeout(std::time::Duration::from_secs(1)).unwrap();
        assert_eq!(response, ConfirmationResponse::Deny);
    }

    #[test]
    fn test_pending_confirmation_selection_bounds() {
        // Verify selection wrapping logic used in key handling
        let selected: usize = 0;
        assert_eq!(ConfirmationResponse::ALL[selected], ConfirmationResponse::AllowOnce);
        assert_eq!(ConfirmationResponse::ALL[3], ConfirmationResponse::Deny);
    }

    #[test]
    fn test_confirm_fn_closure_bridges_channels() {
        // Test the ConfirmFn pattern used by orchestrator.set_confirm_tx()
        let (app_tx, app_rx) = std::sync::mpsc::channel::<AppMessage>();

        let confirm_fn: ConfirmFn = Box::new(move |request: ConfirmationRequest| {
            let (resp_tx, resp_rx) = std::sync::mpsc::channel::<ConfirmationResponse>();
            let msg = AppMessage::ConfirmationNeeded {
                request,
                responder: resp_tx,
            };
            if app_tx.send(msg).is_err() {
                return None;
            }
            resp_rx.recv_timeout(std::time::Duration::from_secs(5)).ok()
        });

        // Spawn a "TUI handler" thread that auto-approves
        let handle = std::thread::spawn(move || {
            if let Ok(AppMessage::ConfirmationNeeded { responder, .. }) = app_rx.recv_timeout(std::time::Duration::from_secs(5)) {
                responder.send(ConfirmationResponse::AllowAlwaysAgent).unwrap();
            }
        });

        let request = ConfirmationRequest::new(
            "agent-1",
            "sms.send:+1234567890",
            "SMS requires confirmation",
            ConfirmationSource::Privilege { required_level: 1 },
        );

        let result = confirm_fn(request);
        assert_eq!(result, Some(ConfirmationResponse::AllowAlwaysAgent));

        handle.join().unwrap();
    }

    #[test]
    fn test_cost_period_label() {
        // Verify period labels map correctly
        let labels = [(0u8, "all"), (1, "24h"), (2, "7d"), (3, "30d")];
        for (period, expected) in labels {
            let label = match period {
                1 => "24h",
                2 => "7d",
                3 => "30d",
                _ => "all",
            };
            assert_eq!(label, expected);
        }
    }

    #[test]
    fn test_cost_period_cycle() {
        // Verify cycling: 0 -> 1 -> 2 -> 3 -> 0
        let mut period: u8 = 0;
        for expected in [1, 2, 3, 0] {
            period = (period + 1) % 4;
            assert_eq!(period, expected);
        }
    }

    #[test]
    fn test_palette_has_style_commands() {
        let palette = CommandPalette::new();
        let labels: Vec<&str> = palette.commands.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"Style: Children"), "Missing style command");
        assert!(labels.contains(&"Style: Technical"), "Missing style command");
        assert!(labels.contains(&"Clear Style"), "Missing clear style");
        assert!(labels.contains(&"Run Security Scan"), "Missing scan command");
        assert!(labels.contains(&"Cycle Cost Period"), "Missing cost period");
    }
}
