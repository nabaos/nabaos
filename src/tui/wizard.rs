//! Full-screen setup wizard — immersive TUI for first-run configuration.
//!
//! A 12-step interactive wizard with:
//! - Geometric constellation logo
//! - Provider selection with category headers (including Chinese providers)
//! - API key input with masked display + multi-model selection
//! - Constitution template picker
//! - 25 globally diverse personas + custom/Wikipedia options
//! - Plugin selector (14 modules)
//! - Studio media provider selector
//! - PEA autonomous agent settings (budget, strategy, heartbeat)
//! - Runtime watcher configuration (thresholds, alert channels)
//! - Channel configuration
//! - Agent catalog browser with multi-select + detail popup
//! - Summary screen

use std::io;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, List, ListItem, ListState, Paragraph,
};
use ratatui::Terminal;

// ── Color palette ───────────────────────────────────────────────────────────

const BG: Color = Color::Rgb(22, 22, 30);
const FG: Color = Color::Rgb(200, 200, 210);
const DIM: Color = Color::Rgb(90, 90, 105);
const ACCENT: Color = Color::Rgb(255, 175, 95);
const ACCENT2: Color = Color::Rgb(255, 135, 95);
const HIGHLIGHT_BG: Color = Color::Rgb(50, 48, 65);
const GREEN: Color = Color::Rgb(120, 220, 140);
const BORDER: Color = Color::Rgb(60, 58, 75);
const HEADING: Color = Color::Rgb(160, 155, 180);
const STEP_DONE: Color = Color::Rgb(120, 220, 140);
const STEP_ACTIVE: Color = Color::Rgb(255, 175, 95);
const STEP_TODO: Color = Color::Rgb(70, 68, 85);

// Logo colors — constellation / geometric node graph
const NODE_BRIGHT: Color = Color::Rgb(180, 160, 255);   // bright nodes
const NODE_DIM: Color = Color::Rgb(100, 90, 150);       // dim nodes
const EDGE: Color = Color::Rgb(70, 65, 110);            // connection lines
const GLOW: Color = Color::Rgb(140, 120, 200);          // node glow

// ── ASCII art — geometric constellation ─────────────────────────────────────

fn logo_art() -> Vec<Line<'static>> {
    let s = |text: &'static str, color: Color| Span::styled(text, Style::default().fg(color).bg(BG));

    vec![
        Line::from(vec![
            s("                        ", BG),
            s("◇", NODE_DIM),
        ]),
        Line::from(vec![
            s("                      ", BG),
            s("╱", EDGE),
            s(" ", BG),
            s("·", NODE_DIM),
            s(" ", BG),
            s("╲", EDGE),
        ]),
        Line::from(vec![
            s("               ", BG),
            s("◇", NODE_DIM),
            s("──────", EDGE),
            s("◆", NODE_BRIGHT),
            s("       ", BG),
            s("◆", NODE_BRIGHT),
            s("──────", EDGE),
            s("◇", NODE_DIM),
        ]),
        Line::from(vec![
            s("              ", BG),
            s("╱", EDGE),
            s("       ", BG),
            s("╱", EDGE),
            s(" ", BG),
            s("╲", EDGE),
            s("     ", BG),
            s("╱", EDGE),
            s(" ", BG),
            s("╲", EDGE),
        ]),
        Line::from(vec![
            s("            ", BG),
            s("◆", GLOW),
            s("───", EDGE),
            s("◇", NODE_DIM),
            s("──", EDGE),
            s("◆", NODE_BRIGHT),
            s("───", EDGE),
            s("★", ACCENT),
            s("───", EDGE),
            s("◆", NODE_BRIGHT),
            s("──", EDGE),
            s("◇", NODE_DIM),
            s("───", EDGE),
            s("◆", GLOW),
        ]),
        Line::from(vec![
            s("              ", BG),
            s("╲", EDGE),
            s("       ", BG),
            s("╲", EDGE),
            s(" ", BG),
            s("╱", EDGE),
            s("     ", BG),
            s("╲", EDGE),
            s(" ", BG),
            s("╱", EDGE),
        ]),
        Line::from(vec![
            s("               ", BG),
            s("◇", NODE_DIM),
            s("──────", EDGE),
            s("◆", NODE_BRIGHT),
            s("       ", BG),
            s("◆", NODE_BRIGHT),
            s("──────", EDGE),
            s("◇", NODE_DIM),
        ]),
        Line::from(vec![
            s("                      ", BG),
            s("╲", EDGE),
            s(" ", BG),
            s("·", NODE_DIM),
            s(" ", BG),
            s("╱", EDGE),
        ]),
        Line::from(vec![
            s("                        ", BG),
            s("◇", NODE_DIM),
        ]),
    ]
}

fn title_line() -> Line<'static> {
    Line::from(vec![
        Span::styled("·  ", Style::default().fg(DIM).bg(BG)),
        Span::styled("N a b a O S", Style::default().fg(ACCENT).bg(BG).add_modifier(Modifier::BOLD)),
        Span::styled("  ·", Style::default().fg(DIM).bg(BG)),
    ])
}

fn version_line() -> Line<'static> {
    Line::from(vec![Span::styled(
        format!("v{}", env!("CARGO_PKG_VERSION")),
        Style::default().fg(DIM).bg(BG),
    )])
}

// ── Wizard state ────────────────────────────────────────────────────────────

/// Result returned by the wizard for the caller to persist.
pub struct WizardResult {
    pub provider_id: String,
    pub provider_name: String,
    pub base_url: String,
    pub api_key: String,
    pub models: Vec<String>,
    pub primary_model: String,
    pub constitution: String,
    pub persona: String,
    pub enable_telegram: bool,
    pub telegram_token: String,
    pub telegram_chat_ids: String,
    pub enable_web: bool,
    pub web_password: String,
    pub selected_agents: Vec<String>,
    pub download_webbert: bool,
    pub custom_provider_name: String,
    pub custom_provider_url: String,
    pub enabled_plugins: Vec<String>,
    pub studio_providers: Vec<String>,
    pub pea_budget_usd: f64,
    pub pea_budget_strategy: String,
    pub pea_heartbeat_secs: u64,
    pub web_port: String,
    pub web_access: String,
    pub web_allowed_ips: String,
    pub studio_api_keys: Vec<(String, String)>,
    pub enable_watcher: bool,
    pub watcher_alert_threshold: f64,
    pub watcher_pause_threshold: f64,
    pub watcher_alert_channels: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Welcome,
    Provider,
    ApiKeyModel,
    Constitution,
    Persona,
    Plugins,
    Studio,
    Pea,
    Watcher,
    Channels,
    Agents,
    Summary,
}

impl Step {
    fn index(&self) -> usize {
        match self {
            Self::Welcome => 0,
            Self::Provider => 1,
            Self::ApiKeyModel => 2,
            Self::Constitution => 3,
            Self::Persona => 4,
            Self::Plugins => 5,
            Self::Studio => 6,
            Self::Pea => 7,
            Self::Watcher => 8,
            Self::Channels => 9,
            Self::Agents => 10,
            Self::Summary => 11,
        }
    }

    #[allow(dead_code)]
    fn label(&self) -> &'static str {
        match self {
            Self::Welcome => "Welcome",
            Self::Provider => "Provider",
            Self::ApiKeyModel => "Models",
            Self::Constitution => "Rules",
            Self::Persona => "Style",
            Self::Plugins => "Plugins",
            Self::Studio => "Studio",
            Self::Pea => "PEA",
            Self::Watcher => "Watcher",
            Self::Agents => "Agents",
            Self::Channels => "Channels",
            Self::Summary => "Done",
        }
    }

    fn all() -> &'static [Step] {
        &[
            Step::Welcome, Step::Provider, Step::ApiKeyModel, Step::Constitution,
            Step::Persona, Step::Plugins, Step::Studio, Step::Pea,
            Step::Watcher, Step::Channels, Step::Agents, Step::Summary,
        ]
    }

    #[cfg(test)]
    fn next(&self) -> Self {
        match self {
            Self::Welcome => Self::Provider,
            Self::Provider => Self::ApiKeyModel,
            Self::ApiKeyModel => Self::Constitution,
            Self::Constitution => Self::Persona,
            Self::Persona => Self::Plugins,
            Self::Plugins => Self::Studio,
            Self::Studio => Self::Pea,
            Self::Pea => Self::Watcher,
            Self::Watcher => Self::Channels,
            Self::Channels => Self::Agents,
            Self::Agents => Self::Summary,
            Self::Summary => Self::Summary,
        }
    }

    fn prev(&self) -> Self {
        match self {
            Self::Welcome => Self::Welcome,
            Self::Provider => Self::Welcome,
            Self::ApiKeyModel => Self::Provider,
            Self::Constitution => Self::ApiKeyModel,
            Self::Persona => Self::Constitution,
            Self::Plugins => Self::Persona,
            Self::Studio => Self::Plugins,
            Self::Pea => Self::Studio,
            Self::Watcher => Self::Pea,
            Self::Channels => Self::Watcher,
            Self::Agents => Self::Channels,
            Self::Summary => Self::Agents,
        }
    }
}

/// A selectable item in a list.
struct SelectItem {
    id: String,
    label: String,
    hint: String,
    is_header: bool,
    base_url: String,
}

/// Background message for model discovery.
enum BgMessage {
    ModelsFound(Vec<String>),
    ModelsError(String),
}

struct AgentItem {
    name: String,
    category: String,
    description: String,
    selected: bool,
    // Extended fields for detail popup
    version: String,
    author: String,
    permissions: Vec<String>,
    license: String,
}

struct PersonaItem {
    id: String,
    name: String,
    description: String,
    category: String,
}

struct PluginItem {
    id: String,
    name: String,
    description: String,
    selected: bool,
}

struct StudioItem {
    id: String,
    name: String,
    description: String,
    selected: bool,
    api_key: String,
    needs_key: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProviderView { Main, Custom }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PersonaEdit { None, Custom, Wikipedia }

const PEA_STRATEGIES: &[(&str, &str)] = &[
    ("adaptive", "Adjusts spending based on task complexity and success rate"),
    ("aggressive", "Prioritizes quality over cost, uses expensive models freely"),
    ("conservative", "Prefers cheaper models, escalates only when necessary"),
    ("minimal", "Minimizes spending, stays on cache/cheap layer as much as possible"),
];

struct WizardState {
    step: Step,
    should_quit: bool,
    confirmed: bool,

    // Provider selection
    provider_items: Vec<SelectItem>,
    provider_state: ListState,

    // API key input
    api_key_input: String,
    api_key_cursor: usize,
    show_api_key: bool,

    // Model selection — multi-select
    models: Vec<String>,
    model_selected: Vec<bool>,
    model_state: ListState,
    models_loading: bool,
    models_error: Option<String>,
    bg_rx: mpsc::Receiver<BgMessage>,
    bg_tx: mpsc::Sender<BgMessage>,

    // Selected provider info
    selected_provider_id: String,
    selected_provider_name: String,
    selected_base_url: String,
    primary_model: String,

    // Constitution
    constitution_items: Vec<(String, String)>,
    constitution_state: ListState,
    selected_constitution: String,

    // Persona
    persona_items: Vec<PersonaItem>,
    persona_state: ListState,
    selected_persona: String,

    // Plugins (new)
    plugin_items: Vec<PluginItem>,
    plugin_state: ListState,

    // Studio (new)
    studio_items: Vec<StudioItem>,
    studio_state: ListState,

    // Custom provider
    provider_view: ProviderView,
    custom_provider_name: String,
    custom_provider_url: String,
    custom_provider_field: usize, // 0=name, 1=url

    // Custom persona
    persona_edit_mode: PersonaEdit,
    custom_persona_text: String,
    wikipedia_url: String,

    // PEA settings
    pea_strategy_idx: usize,
    pea_budget_input: String,
    pea_heartbeat_input: String,
    pea_field: usize, // 0=strategy, 1=budget, 2=heartbeat

    // Watcher settings
    watcher_enabled: bool,
    watcher_alert_idx: usize,    // 0=0.5, 1=0.7, 2=0.9
    watcher_pause_idx: usize,    // 0=0.8, 1=0.9, 2=0.95
    watcher_channel_idx: usize,  // 0=telegram, 1=web, 2=both
    watcher_focus: usize,        // 0=toggle, 1=alert, 2=pause, 3=channel

    // Agents
    agent_items: Vec<AgentItem>,
    agent_state: ListState,
    agent_search: String,
    show_agent_detail: bool,

    // Channels
    channel_focus: usize,
    telegram_enabled: bool,
    telegram_token: String,
    telegram_editing: bool,
    telegram_chat_ids: String,
    telegram_chat_ids_editing: bool,
    web_enabled: bool,
    web_password: String,
    web_editing: bool,
    download_webbert: bool,
    web_port: String,
    web_access: usize, // 0=private, 1=public
    web_allowed_ips: String,
    channel_sub_field: usize,

    // Studio key editing
    studio_editing_key: bool,
    studio_key_idx: Option<usize>,

