//! Agents tab — Play Store-style agent lifecycle manager.
//!
//! Features:
//! - Category filter bar with quick-select buttons
//! - Status indicators: ● running, ◌ stopped, ○ not installed
//! - Action keys: i install, u uninstall, s start/stop, p provider
//! - Search filter with / prefix
//! - Install/provider/confirm modal dialogs

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

use super::Tab;

// ── Agent state ─────────────────────────────────────────────────────────────

/// Agent lifecycle state for display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayState {
    NotInstalled,
    Stopped,
    Running,
    Paused,
    Disabled,
}

impl DisplayState {
    pub fn symbol(&self) -> &'static str {
        match self {
            Self::Running => "●",
            Self::Paused => "◌",
            Self::Stopped => "◉",
            Self::Disabled => "⊘",
            Self::NotInstalled => "○",
        }
    }

    pub fn color(&self) -> Color {
        match self {
            Self::Running => Color::Green,
            Self::Paused => Color::Yellow,
            Self::Stopped => Color::Rgb(100, 100, 120),
            Self::Disabled => Color::Red,
            Self::NotInstalled => Color::DarkGray,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::Paused => "Paused",
            Self::Stopped => "Stopped",
            Self::Disabled => "Disabled",
            Self::NotInstalled => "Not installed",
        }
    }
}

/// Full agent entry for display (merged from catalog + store + runtime).
#[derive(Clone)]
pub struct AgentEntry {
    pub name: String,
    pub version: String,
    pub category: String,
    pub description: String,
    pub author: String,
    pub permissions: Vec<String>,
    pub state: DisplayState,
    pub installed: bool,
}

/// What modal dialog is currently shown.
#[derive(Clone)]
pub enum AgentModal {
    None,
    /// Install confirmation — shows agent name and requested permissions.
    InstallConfirm { agent_name: String, permissions: Vec<String> },
    /// Confirm destructive action (uninstall, stop).
    ActionConfirm { agent_name: String, action: String },
    /// Status message (toast-style, auto-dismiss on any key).
    StatusMessage { message: String, is_error: bool },
}

/// Action request from the tab to the app.
#[derive(Clone)]
pub enum AgentAction {
    Install(String),
    Uninstall(String),
    Start(String),
    Stop(String),
}

// ── Categories ──────────────────────────────────────────────────────────────

const CATEGORIES: &[&str] = &[
    "All",
    "Dev",
    "Research",
    "Finance",
    "Security",
    "Creative",
    "Productivity",
    "Communication",
];

// ── AgentsTab ───────────────────────────────────────────────────────────────

pub struct AgentsTab {
    pub agents: Vec<AgentEntry>,
    pub state: ListState,
    pub search: String,
    pub search_active: bool,
    pub category_filter: usize, // index into CATEGORIES
    pub modal: AgentModal,
    pub pending_action: Option<AgentAction>,
}

impl AgentsTab {
    pub fn new() -> Self {
        Self {
            agents: Vec::new(),
            state: ListState::default(),
            search: String::new(),
            search_active: false,
            category_filter: 0,
            modal: AgentModal::None,
            pending_action: None,
        }
    }

    pub fn set_agents(&mut self, agents: Vec<AgentEntry>) {
        self.agents = agents;
        if !self.agents.is_empty() && self.state.selected().is_none() {
            self.state.select(Some(0));
        }
    }

    /// Take any pending action (consumed by app).
    pub fn take_action(&mut self) -> Option<AgentAction> {
        self.pending_action.take()
    }

    /// Show a status message after an action completes.
    pub fn show_status(&mut self, message: String, is_error: bool) {
        self.modal = AgentModal::StatusMessage { message, is_error };
    }

    pub fn filtered(&self) -> Vec<&AgentEntry> {
        let cat = CATEGORIES[self.category_filter];
        self.agents
            .iter()
            .filter(|a| {
                // Category filter
                if cat != "All" {
                    let c = a.category.to_lowercase();
                    let f = cat.to_lowercase();
                    if !c.contains(&f) {
                        return false;
                    }
                }
                // Search filter
                if !self.search.is_empty() {
                    let q = self.search.to_lowercase();
                    if !a.name.to_lowercase().contains(&q)
                        && !a.category.to_lowercase().contains(&q)
                        && !a.description.to_lowercase().contains(&q)
                        && !a.author.to_lowercase().contains(&q)
                    {
                        return false;
                    }
                }
                true
            })
            .collect()
    }

    /// Get the currently selected agent from the filtered list.
    pub fn selected_agent(&self) -> Option<&AgentEntry> {
        let filtered = self.filtered();
        self.state.selected().and_then(|i| filtered.get(i).copied())
    }

