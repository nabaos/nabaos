//! Full-screen setup wizard — immersive TUI for first-run configuration.
//!
//! A 5-step interactive wizard with:
//! - Rising sun ASCII art logo
//! - Scrollable provider selection with category headers
//! - API key input with masked display
//! - Background model discovery
//! - Constitution template picker
//! - Channel configuration
//! - Animated summary screen

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
// Muted, cohesive scheme — warm amber/gold for accents, slate for structure.

const BG: Color = Color::Rgb(22, 22, 30);         // deep navy-black
const FG: Color = Color::Rgb(200, 200, 210);       // soft white
const DIM: Color = Color::Rgb(90, 90, 105);        // muted gray
const ACCENT: Color = Color::Rgb(255, 175, 95);    // warm amber
const ACCENT2: Color = Color::Rgb(255, 135, 95);   // coral
const HIGHLIGHT_BG: Color = Color::Rgb(50, 48, 65); // selection bar
const GREEN: Color = Color::Rgb(120, 220, 140);    // success
const BORDER: Color = Color::Rgb(60, 58, 75);      // border gray
const HEADING: Color = Color::Rgb(160, 155, 180);  // section headers
const SUN_CORE: Color = Color::Rgb(255, 200, 80);  // sun center
const SUN_RAY: Color = Color::Rgb(255, 165, 70);   // sun rays
const SUN_GLOW: Color = Color::Rgb(200, 120, 60);  // sun glow
const SUN_HORIZON: Color = Color::Rgb(120, 80, 50); // horizon line
const STEP_DONE: Color = Color::Rgb(120, 220, 140); // completed step
const STEP_ACTIVE: Color = Color::Rgb(255, 175, 95); // current step
const STEP_TODO: Color = Color::Rgb(70, 68, 85);     // future step

// ── ASCII art ───────────────────────────────────────────────────────────────