    // Animation
    start_time: Instant,
}

impl WizardState {
    fn new() -> Self {
        let (bg_tx, bg_rx) = mpsc::channel();

        let mut provider_items = Vec::new();

        // Popular
        provider_items.push(SelectItem { id: String::new(), label: "Popular".into(), hint: String::new(), is_header: true, base_url: String::new() });
        for (id, name, hint) in &[
            ("anthropic", "Anthropic", "Claude Sonnet 4.6"),
            ("openai", "OpenAI", "GPT-4o"),
            ("google", "Google", "Gemini 2.0 Flash"),
            ("deepseek", "DeepSeek", "deepseek-chat"),
            ("groq", "Groq", "fast inference"),
        ] {
            provider_items.push(SelectItem { id: id.to_string(), label: name.to_string(), hint: hint.to_string(), is_header: false, base_url: String::new() });
        }

        // Aggregators
        provider_items.push(SelectItem { id: String::new(), label: "Aggregators".into(), hint: "any model through a single API".into(), is_header: true, base_url: String::new() });
        for (id, name, hint) in &[
            ("openrouter", "OpenRouter", "any model via unified API"),
            ("nanogpt", "NanoGPT", "pay-per-token, no subscription"),
            ("together", "Together AI", "open-source models"),
            ("fireworks", "Fireworks AI", "fast open-source models"),
            ("mistral", "Mistral AI", "Mistral & Mixtral"),
            ("cerebras", "Cerebras", "fastest inference"),
            ("perplexity", "Perplexity", "search-augmented LLM"),
            ("deepinfra", "DeepInfra", "serverless GPU inference"),
            ("huggingface", "Hugging Face", "inference API"),
        ] {
            provider_items.push(SelectItem { id: id.to_string(), label: name.to_string(), hint: hint.to_string(), is_header: false, base_url: String::new() });
        }

        // Self-hosted
        provider_items.push(SelectItem { id: String::new(), label: "Self-hosted".into(), hint: "no API key needed".into(), is_header: true, base_url: String::new() });
        for (id, name, hint) in &[
            ("ollama", "Ollama", "localhost:11434"),
            ("lmstudio", "LM Studio", "localhost:1234"),
            ("llamacpp", "llama.cpp", "localhost:8080"),
            ("jan", "Jan", "localhost:1337"),
            ("localai", "LocalAI", "localhost:8080"),
            ("litellm", "LiteLLM", "localhost:4000"),
        ] {
            provider_items.push(SelectItem { id: id.to_string(), label: name.to_string(), hint: hint.to_string(), is_header: false, base_url: String::new() });
        }

        // Enterprise
        provider_items.push(SelectItem { id: String::new(), label: "Enterprise".into(), hint: String::new(), is_header: true, base_url: String::new() });
        for (id, name, hint) in &[
            ("bedrock", "AWS Bedrock", "Claude via AWS"),
            ("azure_openai", "Azure OpenAI", "GPT via Azure"),
        ] {
            provider_items.push(SelectItem { id: id.to_string(), label: name.to_string(), hint: hint.to_string(), is_header: false, base_url: String::new() });
        }

        // Chinese
        provider_items.push(SelectItem { id: String::new(), label: "Chinese".into(), hint: String::new(), is_header: true, base_url: String::new() });
        for (id, name, hint) in &[
            ("qwen", "Qwen (DashScope)", "Alibaba Cloud"),
            ("kimi", "Kimi (Moonshot AI)", "long-context"),
            ("baichuan", "Baichuan", "bilingual"),
            ("yi", "Yi (01.AI)", "open-source"),
            ("zhipu", "Zhipu (GLM)", "ChatGLM"),
            ("minimax", "MiniMax", "multimodal"),
        ] {
            provider_items.push(SelectItem { id: id.to_string(), label: name.to_string(), hint: hint.to_string(), is_header: false, base_url: String::new() });
        }

        // Custom provider option at bottom
        provider_items.push(SelectItem { id: String::new(), label: "Custom".into(), hint: String::new(), is_header: true, base_url: String::new() });
        provider_items.push(SelectItem { id: "custom".into(), label: "Custom Provider...".into(), hint: "enter name and URL".into(), is_header: false, base_url: String::new() });

        // Fill in base URLs from catalog
        let catalog = crate::providers::catalog::builtin_providers();
        for item in provider_items.iter_mut() {
            if !item.is_header {
                if let Some(p) = catalog.iter().find(|p| p.id == item.id) {
                    item.base_url = p.base_url.clone();
                }
            }
        }

        let mut provider_state = ListState::default();
        if let Some(idx) = provider_items.iter().position(|i| !i.is_header) {
            provider_state.select(Some(idx));
        }

        // Constitution templates
        let constitution_items: Vec<(String, String)> = vec![
            ("default".into(), "General-purpose with sensible defaults".into()),
            ("solopreneur".into(), "Solo business owner / indie hacker".into()),
            ("freelancer".into(), "Client and project management".into()),
            ("digital-marketer".into(), "Marketing automation".into()),
            ("student".into(), "Academic research and study".into()),
            ("sales".into(), "Sales pipeline and CRM".into()),
            ("customer-support".into(), "Support and ticket management".into()),
            ("legal".into(), "Legal research and compliance".into()),
            ("ecommerce".into(), "E-commerce and inventory".into()),
            ("hr".into(), "Human resources and recruitment".into()),
            ("finance".into(), "Financial analysis and trading".into()),
            ("healthcare".into(), "Healthcare and compliance".into()),
            ("engineering".into(), "Software engineering and DevOps".into()),
            ("media".into(), "Media production and content".into()),
            ("government".into(), "Public sector compliance".into()),
            ("ngo".into(), "Non-profit operations".into()),
            ("logistics".into(), "Supply chain management".into()),
            ("research".into(), "Scientific research and data".into()),
            ("consulting".into(), "Advisory services".into()),
            ("creative".into(), "Creative arts and design".into()),
            ("agriculture".into(), "Farming operations".into()),
        ];
        let mut constitution_state = ListState::default();
        constitution_state.select(Some(0));

        // Personas — 25 globally diverse + custom options
        let persona_items = vec![
            // Default
            PersonaItem { id: "nyaya".into(), name: "Nyaya".into(), description: "Balanced, adaptive (default)".into(), category: "Default".into() },
            // Philosophical
            PersonaItem { id: "socrates".into(), name: "Socrates".into(), description: "Socratic method, questioning".into(), category: "Philosophical".into() },
            PersonaItem { id: "confucius".into(), name: "Confucius".into(), description: "Harmonious, virtue-focused".into(), category: "Philosophical".into() },
            PersonaItem { id: "seneca".into(), name: "Seneca".into(), description: "Stoic, practical wisdom".into(), category: "Philosophical".into() },
            PersonaItem { id: "hypatia".into(), name: "Hypatia".into(), description: "Scholarly, mathematical".into(), category: "Philosophical".into() },
            // Fictional
            PersonaItem { id: "sherlock".into(), name: "Sherlock".into(), description: "Deductive, observant".into(), category: "Fictional".into() },
            PersonaItem { id: "jarvis".into(), name: "J.A.R.V.I.S.".into(), description: "Witty, sardonic assistant".into(), category: "Fictional".into() },
            PersonaItem { id: "wednesday".into(), name: "Wednesday".into(), description: "Dry, blunt, deadpan".into(), category: "Fictional".into() },
            PersonaItem { id: "gandalf".into(), name: "Gandalf".into(), description: "Sage, cryptic wisdom".into(), category: "Fictional".into() },
            PersonaItem { id: "spock".into(), name: "Spock".into(), description: "Logical, precise".into(), category: "Fictional".into() },
            PersonaItem { id: "cortana".into(), name: "Cortana".into(), description: "Warm, mission-focused".into(), category: "Fictional".into() },
            // Scientific
            PersonaItem { id: "curie".into(), name: "Marie Curie".into(), description: "Rigorous, persistent".into(), category: "Scientific".into() },
            PersonaItem { id: "feynman".into(), name: "Feynman".into(), description: "Playful, explains simply".into(), category: "Scientific".into() },
            PersonaItem { id: "turing".into(), name: "Turing".into(), description: "Analytical, pattern-seeking".into(), category: "Scientific".into() },
            PersonaItem { id: "lovelace".into(), name: "Ada Lovelace".into(), description: "Visionary, poetic".into(), category: "Scientific".into() },
            // Leadership
            PersonaItem { id: "sun_tzu".into(), name: "Sun Tzu".into(), description: "Strategic, concise".into(), category: "Leadership".into() },
            PersonaItem { id: "cleopatra".into(), name: "Cleopatra".into(), description: "Diplomatic, persuasive".into(), category: "Leadership".into() },
            PersonaItem { id: "mandela".into(), name: "Mandela".into(), description: "Patient, principled".into(), category: "Leadership".into() },
            // Creative
            PersonaItem { id: "da_vinci".into(), name: "Da Vinci".into(), description: "Polymath, inventive".into(), category: "Creative".into() },
            PersonaItem { id: "frida".into(), name: "Frida Kahlo".into(), description: "Passionate, authentic".into(), category: "Creative".into() },
            // Archetypes
            PersonaItem { id: "butler".into(), name: "Butler".into(), description: "Formal, organized".into(), category: "Archetype".into() },
            PersonaItem { id: "coach".into(), name: "Coach".into(), description: "Motivational, encouraging".into(), category: "Archetype".into() },
            PersonaItem { id: "hacker".into(), name: "Hacker".into(), description: "Terse, efficient".into(), category: "Archetype".into() },
            PersonaItem { id: "professor".into(), name: "Professor".into(), description: "Academic, thorough".into(), category: "Archetype".into() },
            PersonaItem { id: "pirate".into(), name: "Pirate".into(), description: "Playful, adventurous".into(), category: "Archetype".into() },
            // Custom
            PersonaItem { id: "custom".into(), name: "Custom".into(), description: "Create custom or import from SillyTavern".into(), category: "Custom".into() },
            PersonaItem { id: "wikipedia".into(), name: "From Wikipedia".into(), description: "Generate persona from a Wikipedia URL".into(), category: "Custom".into() },
        ];
        let mut persona_state = ListState::default();
        persona_state.select(Some(0));

        // Agents — load from catalog
        let mut agent_items = Vec::new();
        let data_dir = std::env::var("NABA_DATA_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                std::env::var("HOME")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|_| std::path::PathBuf::from("/tmp"))
                    .join(".nabaos")
            });
        let catalog_dir = data_dir.join("catalog");
        let agent_catalog = crate::agent_os::catalog::AgentCatalog::new(&catalog_dir);
        if let Ok(entries) = agent_catalog.list() {
            for entry in entries {
                agent_items.push(AgentItem {
                    name: entry.name,
                    category: entry.category,
                    description: entry.description,
                    selected: false,
                    version: entry.version,
                    author: entry.author,
                    permissions: entry.permissions,
                    license: "MIT".to_string(),
                });
            }
        }
        // Fallback starter agents if catalog is empty
        if agent_items.is_empty() {
            for (name, cat, desc, perms) in &[
                ("morning-briefing", "Daily Productivity", "Calendar, weather, news summary", "calendar.read, weather.read, news.read"),
                ("email-assistant", "Email & Communication", "Smart email triage and drafting", "email.read, email.write, contacts.read"),
                ("dev-helper", "Developer & DevOps", "Git, CI status, PR summaries", "git.read, ci.read, github.read"),
                ("research-digest", "Research & Analysis", "Summarize papers and articles", "web.read, pdf.read, filesystem.write"),
                ("budget-tracker", "Finance & Budgeting", "Track expenses and budgets", "finance.read, spreadsheet.write"),
                ("social-scheduler", "Social Media", "Schedule and manage posts", "social.write, schedule.write"),
            ] {
                agent_items.push(AgentItem {
                    name: name.to_string(),
                    category: cat.to_string(),
                    description: desc.to_string(),
                    selected: false,
                    version: "1.0.0".to_string(),
                    author: "NabaOS Core".to_string(),
                    permissions: perms.split(", ").map(|s| s.to_string()).collect(),
                    license: "MIT".to_string(),
                });
            }
        }
        let mut agent_state = ListState::default();
        if !agent_items.is_empty() {
            agent_state.select(Some(0));
        }

        // Plugins — 14 modules
        let plugin_items = vec![
            PluginItem { id: "browser".into(), name: "Browser".into(), description: "Web browsing and scraping".into(), selected: false },
            PluginItem { id: "pdf".into(), name: "PDF".into(), description: "PDF reading and generation".into(), selected: false },
            PluginItem { id: "latex".into(), name: "LaTeX".into(), description: "LaTeX document compilation".into(), selected: false },
            PluginItem { id: "voice".into(), name: "Voice".into(), description: "Speech-to-text and TTS".into(), selected: false },
            PluginItem { id: "csv_data".into(), name: "CSV/Data".into(), description: "CSV and structured data processing".into(), selected: true },
            PluginItem { id: "database".into(), name: "Database".into(), description: "SQL database access".into(), selected: false },
            PluginItem { id: "git".into(), name: "Git".into(), description: "Git repository operations".into(), selected: true },
            PluginItem { id: "filesystem".into(), name: "Filesystem".into(), description: "Local file operations".into(), selected: true },
            PluginItem { id: "deploy".into(), name: "Deploy".into(), description: "Deployment automation".into(), selected: false },
            PluginItem { id: "homeassistant".into(), name: "Home Assistant".into(), description: "Smart home integration".into(), selected: false },
            PluginItem { id: "oauth".into(), name: "OAuth".into(), description: "OAuth provider integrations".into(), selected: false },
            PluginItem { id: "research".into(), name: "Research".into(), description: "Academic paper search".into(), selected: false },
            PluginItem { id: "tracking".into(), name: "Tracking".into(), description: "Package and order tracking".into(), selected: false },
            PluginItem { id: "hardware".into(), name: "Hardware".into(), description: "Hardware monitoring".into(), selected: false },
        ];
        let mut plugin_state = ListState::default();
        plugin_state.select(Some(0));

        // Studio — media providers + image sourcing
        let studio_items = vec![
            StudioItem { id: "unsplash".into(), name: "Unsplash".into(), description: "Free stock photos · unsplash.com/developers → New App → Access Key".into(), selected: false, api_key: String::new(), needs_key: true },
            StudioItem { id: "pexels".into(), name: "Pexels".into(), description: "Free stock photos · pexels.com/api → Get Your API Key".into(), selected: false, api_key: String::new(), needs_key: true },
            StudioItem { id: "comfyui".into(), name: "ComfyUI".into(), description: "Local image generation".into(), selected: false, api_key: String::new(), needs_key: false },
            StudioItem { id: "fal_ai".into(), name: "fal.ai".into(), description: "Cloud image/video generation".into(), selected: false, api_key: String::new(), needs_key: true },
            StudioItem { id: "dall_e".into(), name: "DALL-E".into(), description: "OpenAI image generation".into(), selected: false, api_key: String::new(), needs_key: true },
            StudioItem { id: "runway".into(), name: "Runway".into(), description: "AI video generation".into(), selected: false, api_key: String::new(), needs_key: true },
            StudioItem { id: "elevenlabs".into(), name: "ElevenLabs".into(), description: "Text-to-speech".into(), selected: false, api_key: String::new(), needs_key: true },
            StudioItem { id: "ffmpeg".into(), name: "ffmpeg".into(), description: "Local A/V processing".into(), selected: false, api_key: String::new(), needs_key: false },
        ];
        let mut studio_state = ListState::default();
        studio_state.select(Some(0));