    fn clamp_selection(&mut self) {
        let len = self.filtered().len();
        if len == 0 {
            self.state.select(None);
        } else if let Some(i) = self.state.selected() {
            if i >= len {
                self.state.select(Some(len - 1));
            }
        } else {
            self.state.select(Some(0));
        }
    }
}

impl Tab for AgentsTab {
    fn render(&self, frame: &mut Frame, area: Rect) {
        if self.agents.is_empty() {
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(Line::from(vec![Span::styled(
                    " Agents ",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                )]));
            let content = Paragraph::new(vec![
                Line::from(""),
                Line::from(""),
                Line::from(vec![Span::styled(
                    "  No agents in catalog",
                    Style::default().fg(Color::DarkGray),
                )]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("  Run ", Style::default().fg(Color::DarkGray)),
                    Span::styled("nabaos setup", Style::default().fg(Color::Cyan)),
                    Span::styled(
                        " to initialize the catalog",
                        Style::default().fg(Color::DarkGray),
                    ),
                ]),
            ])
            .block(block);
            frame.render_widget(content, area);
            return;
        }

        let chunks = Layout::vertical([
            Constraint::Length(1), // category filter bar
            Constraint::Length(3), // search
            Constraint::Min(5),   // list
        ])
        .split(area);

        // Category filter bar
        let cat_spans: Vec<Span> = CATEGORIES
            .iter()
            .enumerate()
            .flat_map(|(i, cat)| {
                let style = if i == self.category_filter {
                    Style::default()
                        .fg(Color::Rgb(255, 175, 95))
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                vec![
                    Span::styled(format!(" {} ", cat), style),
                    if i < CATEGORIES.len() - 1 {
                        Span::styled("│", Style::default().fg(Color::Rgb(60, 60, 80)))
                    } else {
                        Span::raw("")
                    },
                ]
            })
            .collect();
        frame.render_widget(
            Paragraph::new(Line::from(cat_spans)),
            chunks[0],
        );

        // Search bar
        let search_border = if self.search_active {
            Color::Cyan
        } else {
            Color::DarkGray
        };
        let search_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(search_border))
            .title(Line::from(vec![Span::styled(
                " / Search ",
                Style::default().fg(Color::Cyan),
            )]));
        let search_display = if self.search.is_empty() && !self.search_active {
            Line::from(vec![Span::styled(
                " Press / to search agents...",
                Style::default().fg(Color::DarkGray),
            )])
        } else {
            Line::from(vec![
                Span::raw(" "),
                Span::styled(self.search.clone(), Style::default().fg(Color::White)),
                if self.search_active {
                    Span::styled("▏", Style::default().fg(Color::Cyan))
                } else {
                    Span::raw("")
                },
            ])
        };
        let search_para = Paragraph::new(search_display).block(search_block);
        frame.render_widget(search_para, chunks[1]);

        // Agent list with adaptive columns
        let filtered = self.filtered();
        let avail = chunks[2].width.saturating_sub(6) as usize;
        let status_w = 2;
        let name_w = 22.min(avail / 3);
        let ver_w = 8;
        let cat_w = 14.min(avail / 4);
        let desc_w = avail.saturating_sub(status_w + name_w + ver_w + cat_w + 4);