fn sun_art() -> Vec<Line<'static>> {
    vec![
        Line::from(vec![
            Span::styled("                    ", Style::default().bg(BG)),
            Span::styled("·  ·", Style::default().fg(SUN_GLOW).bg(BG)),
            Span::styled("  ·", Style::default().fg(SUN_HORIZON).bg(BG)),
        ]),
        Line::from(vec![
            Span::styled("              ", Style::default().bg(BG)),
            Span::styled("·", Style::default().fg(SUN_HORIZON).bg(BG)),
            Span::styled("    ", Style::default().bg(BG)),
            Span::styled("▄▄████▄▄", Style::default().fg(SUN_CORE).bg(BG)),
            Span::styled("    ", Style::default().bg(BG)),
            Span::styled("·", Style::default().fg(SUN_HORIZON).bg(BG)),
        ]),
        Line::from(vec![
            Span::styled("           ", Style::default().bg(BG)),
            Span::styled("·", Style::default().fg(SUN_GLOW).bg(BG)),
            Span::styled("  ", Style::default().bg(BG)),
            Span::styled("▄█████████████▄", Style::default().fg(SUN_CORE).bg(BG)),
            Span::styled("  ", Style::default().bg(BG)),
            Span::styled("·", Style::default().fg(SUN_GLOW).bg(BG)),
        ]),
        Line::from(vec![
            Span::styled("          ", Style::default().bg(BG)),
            Span::styled("╱", Style::default().fg(SUN_RAY).bg(BG)),
            Span::styled(" ", Style::default().bg(BG)),
            Span::styled("██████████████████", Style::default().fg(SUN_CORE).bg(BG)),
            Span::styled(" ", Style::default().bg(BG)),
            Span::styled("╲", Style::default().fg(SUN_RAY).bg(BG)),
        ]),
        Line::from(vec![
            Span::styled("       ", Style::default().bg(BG)),
            Span::styled("── ", Style::default().fg(SUN_RAY).bg(BG)),
            Span::styled("████████████████████", Style::default().fg(SUN_CORE).bg(BG)),
            Span::styled(" ──", Style::default().fg(SUN_RAY).bg(BG)),
        ]),
        Line::from(vec![
            Span::styled("    ", Style::default().bg(BG)),
            Span::styled("─── ", Style::default().fg(SUN_RAY).bg(BG)),
            Span::styled("▀▀████████████████████▀▀", Style::default().fg(SUN_CORE).bg(BG)),
            Span::styled(" ───", Style::default().fg(SUN_RAY).bg(BG)),
        ]),
        Line::from(vec![
            Span::styled("▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁▁", Style::default().fg(SUN_HORIZON).bg(BG)),
        ]),
        Line::from(vec![
            Span::styled("░░░░░▒▒▒▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒░░░░░", Style::default().fg(SUN_GLOW).bg(BG)),
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
    Line::from(vec![
        Span::styled(
            format!("v{}", env!("CARGO_PKG_VERSION")),
            Style::default().fg(DIM).bg(BG),
        ),
    ])
}

// ── Wizard state ────────────────────────────────────────────────────────────

/// Result returned by the wizard for the caller to persist.
pub struct WizardResult {
    pub provider_id: String,
    pub provider_name: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub constitution: String,
    pub enable_telegram: bool,
    pub telegram_token: String,
    pub enable_web: bool,
    pub web_password: String,
    pub starter_agent: Option<String>,
    pub download_webbert: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Welcome,
    Provider,
    ApiKeyModel,
    Constitution,
    Channels,
    Summary,
}

impl Step {
    fn index(&self) -> usize {
        match self {
            Self::Welcome => 0,
            Self::Provider => 1,
            Self::ApiKeyModel => 2,
            Self::Constitution => 3,
            Self::Channels => 4,
            Self::Summary => 5,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Welcome => "Welcome",
            Self::Provider => "Provider",
            Self::ApiKeyModel => "API & Model",
            Self::Constitution => "Constitution",
            Self::Channels => "Channels",
            Self::Summary => "Summary",
        }
    }

    fn all() -> &'static [Step] {
        &[
            Step::Welcome,
            Step::Provider,
            Step::ApiKeyModel,
            Step::Constitution,
            Step::Channels,
            Step::Summary,
        ]
    }

    #[cfg(test)]
    fn next(&self) -> Self {
        match self {
            Self::Welcome => Self::Provider,
            Self::Provider => Self::ApiKeyModel,
            Self::ApiKeyModel => Self::Constitution,
            Self::Constitution => Self::Channels,
            Self::Channels => Self::Summary,
            Self::Summary => Self::Summary,
        }
    }

    fn prev(&self) -> Self {
        match self {
            Self::Welcome => Self::Welcome,
            Self::Provider => Self::Welcome,
            Self::ApiKeyModel => Self::Provider,
            Self::Constitution => Self::ApiKeyModel,
            Self::Channels => Self::Constitution,
            Self::Summary => Self::Channels,
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

    // Model selection
    models: Vec<String>,
    model_state: ListState,
    models_loading: bool,
    models_error: Option<String>,
    bg_rx: mpsc::Receiver<BgMessage>,
    bg_tx: mpsc::Sender<BgMessage>,

    // Selected provider info
    selected_provider_id: String,
    selected_provider_name: String,
    selected_base_url: String,
    selected_model: String,

    // Constitution
    constitution_items: Vec<(String, String)>,
    constitution_state: ListState,
    selected_constitution: String,

    // Channels
    channel_focus: usize, // 0=telegram, 1=web, 2=agent, 3=webbert
    telegram_enabled: bool,
    telegram_token: String,
    telegram_editing: bool,
    web_enabled: bool,
    web_password: String,
    web_editing: bool,
    starter_agent: Option<String>,
    download_webbert: bool,

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
        provider_items.push(SelectItem { id: String::new(), label: "Aggregators".into(), hint: "use any model through a single API".into(), is_header: true, base_url: String::new() });
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

        // Fill in base URLs from catalog
        let catalog = crate::providers::catalog::builtin_providers();
        for item in provider_items.iter_mut() {
            if !item.is_header {
                if let Some(p) = catalog.iter().find(|p| p.id == item.id) {
                    item.base_url = p.base_url.clone();
                }
            }
        }

        // Select first non-header item
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
            model_state: ListState::default(),
            models_loading: false,
            models_error: None,
            bg_rx,
            bg_tx,
            selected_provider_id: String::new(),
            selected_provider_name: String::new(),
            selected_base_url: String::new(),
            selected_model: String::new(),
            constitution_items,
            constitution_state,
            selected_constitution: "default".into(),
            channel_focus: 0,
            telegram_enabled: false,
            telegram_token: String::new(),
            telegram_editing: false,
            web_enabled: false,
            web_password: String::new(),
            web_editing: false,
            starter_agent: None,
            download_webbert: true,
            start_time: Instant::now(),
        }
    }

    fn is_local_provider(&self) -> bool {
        self.selected_base_url.starts_with("http://localhost")
            || self.selected_base_url.starts_with("http://127.")
    }

    /// Lock in the provider selection and move to API key step.
    fn confirm_provider(&mut self) {
        if let Some(idx) = self.provider_state.selected() {
            let item = &self.provider_items[idx];
            if item.is_header {
                return;
            }
            self.selected_provider_id = item.id.clone();
            self.selected_provider_name = item.label.clone();
            self.selected_base_url = item.base_url.clone();

            // Pre-fill model from catalog default
            let catalog = crate::providers::catalog::builtin_providers();
            if let Some(p) = catalog.iter().find(|p| p.id == self.selected_provider_id) {
                if !p.default_model.is_empty() {
                    self.selected_model = p.default_model.clone();
                }
            }

            self.step = Step::ApiKeyModel;
        }
    }

    /// Start background model discovery.
    fn discover_models(&mut self) {
        if self.selected_base_url.is_empty() {
            return;
        }
        self.models_loading = true;
        self.models_error = None;
        self.models.clear();

        let tx = self.bg_tx.clone();
        let base_url = self.selected_base_url.clone();
        let api_key = self.api_key_input.clone();

        std::thread::spawn(move || {
            match crate::providers::discovery::fetch_available_models(&base_url, &api_key) {
                Ok(models) if !models.is_empty() => {
                    tx.send(BgMessage::ModelsFound(models)).ok();
                }
                Ok(_) => {
                    tx.send(BgMessage::ModelsError("No models found".into())).ok();
                }
                Err(e) => {
                    tx.send(BgMessage::ModelsError(e.to_string())).ok();
                }
            }
        });
    }

    /// Poll background messages.
    fn poll_bg(&mut self) {
        while let Ok(msg) = self.bg_rx.try_recv() {
            match msg {
                BgMessage::ModelsFound(m) => {
                    self.models_loading = false;
                    self.models = m;
                    if !self.models.is_empty() {
                        self.model_state.select(Some(0));
                        self.selected_model = self.models[0].clone();
                    }
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
            if !self.provider_items[i].is_header {
                break;
            }
        }
        self.provider_state.select(Some(i));
    }

    fn move_provider_down(&mut self) {
        let len = self.provider_items.len();
        if len == 0 { return; }
        let mut i = self.provider_state.selected().unwrap_or(0);
        loop {
            i = (i + 1) % len;
            if !self.provider_items[i].is_header {
                break;
            }
        }
        self.provider_state.select(Some(i));
    }

    fn into_result(self) -> WizardResult {
        WizardResult {
            provider_id: self.selected_provider_id,
            provider_name: self.selected_provider_name,
            base_url: self.selected_base_url,
            api_key: self.api_key_input,
            model: self.selected_model,
            constitution: self.selected_constitution,
            enable_telegram: self.telegram_enabled,
            telegram_token: self.telegram_token,
            enable_web: self.web_enabled,
            web_password: self.web_password,
            starter_agent: self.starter_agent,
            download_webbert: self.download_webbert,
        }
    }
}

// ── Public API ──────────────────────────────────────────────────────────────

/// Run the interactive setup wizard. Returns `None` if the user quits.
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

        if state.should_quit {
            break Ok(None);
        }
        if state.confirmed {
            break Ok(Some(state.into_result()));
        }
    };

    disable_raw_mode().ok();
    io::stdout().execute(LeaveAlternateScreen).ok();

    result
}

// ── Key handling ────────────────────────────────────────────────────────────

fn handle_key(state: &mut WizardState, key: crossterm::event::KeyEvent) {
    // Global: Ctrl+C or Esc to quit
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        state.should_quit = true;
        return;
    }
    if key.code == KeyCode::Esc {
        if state.step == Step::Welcome {
            state.should_quit = true;
        } else if state.step == Step::ApiKeyModel && state.telegram_editing {
            // noop
        } else {
            state.step = state.step.prev();
        }
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
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => state.move_provider_up(),
                KeyCode::Down | KeyCode::Char('j') => state.move_provider_down(),
                KeyCode::Enter => state.confirm_provider(),
                KeyCode::Char('q') => state.should_quit = true,
                _ => {}
            }
        }
        Step::ApiKeyModel => {
            handle_api_key_model(state, key);
        }
        Step::Constitution => {
            match key.code {
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
                        state.step = Step::Channels;
                    }
                }
                _ => {}
            }
        }
        Step::Channels => {
            handle_channels(state, key);
        }
        Step::Summary => {
            match key.code {
                KeyCode::Enter => {
                    state.confirmed = true;
                }
                KeyCode::Char('b') | KeyCode::Backspace => {
                    state.step = Step::Channels;
                }
                _ => {}
            }
        }
    }
}