        Self {
            step: Step::Welcome,
            should_quit: false,
            confirmed: false,
            provider_items,
            provider_state,
            api_key_input: String::new(),
            api_key_cursor: 0,
            show_api_key: false,
            models: Vec::new(),
            model_selected: Vec::new(),
            model_state: ListState::default(),
            models_loading: false,
            models_error: None,
            bg_rx,
            bg_tx,
            selected_provider_id: String::new(),
            selected_provider_name: String::new(),
            selected_base_url: String::new(),
            primary_model: String::new(),
            constitution_items,
            constitution_state,
            selected_constitution: "default".into(),
            persona_items,
            persona_state,
            selected_persona: "nyaya".into(),
            plugin_items,
            plugin_state,
            studio_items,
            studio_state,
            provider_view: ProviderView::Main,
            custom_provider_name: String::new(),
            custom_provider_url: String::new(),
            custom_provider_field: 0,
            persona_edit_mode: PersonaEdit::None,
            custom_persona_text: String::new(),
            wikipedia_url: String::new(),
            pea_strategy_idx: 0,
            pea_budget_input: "50.00".into(),
            pea_heartbeat_input: "300".into(),
            pea_field: 0,
            watcher_enabled: false,
            watcher_alert_idx: 1,   // 0.7 default
            watcher_pause_idx: 1,   // 0.9 default
            watcher_channel_idx: 0, // telegram default
            watcher_focus: 0,
            agent_items,
            agent_state,
            agent_search: String::new(),
            show_agent_detail: false,
            channel_focus: 0,
            telegram_enabled: false,
            telegram_token: String::new(),
            telegram_editing: false,
            telegram_chat_ids: String::new(),
            telegram_chat_ids_editing: false,
            web_enabled: false,
            web_password: String::new(),
            web_editing: false,
            download_webbert: true,
            web_port: "8919".into(),
            web_access: 0,
            web_allowed_ips: String::new(),
            channel_sub_field: 0,
            studio_editing_key: false,
            studio_key_idx: None,
            start_time: Instant::now(),
        }
    }

    fn is_local_provider(&self) -> bool {
        self.selected_base_url.starts_with("http://localhost")
            || self.selected_base_url.starts_with("http://127.")
    }

    fn confirm_provider(&mut self) {
        if let Some(idx) = self.provider_state.selected() {
            let item = &self.provider_items[idx];
            if item.is_header { return; }
            self.selected_provider_id = item.id.clone();
            self.selected_provider_name = item.label.clone();
            self.selected_base_url = item.base_url.clone();

            let catalog = crate::providers::catalog::builtin_providers();
            if let Some(p) = catalog.iter().find(|p| p.id == self.selected_provider_id) {
                if !p.default_model.is_empty() {
                    self.primary_model = p.default_model.clone();
                }
            }
            self.step = Step::ApiKeyModel;
        }
    }

    fn discover_models(&mut self) {
        if self.selected_base_url.is_empty() { return; }
        self.models_loading = true;
        self.models_error = None;
        self.models.clear();
        self.model_selected.clear();

        let tx = self.bg_tx.clone();
        let base_url = self.selected_base_url.clone();
        let api_key = self.api_key_input.clone();

        std::thread::spawn(move || {
            match crate::providers::discovery::fetch_available_models(&base_url, &api_key) {
                Ok(models) if !models.is_empty() => { tx.send(BgMessage::ModelsFound(models)).ok(); }
                Ok(_) => { tx.send(BgMessage::ModelsError("No models listed on this endpoint".into())).ok(); }
                Err(e) => { tx.send(BgMessage::ModelsError(e.to_string())).ok(); }
            }
        });
    }

    fn poll_bg(&mut self) {
        while let Ok(msg) = self.bg_rx.try_recv() {
            match msg {
                BgMessage::ModelsFound(m) => {
                    self.models_loading = false;
                    self.model_selected = vec![false; m.len()];
                    // Auto-select the primary/default model
                    if !self.primary_model.is_empty() {
                        if let Some(idx) = m.iter().position(|x| x == &self.primary_model) {
                            self.model_selected[idx] = true;
                        }
                    }
                    // If no default matched, select first
                    if self.model_selected.iter().all(|s| !s) && !m.is_empty() {
                        self.model_selected[0] = true;
                        self.primary_model = m[0].clone();
                    }
                    self.models = m;
                    self.model_state.select(Some(0));
                }
                BgMessage::ModelsError(e) => {
                    self.models_loading = false;
                    self.models_error = Some(e);
                }
            }
        }
    }

    fn move_provider_up(&mut self) {
        let len = self.provider_items.len();
        if len == 0 { return; }
        let mut i = self.provider_state.selected().unwrap_or(0);
        loop {
            i = if i == 0 { len - 1 } else { i - 1 };
            if !self.provider_items[i].is_header { break; }
        }
        self.provider_state.select(Some(i));
    }

    fn move_provider_down(&mut self) {
        let len = self.provider_items.len();
        if len == 0 { return; }
        let mut i = self.provider_state.selected().unwrap_or(0);
        loop {
            i = (i + 1) % len;
            if !self.provider_items[i].is_header { break; }
        }
        self.provider_state.select(Some(i));
    }

    fn selected_models(&self) -> Vec<String> {
        self.models.iter().zip(self.model_selected.iter())
            .filter(|(_, sel)| **sel)
            .map(|(m, _)| m.clone())
            .collect()
    }

    fn selected_agents(&self) -> Vec<String> {
        self.agent_items.iter()
            .filter(|a| a.selected)
            .map(|a| a.name.clone())
            .collect()
    }

    fn filtered_agent_indices(&self) -> Vec<usize> {
        if self.agent_search.is_empty() {
            (0..self.agent_items.len()).collect()
        } else {
            let q = self.agent_search.to_lowercase();
            self.agent_items.iter().enumerate()
                .filter(|(_, a)| {
                    a.name.to_lowercase().contains(&q)
                        || a.category.to_lowercase().contains(&q)
                        || a.description.to_lowercase().contains(&q)
                })
                .map(|(i, _)| i)
                .collect()
        }
    }

    fn into_result(self) -> WizardResult {
        let models = self.selected_models();
        let agents = self.selected_agents();
        let primary = if !self.primary_model.is_empty() {
            self.primary_model.clone()
        } else if let Some(first) = models.first() {
            first.clone()
        } else {
            String::new()
        };
        let enabled_plugins: Vec<String> = self.plugin_items.iter()
            .filter(|p| p.selected)
            .map(|p| p.id.clone())
            .collect();
        let studio_providers: Vec<String> = self.studio_items.iter()
            .filter(|s| s.selected)
            .map(|s| s.id.clone())
            .collect();
        let studio_api_keys: Vec<(String, String)> = self.studio_items.iter()
            .filter(|s| s.selected && !s.api_key.is_empty())
            .map(|s| (s.id.clone(), s.api_key.clone()))
            .collect();
        let pea_budget_usd = self.pea_budget_input.parse::<f64>().unwrap_or(50.0);
        let pea_budget_strategy = PEA_STRATEGIES.get(self.pea_strategy_idx)
            .map(|(id, _)| id.to_string())
            .unwrap_or_else(|| "adaptive".to_string());
        let pea_heartbeat_secs = self.pea_heartbeat_input.parse::<u64>().unwrap_or(300);
        // Persona: custom or wikipedia prefix
        let persona = match self.persona_edit_mode {
            PersonaEdit::Custom if !self.custom_persona_text.is_empty() => {
                format!("custom:{}", self.custom_persona_text)
            }
            PersonaEdit::Wikipedia if !self.wikipedia_url.is_empty() => {
                format!("wikipedia:{}", self.wikipedia_url)
            }
            _ => self.selected_persona.clone(),
        };
        let web_access = if self.web_access == 0 { "private".to_string() } else { "public".to_string() };
        const ALERT_THRESHOLDS: [f64; 3] = [0.5, 0.7, 0.9];
        const PAUSE_THRESHOLDS: [f64; 3] = [0.8, 0.9, 0.95];
        let watcher_channels = match self.watcher_channel_idx {
            0 => "telegram",
            1 => "web",
            _ => "telegram,web",
        };
        WizardResult {
            provider_id: self.selected_provider_id,
            provider_name: self.selected_provider_name.clone(),
            base_url: if self.provider_view == ProviderView::Custom { self.custom_provider_url.clone() } else { self.selected_base_url },
            api_key: self.api_key_input,
            models,
            primary_model: primary,
            constitution: self.selected_constitution,
            persona,
            enable_telegram: self.telegram_enabled,
            telegram_token: self.telegram_token,
            telegram_chat_ids: self.telegram_chat_ids,
            enable_web: self.web_enabled,
            web_password: self.web_password,
            selected_agents: agents,
            download_webbert: self.download_webbert,
            custom_provider_name: self.custom_provider_name,
            custom_provider_url: self.custom_provider_url,
            enabled_plugins,
            studio_providers,
            pea_budget_usd,
            pea_budget_strategy,
            pea_heartbeat_secs,
            web_port: self.web_port,
            web_access,
            web_allowed_ips: self.web_allowed_ips,
            studio_api_keys,
            enable_watcher: self.watcher_enabled,
            watcher_alert_threshold: ALERT_THRESHOLDS[self.watcher_alert_idx],
            watcher_pause_threshold: PAUSE_THRESHOLDS[self.watcher_pause_idx],
            watcher_alert_channels: watcher_channels.to_string(),
        }
    }
}

// ── Public API ──────────────────────────────────────────────────────────────

pub fn run_wizard() -> crate::core::error::Result<Option<WizardResult>> {
    enable_raw_mode().map_err(|e| crate::core::error::NyayaError::Config(e.to_string()))?;
    io::stdout()
        .execute(EnterAlternateScreen)
        .map_err(|e| crate::core::error::NyayaError::Config(e.to_string()))?;

    let backend = ratatui::backend::CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)
        .map_err(|e| crate::core::error::NyayaError::Config(e.to_string()))?;

    let mut state = WizardState::new();

    let result = loop {
        state.poll_bg();

        terminal
            .draw(|frame| draw_wizard(frame, &state))
            .map_err(|e| crate::core::error::NyayaError::Config(e.to_string()))?;

        if event::poll(Duration::from_millis(100))
            .map_err(|e| crate::core::error::NyayaError::Config(e.to_string()))?
        {
            if let Event::Key(key) =
                event::read().map_err(|e| crate::core::error::NyayaError::Config(e.to_string()))?
            {
                handle_key(&mut state, key);
            }
        }

        if state.should_quit { break Ok(None); }
        if state.confirmed { break Ok(Some(state.into_result())); }
    };

    disable_raw_mode().ok();
    io::stdout().execute(LeaveAlternateScreen).ok();

    result
}

// ── Key handling ────────────────────────────────────────────────────────────

fn handle_key(state: &mut WizardState, key: crossterm::event::KeyEvent) {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        state.should_quit = true;
        return;
    }
    if key.code == KeyCode::Esc {
        // If editing a text field, just cancel editing
        if state.telegram_chat_ids_editing { state.telegram_chat_ids_editing = false; return; }
        if state.telegram_editing { state.telegram_editing = false; return; }
        if state.web_editing { state.web_editing = false; return; }
        if state.studio_editing_key { state.studio_editing_key = false; state.studio_key_idx = None; return; }
        if state.show_agent_detail { state.show_agent_detail = false; return; }
        // Custom provider/persona: back to main view
        if state.step == Step::Provider && state.provider_view == ProviderView::Custom {
            state.provider_view = ProviderView::Main;
            return;
        }
        if state.step == Step::Persona && state.persona_edit_mode != PersonaEdit::None {
            state.persona_edit_mode = PersonaEdit::None;
            return;
        }
        if state.step == Step::Welcome { state.should_quit = true; }
        else { state.step = state.step.prev(); }
        return;
    }

    match state.step {
        Step::Welcome => {
            if key.code == KeyCode::Enter || key.code == KeyCode::Char(' ') {
                state.step = Step::Provider;
            } else if key.code == KeyCode::Char('q') {
                state.should_quit = true;
            }
        }
        Step::Provider => {
            if state.provider_view == ProviderView::Custom {
                // Custom provider form input
                match key.code {
                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        match state.custom_provider_field {
                            0 => state.custom_provider_name.push(c),
                            _ => state.custom_provider_url.push(c),
                        }
                    }
                    KeyCode::Backspace => {
                        match state.custom_provider_field {
                            0 => { state.custom_provider_name.pop(); }
                            _ => { state.custom_provider_url.pop(); }
                        }
                    }
                    KeyCode::Tab => {
                        state.custom_provider_field = 1 - state.custom_provider_field;
                    }
                    KeyCode::Enter => {
                        if state.custom_provider_field == 0 && !state.custom_provider_name.is_empty() {
                            state.custom_provider_field = 1;
                        } else if !state.custom_provider_url.is_empty() {
                            state.selected_provider_id = "custom".to_string();
                            state.selected_provider_name = state.custom_provider_name.clone();
                            state.selected_base_url = state.custom_provider_url.clone();
                            state.step = Step::ApiKeyModel;
                        }
                    }
                    _ => {}
                }
            } else {
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => state.move_provider_up(),
                    KeyCode::Down | KeyCode::Char('j') => state.move_provider_down(),
                    KeyCode::Enter => {
                        if let Some(idx) = state.provider_state.selected() {
                            let item = &state.provider_items[idx];
                            if !item.is_header && item.id == "custom" {
                                state.provider_view = ProviderView::Custom;
                                state.custom_provider_field = 0;
                            } else {
                                state.confirm_provider();
                            }
                        }
                    }
                    KeyCode::Char('q') => state.should_quit = true,
                    _ => {}
                }
            }
        },
        Step::ApiKeyModel => handle_api_key_model(state, key),
        Step::Constitution => match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                let len = state.constitution_items.len();
                if len > 0 {
                    let i = state.constitution_state.selected().unwrap_or(0);
                    state.constitution_state.select(Some(if i == 0 { len - 1 } else { i - 1 }));
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let len = state.constitution_items.len();
                if len > 0 {
                    let i = state.constitution_state.selected().unwrap_or(0);
                    state.constitution_state.select(Some((i + 1) % len));
                }
            }
            KeyCode::Enter => {
                if let Some(idx) = state.constitution_state.selected() {
                    state.selected_constitution = state.constitution_items[idx].0.clone();
                    state.step = Step::Persona;
                }
            }
            _ => {}
        },
        Step::Persona => {
            if state.persona_edit_mode != PersonaEdit::None {
                // Text input mode for custom/wikipedia
                match key.code {
                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        match state.persona_edit_mode {
                            PersonaEdit::Custom => state.custom_persona_text.push(c),
                            PersonaEdit::Wikipedia => state.wikipedia_url.push(c),
                            _ => {}
                        }
                    }
                    KeyCode::Backspace => {
                        match state.persona_edit_mode {
                            PersonaEdit::Custom => { state.custom_persona_text.pop(); }
                            PersonaEdit::Wikipedia => { state.wikipedia_url.pop(); }
                            _ => {}
                        }
                    }
                    KeyCode::Enter => {
                        let has_text = match state.persona_edit_mode {
                            PersonaEdit::Custom => !state.custom_persona_text.is_empty(),
                            PersonaEdit::Wikipedia => !state.wikipedia_url.is_empty(),
                            _ => false,
                        };
                        if has_text {
                            state.step = Step::Plugins;
                        }
                    }
                    _ => {}
                }
            } else {
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        let len = state.persona_items.len();
                        if len > 0 {
                            let i = state.persona_state.selected().unwrap_or(0);
                            state.persona_state.select(Some(if i == 0 { len - 1 } else { i - 1 }));
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        let len = state.persona_items.len();
                        if len > 0 {
                            let i = state.persona_state.selected().unwrap_or(0);
                            state.persona_state.select(Some((i + 1) % len));
                        }
                    }
                    KeyCode::Enter => {
                        if let Some(idx) = state.persona_state.selected() {
                            let id = &state.persona_items[idx].id;
                            if id == "custom" {
                                state.persona_edit_mode = PersonaEdit::Custom;
                            } else if id == "wikipedia" {
                                state.persona_edit_mode = PersonaEdit::Wikipedia;
                            } else {
                                state.selected_persona = id.clone();
                                state.step = Step::Plugins;
                            }
                        }
                    }
                    _ => {}
                }
            }
        },
        Step::Plugins => handle_plugins(state, key),
        Step::Studio => handle_studio(state, key),
        Step::Pea => handle_pea(state, key),
        Step::Watcher => handle_watcher(state, key),
        Step::Channels => handle_channels(state, key),
        Step::Agents => handle_agents(state, key),
        Step::Summary => match key.code {
            KeyCode::Enter => { state.confirmed = true; }
            KeyCode::Char('b') | KeyCode::Backspace => { state.step = Step::Agents; }
            _ => {}
        },
    }
}

