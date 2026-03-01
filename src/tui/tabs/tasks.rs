//! Tasks tab — PEA objectives with status and budget tracking.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
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
        if self.objectives.is_empty() {
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(Line::from(vec![Span::styled(
                    " Objectives ",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                )]));
            let content = Paragraph::new(vec![
                Line::from(""),
                Line::from(""),
                Line::from(vec![Span::styled(
                    "  No objectives yet",
                    Style::default().fg(Color::DarkGray),
                )]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("  Create one: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        "nabaos pea start \"your goal\" --budget 1.0",
                        Style::default().fg(Color::Cyan),
                    ),
                ]),
            ])
            .block(block);
            frame.render_widget(content, area);
            return;
        }

        let avail = area.width.saturating_sub(6) as usize;
        let budget_w = 30; // bar + cost display
        let desc_w = avail.saturating_sub(budget_w + 2);

        let items: Vec<ListItem> = self
            .objectives
            .iter()
            .map(|obj| {
                let (symbol, color) = match obj.status.as_str() {
                    "active" => ("●", Color::Cyan),
                    "completed" => ("✓", Color::Green),
                    "failed" => ("✗", Color::Red),
                    "paused" => ("◌", Color::Yellow),
                    _ => ("○", Color::DarkGray),
                };

                let frac = if obj.budget > 0.0 {
                    (obj.spent / obj.budget).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let bar_w: usize = 10;
                let filled = (frac * bar_w as f64).round() as usize;
                let empty = bar_w.saturating_sub(filled);
                let bar = format!("{}{}", "█".repeat(filled), "░".repeat(empty));

                let desc = if obj.description.len() > desc_w && desc_w > 1 {
                    format!("{}…", &obj.description[..desc_w - 1])
                } else {
                    obj.description.clone()
                };

                ListItem::new(Line::from(vec![
                    Span::styled(format!("{} ", symbol), Style::default().fg(color)),
                    Span::styled(
                        format!("{:<width$} ", desc, width = desc_w),
                        Style::default().fg(Color::White),
                    ),
                    Span::styled(
                        format!("{} ", bar),
                        Style::default().fg(if frac > 0.8 {
                            Color::Yellow
                        } else {
                            Color::Green
                        }),
                    ),
                    Span::styled(
                        format!("${:.2}/${:.2}", obj.spent, obj.budget),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))
            })
            .collect();

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(Line::from(vec![
                Span::styled(
                    " Objectives ",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("({}) ", self.objectives.len()),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .fg(Color::Cyan)
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