fn handle_api_key_model(state: &mut WizardState, key: crossterm::event::KeyEvent) {
    let is_local = state.is_local_provider();

    // If models are loaded and we're in model selection mode
    if !state.models.is_empty() && !state.api_key_input.is_empty() || (is_local && !state.models.is_empty()) {
        match key.code {
            KeyCode::Tab => {
                state.show_api_key = !state.show_api_key;
            }
            KeyCode::Up | KeyCode::Char('k') if !state.models.is_empty() => {
                let len = state.models.len();
                let i = state.model_state.selected().unwrap_or(0);
                state.model_state.select(Some(if i == 0 { len - 1 } else { i - 1 }));
                state.selected_model = state.models[state.model_state.selected().unwrap_or(0)].clone();
            }
            KeyCode::Down | KeyCode::Char('j') if !state.models.is_empty() => {
                let len = state.models.len();
                let i = state.model_state.selected().unwrap_or(0);
                state.model_state.select(Some((i + 1) % len));
                state.selected_model = state.models[state.model_state.selected().unwrap_or(0)].clone();
            }
            KeyCode::Enter => {
                if let Some(idx) = state.model_state.selected() {
                    if idx < state.models.len() {
                        state.selected_model = state.models[idx].clone();
                    }
                }
                state.step = Step::Constitution;
            }
            KeyCode::Char('s') => {
                // Skip model selection
                state.step = Step::Constitution;
            }
            _ => {}
        }
        return;
    }

    // API key text input mode
    if !is_local {
        match key.code {
            KeyCode::Char(c) => {
                state.api_key_input.insert(state.api_key_cursor, c);
                state.api_key_cursor += 1;
            }
            KeyCode::Backspace => {
                if state.api_key_cursor > 0 {
                    state.api_key_cursor -= 1;
                    state.api_key_input.remove(state.api_key_cursor);
                }
            }
            KeyCode::Left => {
                state.api_key_cursor = state.api_key_cursor.saturating_sub(1);
            }
            KeyCode::Right => {
                state.api_key_cursor = (state.api_key_cursor + 1).min(state.api_key_input.len());
            }
            KeyCode::Tab => {
                state.show_api_key = !state.show_api_key;
            }
            KeyCode::Enter => {
                if !state.api_key_input.is_empty() {
                    state.discover_models();
                } else {
                    // Skip with no API key
                    state.step = Step::Constitution;
                }
            }
            _ => {}
        }
    } else {
        // Local provider — auto-discover on enter
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

fn handle_channels(state: &mut WizardState, key: crossterm::event::KeyEvent) {
    // If editing a text field
    if state.telegram_editing {
        match key.code {
            KeyCode::Char(c) => { state.telegram_token.push(c); }
            KeyCode::Backspace => { state.telegram_token.pop(); }
            KeyCode::Enter | KeyCode::Esc => { state.telegram_editing = false; }
            _ => {}
        }
        return;
    }
    if state.web_editing {
        match key.code {
            KeyCode::Char(c) => { state.web_password.push(c); }
            KeyCode::Backspace => { state.web_password.pop(); }
            KeyCode::Enter | KeyCode::Esc => { state.web_editing = false; }
            _ => {}
        }
        return;
    }

    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            state.channel_focus = state.channel_focus.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.channel_focus = (state.channel_focus + 1).min(3);
        }
        KeyCode::Char(' ') | KeyCode::Enter => {
            match state.channel_focus {
                0 => {
                    state.telegram_enabled = !state.telegram_enabled;
                    if state.telegram_enabled && state.telegram_token.is_empty() {
                        state.telegram_editing = true;
                    }
                }
                1 => {
                    state.web_enabled = !state.web_enabled;
                    if state.web_enabled && state.web_password.is_empty() {
                        state.web_editing = true;
                    }
                }
                2 => {
                    // Cycle starter agent
                    state.starter_agent = match &state.starter_agent {
                        None => Some("morning-briefing".into()),
                        Some(a) if a == "morning-briefing" => Some("email-assistant".into()),
                        Some(a) if a == "email-assistant" => Some("dev-helper".into()),
                        _ => None,
                    };
                }
                3 => {
                    state.download_webbert = !state.download_webbert;
                }
                _ => {}
            }
        }
        KeyCode::Tab => {
            state.step = Step::Summary;
        }
        KeyCode::Char('n') => {
            state.step = Step::Summary;
        }
        _ => {}
    }
}