fn handle_plugins(state: &mut WizardState, key: crossterm::event::KeyEvent) {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            let len = state.plugin_items.len();
            if len > 0 {
                let i = state.plugin_state.selected().unwrap_or(0);
                state.plugin_state.select(Some(if i == 0 { len - 1 } else { i - 1 }));
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let len = state.plugin_items.len();
            if len > 0 {
                let i = state.plugin_state.selected().unwrap_or(0);
                state.plugin_state.select(Some((i + 1) % len));
            }
        }
        KeyCode::Char(' ') => {
            if let Some(idx) = state.plugin_state.selected() {
                if idx < state.plugin_items.len() {
                    state.plugin_items[idx].selected = !state.plugin_items[idx].selected;
                }
            }
        }
        KeyCode::Char('a') => {
            for p in state.plugin_items.iter_mut() { p.selected = true; }
        }
        KeyCode::Char('n') => {
            for p in state.plugin_items.iter_mut() { p.selected = false; }
        }
        KeyCode::Enter => { state.step = Step::Studio; }
        _ => {}
    }
}

fn handle_studio(state: &mut WizardState, key: crossterm::event::KeyEvent) {
    // API key editing mode
    if state.studio_editing_key {
        if let Some(idx) = state.studio_key_idx {
            match key.code {
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    state.studio_items[idx].api_key.push(c);
                }
                KeyCode::Backspace => {
                    state.studio_items[idx].api_key.pop();
                }
                KeyCode::Enter => {
                    state.studio_editing_key = false;
                    state.studio_key_idx = None;
                }
                _ => {}
            }
        }
        return;
    }

    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            let len = state.studio_items.len();
            if len > 0 {
                let i = state.studio_state.selected().unwrap_or(0);
                state.studio_state.select(Some(if i == 0 { len - 1 } else { i - 1 }));
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let len = state.studio_items.len();
            if len > 0 {
                let i = state.studio_state.selected().unwrap_or(0);
                state.studio_state.select(Some((i + 1) % len));
            }
        }
        KeyCode::Char(' ') => {
            if let Some(idx) = state.studio_state.selected() {
                if idx < state.studio_items.len() {
                    let was_selected = state.studio_items[idx].selected;
                    state.studio_items[idx].selected = !was_selected;
                    // If toggling ON a cloud provider, show key input
                    if !was_selected && state.studio_items[idx].needs_key {
                        state.studio_editing_key = true;
                        state.studio_key_idx = Some(idx);
                    }
                }
            }
        }
        KeyCode::Char('a') => {
            for s in state.studio_items.iter_mut() { s.selected = true; }
        }
        KeyCode::Char('n') => {
            for s in state.studio_items.iter_mut() { s.selected = false; }
        }
        KeyCode::Char('s') => {
            // Skip
            state.step = Step::Pea;
        }
        KeyCode::Enter => {
            // Check if any selected cloud provider still needs a key
            let needs_key_idx = state.studio_items.iter().enumerate()
                .find(|(_, s)| s.selected && s.needs_key && s.api_key.is_empty())
                .map(|(i, _)| i);
            if let Some(idx) = needs_key_idx {
                state.studio_editing_key = true;
                state.studio_key_idx = Some(idx);
                state.studio_state.select(Some(idx));
            } else {
                state.step = Step::Pea;
            }
        }
        _ => {}
    }
}

fn handle_pea(state: &mut WizardState, key: crossterm::event::KeyEvent) {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            state.pea_field = state.pea_field.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
            state.pea_field = (state.pea_field + 1).min(2);
        }
        KeyCode::Left => {
            if state.pea_field == 0 {
                state.pea_strategy_idx = state.pea_strategy_idx.checked_sub(1).unwrap_or(PEA_STRATEGIES.len() - 1);
            }
        }
        KeyCode::Right => {
            if state.pea_field == 0 {
                state.pea_strategy_idx = (state.pea_strategy_idx + 1) % PEA_STRATEGIES.len();
            }
        }
        KeyCode::Char(c) if c.is_ascii_digit() || c == '.' => {
            match state.pea_field {
                1 => state.pea_budget_input.push(c),
                2 => { if c.is_ascii_digit() { state.pea_heartbeat_input.push(c); } }
                _ => {}
            }
        }
        KeyCode::Backspace => {
            match state.pea_field {
                1 => { state.pea_budget_input.pop(); }
                2 => { state.pea_heartbeat_input.pop(); }
                _ => {}
            }
        }
        KeyCode::Enter => {
            state.step = Step::Watcher;
        }
        _ => {}
    }
}

fn handle_watcher(state: &mut WizardState, key: crossterm::event::KeyEvent) {
    match key.code {
        KeyCode::Up => {
            if state.watcher_focus > 0 {
                state.watcher_focus -= 1;
            }
        }
        KeyCode::Down => {
            let max = if state.watcher_enabled { 3 } else { 0 };
            if state.watcher_focus < max {
                state.watcher_focus += 1;
            }
        }
        KeyCode::Char(' ') if state.watcher_focus == 0 => {
            state.watcher_enabled = !state.watcher_enabled;
            if !state.watcher_enabled {
                state.watcher_focus = 0;
            }
        }
        KeyCode::Left => {
            match state.watcher_focus {
                1 if state.watcher_alert_idx > 0 => state.watcher_alert_idx -= 1,
                2 if state.watcher_pause_idx > 0 => state.watcher_pause_idx -= 1,
                3 if state.watcher_channel_idx > 0 => state.watcher_channel_idx -= 1,
                _ => {}
            }
        }
        KeyCode::Right => {
            match state.watcher_focus {
                1 if state.watcher_alert_idx < 2 => state.watcher_alert_idx += 1,
                2 if state.watcher_pause_idx < 2 => state.watcher_pause_idx += 1,
                3 if state.watcher_channel_idx < 2 => state.watcher_channel_idx += 1,
                _ => {}
            }
        }
        KeyCode::Enter => {
            state.step = Step::Channels;
        }
        _ => {}
    }
}

fn handle_api_key_model(state: &mut WizardState, key: crossterm::event::KeyEvent) {
    let is_local = state.is_local_provider();

    // If models are loaded — multi-select mode
    if !state.models.is_empty() {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                let len = state.models.len();
                let i = state.model_state.selected().unwrap_or(0);
                state.model_state.select(Some(if i == 0 { len - 1 } else { i - 1 }));
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let len = state.models.len();
                let i = state.model_state.selected().unwrap_or(0);
                state.model_state.select(Some((i + 1) % len));
            }
            KeyCode::Char(' ') => {
                // Toggle selection
                if let Some(idx) = state.model_state.selected() {
                    if idx < state.model_selected.len() {
                        state.model_selected[idx] = !state.model_selected[idx];
                        // Update primary model to first selected
                        if let Some(first) = state.models.iter().zip(state.model_selected.iter())
                            .find(|(_, s)| **s).map(|(m, _)| m.clone()) {
                            state.primary_model = first;
                        }
                    }
                }
            }
            KeyCode::Char('p') => {
                // Set as primary
                if let Some(idx) = state.model_state.selected() {
                    if idx < state.models.len() {
                        state.model_selected[idx] = true;
                        state.primary_model = state.models[idx].clone();
                    }
                }
            }
            KeyCode::Tab => { state.show_api_key = !state.show_api_key; }
            KeyCode::Enter => { state.step = Step::Constitution; }
            _ => {}
        }
        return;
    }

    // API key text input mode
    if !is_local {
        match key.code {
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                state.api_key_input.insert(state.api_key_cursor, c);
                state.api_key_cursor += 1;
            }
            KeyCode::Backspace => {
                if state.api_key_cursor > 0 {
                    state.api_key_cursor -= 1;
                    state.api_key_input.remove(state.api_key_cursor);
                }
            }
            KeyCode::Left => { state.api_key_cursor = state.api_key_cursor.saturating_sub(1); }
            KeyCode::Right => { state.api_key_cursor = (state.api_key_cursor + 1).min(state.api_key_input.len()); }
            KeyCode::Tab => { state.show_api_key = !state.show_api_key; }
            KeyCode::Enter => {
                if !state.api_key_input.is_empty() {
                    state.discover_models();
                } else {
                    state.step = Step::Constitution;
                }
            }
            _ => {}
        }
    } else {
        match key.code {
            KeyCode::Enter => {
                if state.models.is_empty() && !state.models_loading {
                    state.discover_models();
                } else {
                    state.step = Step::Constitution;
                }
            }
            _ => {}
        }
    }
}

fn handle_agents(state: &mut WizardState, key: crossterm::event::KeyEvent) {
    // Agent detail popup mode
    if state.show_agent_detail {
        match key.code {
            KeyCode::Char(' ') => {
                let filtered = state.filtered_agent_indices();
                if let Some(view_idx) = state.agent_state.selected() {
                    if view_idx < filtered.len() {
                        let real_idx = filtered[view_idx];
                        state.agent_items[real_idx].selected = !state.agent_items[real_idx].selected;
                    }
                }
            }
            KeyCode::Esc | KeyCode::Char('d') | KeyCode::Char('i') | KeyCode::Char('q') => {
                state.show_agent_detail = false;
            }
            _ => {}
        }
        return;
    }

    let filtered = state.filtered_agent_indices();

    match key.code {
        KeyCode::Up | KeyCode::Char('k') if state.agent_search.is_empty() => {
            if !filtered.is_empty() {
                let i = state.agent_state.selected().unwrap_or(0);
                state.agent_state.select(Some(if i == 0 { filtered.len() - 1 } else { i - 1 }));
            }
        }
        KeyCode::Down | KeyCode::Char('j') if state.agent_search.is_empty() => {
            if !filtered.is_empty() {
                let i = state.agent_state.selected().unwrap_or(0);
                state.agent_state.select(Some((i + 1) % filtered.len()));
            }
        }
        KeyCode::Up => {
            if !filtered.is_empty() {
                let i = state.agent_state.selected().unwrap_or(0);
                state.agent_state.select(Some(if i == 0 { filtered.len() - 1 } else { i - 1 }));
            }
        }
        KeyCode::Down => {
            if !filtered.is_empty() {
                let i = state.agent_state.selected().unwrap_or(0);
                state.agent_state.select(Some((i + 1) % filtered.len()));
            }
        }
        KeyCode::Char(' ') => {
            if let Some(view_idx) = state.agent_state.selected() {
                if view_idx < filtered.len() {
                    let real_idx = filtered[view_idx];
                    state.agent_items[real_idx].selected = !state.agent_items[real_idx].selected;
                }
            }
        }
        KeyCode::Char('a') if state.agent_search.is_empty() => {
            // Select all visible
            for &idx in &filtered {
                state.agent_items[idx].selected = true;
            }
        }
        KeyCode::Char('n') if state.agent_search.is_empty() => {
            // Select none
            for &idx in &filtered {
                state.agent_items[idx].selected = false;
            }
        }
        KeyCode::Enter => {
            state.step = Step::Summary;
        }
        KeyCode::Char('d') | KeyCode::Char('i') if state.agent_search.is_empty() => {
            state.show_agent_detail = true;
        }
        KeyCode::Backspace => {
            if !state.agent_search.is_empty() {
                state.agent_search.pop();
                state.agent_state.select(Some(0));
            }
        }
        KeyCode::Char(c) if !state.agent_search.is_empty() || (c != 'k' && c != 'j' && c != 'a' && c != 'n' && c != 'd' && c != 'i') => {
            // Search mode — any letter starts filtering
            if c.is_alphanumeric() || c == '-' || c == '_' {
                state.agent_search.push(c);
                state.agent_state.select(Some(0));
            }
        }
        _ => {}
    }
}

