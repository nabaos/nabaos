//! Resources tab — registered resources and lease management.
//!
//! Features:
//! - Resource list with type/status/lease indicators
//! - Lease viewer with quota usage bars
//! - Register new resource modal (id, name, type)
//! - Delete resource with confirmation
//! - Status symbols per resource type

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

use super::Tab;

// ── Display types ───────────────────────────────────────────────────────────

/// Summary of a resource for display.
#[derive(Clone)]
pub struct ResourceSummary {
    pub id: String,
    pub name: String,
    pub resource_type: String,
    pub status: String,
    pub active_leases: usize,
    pub cost_model: String,
    pub registered_at: i64,
    pub metadata: Vec<(String, String)>,
}

/// Summary of an active lease for display.
#[derive(Clone)]
pub struct LeaseSummary {
    pub lease_id: String,
    pub resource_id: String,
    pub agent_id: String,
    pub capabilities: Vec<String>,
    pub used_cost_usd: f64,
    pub max_cost_usd: Option<f64>,
    pub used_calls: u64,
    pub max_calls: Option<u64>,
    pub started_at: i64,
    pub expires_at: Option<i64>,
    pub status: String,
}

/// What view is active.
#[derive(Clone, PartialEq)]
pub enum ResourceView {
    /// Top-level resource list.
    Resources,
    /// Lease list for a selected resource.
    Leases(String), // resource_id
}

/// Modal dialogs.
#[derive(Clone)]
pub enum ResourceModal {
    None,
    /// Register new resource — input fields.
    Register {
        fields: Vec<(String, String)>, // (label, value)
        active_field: usize,
        type_index: usize, // index into RESOURCE_TYPES
    },
    /// Confirm delete.
    DeleteConfirm {
        resource_id: String,
        resource_name: String,
    },
    /// Status message.
    StatusMessage {
        message: String,
        is_error: bool,
    },
}

/// Action request from tab to app.
#[derive(Clone)]
pub enum ResourceAction {
    Register {
        id: String,
        name: String,
        resource_type: String,
    },
    Delete {
        resource_id: String,
    },
    LoadLeases {
        resource_id: String,
    },
}

const RESOURCE_TYPES: &[&str] = &["Compute", "Financial", "Device", "ApiService"];

// ── ResourcesTab ────────────────────────────────────────────────────────────

pub struct ResourcesTab {
    pub resources: Vec<ResourceSummary>,
    pub leases: Vec<LeaseSummary>,
    pub state: ListState,
    pub lease_state: ListState,
    pub view: ResourceView,
    pub modal: ResourceModal,
    pub pending_action: Option<ResourceAction>,
}

impl ResourcesTab {
    pub fn new() -> Self {
        Self {
            resources: Vec::new(),
            leases: Vec::new(),
            state: ListState::default(),
            lease_state: ListState::default(),
            view: ResourceView::Resources,
            modal: ResourceModal::None,
            pending_action: None,
        }
    }

    pub fn set_resources(&mut self, resources: Vec<ResourceSummary>) {
        self.resources = resources;
        if !self.resources.is_empty() && self.state.selected().is_none() {
            self.state.select(Some(0));
        }
    }

    pub fn set_leases(&mut self, leases: Vec<LeaseSummary>) {
        self.leases = leases;
        if !self.leases.is_empty() {
            self.lease_state.select(Some(0));
        } else {
            self.lease_state.select(None);
        }
    }

    /// Get the currently selected resource, if any.
    pub fn selected(&self) -> Option<&ResourceSummary> {
        self.state.selected().and_then(|i| self.resources.get(i))
    }

    /// Get the currently selected lease, if any.
    pub fn selected_lease(&self) -> Option<&LeaseSummary> {
        self.lease_state.selected().and_then(|i| self.leases.get(i))
    }

    /// Take any pending action.
    pub fn take_action(&mut self) -> Option<ResourceAction> {
        self.pending_action.take()
    }

