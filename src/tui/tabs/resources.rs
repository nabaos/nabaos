//! Resources tab — registered resources and lease management.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use super::Tab;

/// Summary of a resource for display.
#[derive(Clone)]
pub struct ResourceSummary {
    pub id: String,
    pub name: String,
    pub resource_type: String,
    pub status: String,
    pub active_leases: usize,
}

pub struct ResourcesTab {
    pub resources: Vec<ResourceSummary>,
    pub state: ListState,
}

impl ResourcesTab {
    pub fn new() -> Self {
        Self {
            resources: Vec::new(),
            state: ListState::default(),
        }
    }

    pub fn set_resources(&mut self, resources: Vec<ResourceSummary>) {
        self.resources = resources;
        if !self.resources.is_empty() && self.state.selected().is_none() {
            self.state.select(Some(0));
        }
    }

    /// Get the currently selected resource, if any.
    pub fn selected(&self) -> Option<&ResourceSummary> {
        self.state.selected().and_then(|i| self.resources.get(i))
    }
}

impl Tab for ResourcesTab {
    fn render(&self, frame: &mut Frame, area: Rect) {
        if self.resources.is_empty() {
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(Line::from(vec![Span::styled(
                    " Resources ",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                )]));
            let content = Paragraph::new(vec![
                Line::from(""),
                Line::from(""),
                Line::from(vec![Span::styled(
                    "  No resources registered",
                    Style::default().fg(Color::DarkGray),
                )]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("  Register: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        "nabaos resource register <id> <name> <type>",
                        Style::default().fg(Color::Cyan),
                    ),
                ]),
            ])
            .block(block);
            frame.render_widget(content, area);
            return;
        }

        let avail = area.width.saturating_sub(6) as usize;
        let name_w = 24.min(avail / 3);
        let type_w = 14.min(avail / 4);
        let status_w = 12;
        let _lease_w = avail.saturating_sub(name_w + type_w + status_w + 3);

        let items: Vec<ListItem> = self
            .resources
            .iter()
            .map(|r| {
                let status_color = match r.status.as_str() {
                    "available" => Color::Green,
                    "in_use" => Color::Cyan,
                    "provisioning" => Color::Yellow,
                    "degraded" => Color::Yellow,
                    "offline" => Color::Red,
                    "terminated" => Color::DarkGray,
                    _ => Color::DarkGray,
                };
                let name = if r.name.len() > name_w && name_w > 1 {
                    format!("{}…", &r.name[..name_w - 1])
                } else {
                    r.name.clone()
                };
                let rtype = if r.resource_type.len() > type_w && type_w > 1 {
                    format!("{}…", &r.resource_type[..type_w - 1])
                } else {
                    r.resource_type.clone()
                };

                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{:<width$} ", name, width = name_w),
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{:<width$} ", rtype, width = type_w),
                        Style::default().fg(Color::Blue),
                    ),
                    Span::styled(
                        format!("{:<width$} ", r.status, width = status_w),
                        Style::default().fg(status_color),
                    ),
                    Span::styled(
                        if r.active_leases > 0 {
                            format!("{} lease{}", r.active_leases, if r.active_leases == 1 { "" } else { "s" })
                        } else {
                            "no leases".to_string()
                        },
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
                    " Resources ",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("({}) ", self.resources.len()),
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
                let len = self.resources.len();
                if len > 0 {
                    let i = self.state.selected().unwrap_or(0);
                    self.state.select(Some((i + 1) % len));
                }
                true
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let len = self.resources.len();
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