fn find_free_port(start: u16) -> u16 {
    for port in start..10000 {
        if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return port;
        }
    }
    start
}

fn handle_channels(state: &mut WizardState, key: crossterm::event::KeyEvent) {
    // Sub-field editing: telegram token
    if state.telegram_editing {
        match key.code {
            KeyCode::Char(c) => { state.telegram_token.push(c); }
            KeyCode::Backspace => { state.telegram_token.pop(); }
            KeyCode::Enter => { state.telegram_editing = false; }
            _ => {}
        }
        return;
    }
    // Sub-field editing: telegram chat IDs
    if state.telegram_chat_ids_editing {
        match key.code {
            KeyCode::Char(c) if c.is_ascii_digit() || c == ',' || c == ' ' || c == '-' => {
                state.telegram_chat_ids.push(c);
            }
            KeyCode::Backspace => { state.telegram_chat_ids.pop(); }
            KeyCode::Enter => { state.telegram_chat_ids_editing = false; }
            _ => {}
        }
        return;
    }
    // Sub-field editing: web password
    if state.web_editing {
        match key.code {
            KeyCode::Char(c) => { state.web_password.push(c); }
            KeyCode::Backspace => { state.web_password.pop(); }
            KeyCode::Enter => {
                state.web_editing = false;
                // After password, move to port sub-field
                state.channel_sub_field = 1;
            }
            _ => {}
        }
        return;
    }
    // Sub-field editing within Web channel (port, access, allowed IPs)
    if state.web_enabled && state.channel_focus == 1 && state.channel_sub_field > 0 {
        match state.channel_sub_field {
            1 => {
                // Port editing
                match key.code {
                    KeyCode::Char(c) if c.is_ascii_digit() => { state.web_port.push(c); }
                    KeyCode::Backspace => { state.web_port.pop(); }
                    KeyCode::Tab | KeyCode::Down => { state.channel_sub_field = 2; }
                    KeyCode::Up => { state.channel_sub_field = 0; }
                    KeyCode::Enter => { state.channel_sub_field = 0; }
                    _ => {}
                }
                return;
            }
            2 => {
                // Access mode cycle
                match key.code {
                    KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') => {
                        state.web_access = 1 - state.web_access;
                    }
                    KeyCode::Tab | KeyCode::Down => {
                        if state.web_access == 0 {
                            state.channel_sub_field = 3; // allowed IPs
                        } else {
                            state.channel_sub_field = 0;
                        }
                    }
                    KeyCode::Up => { state.channel_sub_field = 1; }
                    KeyCode::Enter => { state.channel_sub_field = 0; }
                    _ => {}
                }
                return;
            }
            3 => {
                // Allowed IPs editing
                match key.code {
                    KeyCode::Char(c) => { state.web_allowed_ips.push(c); }
                    KeyCode::Backspace => { state.web_allowed_ips.pop(); }
                    KeyCode::Tab | KeyCode::Down | KeyCode::Enter => { state.channel_sub_field = 0; }
                    KeyCode::Up => { state.channel_sub_field = 2; }
                    _ => {}
                }
                return;
            }
            _ => {}
        }
    }

    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            state.channel_focus = state.channel_focus.saturating_sub(1);
            state.channel_sub_field = 0;
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.channel_focus = (state.channel_focus + 1).min(2);
            state.channel_sub_field = 0;
        }
        KeyCode::Char(' ') => {
            match state.channel_focus {
                0 => {
                    state.telegram_enabled = !state.telegram_enabled;
                    if state.telegram_enabled && state.telegram_token.is_empty() {
                        state.telegram_editing = true;
                    }
                }
                1 => {
                    state.web_enabled = !state.web_enabled;
                    if state.web_enabled {
                        if state.web_password.is_empty() {
                            state.web_editing = true;
                        }
                        // Check port availability
                        if let Ok(port) = state.web_port.parse::<u16>() {
                            if std::net::TcpListener::bind(("127.0.0.1", port)).is_err() {
                                let free = find_free_port(port + 1);
                                state.web_port = free.to_string();
                            }
                        }
                    }
                }
                2 => {
                    state.download_webbert = !state.download_webbert;
                }
                _ => {}
            }
        }
        KeyCode::Tab => {
            // Tab into sub-fields when on an expanded channel
            if state.channel_focus == 0 && state.telegram_enabled && !state.telegram_token.is_empty() {
                state.telegram_chat_ids_editing = true;
            } else if state.channel_focus == 1 && state.web_enabled {
                state.channel_sub_field = 1;
            }
        }
        KeyCode::Enter => {
            // If focused on Telegram and token already set, edit chat IDs
            if state.channel_focus == 0 && state.telegram_enabled && !state.telegram_token.is_empty() {
                state.telegram_chat_ids_editing = true;
            } else {
                state.step = Step::Agents;
            }
        }
        _ => {}
    }
}

// ── Drawing ─────────────────────────────────────────────────────────────────

fn draw_wizard(frame: &mut ratatui::Frame, state: &WizardState) {
    let size = frame.area();
    let bg_block = Block::default().style(Style::default().bg(BG));
    frame.render_widget(bg_block, size);

    let outer = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(BG));
    let inner = outer.inner(size);
    frame.render_widget(outer, size);

    match state.step {
        Step::Welcome => draw_welcome(frame, inner),
        Step::Provider => draw_provider(frame, inner, state),
        Step::ApiKeyModel => draw_api_key_model(frame, inner, state),
        Step::Constitution => draw_constitution(frame, inner, state),
        Step::Persona => draw_persona(frame, inner, state),
        Step::Plugins => draw_plugins(frame, inner, state),
        Step::Studio => draw_studio(frame, inner, state),
        Step::Pea => draw_pea(frame, inner, state),
        Step::Watcher => draw_watcher(frame, inner, state),
        Step::Channels => draw_channels(frame, inner, state),
        Step::Agents => draw_agents(frame, inner, state),
        Step::Summary => draw_summary(frame, inner, state),
    }

    // Agent detail popup overlay
    if state.show_agent_detail {
        draw_agent_detail(frame, inner, state);
    }
}

fn draw_step_indicator(frame: &mut ratatui::Frame, area: Rect, current: Step) {
    let steps = Step::all();
    let mut spans = Vec::new();

    for (i, step) in steps.iter().enumerate() {
        let color = if step.index() < current.index() { STEP_DONE }
            else if *step == current { STEP_ACTIVE }
            else { STEP_TODO };

        let symbol = if step.index() < current.index() { "●" }
            else if *step == current { "◉" }
            else { "○" };

        spans.push(Span::styled(format!("{}", symbol), Style::default().fg(color).bg(BG)));
        if i < steps.len() - 1 {
            spans.push(Span::styled("──", Style::default().fg(STEP_TODO).bg(BG)));
        }
    }

    let p = Paragraph::new(Line::from(spans))
        .alignment(Alignment::Center)
        .style(Style::default().bg(BG));
    frame.render_widget(p, area);
}

fn draw_hint_bar(frame: &mut ratatui::Frame, area: Rect, hints: &[(&str, &str)]) {
    let mut spans = Vec::new();
    for (i, (key, desc)) in hints.iter().enumerate() {
        spans.push(Span::styled(format!(" {} ", key), Style::default().fg(ACCENT).bg(BG).add_modifier(Modifier::BOLD)));
        spans.push(Span::styled(desc.to_string(), Style::default().fg(DIM).bg(BG)));
        if i < hints.len() - 1 {
            spans.push(Span::styled(" · ", Style::default().fg(STEP_TODO).bg(BG)));
        }
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).alignment(Alignment::Center).style(Style::default().bg(BG)),
        area,
    );
}

fn draw_welcome(frame: &mut ratatui::Frame, area: Rect) {
    let chunks = Layout::vertical([
        Constraint::Percentage(12),
        Constraint::Length(9),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Min(0),
    ]).split(area);

    frame.render_widget(Paragraph::new(logo_art()).alignment(Alignment::Center).style(Style::default().bg(BG)), chunks[1]);
    frame.render_widget(Paragraph::new(title_line()).alignment(Alignment::Center).style(Style::default().bg(BG)), chunks[2]);
    frame.render_widget(Paragraph::new(version_line()).alignment(Alignment::Center).style(Style::default().bg(BG)), chunks[3]);

    let tagline = Paragraph::new(vec![
        Line::from(""),
        Line::from(vec![Span::styled("The agent that uses all agents", Style::default().fg(FG).bg(BG))]),
        Line::from(vec![Span::styled("cached when possible · autonomous when needed · independent always", Style::default().fg(DIM).bg(BG))]),
    ]).alignment(Alignment::Center).style(Style::default().bg(BG));
    frame.render_widget(tagline, chunks[5]);

    draw_hint_bar(frame, chunks[7], &[("Enter", "begin setup"), ("Esc", "quit")]);
}

fn draw_provider(frame: &mut ratatui::Frame, area: Rect, state: &WizardState) {
    let chunks = Layout::vertical([
        Constraint::Length(1), Constraint::Length(1), Constraint::Min(8), Constraint::Length(1),
    ]).split(area);

    draw_step_indicator(frame, chunks[0], Step::Provider);

    if state.provider_view == ProviderView::Custom {
        // Custom provider form
        let content_area = centered_rect(55, 40, chunks[2]);
        let block = Block::default()
            .borders(Borders::ALL).border_type(BorderType::Rounded)
            .border_style(Style::default().fg(BORDER))
            .title(Line::from(vec![
                Span::styled(" ", Style::default().bg(BG)),
                Span::styled("Custom Provider", Style::default().fg(ACCENT).bg(BG).add_modifier(Modifier::BOLD)),
                Span::styled(" ", Style::default().bg(BG)),
            ])).style(Style::default().bg(BG));
        let block_inner = block.inner(content_area);
        frame.render_widget(block, content_area);

        let name_fg = if state.custom_provider_field == 0 { ACCENT } else { FG };
        let url_fg = if state.custom_provider_field == 1 { ACCENT } else { FG };
        let name_cursor = if state.custom_provider_field == 0 { "_" } else { "" };
        let url_cursor = if state.custom_provider_field == 1 { "_" } else { "" };

        let lines = vec![
            Line::from(""),
            Line::from(vec![Span::styled("  Provider Name", Style::default().fg(HEADING).bg(BG).add_modifier(Modifier::BOLD))]),
            Line::from(vec![
                Span::styled(format!("  {}{}", state.custom_provider_name, name_cursor), Style::default().fg(name_fg).bg(BG)),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled("  Base URL", Style::default().fg(HEADING).bg(BG).add_modifier(Modifier::BOLD))]),
            Line::from(vec![
                Span::styled(format!("  {}{}", state.custom_provider_url, url_cursor), Style::default().fg(url_fg).bg(BG)),
            ]),
        ];
        frame.render_widget(Paragraph::new(lines).style(Style::default().bg(BG)), block_inner);
        draw_hint_bar(frame, chunks[3], &[("Tab", "switch field"), ("Enter", "confirm"), ("Esc", "back")]);
        return;
    }

    let list_area = centered_rect(60, 90, chunks[2]);

    let items: Vec<ListItem> = state.provider_items.iter().map(|item| {
        if item.is_header {
            let mut spans = vec![
                Span::styled(format!("  {} ", item.label), Style::default().fg(HEADING).bg(BG).add_modifier(Modifier::BOLD)),
            ];
            if !item.hint.is_empty() {
                spans.push(Span::styled(format!("  {}", item.hint), Style::default().fg(DIM).bg(BG)));
            }
            ListItem::new(Line::from(spans)).style(Style::default().bg(BG))
        } else {
            ListItem::new(Line::from(vec![
                Span::styled("    ", Style::default().bg(BG)),
                Span::styled(format!("{:<22}", item.label), Style::default().fg(FG).bg(BG)),
                Span::styled(item.hint.clone(), Style::default().fg(DIM).bg(BG)),
            ])).style(Style::default().bg(BG))
        }
    }).collect();

    let block = Block::default()
        .borders(Borders::ALL).border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .title(Line::from(vec![
            Span::styled(" ", Style::default().bg(BG)),
            Span::styled("LLM Provider", Style::default().fg(ACCENT).bg(BG).add_modifier(Modifier::BOLD)),
            Span::styled(" ", Style::default().bg(BG)),
        ])).style(Style::default().bg(BG));

    let list = List::new(items).block(block)
        .highlight_style(Style::default().bg(HIGHLIGHT_BG).fg(ACCENT).add_modifier(Modifier::BOLD))
        .highlight_symbol("  ▸ ");

    frame.render_stateful_widget(list, list_area, &mut state.provider_state.clone());
    draw_hint_bar(frame, chunks[3], &[("↑↓", "navigate"), ("Enter", "select"), ("Esc", "back")]);
}

