//! Agents tab — catalog browser + running agents.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use super::Tab;

/// Catalog entry for display.
#[derive(Clone)]
pub struct AgentEntry {
    pub name: String,
    pub category: String,
    pub description: String,
    pub installed: bool,
}

pub struct AgentsTab {
    pub agents: Vec<AgentEntry>,
    pub state: ListState,
    pub search: String,
}

impl AgentsTab {
    pub fn new() -> Self {
        Self {
            agents: Vec::new(),
            state: ListState::default(),
            search: String::new(),
        }
    }

    pub fn set_agents(&mut self, agents: Vec<AgentEntry>) {
        self.agents = agents;
        if !self.agents.is_empty() && self.state.selected().is_none() {
            self.state.select(Some(0));
        }
    }

    fn filtered(&self) -> Vec<&AgentEntry> {
        if self.search.is_empty() {
            self.agents.iter().collect()
        } else {
            let q = self.search.to_lowercase();
            self.agents
                .iter()
                .filter(|a| {
                    a.name.to_lowercase().contains(&q)
                        || a.category.to_lowercase().contains(&q)
                        || a.description.to_lowercase().contains(&q)
                })
                .collect()
        }
    }
}

impl Tab for AgentsTab {
    fn render(&self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::vertical([
            Constraint::Length(3), // search
            Constraint::Min(5),   // list
        ])
        .split(area);

        // Search bar
        let search_block = Block::default()
            .borders(Borders::ALL)
            .title(" Search agents ");
        let search_para = Paragraph::new(self.search.as_str()).block(search_block);
        frame.render_widget(search_para, chunks[0]);

        // Agent list
        let filtered = self.filtered();
        let items: Vec<ListItem> = filtered
            .iter()
            .map(|a| {
                let status = if a.installed { "✓" } else { " " };
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{} ", status),
                        Style::default().fg(if a.installed {
                            Color::Green
                        } else {
                            Color::DarkGray
                        }),
                    ),
                    Span::styled(
                        format!("{:<22} ", a.name),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{:<14} ", a.category),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::raw(&a.description),
                ]))
            })
            .collect();

        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" Agents ({}) ", filtered.len()));
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
            KeyCode::Down => {
                let len = self.filtered().len();
                if len > 0 {
                    let i = self.state.selected().unwrap_or(0);
                    self.state.select(Some((i + 1) % len));
                }
                true
            }
            KeyCode::Up => {
                let len = self.filtered().len();
                if len > 0 {
                    let i = self.state.selected().unwrap_or(0);
                    self.state.select(Some(i.checked_sub(1).unwrap_or(len - 1)));
                }
                true
            }
            KeyCode::Backspace => {
                self.search.pop();
                true
            }
            KeyCode::Char(c) => {
                self.search.push(c);
                self.state.select(Some(0));
                true
            }
            _ => false,
        }
    }
}
