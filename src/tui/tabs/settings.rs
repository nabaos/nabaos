//! Settings tab — sectioned configuration viewer with inline editing.
//!
//! Features:
//! - Grouped sections: Provider, Constitution, Budget, Channels, System
//! - Enter opens edit modal for selected setting
//! - API key input masked with dots
//! - Reload config with 'r'
//! - Save changes to .env

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

use super::Tab;

// ── Display types ───────────────────────────────────────────────────────────

/// A key-value pair for display, optionally grouped into a section.
#[derive(Clone)]
pub struct ConfigEntry {
    pub key: String,
    pub value: String,
    pub section: String,
    pub editable: bool,
    pub is_secret: bool,
    pub env_key: Option<String>, // Corresponding NABA_ env var name
}

impl ConfigEntry {
    /// Helper to create a simple non-editable entry.
    pub fn info(key: &str, value: &str) -> Self {
        Self {
            key: key.to_string(),
            value: value.to_string(),
            section: String::new(),
            editable: false,
            is_secret: false,
            env_key: None,
        }
    }
}

/// Modal dialogs.
#[derive(Clone)]
pub enum SettingsModal {
    None,
    /// Edit a single value.
    EditValue {
        key: String,
        env_key: String,
        value: String,
        is_secret: bool,
    },
    /// Status message.
    StatusMessage {
        message: String,
        is_error: bool,
    },
}

/// Action request from tab to app.
#[derive(Clone)]
pub enum SettingsAction {
    Save {
        env_key: String,
        value: String,
    },
    Reload,
}

// ── Section constants ───────────────────────────────────────────────────────

const SECTIONS: &[&str] = &["Provider", "Constitution", "Budget", "Channels", "API Keys", "System"];

fn section_color(section: &str) -> Color {
    match section {
        "Provider" => Color::Cyan,
        "Constitution" => Color::Magenta,
        "Budget" => Color::Yellow,
        "Channels" => Color::Green,
        "API Keys" => Color::Rgb(255, 175, 95),
        "System" => Color::Rgb(100, 100, 120),
        _ => Color::DarkGray,
    }
}

// ── SettingsTab ─────────────────────────────────────────────────────────────

pub struct SettingsTab {
    pub entries: Vec<ConfigEntry>,
    pub state: ListState,
    pub modal: SettingsModal,
    pub pending_action: Option<SettingsAction>,
    /// Flat display list: mix of section headers and entries.
    display_items: Vec<DisplayItem>,
}

#[derive(Clone)]
enum DisplayItem {
    SectionHeader(String),
    Entry(usize), // index into entries
}

impl SettingsTab {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            state: ListState::default(),
            modal: SettingsModal::None,
            pending_action: None,
            display_items: Vec::new(),
        }
    }

    pub fn set_entries(&mut self, entries: Vec<ConfigEntry>) {
        self.entries = entries;
        self.rebuild_display();
        if !self.display_items.is_empty() && self.state.selected().is_none() {
            // Select first entry (skip header)
            for (i, item) in self.display_items.iter().enumerate() {
                if matches!(item, DisplayItem::Entry(_)) {
                    self.state.select(Some(i));
                    break;
                }
            }
        }
    }

    fn rebuild_display(&mut self) {
        self.display_items.clear();
        for section in SECTIONS {
            let section_entries: Vec<usize> = self
                .entries
                .iter()
                .enumerate()
                .filter(|(_, e)| e.section == *section)
                .map(|(i, _)| i)
                .collect();

            if !section_entries.is_empty() {
                self.display_items
                    .push(DisplayItem::SectionHeader(section.to_string()));
                for idx in section_entries {
                    self.display_items.push(DisplayItem::Entry(idx));
                }
            }
        }

        // Entries with no section
        let unsectioned: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.section.is_empty() || !SECTIONS.contains(&e.section.as_str()))
            .map(|(i, _)| i)
            .collect();
        if !unsectioned.is_empty() {
            self.display_items
                .push(DisplayItem::SectionHeader("Other".to_string()));
            for idx in unsectioned {
                self.display_items.push(DisplayItem::Entry(idx));
            }
        }
    }

    /// Get currently selected entry (skipping headers).
    pub fn selected_entry(&self) -> Option<&ConfigEntry> {
        self.state.selected().and_then(|i| {
            if let Some(DisplayItem::Entry(idx)) = self.display_items.get(i) {
                self.entries.get(*idx)
            } else {
                None
            }
        })
    }

    /// Take any pending action.
    pub fn take_action(&mut self) -> Option<SettingsAction> {
        self.pending_action.take()
    }

    /// Show a status message.
    pub fn show_status(&mut self, message: String, is_error: bool) {
        self.modal = SettingsModal::StatusMessage { message, is_error };
    }

    /// Move selection to next entry (skip headers).
    fn select_next(&mut self) {
        let len = self.display_items.len();
        if len == 0 {
            return;
        }
        let start = self.state.selected().map(|i| i + 1).unwrap_or(0);
        for offset in 0..len {
            let idx = (start + offset) % len;
            if matches!(self.display_items.get(idx), Some(DisplayItem::Entry(_))) {
                self.state.select(Some(idx));
                return;
            }
        }
    }

    /// Move selection to previous entry (skip headers).
    fn select_prev(&mut self) {
        let len = self.display_items.len();
        if len == 0 {
            return;
        }
        let start = self.state.selected().unwrap_or(0);
        for offset in 1..=len {
            let idx = (start + len - offset) % len;
            if matches!(self.display_items.get(idx), Some(DisplayItem::Entry(_))) {
                self.state.select(Some(idx));
                return;
            }
        }
    }
}

// ── Tab implementation ──────────────────────────────────────────────────────