fn draw_api_key_model(frame: &mut ratatui::Frame, area: Rect, state: &WizardState) {
    let chunks = Layout::vertical([
        Constraint::Length(1), Constraint::Length(1), Constraint::Min(8), Constraint::Length(1),
    ]).split(area);

    draw_step_indicator(frame, chunks[0], Step::ApiKeyModel);

    let content_area = centered_rect(60, 85, chunks[2]);
    let is_local = state.is_local_provider();

    let block = Block::default()
        .borders(Borders::ALL).border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .title(Line::from(vec![
            Span::styled(" ", Style::default().bg(BG)),
            Span::styled(format!("{} · API & Models", state.selected_provider_name), Style::default().fg(ACCENT).bg(BG).add_modifier(Modifier::BOLD)),
            Span::styled(" ", Style::default().bg(BG)),
        ])).style(Style::default().bg(BG));

    let block_inner = block.inner(content_area);
    frame.render_widget(block, content_area);

    let mut lines = Vec::new();
    lines.push(Line::from(""));

    if !is_local {
        lines.push(Line::from(vec![
            Span::styled("  API Key", Style::default().fg(HEADING).bg(BG).add_modifier(Modifier::BOLD)),
        ]));
        lines.push(Line::from(""));

        let display_key = if state.api_key_input.is_empty() {
            "  enter your API key...".to_string()
        } else if state.show_api_key {
            format!("  {}", state.api_key_input)
        } else {
            format!("  {}", mask_key(&state.api_key_input))
        };

        lines.push(Line::from(vec![
            Span::styled(display_key, Style::default().fg(if state.api_key_input.is_empty() { DIM } else { FG }).bg(BG)),
        ]));
        lines.push(Line::from(vec![
            Span::styled(
                if state.api_key_input.is_empty() { "  Enter with empty to skip" } else { "  Tab show/hide · Enter to discover models" },
                Style::default().fg(DIM).bg(BG),
            ),
        ]));
        let help_url = match state.selected_provider_id.as_str() {
            "anthropic" => "console.anthropic.com/settings/keys",
            "openai" => "platform.openai.com/api-keys",
            "google" => "aistudio.google.com/apikey",
            "deepseek" => "platform.deepseek.com/api_keys",
            "groq" => "console.groq.com/keys",
            "openrouter" => "openrouter.ai/settings/keys",
            "nanogpt" => "nano-gpt.com/api",
            "together" => "api.together.xyz/settings/api-keys",
            "fireworks" => "fireworks.ai/account/api-keys",
            "mistral" => "console.mistral.ai/api-keys",
            "huggingface" => "huggingface.co/settings/tokens",
            "cerebras" => "cloud.cerebras.ai/platform",
            "perplexity" => "perplexity.ai/settings/api",
            "deepinfra" => "deepinfra.com/dash/api_keys",
            _ => "",
        };
        if !help_url.is_empty() {
            lines.push(Line::from(vec![
                Span::styled(format!("  Get your key → {}", help_url), Style::default().fg(DIM).bg(BG)),
            ]));
        }
        lines.push(Line::from(""));
    } else {
        lines.push(Line::from(vec![
            Span::styled("  Local mode", Style::default().fg(GREEN).bg(BG).add_modifier(Modifier::BOLD)),
            Span::styled(" — no API key needed", Style::default().fg(DIM).bg(BG)),
        ]));
        lines.push(Line::from(vec![
            Span::styled(format!("  Server: {}", state.selected_base_url), Style::default().fg(FG).bg(BG)),
        ]));
        lines.push(Line::from(""));
    }

    // Model discovery status
    if state.models_loading {
        let spinner = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        let idx = (state.start_time.elapsed().as_millis() / 100) as usize % spinner.len();
        lines.push(Line::from(vec![
            Span::styled(format!("  {} Discovering models...", spinner[idx]), Style::default().fg(ACCENT).bg(BG)),
        ]));
    } else if let Some(ref err) = state.models_error {
        lines.push(Line::from(vec![
            Span::styled("  ▲ ", Style::default().fg(ACCENT2).bg(BG)),
            Span::styled(err.as_str(), Style::default().fg(DIM).bg(BG)),
        ]));
        if !state.primary_model.is_empty() {
            lines.push(Line::from(vec![
                Span::styled(format!("  Default: {}", state.primary_model), Style::default().fg(FG).bg(BG)),
            ]));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("  Press Enter to continue", Style::default().fg(DIM).bg(BG)),
        ]));
    }

    let text_p = Paragraph::new(lines).style(Style::default().bg(BG));
    frame.render_widget(text_p, block_inner);

    // Model list with multi-select
    if !state.models.is_empty() && !state.models_loading {
        let header_y = if is_local { 4 } else { 7 };
        let model_area = Rect::new(
            block_inner.x + 1,
            block_inner.y + header_y,
            block_inner.width.saturating_sub(2),
            block_inner.height.saturating_sub(header_y + 1),
        );

        let header = Line::from(vec![
            Span::styled(format!("  Models ({}) ", state.models.len()), Style::default().fg(HEADING).bg(BG).add_modifier(Modifier::BOLD)),
            Span::styled("Space=toggle  p=primary  Enter=confirm", Style::default().fg(DIM).bg(BG)),
        ]);
        let header_area = Rect::new(model_area.x, model_area.y.saturating_sub(1), model_area.width, 1);
        frame.render_widget(Paragraph::new(header).style(Style::default().bg(BG)), header_area);

        let model_items: Vec<ListItem> = state.models.iter().enumerate().map(|(i, m)| {
            let selected = state.model_selected.get(i).copied().unwrap_or(false);
            let is_primary = m == &state.primary_model;
            let check = if is_primary { "★" } else if selected { "◆" } else { "◇" };
            let check_color = if is_primary { ACCENT } else if selected { GREEN } else { DIM };
            ListItem::new(Line::from(vec![
                Span::styled(format!("  {} ", check), Style::default().fg(check_color).bg(BG)),
                Span::styled(m.clone(), Style::default().fg(FG).bg(BG)),
            ])).style(Style::default().bg(BG))
        }).collect();

        let model_list = List::new(model_items)
            .highlight_style(Style::default().bg(HIGHLIGHT_BG).fg(ACCENT).add_modifier(Modifier::BOLD))
            .highlight_symbol("▸ ");

        frame.render_stateful_widget(model_list, model_area, &mut state.model_state.clone());
    }

    let hints: Vec<(&str, &str)> = if !state.models.is_empty() {
        vec![("↑↓", "navigate"), ("Space", "toggle"), ("p", "primary"), ("Enter", "next")]
    } else if is_local {
        vec![("Enter", "discover models"), ("Esc", "back")]
    } else {
        vec![("type", "enter key"), ("Tab", "show/hide"), ("Enter", "next"), ("Esc", "back")]
    };
    draw_hint_bar(frame, chunks[3], &hints);
}

fn draw_constitution(frame: &mut ratatui::Frame, area: Rect, state: &WizardState) {
    let chunks = Layout::vertical([
        Constraint::Length(1), Constraint::Length(1), Constraint::Min(8), Constraint::Length(1),
    ]).split(area);
    draw_step_indicator(frame, chunks[0], Step::Constitution);

    let list_area = centered_rect(60, 90, chunks[2]);
    let items: Vec<ListItem> = state.constitution_items.iter().map(|(name, desc)| {
        ListItem::new(Line::from(vec![
            Span::styled("  ", Style::default().bg(BG)),
            Span::styled(format!("{:<20}", name), Style::default().fg(FG).bg(BG)),
            Span::styled(desc.clone(), Style::default().fg(DIM).bg(BG)),
        ])).style(Style::default().bg(BG))
    }).collect();

    let block = Block::default()
        .borders(Borders::ALL).border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .title(Line::from(vec![
            Span::styled(" ", Style::default().bg(BG)),
            Span::styled("Constitution Template", Style::default().fg(ACCENT).bg(BG).add_modifier(Modifier::BOLD)),
            Span::styled(" ", Style::default().bg(BG)),
        ]))
        .title_bottom(Line::from(vec![
            Span::styled(" Safety rules that govern agent behavior ", Style::default().fg(DIM).bg(BG)),
        ])).style(Style::default().bg(BG));

    let list = List::new(items).block(block)
        .highlight_style(Style::default().bg(HIGHLIGHT_BG).fg(ACCENT).add_modifier(Modifier::BOLD))
        .highlight_symbol("  ▸ ");
    frame.render_stateful_widget(list, list_area, &mut state.constitution_state.clone());
    draw_hint_bar(frame, chunks[3], &[("↑↓", "navigate"), ("Enter", "select"), ("Esc", "back")]);
}

fn draw_persona(frame: &mut ratatui::Frame, area: Rect, state: &WizardState) {
    let chunks = Layout::vertical([
        Constraint::Length(1), Constraint::Length(1), Constraint::Min(8), Constraint::Length(1),
    ]).split(area);
    draw_step_indicator(frame, chunks[0], Step::Persona);

    // Custom persona or wikipedia input mode
    if state.persona_edit_mode != PersonaEdit::None {
        let content_area = centered_rect(55, 35, chunks[2]);
        let (title, label, value) = match state.persona_edit_mode {
            PersonaEdit::Custom => ("Custom Persona", "Describe your agent's personality", &state.custom_persona_text),
            PersonaEdit::Wikipedia => ("Wikipedia Persona", "Enter Wikipedia URL", &state.wikipedia_url),
            _ => ("", "", &state.custom_persona_text),
        };
        let block = Block::default()
            .borders(Borders::ALL).border_type(BorderType::Rounded)
            .border_style(Style::default().fg(BORDER))
            .title(Line::from(vec![
                Span::styled(" ", Style::default().bg(BG)),
                Span::styled(title, Style::default().fg(ACCENT).bg(BG).add_modifier(Modifier::BOLD)),
                Span::styled(" ", Style::default().bg(BG)),
            ])).style(Style::default().bg(BG));
        let block_inner = block.inner(content_area);
        frame.render_widget(block, content_area);

        let lines = vec![
            Line::from(""),
            Line::from(vec![Span::styled(format!("  {}", label), Style::default().fg(HEADING).bg(BG).add_modifier(Modifier::BOLD))]),
            Line::from(""),
            Line::from(vec![Span::styled(format!("  {}_", value), Style::default().fg(ACCENT).bg(BG))]),
        ];
        frame.render_widget(Paragraph::new(lines).style(Style::default().bg(BG)), block_inner);
        draw_hint_bar(frame, chunks[3], &[("type", "enter text"), ("Enter", "confirm"), ("Esc", "back")]);
        return;
    }

    let list_area = centered_rect(55, 80, chunks[2]);
    let mut items: Vec<ListItem> = Vec::new();
    let mut last_category = String::new();
    for p in &state.persona_items {
        if p.category != last_category {
            items.push(ListItem::new(Line::from(vec![
                Span::styled(format!("  {} ", p.category), Style::default().fg(HEADING).bg(BG).add_modifier(Modifier::BOLD)),
            ])).style(Style::default().bg(BG)));
            last_category = p.category.clone();
        }
        items.push(ListItem::new(Line::from(vec![
            Span::styled("    ", Style::default().bg(BG)),
            Span::styled(format!("{:<16}", p.name), Style::default().fg(FG).bg(BG).add_modifier(Modifier::BOLD)),
            Span::styled(p.description.clone(), Style::default().fg(DIM).bg(BG)),
        ])).style(Style::default().bg(BG)));
    }

    let block = Block::default()
        .borders(Borders::ALL).border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .title(Line::from(vec![
            Span::styled(" ", Style::default().bg(BG)),
            Span::styled("Persona & Style", Style::default().fg(ACCENT).bg(BG).add_modifier(Modifier::BOLD)),
            Span::styled(" ", Style::default().bg(BG)),
        ]))
        .title_bottom(Line::from(vec![
            Span::styled(" Choose your agent's personality and voice ", Style::default().fg(DIM).bg(BG)),
        ])).style(Style::default().bg(BG));

    let list = List::new(items).block(block)
        .highlight_style(Style::default().bg(HIGHLIGHT_BG).fg(ACCENT).add_modifier(Modifier::BOLD))
        .highlight_symbol("  ▸ ");
    frame.render_stateful_widget(list, list_area, &mut state.persona_state.clone());
    draw_hint_bar(frame, chunks[3], &[("↑↓", "navigate"), ("Enter", "select"), ("Esc", "back")]);
}

fn draw_agents(frame: &mut ratatui::Frame, area: Rect, state: &WizardState) {
    let chunks = Layout::vertical([
        Constraint::Length(1), Constraint::Length(1), Constraint::Min(8), Constraint::Length(1),
    ]).split(area);
    draw_step_indicator(frame, chunks[0], Step::Agents);

    let list_area = centered_rect(70, 90, chunks[2]);
    let filtered = state.filtered_agent_indices();
    let selected_count = state.agent_items.iter().filter(|a| a.selected).count();

    let items: Vec<ListItem> = filtered.iter().map(|&idx| {
        let a = &state.agent_items[idx];
        let check = if a.selected { "◆" } else { "◇" };
        let check_color = if a.selected { GREEN } else { DIM };
        ListItem::new(Line::from(vec![
            Span::styled(format!(" {} ", check), Style::default().fg(check_color).bg(BG)),
            Span::styled(format!("{:<24}", a.name), Style::default().fg(FG).bg(BG)),
            Span::styled(format!("{:<16}", a.category), Style::default().fg(HEADING).bg(BG)),
            Span::styled(a.description.clone(), Style::default().fg(DIM).bg(BG)),
        ])).style(Style::default().bg(BG))
    }).collect();

    let title_text = if state.agent_search.is_empty() {
        format!("Agents ({}) · {} selected", state.agent_items.len(), selected_count)
    } else {
        format!("Search: {} · {} found · {} selected", state.agent_search, filtered.len(), selected_count)
    };

    let block = Block::default()
        .borders(Borders::ALL).border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .title(Line::from(vec![
            Span::styled(" ", Style::default().bg(BG)),
            Span::styled(title_text, Style::default().fg(ACCENT).bg(BG).add_modifier(Modifier::BOLD)),
            Span::styled(" ", Style::default().bg(BG)),
        ])).style(Style::default().bg(BG));

    let list = List::new(items).block(block)
        .highlight_style(Style::default().bg(HIGHLIGHT_BG).fg(ACCENT).add_modifier(Modifier::BOLD))
        .highlight_symbol("▸ ");
    frame.render_stateful_widget(list, list_area, &mut state.agent_state.clone());
    draw_hint_bar(frame, chunks[3], &[("↑↓", "navigate"), ("Space", "toggle"), ("a", "all"), ("n", "none"), ("type", "search"), ("Enter", "next")]);
}

fn draw_plugins(frame: &mut ratatui::Frame, area: Rect, state: &WizardState) {
    let chunks = Layout::vertical([
        Constraint::Length(1), Constraint::Length(1), Constraint::Min(8), Constraint::Length(1),
    ]).split(area);
    draw_step_indicator(frame, chunks[0], Step::Plugins);

    let list_area = centered_rect(60, 90, chunks[2]);
    let selected_count = state.plugin_items.iter().filter(|p| p.selected).count();

    let items: Vec<ListItem> = state.plugin_items.iter().map(|p| {
        let check = if p.selected { "◆" } else { "◇" };
        let check_color = if p.selected { GREEN } else { DIM };
        ListItem::new(Line::from(vec![
            Span::styled(format!("  {} ", check), Style::default().fg(check_color).bg(BG)),
            Span::styled(format!("{:<18}", p.name), Style::default().fg(FG).bg(BG)),
            Span::styled(p.description.clone(), Style::default().fg(DIM).bg(BG)),
        ])).style(Style::default().bg(BG))
    }).collect();

    let block = Block::default()
        .borders(Borders::ALL).border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .title(Line::from(vec![
            Span::styled(" ", Style::default().bg(BG)),
            Span::styled(format!("Plugins · {} enabled", selected_count), Style::default().fg(ACCENT).bg(BG).add_modifier(Modifier::BOLD)),
            Span::styled(" ", Style::default().bg(BG)),
        ]))
        .title_bottom(Line::from(vec![
            Span::styled(" Enable modules for your agent to use ", Style::default().fg(DIM).bg(BG)),
        ])).style(Style::default().bg(BG));

    let list = List::new(items).block(block)
        .highlight_style(Style::default().bg(HIGHLIGHT_BG).fg(ACCENT).add_modifier(Modifier::BOLD))
        .highlight_symbol("▸ ");
    frame.render_stateful_widget(list, list_area, &mut state.plugin_state.clone());
    draw_hint_bar(frame, chunks[3], &[("↑↓", "navigate"), ("Space", "toggle"), ("a", "all"), ("n", "none"), ("Enter", "next")]);
}

