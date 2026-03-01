//! Agents tab — searchable catalog browser with adaptive columns.

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
            Constraint::Length(3), // search
            Constraint::Min(5),   // list
        ])
        .split(area);

        // Search bar
        let search_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(Line::from(vec![Span::styled(
                " Search ",
                Style::default().fg(Color::Cyan),
            )]));
        let search_display = if self.search.is_empty() {
            Line::from(vec![Span::styled(
                " Type to filter agents...",
                Style::default().fg(Color::DarkGray),
            )])
        } else {
            Line::from(vec![
                Span::raw(" "),
                Span::styled(&self.search, Style::default().fg(Color::White)),
            ])
        };
        let search_para = Paragraph::new(search_display).block(search_block);
        frame.render_widget(search_para, chunks[0]);

        // Agent list with adaptive columns
        let filtered = self.filtered();
        let avail = chunks[1].width.saturating_sub(6) as usize; // borders + highlight + status
        let name_w = 22.min(avail / 3);
        let cat_w = 14.min(avail / 4);
        let desc_w = avail.saturating_sub(name_w + cat_w + 2);

        let items: Vec<ListItem> = filtered
            .iter()
            .map(|a| {
                let status = if a.installed { "●" } else { "○" };
                let status_color = if a.installed {
                    Color::Green
                } else {
                    Color::DarkGray
                };
                let name = truncate(&a.name, name_w);
                let cat = truncate(&a.category, cat_w);
                let desc = truncate(&a.description, desc_w);

                ListItem::new(Line::from(vec![
                    Span::styled(format!("{} ", status), Style::default().fg(status_color)),
                    Span::styled(
                        format!("{:<width$} ", name, width = name_w),
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
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
            let empty_block = block;
            let empty_msg = Paragraph::new(vec![
                Line::from(""),
                Line::from(vec![Span::styled(
                    format!("  No agents matching \"{}\"", self.search),
                    Style::default().fg(Color::DarkGray),
                )]),
            ])
            .block(empty_block);
            frame.render_widget(empty_msg, chunks[1]);
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
            frame.render_stateful_widget(list, chunks[1], &mut self.state.clone());
        }
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
                if !self.filtered().is_empty() {
                    self.state.select(Some(0));
                }
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

/// Color-code categories for visual grouping.
fn category_color(cat: &str) -> Color {
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

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else if max <= 1 {
        "…".to_string()
    } else {
        format!("{}…", &s[..max - 1])
    }
}