// ── Drawing ─────────────────────────────────────────────────────────────────

fn draw_wizard(frame: &mut ratatui::Frame, state: &WizardState) {
    let size = frame.area();

    // Fill background
    let bg_block = Block::default().style(Style::default().bg(BG));
    frame.render_widget(bg_block, size);

    // Outer frame with rounded borders
    let outer = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(BG));
    let inner = outer.inner(size);
    frame.render_widget(outer, size);

    match state.step {
        Step::Welcome => draw_welcome(frame, inner, state),
        Step::Provider => draw_provider(frame, inner, state),
        Step::ApiKeyModel => draw_api_key_model(frame, inner, state),
        Step::Constitution => draw_constitution(frame, inner, state),
        Step::Channels => draw_channels(frame, inner, state),
        Step::Summary => draw_summary(frame, inner, state),
    }
}

fn draw_step_indicator(frame: &mut ratatui::Frame, area: Rect, current: Step) {
    let steps = Step::all();
    let mut spans = Vec::new();

    for (i, step) in steps.iter().enumerate() {
        let color = if step.index() < current.index() {
            STEP_DONE
        } else if *step == current {
            STEP_ACTIVE
        } else {
            STEP_TODO
        };

        let symbol = if step.index() < current.index() {
            "●"
        } else if *step == current {
            "◉"
        } else {
            "○"
        };

        spans.push(Span::styled(
            format!(" {} ", symbol),
            Style::default().fg(color).bg(BG),
        ));
        spans.push(Span::styled(
            step.label(),
            Style::default().fg(if *step == current { ACCENT } else { DIM }).bg(BG),
        ));

        if i < steps.len() - 1 {
            spans.push(Span::styled(
                " ── ",
                Style::default().fg(STEP_TODO).bg(BG),
            ));
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
        spans.push(Span::styled(
            format!(" {} ", key),
            Style::default().fg(ACCENT).bg(BG).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            desc.to_string(),
            Style::default().fg(DIM).bg(BG),
        ));
        if i < hints.len() - 1 {
            spans.push(Span::styled(" · ", Style::default().fg(STEP_TODO).bg(BG)));
        }
    }
    let p = Paragraph::new(Line::from(spans))
        .alignment(Alignment::Center)
        .style(Style::default().bg(BG));
    frame.render_widget(p, area);
}

// ── Welcome screen ──────────────────────────────────────────────────────────

fn draw_welcome(frame: &mut ratatui::Frame, area: Rect, _state: &WizardState) {
    let chunks = Layout::vertical([
        Constraint::Percentage(15),
        Constraint::Length(8),   // ASCII art
        Constraint::Length(1),   // title
        Constraint::Length(1),   // version
        Constraint::Length(3),   // spacer
        Constraint::Length(3),   // tagline
        Constraint::Length(2),   // spacer
        Constraint::Length(1),   // hint
        Constraint::Min(0),
    ]).split(area);

    // Sun art
    let art = sun_art();
    let art_p = Paragraph::new(art)
        .alignment(Alignment::Center)
        .style(Style::default().bg(BG));
    frame.render_widget(art_p, chunks[1]);

    // Title
    let title = Paragraph::new(title_line())
        .alignment(Alignment::Center)
        .style(Style::default().bg(BG));
    frame.render_widget(title, chunks[2]);

    // Version
    let ver = Paragraph::new(version_line())
        .alignment(Alignment::Center)
        .style(Style::default().bg(BG));
    frame.render_widget(ver, chunks[3]);

    // Tagline
    let tagline = Paragraph::new(vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "The agent that uses all agents",
                Style::default().fg(FG).bg(BG),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "cached when possible · autonomous when needed · independent always",
                Style::default().fg(DIM).bg(BG),
            ),
        ]),
    ])
    .alignment(Alignment::Center)
    .style(Style::default().bg(BG));
    frame.render_widget(tagline, chunks[5]);

    // Hint
    draw_hint_bar(frame, chunks[7], &[
        ("Enter", "begin setup"),
        ("Esc", "quit"),
    ]);
}