    /// Show a status message.
    pub fn show_status(&mut self, message: String, is_error: bool) {
        self.modal = ResourceModal::StatusMessage { message, is_error };
    }
}

// ── Tab implementation ──────────────────────────────────────────────────────

impl Tab for ResourcesTab {
    fn render(&self, frame: &mut Frame, area: Rect) {
        if self.resources.is_empty() && matches!(self.view, ResourceView::Resources) {
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
                    Span::styled("  Press ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        "r",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        " to register a new resource",
                        Style::default().fg(Color::DarkGray),
                    ),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("  Or use: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        "nabaos resource register <id> <name> <type>",
                        Style::default().fg(Color::Cyan),
                    ),
                ]),
            ])
            .block(block);
            frame.render_widget(content, area);
            // Still render modal overlay if present
            self.render_modal(frame, area);
            return;
        }

        match &self.view {
            ResourceView::Resources => self.render_resources(frame, area),
            ResourceView::Leases(_) => self.render_leases(frame, area),
        }

        self.render_modal(frame, area);
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        // Modal intercepts all keys
        match &mut self.modal {
            ResourceModal::Register {
                fields,
                active_field,
                type_index,
            } => {
                match key.code {
                    KeyCode::Tab | KeyCode::Down => {
                        // 3 fields: id, name, type
                        *active_field = (*active_field + 1) % 3;
                    }
                    KeyCode::Up => {
                        *active_field = active_field.checked_sub(1).unwrap_or(2);
                    }
                    KeyCode::Enter => {
                        let id = fields[0].1.clone();
                        let name = fields[1].1.clone();
                        let rtype = RESOURCE_TYPES[*type_index].to_string();
                        if !id.is_empty() && !name.is_empty() {
                            self.modal = ResourceModal::None;
                            self.pending_action = Some(ResourceAction::Register {
                                id,
                                name,
                                resource_type: rtype,
                            });
                        }
                    }
                    KeyCode::Esc => {
                        self.modal = ResourceModal::None;
                    }
                    KeyCode::Left if *active_field == 2 => {
                        if *type_index > 0 {
                            *type_index -= 1;
                        } else {
                            *type_index = RESOURCE_TYPES.len() - 1;
                        }
                    }
                    KeyCode::Right if *active_field == 2 => {
                        *type_index = (*type_index + 1) % RESOURCE_TYPES.len();
                    }
                    KeyCode::Backspace if *active_field < 2 => {
                        fields[*active_field].1.pop();
                    }
                    KeyCode::Char(c) if *active_field < 2 => {
                        fields[*active_field].1.push(c);
                    }
                    _ => {}
                }
                return true;
            }
            ResourceModal::DeleteConfirm { resource_id, .. } => {
                match key.code {
                    KeyCode::Char('y') | KeyCode::Enter => {
                        let id = resource_id.clone();
                        self.modal = ResourceModal::None;
                        self.pending_action = Some(ResourceAction::Delete { resource_id: id });
                    }
                    _ => {
                        self.modal = ResourceModal::None;
                    }
                }
                return true;
            }
            ResourceModal::StatusMessage { .. } => {
                self.modal = ResourceModal::None;
                return true;
            }
            ResourceModal::None => {}
        }

        match &self.view {
            ResourceView::Resources => self.handle_key_resources(key),
            ResourceView::Leases(_) => self.handle_key_leases(key),
        }
    }
}

// ── Rendering helpers ───────────────────────────────────────────────────────