fn draw_studio(frame: &mut ratatui::Frame, area: Rect, state: &WizardState) {
    let chunks = Layout::vertical([
        Constraint::Length(1), Constraint::Length(1), Constraint::Min(8), Constraint::Length(1),
    ]).split(area);
    draw_step_indicator(frame, chunks[0], Step::Studio);

    // Contextual help for the highlighted provider
    let help_text = match state.studio_state.selected() {
        Some(0) => "Unsplash: Sign up at unsplash.com/developers → Create App → copy Access Key. Free 50 req/hr.",
        Some(1) => "Pexels: Sign up at pexels.com/api → copy API Key from dashboard. Free 200 req/hr.",
        Some(3) => "fal.ai: Sign up at fal.ai → Settings → API Keys → Create Key.",
        _ => "Enable stock photo providers for styled PEA documents. Optional — skip with 's'.",
    };
    frame.render_widget(
        Paragraph::new(help_text).style(Style::default().fg(DIM).bg(BG)).alignment(ratatui::layout::Alignment::Center),
        chunks[1],
    );

    let list_area = centered_rect(60, 75, chunks[2]);
    let selected_count = state.studio_items.iter().filter(|s| s.selected).count();

    let mut items: Vec<ListItem> = Vec::new();
    for (i, s) in state.studio_items.iter().enumerate() {
        let check = if s.selected { "◆" } else { "◇" };
        let check_color = if s.selected { GREEN } else { DIM };
        let mut spans = vec![
            Span::styled(format!("  {} ", check), Style::default().fg(check_color).bg(BG)),
            Span::styled(format!("{:<18}", s.name), Style::default().fg(FG).bg(BG)),
            Span::styled(s.description.clone(), Style::default().fg(DIM).bg(BG)),
        ];
        // Show key status for cloud providers
        if s.selected && s.needs_key {
            if s.api_key.is_empty() {
                spans.push(Span::styled("  ✗ needs key", Style::default().fg(ACCENT2).bg(BG)));
            } else {
                spans.push(Span::styled(format!("  ✓ {}", mask_key(&s.api_key)), Style::default().fg(GREEN).bg(BG)));
            }
        }
        items.push(ListItem::new(Line::from(spans)).style(Style::default().bg(BG)));

        // Show inline key input if editing this provider's key
        if state.studio_editing_key && state.studio_key_idx == Some(i) {
            items.push(ListItem::new(Line::from(vec![
                Span::styled("      API Key: ", Style::default().fg(HEADING).bg(BG)),
                Span::styled(format!("{}_", s.api_key), Style::default().fg(ACCENT).bg(BG)),
            ])).style(Style::default().bg(BG)));
        }
    }

    let block = Block::default()
        .borders(Borders::ALL).border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .title(Line::from(vec![
            Span::styled(" ", Style::default().bg(BG)),
            Span::styled(format!("Studio · {} enabled", selected_count), Style::default().fg(ACCENT).bg(BG).add_modifier(Modifier::BOLD)),
            Span::styled(" ", Style::default().bg(BG)),
        ]))
        .title_bottom(Line::from(vec![
            Span::styled(" Stock photos + media providers · s=skip ", Style::default().fg(DIM).bg(BG)),
        ])).style(Style::default().bg(BG));

    let list = List::new(items).block(block)
        .highlight_style(Style::default().bg(HIGHLIGHT_BG).fg(ACCENT).add_modifier(Modifier::BOLD))
        .highlight_symbol("▸ ");
    frame.render_stateful_widget(list, list_area, &mut state.studio_state.clone());
    let hints: Vec<(&str, &str)> = if state.studio_editing_key {
        vec![("type", "enter key"), ("Enter", "confirm"), ("Esc", "cancel")]
    } else {
        vec![("↑↓", "navigate"), ("Space", "toggle"), ("s", "skip"), ("Enter", "next")]
    };
    draw_hint_bar(frame, chunks[3], &hints);
}

fn draw_pea(frame: &mut ratatui::Frame, area: Rect, state: &WizardState) {
    let chunks = Layout::vertical([
        Constraint::Length(1), Constraint::Length(1), Constraint::Min(8), Constraint::Length(1),
    ]).split(area);
    draw_step_indicator(frame, chunks[0], Step::Pea);

    let content_area = centered_rect(55, 55, chunks[2]);
    let block = Block::default()
        .borders(Borders::ALL).border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .title(Line::from(vec![
            Span::styled(" ", Style::default().bg(BG)),
            Span::styled("PEA · Autonomous Agent Settings", Style::default().fg(ACCENT).bg(BG).add_modifier(Modifier::BOLD)),
            Span::styled(" ", Style::default().bg(BG)),
        ])).style(Style::default().bg(BG));

    let block_inner = block.inner(content_area);
    frame.render_widget(block, content_area);

    let (strategy_id, strategy_desc) = PEA_STRATEGIES.get(state.pea_strategy_idx).unwrap_or(&("adaptive", ""));

    let field_style = |idx: usize| -> (Color, Color) {
        if state.pea_field == idx { (ACCENT, HIGHLIGHT_BG) } else { (FG, BG) }
    };

    let mut lines = Vec::new();
    lines.push(Line::from(""));

    // Strategy field
    let (fg0, bg0) = field_style(0);
    let marker0 = if state.pea_field == 0 { "▸" } else { " " };
    lines.push(Line::from(vec![
        Span::styled(format!("  {} ", marker0), Style::default().fg(ACCENT).bg(bg0)),
        Span::styled("Strategy    ", Style::default().fg(HEADING).bg(bg0).add_modifier(Modifier::BOLD)),
        Span::styled("◄ ", Style::default().fg(DIM).bg(bg0)),
        Span::styled(strategy_id.to_string(), Style::default().fg(fg0).bg(bg0).add_modifier(Modifier::BOLD)),
        Span::styled(" ►", Style::default().fg(DIM).bg(bg0)),
    ]));
    lines.push(Line::from(vec![
        Span::styled(format!("               {}", strategy_desc), Style::default().fg(DIM).bg(BG)),
    ]));
    lines.push(Line::from(""));

    // Budget field
    let (fg1, bg1) = field_style(1);
    let marker1 = if state.pea_field == 1 { "▸" } else { " " };
    let budget_display = if state.pea_field == 1 {
        format!("${}_", state.pea_budget_input)
    } else {
        format!("${}", state.pea_budget_input)
    };
    lines.push(Line::from(vec![
        Span::styled(format!("  {} ", marker1), Style::default().fg(ACCENT).bg(bg1)),
        Span::styled("Budget      ", Style::default().fg(HEADING).bg(bg1).add_modifier(Modifier::BOLD)),
        Span::styled(budget_display, Style::default().fg(fg1).bg(bg1).add_modifier(Modifier::BOLD)),
        Span::styled(" /month", Style::default().fg(DIM).bg(bg1)),
    ]));
    lines.push(Line::from(""));

    // Heartbeat field
    let (fg2, bg2) = field_style(2);
    let marker2 = if state.pea_field == 2 { "▸" } else { " " };
    let hb_display = if state.pea_field == 2 {
        format!("{}_", state.pea_heartbeat_input)
    } else {
        state.pea_heartbeat_input.clone()
    };
    lines.push(Line::from(vec![
        Span::styled(format!("  {} ", marker2), Style::default().fg(ACCENT).bg(bg2)),
        Span::styled("Heartbeat   ", Style::default().fg(HEADING).bg(bg2).add_modifier(Modifier::BOLD)),
        Span::styled(hb_display, Style::default().fg(fg2).bg(bg2).add_modifier(Modifier::BOLD)),
        Span::styled(" seconds", Style::default().fg(DIM).bg(bg2)),
    ]));

    frame.render_widget(Paragraph::new(lines).style(Style::default().bg(BG)), block_inner);

    draw_hint_bar(frame, chunks[3], &[("↑↓", "field"), ("←→", "strategy"), ("type", "edit value"), ("Enter", "next"), ("Esc", "back")]);
}