impl Tab for SettingsTab {
    fn render(&self, frame: &mut Frame, area: Rect) {
        let avail = area.width.saturating_sub(4) as usize;
        let key_w = 20.min(avail / 3);

        let items: Vec<ListItem> = self
            .display_items
            .iter()
            .map(|item| match item {
                DisplayItem::SectionHeader(section) => {
                    let color = section_color(section);
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            format!("  ── {} ", section),
                            Style::default()
                                .fg(color)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            "─".repeat(avail.saturating_sub(section.len() + 6)),
                            Style::default().fg(Color::Rgb(40, 40, 50)),
                        ),
                    ]))
                }
                DisplayItem::Entry(idx) => {
                    if let Some(e) = self.entries.get(*idx) {
                        let val_w = avail.saturating_sub(key_w + 5);
                        let display_val = if e.is_secret && !e.value.is_empty() {
                            mask_value(&e.value)
                        } else {
                            e.value.clone()
                        };
                        let val = truncate(&display_val, val_w);
                        let edit_marker = if e.editable { " ✎" } else { "" };

                        ListItem::new(Line::from(vec![
                            Span::styled(
                                format!("    {:<width$} ", e.key, width = key_w),
                                Style::default().fg(Color::DarkGray),
                            ),
                            Span::styled(
                                val,
                                Style::default()
                                    .fg(Color::White)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(
                                edit_marker.to_string(),
                                Style::default().fg(Color::Rgb(100, 100, 120)),
                            ),
                        ]))
                    } else {
                        ListItem::new(Line::from(""))
                    }
                }
            })
            .collect();

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(Line::from(vec![Span::styled(
                " Settings ",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )]));

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .add_modifier(Modifier::BOLD)
                    .bg(Color::DarkGray),
            )
            .highlight_symbol("▸ ");

        frame.render_stateful_widget(list, area, &mut self.state.clone());

        // Modal overlay
        match &self.modal {
            SettingsModal::None => {}
            SettingsModal::EditValue {
                key,
                value,
                is_secret,
                ..
            } => {
                draw_edit_modal(frame, area, key, value, *is_secret);
            }
            SettingsModal::StatusMessage { message, is_error } => {
                draw_status_modal(frame, area, message, *is_error);
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        // Modal intercepts all keys
        match &mut self.modal {
            SettingsModal::EditValue {
                env_key,
                value,
                ..
            } => {
                match key.code {
                    KeyCode::Enter => {
                        let ek = env_key.clone();
                        let v = value.clone();
                        self.modal = SettingsModal::None;
                        self.pending_action = Some(SettingsAction::Save {
                            env_key: ek,
                            value: v,
                        });
                    }
                    KeyCode::Esc => {
                        self.modal = SettingsModal::None;
                    }
                    KeyCode::Backspace => {
                        value.pop();
                    }
                    KeyCode::Char(c) => {
                        value.push(c);
                    }
                    _ => {}
                }
                return true;
            }
            SettingsModal::StatusMessage { .. } => {
                self.modal = SettingsModal::None;
                return true;
            }
            SettingsModal::None => {}
        }

        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                self.select_next();
                true
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.select_prev();
                true
            }
            // Enter — edit selected setting
            KeyCode::Enter => {
                if let Some(entry) = self.selected_entry() {
                    if entry.editable {
                        if let Some(ref ek) = entry.env_key {
                            self.modal = SettingsModal::EditValue {
                                key: entry.key.clone(),
                                env_key: ek.clone(),
                                value: entry.value.clone(),
                                is_secret: entry.is_secret,
                            };
                        }
                    }
                }
                true
            }
            // r — reload config
            KeyCode::Char('r') => {
                self.pending_action = Some(SettingsAction::Reload);
                true
            }
            _ => false,
        }
    }
}

// ── Modal rendering ─────────────────────────────────────────────────────────

fn draw_edit_modal(
    frame: &mut Frame,
    area: Rect,
    key: &str,
    value: &str,
    is_secret: bool,
) {
    let h = 9.min(area.height.saturating_sub(4));
    let w = 55.min(area.width.saturating_sub(4));
    let x = (area.width.saturating_sub(w)) / 2 + area.x;
    let y = (area.height.saturating_sub(h)) / 2 + area.y;
    let modal_area = Rect::new(x, y, w, h);

    frame.render_widget(Clear, modal_area);

    let display_val = if is_secret && !value.is_empty() {
        // Show last 4 chars, mask the rest
        let len = value.len();
        if len > 4 {
            format!("{}…{}", "•".repeat(len.min(20) - 4), &value[len - 4..])
        } else {
            value.to_string()
        }
    } else {
        value.to_string()
    };

    let lines = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            format!("  Edit: {}", key),
            Style::default()
                .fg(Color::Rgb(255, 175, 95))
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                format!("{}▏", display_val),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "  [Enter] ",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("Save  ", Style::default().fg(Color::Rgb(200, 200, 210))),
            Span::styled(
                "[Esc] ",
                Style::default()
                    .fg(Color::Red)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("Cancel", Style::default().fg(Color::Rgb(200, 200, 210))),
        ]),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(255, 175, 95)))
        .style(Style::default().bg(Color::Rgb(22, 22, 30)))
        .title(Line::from(vec![Span::styled(
            " Edit Setting ",
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

fn mask_value(s: &str) -> String {
    let len = s.len();
    if len <= 4 {
        "•".repeat(len)
    } else {
        format!("•••{}…{}", "•".repeat((len - 4).min(8)), &s[len - 4..])
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else if max <= 1 {
        "…".to_string()
    } else {
        format!("{}…", &s[..max - 1])
    }
}