        let items: Vec<ListItem> = filtered
            .iter()
            .map(|a| {
                let name = truncate(&a.name, name_w);
                let ver = truncate(&a.version, ver_w);
                let cat = truncate(&a.category, cat_w);
                let desc = truncate(&a.description, desc_w);

                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{} ", a.state.symbol()),
                        Style::default().fg(a.state.color()),
                    ),
                    Span::styled(
                        format!("{:<width$} ", name, width = name_w),
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{:<width$} ", ver, width = ver_w),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(
                        format!("{:<width$} ", cat, width = cat_w),
                        Style::default().fg(category_color(&a.category)),
                    ),
                    Span::styled(desc, Style::default().fg(Color::DarkGray)),
                ]))
            })
            .collect();

        let title = if filtered.len() != self.agents.len() {
            format!(" Agents ({}/{}) ", filtered.len(), self.agents.len())
        } else {
            format!(" Agents ({}) ", self.agents.len())
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(Line::from(vec![Span::styled(
                title,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )]));

        if filtered.is_empty() && !self.search.is_empty() {
            let empty_msg = Paragraph::new(vec![
                Line::from(""),
                Line::from(vec![Span::styled(
                    format!("  No agents matching \"{}\"", self.search),
                    Style::default().fg(Color::DarkGray),
                )]),
            ])
            .block(block);
            frame.render_widget(empty_msg, chunks[2]);
        } else {
            let list = List::new(items)
                .block(block)
                .highlight_style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                        .bg(Color::DarkGray),
                )
                .highlight_symbol("▸ ");
            frame.render_stateful_widget(list, chunks[2], &mut self.state.clone());
        }

        // Modal overlay
        match &self.modal {
            AgentModal::None => {}
            AgentModal::InstallConfirm {
                agent_name,
                permissions,
            } => {
                draw_install_modal(frame, area, agent_name, permissions);
            }
            AgentModal::ActionConfirm { agent_name, action } => {
                draw_confirm_modal(frame, area, agent_name, action);
            }
            AgentModal::StatusMessage { message, is_error } => {
                draw_status_modal(frame, area, message, *is_error);
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        // Modal intercepts all keys
        match &self.modal {
            AgentModal::InstallConfirm { agent_name, .. } => {
                match key.code {
                    KeyCode::Char('y') | KeyCode::Enter => {
                        let name = agent_name.clone();
                        self.pending_action = Some(AgentAction::Install(name));
                        self.modal = AgentModal::None;
                    }
                    _ => {
                        self.modal = AgentModal::None;
                    }
                }
                return true;
            }
            AgentModal::ActionConfirm { agent_name, action } => {
                match key.code {
                    KeyCode::Char('y') | KeyCode::Enter => {
                        let name = agent_name.clone();
                        let act = action.clone();
                        self.modal = AgentModal::None;
                        match act.as_str() {
                            "uninstall" => {
                                self.pending_action = Some(AgentAction::Uninstall(name));
                            }
                            "stop" => {
                                self.pending_action = Some(AgentAction::Stop(name));
                            }
                            _ => {}
                        }
                    }
                    _ => {
                        self.modal = AgentModal::None;
                    }
                }
                return true;
            }
            AgentModal::StatusMessage { .. } => {
                self.modal = AgentModal::None;
                return true;
            }
            AgentModal::None => {}
        }

        // Search mode
        if self.search_active {
            match key.code {
                KeyCode::Esc => {
                    self.search_active = false;
                    self.search.clear();
                    self.clamp_selection();
                }
                KeyCode::Enter => {
                    self.search_active = false;
                }
                KeyCode::Backspace => {
                    self.search.pop();
                    self.clamp_selection();
                }
                KeyCode::Char(c) => {
                    self.search.push(c);
                    self.clamp_selection();
                }
                _ => {}
            }
            return true;
        }

        // Normal mode
        match key.code {
            KeyCode::Char('/') => {
                self.search_active = true;
                self.search.clear();
                true
            }
            KeyCode::Esc => {
                if !self.search.is_empty() {
                    self.search.clear();
                    self.clamp_selection();
                } else if self.category_filter != 0 {
                    self.category_filter = 0;
                    self.clamp_selection();
                }
                true
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let len = self.filtered().len();
                if len > 0 {
                    let i = self.state.selected().unwrap_or(0);
                    self.state.select(Some((i + 1) % len));
                }
                true
            }
            KeyCode::Up | KeyCode::Char('k') => {
                let len = self.filtered().len();
                if len > 0 {
                    let i = self.state.selected().unwrap_or(0);
                    self.state.select(Some(i.checked_sub(1).unwrap_or(len - 1)));
                }
                true
            }
            // Category filter with left/right
            KeyCode::Left | KeyCode::Char('h') => {
                if self.category_filter > 0 {
                    self.category_filter -= 1;
                } else {
                    self.category_filter = CATEGORIES.len() - 1;
                }
                self.clamp_selection();
                true
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.category_filter = (self.category_filter + 1) % CATEGORIES.len();
                self.clamp_selection();
                true
            }
            // Actions
            KeyCode::Char('i') => {
                if let Some(agent) = self.selected_agent() {
                    if !agent.installed {
                        self.modal = AgentModal::InstallConfirm {
                            agent_name: agent.name.clone(),
                            permissions: agent.permissions.clone(),
                        };
                    }
                }
                true
            }
            KeyCode::Char('u') => {
                if let Some(agent) = self.selected_agent() {
                    if agent.installed {
                        self.modal = AgentModal::ActionConfirm {
                            agent_name: agent.name.clone(),
                            action: "uninstall".to_string(),
                        };
                    }
                }
                true
            }
            KeyCode::Char('s') => {
                if let Some(agent) = self.selected_agent() {
                    match agent.state {
                        DisplayState::Stopped | DisplayState::Paused => {
                            self.pending_action =
                                Some(AgentAction::Start(agent.name.clone()));
                        }
                        DisplayState::Running => {
                            self.modal = AgentModal::ActionConfirm {
                                agent_name: agent.name.clone(),
                                action: "stop".to_string(),
                            };
                        }
                        _ => {}
                    }
                }
                true
            }
            _ => false,
        }
    }
}

// ── Modal rendering ─────────────────────────────────────────────────────────

