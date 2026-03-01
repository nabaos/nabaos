//! Settings tab — system configuration viewer with responsive layout.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use ratatui::Frame;

use super::Tab;

/// A key-value pair for display.
#[derive(Clone)]
pub struct ConfigEntry {
    pub key: String,
    pub value: String,
}

pub struct SettingsTab {
    pub entries: Vec<ConfigEntry>,
    pub state: ListState,
}

impl SettingsTab {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            state: ListState::default(),
        }
    }

    pub fn set_entries(&mut self, entries: Vec<ConfigEntry>) {
        self.entries = entries;
        if !self.entries.is_empty() && self.state.selected().is_none() {
            self.state.select(Some(0));
        }
    }
}

impl Tab for SettingsTab {
    fn render(&self, frame: &mut Frame, area: Rect) {
        let avail = area.width.saturating_sub(4) as usize;
        let key_w = 20.min(avail / 3);

        let items: Vec<ListItem> = self
            .entries
            .iter()
            .map(|e| {
                let val_w = avail.saturating_sub(key_w + 4);
                let val = if e.value.len() > val_w && val_w > 1 {
                    format!("{}…", &e.value[..val_w - 1])
                } else {
                    e.value.clone()
                };
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("  {:<width$}  ", e.key, width = key_w),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(
                        val,
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]))
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
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                let len = self.entries.len();
                if len > 0 {
                    let i = self.state.selected().unwrap_or(0);
                    self.state.select(Some((i + 1) % len));
                }
                true
            }
            KeyCode::Up | KeyCode::Char('k') => {
                let len = self.entries.len();
                if len > 0 {
                    let i = self.state.selected().unwrap_or(0);
                    self.state.select(Some(i.checked_sub(1).unwrap_or(len - 1)));
                }
                true
            }
            _ => false,
        }
    }
}