impl ResourcesTab {
    fn render_resources(&self, frame: &mut Frame, area: Rect) {
        let avail = area.width.saturating_sub(6) as usize;
        let sym_w = 2;
        let name_w = 22.min(avail / 3);
        let type_w = 12.min(avail / 4);
        let status_w = 14;
        let lease_w = avail.saturating_sub(sym_w + name_w + type_w + status_w + 4);

        let items: Vec<ListItem> = self
            .resources
            .iter()
            .map(|r| {
                let sc = resource_status_color(&r.status);
                let sym = resource_type_symbol(&r.resource_type);
                let name = truncate(&r.name, name_w);
                let rtype = truncate(&r.resource_type, type_w);

                let lease_text = if r.active_leases > 0 {
                    format!(
                        "{} lease{}",
                        r.active_leases,
                        if r.active_leases == 1 { "" } else { "s" }
                    )
                } else {
                    "no leases".to_string()
                };

                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{} ", sym),
                        Style::default().fg(resource_type_color(&r.resource_type)),
                    ),
                    Span::styled(
                        format!("{:<width$} ", name, width = name_w),
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{:<width$} ", rtype, width = type_w),
                        Style::default().fg(resource_type_color(&r.resource_type)),
                    ),
                    Span::styled(
                        format!("{:<width$} ", r.status, width = status_w),
                        Style::default().fg(sc),
                    ),
                    Span::styled(
                        truncate(&lease_text, lease_w),
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

    fn render_leases(&self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::vertical([
            Constraint::Length(1), // breadcrumb
            Constraint::Min(5),   // lease list
        ])
        .split(area);

        // Breadcrumb
        let res_name = if let ResourceView::Leases(ref res_id) = self.view {
            self.resources
                .iter()
                .find(|r| r.id == *res_id)
                .map(|r| r.name.as_str())
                .unwrap_or(res_id.as_str())
        } else {
            ""
        };
        let breadcrumb = Line::from(vec![
            Span::styled(" ← ", Style::default().fg(Color::Cyan)),
            Span::styled("Resources", Style::default().fg(Color::DarkGray)),
            Span::styled(" / ", Style::default().fg(Color::Rgb(60, 60, 80))),
            Span::styled(
                res_name.to_string(),
                Style::default()
                    .fg(Color::Rgb(255, 175, 95))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  ({} leases)", self.leases.len()),
                Style::default().fg(Color::DarkGray),
            ),
        ]);
        frame.render_widget(Paragraph::new(breadcrumb), chunks[0]);

        if self.leases.is_empty() {
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(Line::from(vec![Span::styled(
                    " Leases ",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                )]));
            let content = Paragraph::new(vec![
                Line::from(""),
                Line::from(vec![Span::styled(
                    "  No active leases",
                    Style::default().fg(Color::DarkGray),
                )]),
            ])
            .block(block);
            frame.render_widget(content, chunks[1]);
            return;
        }

        let avail = chunks[1].width.saturating_sub(6) as usize;
        let agent_w = 18.min(avail / 3);
        let status_w = 10;
        let usage_w = avail.saturating_sub(agent_w + status_w + 2);

        let items: Vec<ListItem> = self
            .leases
            .iter()
            .map(|l| {
                let sc = lease_status_color(&l.status);
                let agent = truncate(&l.agent_id, agent_w);

                // Build usage string
                let usage = if let Some(max) = l.max_calls {
                    format!("{}/{} calls", l.used_calls, max)
                } else if l.max_cost_usd.is_some() {
                    format!(
                        "${:.2}/${:.2}",
                        l.used_cost_usd,
                        l.max_cost_usd.unwrap_or(0.0)
                    )
                } else {
                    format!("{} calls", l.used_calls)
                };

                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{:<width$} ", agent, width = agent_w),
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{:<width$} ", l.status, width = status_w),
                        Style::default().fg(sc),
                    ),
                    Span::styled(
                        truncate(&usage, usage_w),
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
                    " Leases ",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("({}) ", self.leases.len()),
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

        frame.render_stateful_widget(list, chunks[1], &mut self.lease_state.clone());
    }

    fn render_modal(&self, frame: &mut Frame, area: Rect) {
        match &self.modal {
            ResourceModal::None => {}
            ResourceModal::Register {
                fields,
                active_field,
                type_index,
            } => {
                draw_register_modal(frame, area, fields, *active_field, *type_index);
            }
            ResourceModal::DeleteConfirm {
                resource_name, ..
            } => {
                draw_delete_modal(frame, area, resource_name);
            }
            ResourceModal::StatusMessage { message, is_error } => {
                draw_status_modal(frame, area, message, *is_error);
            }
        }
    }

    fn handle_key_resources(&mut self, key: KeyEvent) -> bool {
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
            // Enter — view leases
            KeyCode::Enter => {
                if let Some(res) = self.selected() {
                    let res_id = res.id.clone();
                    self.view = ResourceView::Leases(res_id.clone());
                    self.pending_action = Some(ResourceAction::LoadLeases {
                        resource_id: res_id,
                    });
                }
                true
            }
            // r — register new resource
            KeyCode::Char('r') => {
                self.modal = ResourceModal::Register {
                    fields: vec![
                        ("ID".to_string(), String::new()),
                        ("Name".to_string(), String::new()),
                    ],
                    active_field: 0,
                    type_index: 0,
                };
                true
            }
            // d — delete selected resource
            KeyCode::Char('d') => {
                if let Some(res) = self.selected() {
                    self.modal = ResourceModal::DeleteConfirm {
                        resource_id: res.id.clone(),
                        resource_name: res.name.clone(),
                    };
                }
                true
            }
            _ => false,
        }
    }

    fn handle_key_leases(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                let len = self.leases.len();
                if len > 0 {
                    let i = self.lease_state.selected().unwrap_or(0);
                    self.lease_state.select(Some((i + 1) % len));
                }
                true
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let len = self.leases.len();
                if len > 0 {
                    let i = self.lease_state.selected().unwrap_or(0);
                    self.lease_state
                        .select(Some(i.checked_sub(1).unwrap_or(len - 1)));
                }
                true
            }
            // Esc / Backspace — back to resources
            KeyCode::Esc | KeyCode::Backspace => {
                self.view = ResourceView::Resources;
                self.leases.clear();
                self.lease_state.select(None);
                true
            }
            _ => false,
        }
    }
}

// ── Modal rendering ─────────────────────────────────────────────────────────

fn draw_register_modal(
    frame: &mut Frame,
    area: Rect,
    fields: &[(String, String)],
    active_field: usize,
    type_index: usize,
) {
    let h = 14.min(area.height.saturating_sub(4));
    let w = 55.min(area.width.saturating_sub(4));
    let x = (area.width.saturating_sub(w)) / 2 + area.x;
    let y = (area.height.saturating_sub(h)) / 2 + area.y;
    let modal_area = Rect::new(x, y, w, h);

    frame.render_widget(Clear, modal_area);

    let mut lines = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            "  Register New Resource",
            Style::default()
                .fg(Color::Rgb(255, 175, 95))
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
    ];

    // ID field
    let id_style = if active_field == 0 {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::Rgb(200, 200, 210))
    };
    let id_val = &fields[0].1;
    lines.push(Line::from(vec![
        Span::styled("  ID: ", id_style),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  ", Style::default()),
        if active_field == 0 {
            Span::styled(
                format!("{}▏", id_val),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled(
                if id_val.is_empty() { "—" } else { id_val.as_str() },
                Style::default().fg(Color::DarkGray),
            )
        },
    ]));

    // Name field
    let name_style = if active_field == 1 {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::Rgb(200, 200, 210))
    };
    let name_val = &fields[1].1;
    lines.push(Line::from(vec![
        Span::styled("  Name: ", name_style),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  ", Style::default()),
        if active_field == 1 {
            Span::styled(
                format!("{}▏", name_val),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled(
                if name_val.is_empty() {
                    "—"
                } else {
                    name_val.as_str()
                },
                Style::default().fg(Color::DarkGray),
            )
        },
    ]));

    // Type selector
    let type_style = if active_field == 2 {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::Rgb(200, 200, 210))
    };
    lines.push(Line::from(vec![
        Span::styled("  Type: ", type_style),
    ]));