// ── Provider selection ──────────────────────────────────────────────────────

fn draw_provider(frame: &mut ratatui::Frame, area: Rect, state: &WizardState) {
    let chunks = Layout::vertical([
        Constraint::Length(1),  // step indicator
        Constraint::Length(1),  // spacer
        Constraint::Min(8),    // provider list
        Constraint::Length(1),  // hint bar
    ]).split(area);

    draw_step_indicator(frame, chunks[0], Step::Provider);

    // Provider list in a centered box
    let list_area = centered_rect(60, 90, chunks[2]);

    let items: Vec<ListItem> = state.provider_items.iter().map(|item| {
        if item.is_header {
            let mut spans = vec![
                Span::styled(
                    format!("  {} ", item.label),
                    Style::default().fg(HEADING).bg(BG).add_modifier(Modifier::BOLD),
                ),
            ];
            if !item.hint.is_empty() {
                spans.push(Span::styled(
                    format!("  {}", item.hint),
                    Style::default().fg(DIM).bg(BG),
                ));
            }
            ListItem::new(Line::from(spans)).style(Style::default().bg(BG))
        } else {
            ListItem::new(Line::from(vec![
                Span::styled("    ", Style::default().bg(BG)),
                Span::styled(
                    format!("{:<22}", item.label),
                    Style::default().fg(FG).bg(BG),
                ),
                Span::styled(
                    item.hint.clone(),
                    Style::default().fg(DIM).bg(BG),
                ),
            ])).style(Style::default().bg(BG))
        }
    }).collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .title(Line::from(vec![
            Span::styled(" ", Style::default().bg(BG)),
            Span::styled("LLM Provider", Style::default().fg(ACCENT).bg(BG).add_modifier(Modifier::BOLD)),
            Span::styled(" ", Style::default().bg(BG)),
        ]))
        .style(Style::default().bg(BG));

    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .bg(HIGHLIGHT_BG)
                .fg(ACCENT)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("  ▸ ");

    frame.render_stateful_widget(list, list_area, &mut state.provider_state.clone());

    draw_hint_bar(frame, chunks[3], &[
        ("↑↓", "navigate"),
        ("Enter", "select"),
        ("Esc", "back"),
    ]);
}

