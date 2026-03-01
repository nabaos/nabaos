//! Settings tab — read-only config viewer.

use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem};
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
}

impl SettingsTab {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Populate from config.
    pub fn set_entries(&mut self, entries: Vec<ConfigEntry>) {
        self.entries = entries;
    }
}

impl Tab for SettingsTab {
    fn render(&self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .entries
            .iter()
            .map(|e| {
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("  {:<24} ", e.key),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(&e.value, Style::default().fg(Color::Cyan)),
                ]))
            })
            .collect();

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Settings (read-only) ");

        let list = List::new(items).block(block);
        frame.render_widget(list, area);
    }

    fn handle_key(&mut self, _key: KeyEvent) -> bool {
        // Settings tab is read-only — no key handling
        false
    }
}
