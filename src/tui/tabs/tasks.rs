//! Tasks tab — PEA objectives + task tree.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use ratatui::Frame;

use super::Tab;

/// Displayable objective summary.
#[derive(Clone)]
pub struct ObjectiveSummary {
    pub id: String,
    pub description: String,
    pub status: String,
    pub spent: f64,
    pub budget: f64,
}

pub struct TasksTab {
    pub objectives: Vec<ObjectiveSummary>,
    pub state: ListState,
}

impl TasksTab {
    pub fn new() -> Self {
        Self {
            objectives: Vec::new(),
            state: ListState::default(),
        }
    }

    /// Replace objectives list (called on refresh).
    pub fn set_objectives(&mut self, objectives: Vec<ObjectiveSummary>) {
        self.objectives = objectives;
        if !self.objectives.is_empty() && self.state.selected().is_none() {
            self.state.select(Some(0));
        }
    }
}

impl Tab for TasksTab {
    fn render(&self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .objectives
            .iter()
            .map(|obj| {
                let (symbol, color) = match obj.status.as_str() {
                    "active" => ("\u{25cf}", Color::Cyan),    // ●
                    "completed" => ("\u{2713}", Color::Green), // ✓
                    "failed" => ("\u{2717}", Color::Red),      // ✗
                    "paused" => ("\u{25cb}", Color::DarkGray), // ○
                    _ => (" ", Color::White),
                };
                let frac = if obj.budget > 0.0 {
                    obj.spent / obj.budget
                } else {
                    0.0
                };
                let bar_len: usize = 8;
                let filled = (frac * bar_len as f64).round() as usize;
                let empty = bar_len.saturating_sub(filled);
                let bar = format!(
                    "{}{}",
                    "\u{2588}".repeat(filled),
                    "\u{2591}".repeat(empty)
                );
                ListItem::new(Line::from(vec![
                    Span::styled(format!("{} ", symbol), Style::default().fg(color)),
                    Span::raw(&obj.description),
                    Span::styled(
                        format!("  {} ${:.2}/${:.2}", bar, obj.spent, obj.budget),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))
            })
            .collect();

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Objectives ");

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
            KeyCode::Char('j') | KeyCode::Down => {
                let len = self.objectives.len();
                if len > 0 {
                    let i = self.state.selected().unwrap_or(0);
                    self.state.select(Some((i + 1) % len));
                }
                true
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let len = self.objectives.len();
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