// ── API Key + Model ─────────────────────────────────────────────────────────

fn draw_api_key_model(frame: &mut ratatui::Frame, area: Rect, state: &WizardState) {
    let chunks = Layout::vertical([
        Constraint::Length(1),  // step indicator
        Constraint::Length(1),  // spacer
        Constraint::Min(8),    // content
        Constraint::Length(1),  // hint bar
    ]).split(area);

    draw_step_indicator(frame, chunks[0], Step::ApiKeyModel);

    let content_area = centered_rect(60, 80, chunks[2]);
    let is_local = state.is_local_provider();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .title(Line::from(vec![
            Span::styled(" ", Style::default().bg(BG)),
            Span::styled(
                format!("{} · API & Model", state.selected_provider_name),
                Style::default().fg(ACCENT).bg(BG).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" ", Style::default().bg(BG)),
        ]))
        .style(Style::default().bg(BG));

    let block_inner = block.inner(content_area);
    frame.render_widget(block, content_area);

    let mut lines = Vec::new();
    lines.push(Line::from(""));

    if !is_local {
        // API key input
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

        let key_color = if state.api_key_input.is_empty() { DIM } else { FG };
        lines.push(Line::from(vec![
            Span::styled(display_key, Style::default().fg(key_color).bg(BG)),
        ]));

        if !state.api_key_input.is_empty() {
            lines.push(Line::from(vec![
                Span::styled(
                    "  Tab to show/hide · Enter to discover models",
                    Style::default().fg(DIM).bg(BG),
                ),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled(
                    "  Enter with empty field to skip",
                    Style::default().fg(DIM).bg(BG),
                ),
            ]));
        }

        lines.push(Line::from(""));
    } else {
        lines.push(Line::from(vec![
            Span::styled("  Local mode", Style::default().fg(GREEN).bg(BG).add_modifier(Modifier::BOLD)),
            Span::styled(" — no API key needed", Style::default().fg(DIM).bg(BG)),
        ]));
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(
                format!("  Server: {}", state.selected_base_url),
                Style::default().fg(FG).bg(BG),
            ),
        ]));
        lines.push(Line::from(""));
    }

    // Model discovery status
    if state.models_loading {
        let spinner = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        let idx = (state.start_time.elapsed().as_millis() / 100) as usize % spinner.len();
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {} Discovering models...", spinner[idx]),
                Style::default().fg(ACCENT).bg(BG),
            ),
        ]));
    } else if let Some(ref err) = state.models_error {
        lines.push(Line::from(vec![
            Span::styled("  ▲ ", Style::default().fg(ACCENT2).bg(BG)),
            Span::styled(err.as_str(), Style::default().fg(DIM).bg(BG)),
        ]));
        if !state.selected_model.is_empty() {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  Default: {}", state.selected_model),
                    Style::default().fg(FG).bg(BG),
                ),
            ]));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("  Press Enter to continue", Style::default().fg(DIM).bg(BG)),
        ]));
    }

    let text_p = Paragraph::new(lines).style(Style::default().bg(BG));
    frame.render_widget(text_p, block_inner);

    // If models are loaded, show them as a list below
    if !state.models.is_empty() && !state.models_loading {
        let model_lines_start = if is_local { 4 } else { 7 };
        let model_area = Rect::new(
            block_inner.x + 2,
            block_inner.y + model_lines_start,
            block_inner.width.saturating_sub(4),
            block_inner.height.saturating_sub(model_lines_start + 2),
        );

        let model_header = Line::from(vec![
            Span::styled(
                format!("  Models ({})", state.models.len()),
                Style::default().fg(HEADING).bg(BG).add_modifier(Modifier::BOLD),
            ),
        ]);
        let header_area = Rect::new(model_area.x, model_area.y.saturating_sub(1), model_area.width, 1);
        frame.render_widget(Paragraph::new(model_header).style(Style::default().bg(BG)), header_area);

        let model_items: Vec<ListItem> = state.models.iter().map(|m| {
            ListItem::new(Line::from(vec![
                Span::styled("  ", Style::default().bg(BG)),
                Span::styled(m.clone(), Style::default().fg(FG).bg(BG)),
            ])).style(Style::default().bg(BG))
        }).collect();

        let model_list = List::new(model_items)
            .highlight_style(
                Style::default()
                    .bg(HIGHLIGHT_BG)
                    .fg(ACCENT)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▸ ");

        frame.render_stateful_widget(model_list, model_area, &mut state.model_state.clone());
    }

    let hints: Vec<(&str, &str)> = if !state.models.is_empty() {
        vec![("↑↓", "select model"), ("Enter", "confirm"), ("s", "skip"), ("Esc", "back")]
    } else if is_local {
        vec![("Enter", "discover models"), ("Esc", "back")]
    } else {
        vec![("type", "enter key"), ("Tab", "show/hide"), ("Enter", "next"), ("Esc", "back")]
    };
    draw_hint_bar(frame, chunks[3], &hints);
}

