//! Workflows tab — workflow definition browser and instance monitoring.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use super::Tab;

/// Summary of a workflow definition for display.
#[derive(Clone)]
pub struct WorkflowSummary {
    pub id: String,
    pub name: String,
    pub instance_count: usize,
    pub last_status: String,
}

pub struct WorkflowsTab {
    pub workflows: Vec<WorkflowSummary>,
    pub state: ListState,
}

impl WorkflowsTab {
    pub fn new() -> Self {
        Self {
            workflows: Vec::new(),
            state: ListState::default(),
        }
    }

    pub fn set_workflows(&mut self, workflows: Vec<WorkflowSummary>) {
        self.workflows = workflows;
        if !self.workflows.is_empty() && self.state.selected().is_none() {
            self.state.select(Some(0));
        }
    }

    /// Get the currently selected workflow, if any.
    pub fn selected(&self) -> Option<&WorkflowSummary> {
        self.state.selected().and_then(|i| self.workflows.get(i))
    }
}

impl Tab for WorkflowsTab {
    fn render(&self, frame: &mut Frame, area: Rect) {
        if self.workflows.is_empty() {
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(Line::from(vec![Span::styled(
                    " Workflows ",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                )]));
            let content = Paragraph::new(vec![
                Line::from(""),
                Line::from(""),
                Line::from(vec![Span::styled(
                    "  No workflow definitions",
                    Style::default().fg(Color::DarkGray),
                )]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("  Define workflows in ", Style::default().fg(Color::DarkGray)),
                    Span::styled("~/.nabaos/chains/", Style::default().fg(Color::Cyan)),
                ]),
            ])
            .block(block);
            frame.render_widget(content, area);
            return;
        }

        let avail = area.width.saturating_sub(6) as usize;
        let name_w = 30.min(avail / 2);
        let status_w = 12;
        let count_w = 10;
        let _rest = avail.saturating_sub(name_w + status_w + count_w + 3);

        let items: Vec<ListItem> = self
            .workflows
            .iter()
            .map(|w| {
                let status_color = match w.last_status.as_str() {
                    "completed" => Color::Green,
                    "running" => Color::Cyan,
                    "failed" => Color::Red,
                    "cancelled" => Color::Yellow,
                    _ => Color::DarkGray,
                };
                let name = if w.name.len() > name_w && name_w > 1 {
                    format!("{}…", &w.name[..name_w - 1])
                } else {
                    w.name.clone()
                };

                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{:<width$} ", name, width = name_w),
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{:<width$} ", w.last_status, width = status_w),
                        Style::default().fg(status_color),
                    ),
                    Span::styled(
                        format!("{} instances", w.instance_count),
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
                    " Workflows ",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("({}) ", self.workflows.len()),
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
                let len = self.workflows.len();
                if len > 0 {
                    let i = self.state.selected().unwrap_or(0);
                    self.state.select(Some((i + 1) % len));
                }
                true
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let len = self.workflows.len();
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