    let type_spans: Vec<Span> = RESOURCE_TYPES
        .iter()
        .enumerate()
        .flat_map(|(i, t)| {
            let style = if i == type_index {
                Style::default()
                    .fg(Color::Rgb(255, 175, 95))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let prefix = if i == type_index { "▸ " } else { "  " };
            vec![
                Span::styled(format!("{}{}", prefix, t), style),
                Span::raw("  "),
            ]
        })
        .collect();
    lines.push(Line::from(type_spans));

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(
            "  [Enter] ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("Register  ", Style::default().fg(Color::Rgb(200, 200, 210))),
        Span::styled(
            "[Tab] ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("Next  ", Style::default().fg(Color::Rgb(200, 200, 210))),
        Span::styled(
            "[Esc] ",
            Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("Cancel", Style::default().fg(Color::Rgb(200, 200, 210))),
    ]));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(255, 175, 95)))
        .style(Style::default().bg(Color::Rgb(22, 22, 30)))
        .title(Line::from(vec![Span::styled(
            " Register Resource ",
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

fn draw_delete_modal(frame: &mut Frame, area: Rect, resource_name: &str) {
    let h = 7.min(area.height.saturating_sub(4));
    let w = 50.min(area.width.saturating_sub(4));
    let x = (area.width.saturating_sub(w)) / 2 + area.x;
    let y = (area.height.saturating_sub(h)) / 2 + area.y;
    let modal_area = Rect::new(x, y, w, h);

    frame.render_widget(Clear, modal_area);

    let lines = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            format!("  Delete \"{}\"?", resource_name),
            Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "  [y/Enter] ",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("Confirm  ", Style::default().fg(Color::Rgb(200, 200, 210))),
            Span::styled(
                "[n/Esc] ",
                Style::default()
                    .fg(Color::Red)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("Cancel", Style::default().fg(Color::Rgb(200, 200, 210))),
        ]),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red))
        .style(Style::default().bg(Color::Rgb(22, 22, 30)))
        .title(Line::from(vec![Span::styled(
            " Delete Resource ",
            Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD),
        )]));

    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
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

fn resource_status_color(status: &str) -> Color {
    let s = status.to_lowercase();
    if s == "available" {
        Color::Green
    } else if s.starts_with("inuse") || s.starts_with("in_use") {
        Color::Cyan
    } else if s == "provisioning" {
        Color::Yellow
    } else if s == "degraded" {
        Color::Yellow
    } else if s == "offline" {
        Color::Red
    } else if s == "terminated" {
        Color::DarkGray
    } else {
        Color::DarkGray
    }
}

fn resource_type_symbol(rtype: &str) -> &'static str {
    match rtype.to_lowercase().as_str() {
        "compute" => "⚙",
        "financial" => "₿",
        "device" => "◈",
        "apiservice" => "⌘",
        _ => "·",
    }
}

fn resource_type_color(rtype: &str) -> Color {
    match rtype.to_lowercase().as_str() {
        "compute" => Color::Cyan,
        "financial" => Color::Yellow,
        "device" => Color::Magenta,
        "apiservice" => Color::Blue,
        _ => Color::DarkGray,
    }
}

fn lease_status_color(status: &str) -> Color {
    match status.to_lowercase().as_str() {
        "active" => Color::Green,
        "expired" => Color::Yellow,
        "revoked" => Color::Red,
        "released" => Color::DarkGray,
        _ => Color::DarkGray,
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

/// Format a quota usage bar.
pub fn quota_bar(used: f64, max: f64, width: usize) -> (String, String, Color) {
    let ratio = if max > 0.0 { (used / max).min(1.0) } else { 0.0 };
    let filled = (ratio * width as f64) as usize;
    let empty = width.saturating_sub(filled);
    let color = if ratio >= 0.9 {
        Color::Red
    } else if ratio >= 0.7 {
        Color::Yellow
    } else {
        Color::Cyan
    };
    ("█".repeat(filled), "░".repeat(empty), color)
}