// ── Constitution ────────────────────────────────────────────────────────────

fn draw_constitution(frame: &mut ratatui::Frame, area: Rect, state: &WizardState) {
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(8),
        Constraint::Length(1),
    ]).split(area);

    draw_step_indicator(frame, chunks[0], Step::Constitution);

    let list_area = centered_rect(60, 90, chunks[2]);

    let items: Vec<ListItem> = state.constitution_items.iter().map(|(name, desc)| {
        ListItem::new(Line::from(vec![
            Span::styled("  ", Style::default().bg(BG)),
            Span::styled(
                format!("{:<20}", name),
                Style::default().fg(FG).bg(BG),
            ),
            Span::styled(
                desc.clone(),
                Style::default().fg(DIM).bg(BG),
            ),
        ])).style(Style::default().bg(BG))
    }).collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .title(Line::from(vec![
            Span::styled(" ", Style::default().bg(BG)),
            Span::styled(
                "Constitution Template",
                Style::default().fg(ACCENT).bg(BG).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" ", Style::default().bg(BG)),
        ]))
        .title_bottom(Line::from(vec![
            Span::styled(
                " Safety rules that govern what your agent can and cannot do ",
                Style::default().fg(DIM).bg(BG),
            ),
        ]))
        .style(Style::default().bg(BG));

    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .bg(HIGHLIGHT_BG)
                .fg(ACCENT)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("  ▸ ");

    frame.render_stateful_widget(list, list_area, &mut state.constitution_state.clone());

    draw_hint_bar(frame, chunks[3], &[
        ("↑↓", "navigate"),
        ("Enter", "select"),
        ("Esc", "back"),
    ]);
}

// ── Channels ────────────────────────────────────────────────────────────────

fn draw_channels(frame: &mut ratatui::Frame, area: Rect, state: &WizardState) {
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(8),
        Constraint::Length(1),
    ]).split(area);

    draw_step_indicator(frame, chunks[0], Step::Channels);

    let content_area = centered_rect(55, 60, chunks[2]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .title(Line::from(vec![
            Span::styled(" ", Style::default().bg(BG)),
            Span::styled(
                "Channels & Extras",
                Style::default().fg(ACCENT).bg(BG).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" ", Style::default().bg(BG)),
        ]))
        .style(Style::default().bg(BG));

    let block_inner = block.inner(content_area);
    frame.render_widget(block, content_area);

    let items = vec![
        (0, "Telegram Bot", state.telegram_enabled, &state.telegram_token, state.telegram_editing),
        (1, "Web Dashboard", state.web_enabled, &state.web_password, state.web_editing),
    ];

    let mut lines = Vec::new();
    lines.push(Line::from(""));

    for (idx, label, enabled, token, editing) in &items {
        let focused = state.channel_focus == *idx;
        let marker = if focused { "▸" } else { " " };
        let check = if *enabled { "◆" } else { "◇" };
        let check_color = if *enabled { GREEN } else { DIM };

        let bg = if focused { HIGHLIGHT_BG } else { BG };
        lines.push(Line::from(vec![
            Span::styled(format!("  {} ", marker), Style::default().fg(ACCENT).bg(bg)),
            Span::styled(format!("{} ", check), Style::default().fg(check_color).bg(bg)),
            Span::styled(
                format!("{:<18}", label),
                Style::default().fg(if focused { ACCENT } else { FG }).bg(bg).add_modifier(if focused { Modifier::BOLD } else { Modifier::empty() }),
            ),
            Span::styled(
                if *editing {
                    format!("{}_", token)
                } else if token.is_empty() && *enabled {
                    "enter token...".to_string()
                } else if !token.is_empty() {
                    mask_key(token)
                } else {
                    String::new()
                },
                Style::default().fg(if *editing { ACCENT } else { DIM }).bg(bg),
            ),
        ]));
    }

    lines.push(Line::from(""));

    // Starter agent
    {
        let focused = state.channel_focus == 2;
        let marker = if focused { "▸" } else { " " };
        let bg = if focused { HIGHLIGHT_BG } else { BG };
        let agent_label = match &state.starter_agent {
            Some(a) => format!("◆ {}", a),
            None => "◇ none".to_string(),
        };
        lines.push(Line::from(vec![
            Span::styled(format!("  {} ", marker), Style::default().fg(ACCENT).bg(bg)),
            Span::styled(
                "Starter Agent     ",
                Style::default().fg(if focused { ACCENT } else { FG }).bg(bg).add_modifier(if focused { Modifier::BOLD } else { Modifier::empty() }),
            ),
            Span::styled(
                agent_label,
                Style::default().fg(if state.starter_agent.is_some() { GREEN } else { DIM }).bg(bg),
            ),
        ]));
    }

    // WebBERT
    {
        let focused = state.channel_focus == 3;
        let marker = if focused { "▸" } else { " " };
        let check = if state.download_webbert { "◆" } else { "◇" };
        let check_color = if state.download_webbert { GREEN } else { DIM };
        let bg = if focused { HIGHLIGHT_BG } else { BG };
        lines.push(Line::from(vec![
            Span::styled(format!("  {} ", marker), Style::default().fg(ACCENT).bg(bg)),
            Span::styled(format!("{} ", check), Style::default().fg(check_color).bg(bg)),
            Span::styled(
                "WebBERT Model     ",
                Style::default().fg(if focused { ACCENT } else { FG }).bg(bg).add_modifier(if focused { Modifier::BOLD } else { Modifier::empty() }),
            ),
            Span::styled(
                "~256MB · local browser classifier",
                Style::default().fg(DIM).bg(bg),
            ),
        ]));
    }

    let p = Paragraph::new(lines).style(Style::default().bg(BG));
    frame.render_widget(p, block_inner);

    draw_hint_bar(frame, chunks[3], &[
        ("↑↓", "navigate"),
        ("Space", "toggle"),
        ("n/Tab", "next"),
        ("Esc", "back"),
    ]);
}