fn draw_agent_detail(frame: &mut ratatui::Frame, area: Rect, state: &WizardState) {
    let filtered = state.filtered_agent_indices();
    let view_idx = state.agent_state.selected().unwrap_or(0);
    if view_idx >= filtered.len() { return; }
    let agent = &state.agent_items[filtered[view_idx]];

    let popup = centered_rect(60, 60, area);

    // Clear background
    let clear = Block::default().style(Style::default().bg(Color::Rgb(15, 15, 20)));
    frame.render_widget(clear, popup);

    let block = Block::default()
        .borders(Borders::ALL).border_type(BorderType::Double)
        .border_style(Style::default().fg(ACCENT))
        .title(Line::from(vec![
            Span::styled(" ", Style::default().bg(BG)),
            Span::styled(agent.name.clone(), Style::default().fg(ACCENT).bg(BG).add_modifier(Modifier::BOLD)),
            Span::styled(" ", Style::default().bg(BG)),
        ])).style(Style::default().bg(BG));

    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let row = |label: &str, value: &str| -> Line<'static> {
        Line::from(vec![
            Span::styled(format!("  {:<14}", label), Style::default().fg(HEADING).bg(BG)),
            Span::styled(value.to_string(), Style::default().fg(FG).bg(BG)),
        ])
    };

    let check = if agent.selected { "◆ Selected" } else { "◇ Not selected" };
    let check_color = if agent.selected { GREEN } else { DIM };

    let mut lines = vec![
        Line::from(""),
        row("Name", &agent.name),
        row("Version", &agent.version),
        row("Category", &agent.category),
        row("Author", &agent.author),
        row("License", &agent.license),
        Line::from(""),
        row("Description", &agent.description),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Permissions   ", Style::default().fg(HEADING).bg(BG)),
            Span::styled(agent.permissions.join(", "), Style::default().fg(ACCENT2).bg(BG)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(format!("  {} ", check), Style::default().fg(check_color).bg(BG)),
            Span::styled("  Space=toggle · Esc=close", Style::default().fg(DIM).bg(BG)),
        ]),
    ];
    // Pad to fill
    while lines.len() < inner.height as usize {
        lines.push(Line::from(""));
    }

    frame.render_widget(Paragraph::new(lines).style(Style::default().bg(BG)), inner);
}

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
    let marker = if focused { "\u{25b8}" } else { " " };
    let check = if state.watcher_enabled { "\u{25c6}" } else { "\u{25c7}" };
    let check_color = if state.watcher_enabled { GREEN } else { DIM };
    let bg = if focused { HIGHLIGHT_BG } else { BG };
    lines.push(Line::from(vec![
        Span::styled(format!("  {} ", marker), Style::default().fg(ACCENT).bg(bg)),
        Span::styled(format!("{} ", check), Style::default().fg(check_color).bg(bg)),
        Span::styled("Enable Watcher", Style::default().fg(if focused { ACCENT } else { FG }).bg(bg).add_modifier(if focused { Modifier::BOLD } else { Modifier::empty() })),
    ]));

    if state.watcher_enabled {
        lines.push(Line::from(""));

        let f = state.watcher_focus == 1;
        let m = if f { "\u{25b8}" } else { " " };
        lines.push(Line::from(vec![
            Span::styled(format!("      {} ", m), Style::default().fg(ACCENT).bg(if f { HIGHLIGHT_BG } else { BG })),
            Span::styled("Alert Threshold ", Style::default().fg(HEADING).bg(if f { HIGHLIGHT_BG } else { BG })),
            Span::styled(format!("\u{25c4} {} \u{25ba}", alert_vals[state.watcher_alert_idx]), Style::default().fg(if f { ACCENT } else { FG }).bg(if f { HIGHLIGHT_BG } else { BG })),
        ]));

        let f = state.watcher_focus == 2;
        let m = if f { "\u{25b8}" } else { " " };
        lines.push(Line::from(vec![
            Span::styled(format!("      {} ", m), Style::default().fg(ACCENT).bg(if f { HIGHLIGHT_BG } else { BG })),
            Span::styled("Pause Threshold ", Style::default().fg(HEADING).bg(if f { HIGHLIGHT_BG } else { BG })),
            Span::styled(format!("\u{25c4} {} \u{25ba}", pause_vals[state.watcher_pause_idx]), Style::default().fg(if f { ACCENT } else { FG }).bg(if f { HIGHLIGHT_BG } else { BG })),
        ]));

        let f = state.watcher_focus == 3;
        let m = if f { "\u{25b8}" } else { " " };
        lines.push(Line::from(vec![
            Span::styled(format!("      {} ", m), Style::default().fg(ACCENT).bg(if f { HIGHLIGHT_BG } else { BG })),
            Span::styled("Alert Channel   ", Style::default().fg(HEADING).bg(if f { HIGHLIGHT_BG } else { BG })),
            Span::styled(format!("\u{25c4} {} \u{25ba}", channel_vals[state.watcher_channel_idx]), Style::default().fg(if f { ACCENT } else { FG }).bg(if f { HIGHLIGHT_BG } else { BG })),
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
    draw_hint_bar(frame, chunks[3], &[("\u{2191}\u{2193}", "navigate"), ("Space", "toggle"), ("\u{2190}\u{2192}", "change"), ("Enter", "next"), ("Esc", "back")]);
}

fn draw_channels(frame: &mut ratatui::Frame, area: Rect, state: &WizardState) {
    let chunks = Layout::vertical([
        Constraint::Length(1), Constraint::Length(1), Constraint::Min(8), Constraint::Length(1),
    ]).split(area);
    draw_step_indicator(frame, chunks[0], Step::Channels);

    let content_area = centered_rect(55, 65, chunks[2]);
    let block = Block::default()
        .borders(Borders::ALL).border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .title(Line::from(vec![
            Span::styled(" ", Style::default().bg(BG)),
            Span::styled("Channels & Extras", Style::default().fg(ACCENT).bg(BG).add_modifier(Modifier::BOLD)),
            Span::styled(" ", Style::default().bg(BG)),
        ])).style(Style::default().bg(BG));

    let block_inner = block.inner(content_area);
    frame.render_widget(block, content_area);

    let mut lines = Vec::new();
    lines.push(Line::from(""));

    // Telegram
    {
        let focused = state.channel_focus == 0;
        let marker = if focused { "▸" } else { " " };
        let check = if state.telegram_enabled { "◆" } else { "◇" };
        let check_color = if state.telegram_enabled { GREEN } else { DIM };
        let bg = if focused { HIGHLIGHT_BG } else { BG };

        let mut spans = vec![
            Span::styled(format!("  {} ", marker), Style::default().fg(ACCENT).bg(bg)),
            Span::styled(format!("{} ", check), Style::default().fg(check_color).bg(bg)),
            Span::styled(format!("{:<18}", "Telegram Bot"), Style::default().fg(if focused { ACCENT } else { FG }).bg(bg).add_modifier(if focused { Modifier::BOLD } else { Modifier::empty() })),
        ];
        if state.telegram_editing {
            spans.push(Span::styled(format!("{}_", state.telegram_token), Style::default().fg(ACCENT).bg(bg)));
        } else if !state.telegram_token.is_empty() {
            spans.push(Span::styled(mask_key(&state.telegram_token), Style::default().fg(DIM).bg(bg)));
        }
        lines.push(Line::from(spans));
        if state.telegram_editing || (focused && state.telegram_token.is_empty()) {
            lines.push(Line::from(vec![
                Span::styled("      Talk to @BotFather on Telegram:", Style::default().fg(DIM).bg(BG)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("      1. Send /newbot, choose a name", Style::default().fg(DIM).bg(BG)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("      2. Copy the token (123456:ABC-DEF...)", Style::default().fg(DIM).bg(BG)),
            ]));
        }

        // Chat IDs sub-field
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
    }

    // Web Dashboard
    {
        let focused = state.channel_focus == 1;
        let marker = if focused && state.channel_sub_field == 0 { "▸" } else { " " };
        let check = if state.web_enabled { "◆" } else { "◇" };
        let check_color = if state.web_enabled { GREEN } else { DIM };
        let bg = if focused && state.channel_sub_field == 0 { HIGHLIGHT_BG } else { BG };

        let mut spans = vec![
            Span::styled(format!("  {} ", marker), Style::default().fg(ACCENT).bg(bg)),
            Span::styled(format!("{} ", check), Style::default().fg(check_color).bg(bg)),
            Span::styled(format!("{:<18}", "Web Dashboard"), Style::default().fg(if focused { ACCENT } else { FG }).bg(bg).add_modifier(if focused { Modifier::BOLD } else { Modifier::empty() })),
        ];
        if state.web_editing {
            spans.push(Span::styled(format!("pw: {}_", state.web_password), Style::default().fg(ACCENT).bg(bg)));
        } else if !state.web_password.is_empty() {
            spans.push(Span::styled(format!("pw: {}", mask_key(&state.web_password)), Style::default().fg(DIM).bg(bg)));
        }
        lines.push(Line::from(spans));
        if state.web_editing {
            lines.push(Line::from(vec![
                Span::styled("      Choose a login password for the web dashboard", Style::default().fg(DIM).bg(BG)),
            ]));
        }

        // Web sub-fields (shown when web is enabled)
        if state.web_enabled {
            let port_focused = focused && state.channel_sub_field == 1;
            let port_marker = if port_focused { "▸" } else { " " };
            let port_cursor = if port_focused { "_" } else { "" };
            lines.push(Line::from(vec![
                Span::styled(format!("      {} ", port_marker), Style::default().fg(ACCENT).bg(if port_focused { HIGHLIGHT_BG } else { BG })),
                Span::styled("Port          ", Style::default().fg(HEADING).bg(if port_focused { HIGHLIGHT_BG } else { BG })),
                Span::styled(format!("{}{}", state.web_port, port_cursor), Style::default().fg(if port_focused { ACCENT } else { FG }).bg(if port_focused { HIGHLIGHT_BG } else { BG })),
            ]));

            let access_focused = focused && state.channel_sub_field == 2;
            let access_marker = if access_focused { "▸" } else { " " };
            let access_label = if state.web_access == 0 { "Private (localhost)" } else { "Public (any IP)" };
            lines.push(Line::from(vec![
                Span::styled(format!("      {} ", access_marker), Style::default().fg(ACCENT).bg(if access_focused { HIGHLIGHT_BG } else { BG })),
                Span::styled("Access        ", Style::default().fg(HEADING).bg(if access_focused { HIGHLIGHT_BG } else { BG })),
                Span::styled(format!("◄ {} ►", access_label), Style::default().fg(if access_focused { ACCENT } else { FG }).bg(if access_focused { HIGHLIGHT_BG } else { BG })),
            ]));

            if state.web_access == 0 {
                let ips_focused = focused && state.channel_sub_field == 3;
                let ips_marker = if ips_focused { "▸" } else { " " };
                let ips_cursor = if ips_focused { "_" } else { "" };
                lines.push(Line::from(vec![
                    Span::styled(format!("      {} ", ips_marker), Style::default().fg(ACCENT).bg(if ips_focused { HIGHLIGHT_BG } else { BG })),
                    Span::styled("Allowed IPs   ", Style::default().fg(HEADING).bg(if ips_focused { HIGHLIGHT_BG } else { BG })),
                    Span::styled(
                        if state.web_allowed_ips.is_empty() && !ips_focused { "optional".to_string() } else { format!("{}{}", state.web_allowed_ips, ips_cursor) },
                        Style::default().fg(if ips_focused { ACCENT } else { DIM }).bg(if ips_focused { HIGHLIGHT_BG } else { BG }),
                    ),
                ]));
            }
        }
    }

    // WebBERT
    {
        let focused = state.channel_focus == 2;
        let marker = if focused { "▸" } else { " " };
        let check = if state.download_webbert { "◆" } else { "◇" };
        let check_color = if state.download_webbert { GREEN } else { DIM };
        let bg = if focused { HIGHLIGHT_BG } else { BG };

        lines.push(Line::from(vec![
            Span::styled(format!("  {} ", marker), Style::default().fg(ACCENT).bg(bg)),
            Span::styled(format!("{} ", check), Style::default().fg(check_color).bg(bg)),
            Span::styled(format!("{:<18}", "WebBERT Model"), Style::default().fg(if focused { ACCENT } else { FG }).bg(bg).add_modifier(if focused { Modifier::BOLD } else { Modifier::empty() })),
            Span::styled("~256MB local classifier", Style::default().fg(DIM).bg(bg)),
        ]));
    }

    frame.render_widget(Paragraph::new(lines).style(Style::default().bg(BG)), block_inner);
    draw_hint_bar(frame, chunks[3], &[("↑↓", "navigate"), ("Space", "toggle"), ("Tab", "sub-fields"), ("Enter", "next"), ("Esc", "back")]);
}

fn draw_summary(frame: &mut ratatui::Frame, area: Rect, state: &WizardState) {
    let chunks = Layout::vertical([
        Constraint::Length(1), Constraint::Length(1), Constraint::Min(8), Constraint::Length(1),
    ]).split(area);
    draw_step_indicator(frame, chunks[0], Step::Summary);

    let content_area = centered_rect(55, 75, chunks[2]);
    let block = Block::default()
        .borders(Borders::ALL).border_type(BorderType::Rounded)
        .border_style(Style::default().fg(GREEN))
        .title(Line::from(vec![
            Span::styled(" ", Style::default().bg(BG)),
            Span::styled("Ready to Go", Style::default().fg(GREEN).bg(BG).add_modifier(Modifier::BOLD)),
            Span::styled(" ", Style::default().bg(BG)),
        ])).style(Style::default().bg(BG));

    let block_inner = block.inner(content_area);
    frame.render_widget(block, content_area);

    let mut lines = Vec::new();
    lines.push(Line::from(""));

    let row = |label: &str, value: &str, color: Color| -> Line<'static> {
        Line::from(vec![
            Span::styled(format!("    {:<18}", label), Style::default().fg(DIM).bg(BG)),
            Span::styled(value.to_string(), Style::default().fg(color).bg(BG).add_modifier(Modifier::BOLD)),
        ])
    };

    if state.provider_view == ProviderView::Custom {
        lines.push(row("Provider", &format!("{} (custom)", state.custom_provider_name), FG));
        lines.push(row("URL", &state.custom_provider_url, DIM));
    } else {
        lines.push(row("Provider", &state.selected_provider_name, FG));
    }

    let selected_models = state.selected_models();
    if !selected_models.is_empty() {
        lines.push(row("Primary Model", &state.primary_model, FG));
        if selected_models.len() > 1 {
            lines.push(row("Extra Models", &format!("{} more", selected_models.len() - 1), DIM));
        }
    }

    if !state.api_key_input.is_empty() {
        lines.push(row("API Key", &mask_key(&state.api_key_input), GREEN));
    }
    lines.push(row("Constitution", &state.selected_constitution, FG));
    lines.push(row("Persona", &state.selected_persona, FG));

    let plugin_count = state.plugin_items.iter().filter(|p| p.selected).count();
    if plugin_count > 0 {
        lines.push(row("Plugins", &format!("{} enabled", plugin_count), FG));
    }
    let studio_count = state.studio_items.iter().filter(|s| s.selected).count();
    if studio_count > 0 {
        lines.push(row("Studio", &format!("{} providers", studio_count), FG));
    }

    let strategy = PEA_STRATEGIES.get(state.pea_strategy_idx).map(|(id, _)| *id).unwrap_or("adaptive");
    lines.push(row("PEA Strategy", strategy, FG));
    lines.push(row("PEA Budget", &format!("${}/mo", state.pea_budget_input), FG));
    lines.push(row("PEA Heartbeat", &format!("{}s", state.pea_heartbeat_input), FG));

    let selected_agents = state.selected_agents();
    if !selected_agents.is_empty() {
        lines.push(row("Agents", &format!("{} selected", selected_agents.len()), FG));
    }

    // Studio key status
    let studio_with_keys: Vec<&StudioItem> = state.studio_items.iter()
        .filter(|s| s.selected && s.needs_key)
        .collect();
    if !studio_with_keys.is_empty() {
        for s in &studio_with_keys {
            let status = if s.api_key.is_empty() { "✗" } else { "✓" };
            let color = if s.api_key.is_empty() { ACCENT2 } else { GREEN };
            lines.push(Line::from(vec![
                Span::styled(format!("    {:<18}", format!("  {} {}", status, s.name)), Style::default().fg(color).bg(BG)),
            ]));
        }
    }

    lines.push(Line::from(""));

    let ck = |on: bool| if on { "◆" } else { "◇" };
    let cc = |on: bool| if on { GREEN } else { DIM };

    lines.push(Line::from(vec![
        Span::styled("      ", Style::default().bg(BG)),
        Span::styled(ck(state.telegram_enabled), Style::default().fg(cc(state.telegram_enabled)).bg(BG)),
        Span::styled(" Telegram  ", Style::default().fg(FG).bg(BG)),
        Span::styled(ck(state.web_enabled), Style::default().fg(cc(state.web_enabled)).bg(BG)),
        Span::styled(" Web  ", Style::default().fg(FG).bg(BG)),
        Span::styled(ck(state.download_webbert), Style::default().fg(cc(state.download_webbert)).bg(BG)),
        Span::styled(" WebBERT", Style::default().fg(FG).bg(BG)),
    ]));

    if state.web_enabled {
        let access_label = if state.web_access == 0 { "private" } else { "public" };
        lines.push(row("Web Port", &format!(":{} ({})", state.web_port, access_label), DIM));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("    Press Enter to write configuration", Style::default().fg(ACCENT).bg(BG)),
    ]));

    frame.render_widget(Paragraph::new(lines).style(Style::default().bg(BG)), block_inner);
    draw_hint_bar(frame, chunks[3], &[("Enter", "confirm & write"), ("b", "go back"), ("Esc", "back")]);
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let [_, center_v, _] = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ]).areas(area);
    let [_, center, _] = Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ]).areas(center_v);
    center
}

fn mask_key(key: &str) -> String {
    if key.len() > 8 {
        format!("{}···{}", &key[..4], &key[key.len() - 4..])
    } else if key.is_empty() {
        String::new()
    } else {
        "••••••••".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_key() {
        assert_eq!(mask_key("sk-abc123456789xyz"), "sk-a···9xyz");
        assert_eq!(mask_key("short"), "••••••••");
        assert_eq!(mask_key(""), "");
    }

    #[test]
    fn test_step_navigation() {
        assert_eq!(Step::Welcome.next(), Step::Provider);
        assert_eq!(Step::Provider.next(), Step::ApiKeyModel);
        assert_eq!(Step::ApiKeyModel.next(), Step::Constitution);
        assert_eq!(Step::Constitution.next(), Step::Persona);
        assert_eq!(Step::Persona.next(), Step::Plugins);
        assert_eq!(Step::Plugins.next(), Step::Studio);
        assert_eq!(Step::Studio.next(), Step::Pea);
        assert_eq!(Step::Pea.next(), Step::Watcher);
        assert_eq!(Step::Watcher.next(), Step::Channels);
        assert_eq!(Step::Channels.next(), Step::Agents);
        assert_eq!(Step::Agents.next(), Step::Summary);
        assert_eq!(Step::Summary.prev(), Step::Agents);
        assert_eq!(Step::Welcome.prev(), Step::Welcome);
    }

    #[test]
    fn test_wizard_state_creation() {
        let state = WizardState::new();
        assert_eq!(state.step, Step::Welcome);
        assert!(!state.provider_items.is_empty());
        assert!(!state.constitution_items.is_empty());
        assert!(!state.persona_items.is_empty());
        assert!(!state.should_quit);
        assert!(!state.confirmed);
    }
}
