//! History tab — past queries + costs.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use ratatui::Frame;

use super::Tab;

/// A historical query entry.
#[derive(Clone)]
pub struct HistoryEntry {
    pub timestamp: String,
    pub query: String,
    pub tier: String,
    pub cost: f64,
    pub latency_ms: f64,
}

pub struct HistoryTab {
    pub entries: Vec<HistoryEntry>,
    pub state: ListState,
}

impl HistoryTab {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            state: ListState::default(),
        }
    }

    pub fn push(&mut self, entry: HistoryEntry) {
        self.entries.push(entry);
    }
}

impl Tab for HistoryTab {
    fn render(&self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .entries
            .iter()
            .rev() // most recent first
            .map(|e| {
                let cost_str = if e.cost < 0.001 {
                    "$0.00".to_string()
                } else {
                    format!("${:.3}", e.cost)
                };
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{} ", e.timestamp),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::raw(&e.query),
                    Span::styled(
                        format!("  [{}]", e.tier),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::styled(
                        format!("  {} {:.0}ms", cost_str, e.latency_ms),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))
            })
            .collect();

        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" History ({}) ", self.entries.len()));

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .add_modifier(Modifier::BOLD)
                    .bg(Color::DarkGray),
            );

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