// ── Summary ─────────────────────────────────────────────────────────────────

fn draw_summary(frame: &mut ratatui::Frame, area: Rect, state: &WizardState) {
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(8),
        Constraint::Length(1),
    ]).split(area);

    draw_step_indicator(frame, chunks[0], Step::Summary);

    let content_area = centered_rect(55, 70, chunks[2]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(GREEN))
        .title(Line::from(vec![
            Span::styled(" ", Style::default().bg(BG)),
            Span::styled(
                "Ready to Go",
                Style::default().fg(GREEN).bg(BG).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" ", Style::default().bg(BG)),
        ]))
        .style(Style::default().bg(BG));

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

    lines.push(row("Provider", &state.selected_provider_name, FG));
    if !state.selected_model.is_empty() {
        lines.push(row("Model", &state.selected_model, FG));
    }
    if !state.api_key_input.is_empty() {
        lines.push(row("API Key", &mask_key(&state.api_key_input), GREEN));
    }
    lines.push(row("Constitution", &state.selected_constitution, FG));

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("    Channels", Style::default().fg(HEADING).bg(BG).add_modifier(Modifier::BOLD)),
    ]));

    let check = |enabled: bool| if enabled { format!("◆") } else { format!("◇") };
    let check_color = |enabled: bool| if enabled { GREEN } else { DIM };

    lines.push(Line::from(vec![
        Span::styled("      ", Style::default().bg(BG)),
        Span::styled(check(state.telegram_enabled), Style::default().fg(check_color(state.telegram_enabled)).bg(BG)),
        Span::styled(" Telegram", Style::default().fg(FG).bg(BG)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("      ", Style::default().bg(BG)),
        Span::styled(check(state.web_enabled), Style::default().fg(check_color(state.web_enabled)).bg(BG)),
        Span::styled(" Web Dashboard", Style::default().fg(FG).bg(BG)),
    ]));

    if let Some(ref agent) = state.starter_agent {
        lines.push(Line::from(vec![
            Span::styled("      ", Style::default().bg(BG)),
            Span::styled("◆", Style::default().fg(GREEN).bg(BG)),
            Span::styled(format!(" Agent: {}", agent), Style::default().fg(FG).bg(BG)),
        ]));
    }

    lines.push(Line::from(vec![
        Span::styled("      ", Style::default().bg(BG)),
        Span::styled(check(state.download_webbert), Style::default().fg(check_color(state.download_webbert)).bg(BG)),
        Span::styled(" WebBERT (local ML)", Style::default().fg(FG).bg(BG)),
    ]));

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(
            "    Press Enter to write configuration",
            Style::default().fg(ACCENT).bg(BG),
        ),
    ]));

    let p = Paragraph::new(lines).style(Style::default().bg(BG));
    frame.render_widget(p, block_inner);

    draw_hint_bar(frame, chunks[3], &[
        ("Enter", "confirm & write"),
        ("b", "go back"),
        ("Esc", "back"),
    ]);
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Create a centered rect using percentage of parent.
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
        assert_eq!(Step::Summary.prev(), Step::Channels);
        assert_eq!(Step::Welcome.prev(), Step::Welcome);
    }

    #[test]
    fn test_wizard_state_creation() {
        let state = WizardState::new();
        assert_eq!(state.step, Step::Welcome);
        assert!(!state.provider_items.is_empty());
        assert!(!state.constitution_items.is_empty());
        assert!(!state.should_quit);
        assert!(!state.confirmed);
    }
}