fn draw_install_modal(
    frame: &mut Frame,
    area: Rect,
    agent_name: &str,
    permissions: &[String],
) {
    let perm_count = permissions.len();
    let h = (8 + perm_count as u16).min(area.height.saturating_sub(4));
    let w = 50.min(area.width.saturating_sub(4));
    let x = (area.width.saturating_sub(w)) / 2 + area.x;
    let y = (area.height.saturating_sub(h)) / 2 + area.y;
    let modal_area = Rect::new(x, y, w, h);

    frame.render_widget(Clear, modal_area);

    let mut lines = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            format!("  Install \"{}\"?", agent_name),
            Style::default()
                .fg(Color::Rgb(255, 175, 95))
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
    ];

    if permissions.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "  No permissions required",
            Style::default().fg(Color::DarkGray),
        )]));
    } else {
        lines.push(Line::from(vec![Span::styled(
            "  Requested permissions:",
            Style::default().fg(Color::Rgb(200, 200, 210)),
        )]));
        for perm in permissions {
            lines.push(Line::from(vec![
                Span::styled("    ◦ ", Style::default().fg(Color::Yellow)),
                Span::styled(perm.clone(), Style::default().fg(Color::White)),
            ]));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  [y/Enter] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::styled("Accept  ", Style::default().fg(Color::Rgb(200, 200, 210))),
        Span::styled("[n/Esc] ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        Span::styled("Cancel", Style::default().fg(Color::Rgb(200, 200, 210))),
    ]));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(255, 175, 95)))
        .style(Style::default().bg(Color::Rgb(22, 22, 30)))
        .title(Line::from(vec![Span::styled(
            " Install Agent ",
            Style::default()
                .fg(Color::Rgb(255, 175, 95))
                .add_modifier(Modifier::BOLD),
        )]));

    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false })
            .style(Style::default().bg(Color::Rgb(22, 22, 30))),
        modal_area,
    );
}

fn draw_confirm_modal(frame: &mut Frame, area: Rect, agent_name: &str, action: &str) {
    let h = 7.min(area.height.saturating_sub(4));
    let w = 45.min(area.width.saturating_sub(4));
    let x = (area.width.saturating_sub(w)) / 2 + area.x;
    let y = (area.height.saturating_sub(h)) / 2 + area.y;
    let modal_area = Rect::new(x, y, w, h);

    frame.render_widget(Clear, modal_area);

    let lines = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            format!("  {} \"{}\"?", capitalize(action), agent_name),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  [y/Enter] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::styled("Confirm  ", Style::default().fg(Color::Rgb(200, 200, 210))),
            Span::styled("[n/Esc] ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::styled("Cancel", Style::default().fg(Color::Rgb(200, 200, 210))),
        ]),
    ];

    let border_color = if action == "uninstall" {
        Color::Red
    } else {
        Color::Yellow
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(Color::Rgb(22, 22, 30)))
        .title(Line::from(vec![Span::styled(
            format!(" {} ", capitalize(action)),
            Style::default()
                .fg(border_color)
                .add_modifier(Modifier::BOLD),
        )]));

    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .style(Style::default().bg(Color::Rgb(22, 22, 30))),
        modal_area,
    );
}

fn draw_status_modal(frame: &mut Frame, area: Rect, message: &str, is_error: bool) {
    let h = 5.min(area.height.saturating_sub(4));
    let w = 50.min(area.width.saturating_sub(4));
    let x = (area.width.saturating_sub(w)) / 2 + area.x;
    let y = (area.height.saturating_sub(h)) / 2 + area.y;
    let modal_area = Rect::new(x, y, w, h);

    frame.render_widget(Clear, modal_area);

    let color = if is_error { Color::Red } else { Color::Green };
    let icon = if is_error { "✗" } else { "✓" };

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(format!("  {} ", icon), Style::default().fg(color)),
            Span::styled(message.to_string(), Style::default().fg(Color::White)),
        ]),
        Line::from(vec![Span::styled(
            "  Press any key to close",
            Style::default().fg(Color::DarkGray),
        )]),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(color))
        .style(Style::default().bg(Color::Rgb(22, 22, 30)));

    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .style(Style::default().bg(Color::Rgb(22, 22, 30))),
        modal_area,
    );
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Color-code categories for visual grouping.
pub fn category_color(cat: &str) -> Color {
    let c = cat.to_lowercase();
    if c.contains("research") || c.contains("analysis") {
        Color::Blue
    } else if c.contains("productivity") || c.contains("workflow") {
        Color::Green
    } else if c.contains("finance") || c.contains("trading") {
        Color::Yellow
    } else if c.contains("security") || c.contains("compliance") {
        Color::Red
    } else if c.contains("development") || c.contains("devops") {
        Color::Cyan
    } else if c.contains("communication") || c.contains("social") {
        Color::Magenta
    } else if c.contains("creative") || c.contains("design") {
        Color::LightMagenta
    } else {
        Color::DarkGray
    }
}

pub fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else if max <= 1 {
        "…".to_string()
    } else {
        format!("{}…", &s[..max - 1])
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().to_string() + c.as_str(),
    }
}
