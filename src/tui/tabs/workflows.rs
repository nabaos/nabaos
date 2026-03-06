//! Workflows tab — definition browser + instance monitoring.
//!
//! Features:
//! - Workflow definition list with instance counts and last status
//! - Instance sub-list for selected workflow (Enter to expand)
//! - Action keys: n start new instance, c cancel running instance
//! - Status indicators matching WorkflowStatus variants
//! - Start modal with parameter input
//! - Confirm modal for cancel

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

use super::Tab;

// ── Display types ───────────────────────────────────────────────────────────

/// Summary of a workflow definition for display.
#[derive(Clone)]
pub struct WorkflowSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub node_count: usize,
    pub param_names: Vec<String>,
    pub instance_count: usize,
    pub active_count: usize,
    pub last_status: String,
    pub max_instances: u64,
    pub global_timeout_secs: u64,
}

/// Summary of a running/completed workflow instance.
#[derive(Clone)]
pub struct InstanceSummary {
    pub instance_id: String,
    pub workflow_id: String,
    pub status: String,
    pub cursor_node: usize,
    pub node_count: usize,
    pub error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub execution_ms: i64,
    pub node_names: Vec<(String, String)>, // (node_id, node_type) for DAG display
}

/// What view is active in the workflows tab.
#[derive(Clone, PartialEq)]
pub enum WorkflowView {
    /// Top-level definition list.
    Definitions,
    /// Instance list for a selected workflow definition.
    Instances(String), // workflow_id
}

/// Modal dialogs.
#[derive(Clone)]
pub enum WorkflowModal {
    None,
    /// Start new instance — shows param input fields.
    StartInstance {
        workflow_id: String,
        workflow_name: String,
        param_names: Vec<String>,
        param_values: Vec<String>,
        active_field: usize,
    },
    /// Confirm cancel of running instance.
    CancelConfirm {
        instance_id: String,
    },
    /// Status message (toast).
    StatusMessage {
        message: String,
        is_error: bool,
    },
}

/// Action request from tab to app.
#[derive(Clone)]
pub enum WorkflowAction {
    Start {
        workflow_id: String,
        params: Vec<(String, String)>,
    },
    Cancel {
        instance_id: String,
    },
    /// Load instances for a workflow.
    LoadInstances {
        workflow_id: String,
    },
}

// ── WorkflowsTab ────────────────────────────────────────────────────────────

pub struct WorkflowsTab {
    pub workflows: Vec<WorkflowSummary>,
    pub instances: Vec<InstanceSummary>,
    pub state: ListState,
    pub instance_state: ListState,
    pub view: WorkflowView,
    pub modal: WorkflowModal,
    pub pending_action: Option<WorkflowAction>,
}

impl WorkflowsTab {
    pub fn new() -> Self {
        Self {
            workflows: Vec::new(),
            instances: Vec::new(),
            state: ListState::default(),
            instance_state: ListState::default(),
            view: WorkflowView::Definitions,
            modal: WorkflowModal::None,
            pending_action: None,
        }
    }

    pub fn set_workflows(&mut self, workflows: Vec<WorkflowSummary>) {
        self.workflows = workflows;
        if !self.workflows.is_empty() && self.state.selected().is_none() {
            self.state.select(Some(0));
        }
    }

    pub fn set_instances(&mut self, instances: Vec<InstanceSummary>) {
        self.instances = instances;
        if !self.instances.is_empty() {
            self.instance_state.select(Some(0));
        } else {
            self.instance_state.select(None);
        }
    }

    /// Get the currently selected workflow, if any.
    pub fn selected(&self) -> Option<&WorkflowSummary> {
        self.state.selected().and_then(|i| self.workflows.get(i))
    }

    /// Get the currently selected instance, if any.
    pub fn selected_instance(&self) -> Option<&InstanceSummary> {
        self.instance_state
            .selected()
            .and_then(|i| self.instances.get(i))
    }

    /// Take any pending action (consumed by app).
    pub fn take_action(&mut self) -> Option<WorkflowAction> {
        self.pending_action.take()
    }

    /// Show a status message after an action completes.
    pub fn show_status(&mut self, message: String, is_error: bool) {
        self.modal = WorkflowModal::StatusMessage { message, is_error };
    }
}

// ── Tab implementation ──────────────────────────────────────────────────────

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
                    Span::styled(
                        "  Define workflows in ",
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled("~/.nabaos/chains/", Style::default().fg(Color::Cyan)),
                ]),
            ])
            .block(block);
            frame.render_widget(content, area);
            return;
        }

        match &self.view {
            WorkflowView::Definitions => self.render_definitions(frame, area),
            WorkflowView::Instances(_) => self.render_instances(frame, area),
        }

        // Modal overlay
        match &self.modal {
            WorkflowModal::None => {}
            WorkflowModal::StartInstance {
                workflow_name,
                param_names,
                param_values,
                active_field,
                ..
            } => {
                draw_start_modal(frame, area, workflow_name, param_names, param_values, *active_field);
            }
            WorkflowModal::CancelConfirm { instance_id } => {
                draw_cancel_modal(frame, area, instance_id);
            }
            WorkflowModal::StatusMessage { message, is_error } => {
                draw_status_modal(frame, area, message, *is_error);
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        // Modal intercepts all keys
        match &mut self.modal {
            WorkflowModal::StartInstance {
                workflow_id,
                param_names,
                param_values,
                active_field,
                ..
            } => {
                match key.code {
                    KeyCode::Tab | KeyCode::Down => {
                        if !param_names.is_empty() {
                            *active_field = (*active_field + 1) % param_names.len();
                        }
                    }
                    KeyCode::Up => {
                        if !param_names.is_empty() {
                            *active_field = active_field.checked_sub(1).unwrap_or(param_names.len() - 1);
                        }
                    }
                    KeyCode::Enter => {
                        let params: Vec<(String, String)> = param_names
                            .iter()
                            .zip(param_values.iter())
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        let wf_id = workflow_id.clone();
                        self.modal = WorkflowModal::None;
                        self.pending_action = Some(WorkflowAction::Start {
                            workflow_id: wf_id,
                            params,
                        });
                    }
                    KeyCode::Esc => {
                        self.modal = WorkflowModal::None;
                    }
                    KeyCode::Backspace => {
                        if let Some(val) = param_values.get_mut(*active_field) {
                            val.pop();
                        }
                    }
                    KeyCode::Char(c) => {
                        if let Some(val) = param_values.get_mut(*active_field) {
                            val.push(c);
                        }
                    }
                    _ => {}
                }
                return true;
            }
            WorkflowModal::CancelConfirm { instance_id } => {
                match key.code {
                    KeyCode::Char('y') | KeyCode::Enter => {
                        let id = instance_id.clone();
                        self.modal = WorkflowModal::None;
                        self.pending_action = Some(WorkflowAction::Cancel { instance_id: id });
                    }
                    _ => {
                        self.modal = WorkflowModal::None;
                    }
                }
                return true;
            }
            WorkflowModal::StatusMessage { .. } => {
                self.modal = WorkflowModal::None;
                return true;
            }
            WorkflowModal::None => {}
        }

        match &self.view {
            WorkflowView::Definitions => self.handle_key_definitions(key),
            WorkflowView::Instances(_) => self.handle_key_instances(key),
        }
    }
}

// ── Rendering helpers ───────────────────────────────────────────────────────

impl WorkflowsTab {
    fn render_definitions(&self, frame: &mut Frame, area: Rect) {
        let avail = area.width.saturating_sub(6) as usize;
        let name_w = 28.min(avail / 3);
        let status_w = 12;
        let inst_w = 14;
        let desc_w = avail.saturating_sub(name_w + status_w + inst_w + 3);

        let items: Vec<ListItem> = self
            .workflows
            .iter()
            .map(|w| {
                let status_color = status_color(&w.last_status);
                let name = truncate(&w.name, name_w);
                let desc = truncate(&w.description, desc_w);

                let inst_text = if w.active_count > 0 {
                    format!("{} active", w.active_count)
                } else if w.instance_count > 0 {
                    format!("{} total", w.instance_count)
                } else {
                    "no runs".to_string()
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
                        format!("{:<width$} ", inst_text, width = inst_w),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(desc, Style::default().fg(Color::Rgb(100, 100, 120))),
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

    fn render_instances(&self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::vertical([
            Constraint::Length(1), // breadcrumb
            Constraint::Min(5),   // instance list
        ])
        .split(area);

        // Breadcrumb: show which workflow we're viewing
        let wf_name = if let WorkflowView::Instances(ref wf_id) = self.view {
            self.workflows
                .iter()
                .find(|w| w.id == *wf_id)
                .map(|w| w.name.as_str())
                .unwrap_or(wf_id.as_str())
        } else {
            ""
        };
        let breadcrumb = Line::from(vec![
            Span::styled(" ← ", Style::default().fg(Color::Cyan)),
            Span::styled("Workflows", Style::default().fg(Color::DarkGray)),
            Span::styled(" / ", Style::default().fg(Color::Rgb(60, 60, 80))),
            Span::styled(
                wf_name.to_string(),
                Style::default()
                    .fg(Color::Rgb(255, 175, 95))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  ({} instances)", self.instances.len()),
                Style::default().fg(Color::DarkGray),
            ),
        ]);
        frame.render_widget(Paragraph::new(breadcrumb), chunks[0]);

        if self.instances.is_empty() {
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(Line::from(vec![Span::styled(
                    " Instances ",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                )]));
            let content = Paragraph::new(vec![
                Line::from(""),
                Line::from(vec![Span::styled(
                    "  No instances yet",
                    Style::default().fg(Color::DarkGray),
                )]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("  Press ", Style::default().fg(Color::DarkGray)),
                    Span::styled("n", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    Span::styled(" to start a new instance", Style::default().fg(Color::DarkGray)),
                ]),
            ])
            .block(block);
            frame.render_widget(content, chunks[1]);
            return;
        }

        let avail = chunks[1].width.saturating_sub(6) as usize;
        let id_w = 10;
        let status_w = 12;
        let progress_w = 12;
        let time_w = avail.saturating_sub(id_w + status_w + progress_w + 3);

        let items: Vec<ListItem> = self
            .instances
            .iter()
            .map(|inst| {
                let sc = status_color(&inst.status);
                let short_id = truncate(&inst.instance_id, id_w);
                let progress = if inst.node_count > 0 {
                    format!("{}/{} nodes", inst.cursor_node.min(inst.node_count), inst.node_count)
                } else {
                    "—".to_string()
                };
                let elapsed = format_duration_ms(inst.execution_ms);

                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{} ", status_symbol(&inst.status)),
                        Style::default().fg(sc),
                    ),
                    Span::styled(
                        format!("{:<width$} ", short_id, width = id_w),
                        Style::default().fg(Color::White),
                    ),
                    Span::styled(
                        format!("{:<width$} ", inst.status, width = status_w),
                        Style::default().fg(sc),
                    ),
                    Span::styled(
                        format!("{:<width$} ", progress, width = progress_w),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(
                        truncate(&elapsed, time_w),
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
                    " Instances ",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("({}) ", self.instances.len()),
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

        frame.render_stateful_widget(list, chunks[1], &mut self.instance_state.clone());
    }

    fn handle_key_definitions(&mut self, key: KeyEvent) -> bool {
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
            // Enter — drill into instances
            KeyCode::Enter => {
                if let Some(wf) = self.selected() {
                    let wf_id = wf.id.clone();
                    self.view = WorkflowView::Instances(wf_id.clone());
                    self.pending_action = Some(WorkflowAction::LoadInstances {
                        workflow_id: wf_id,
                    });
                }
                true
            }
            // n — start new instance
            KeyCode::Char('n') => {
                if let Some(wf) = self.selected() {
                    if wf.param_names.is_empty() {
                        // No params needed, start directly
                        self.pending_action = Some(WorkflowAction::Start {
                            workflow_id: wf.id.clone(),
                            params: Vec::new(),
                        });
                    } else {
                        self.modal = WorkflowModal::StartInstance {
                            workflow_id: wf.id.clone(),
                            workflow_name: wf.name.clone(),
                            param_names: wf.param_names.clone(),
                            param_values: vec![String::new(); wf.param_names.len()],
                            active_field: 0,
                        };
                    }
                }
                true
            }
            _ => false,
        }
    }

    fn handle_key_instances(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                let len = self.instances.len();
                if len > 0 {
                    let i = self.instance_state.selected().unwrap_or(0);
                    self.instance_state.select(Some((i + 1) % len));
                }
                true
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let len = self.instances.len();
                if len > 0 {
                    let i = self.instance_state.selected().unwrap_or(0);
                    self.instance_state
                        .select(Some(i.checked_sub(1).unwrap_or(len - 1)));
                }
                true
            }
            // Esc / Backspace — go back to definitions
            KeyCode::Esc | KeyCode::Backspace => {
                self.view = WorkflowView::Definitions;
                self.instances.clear();
                self.instance_state.select(None);
                true
            }
            // c — cancel selected instance
            KeyCode::Char('c') => {
                if let Some(inst) = self.selected_instance() {
                    if !is_terminal(&inst.status) {
                        self.modal = WorkflowModal::CancelConfirm {
                            instance_id: inst.instance_id.clone(),
                        };
                    }
                }
                true
            }
            // n — start new instance (also available from instance view)
            KeyCode::Char('n') => {
                if let WorkflowView::Instances(ref wf_id) = self.view {
                    if let Some(wf) = self.workflows.iter().find(|w| w.id == *wf_id) {
                        if wf.param_names.is_empty() {
                            self.pending_action = Some(WorkflowAction::Start {
                                workflow_id: wf.id.clone(),
                                params: Vec::new(),
                            });
                        } else {
                            self.modal = WorkflowModal::StartInstance {
                                workflow_id: wf.id.clone(),
                                workflow_name: wf.name.clone(),
                                param_names: wf.param_names.clone(),
                                param_values: vec![String::new(); wf.param_names.len()],
                                active_field: 0,
                            };
                        }
                    }
                }
                true
            }
            _ => false,
        }
    }
}

// ── Modal rendering ─────────────────────────────────────────────────────────

fn draw_start_modal(
    frame: &mut Frame,
    area: Rect,
    workflow_name: &str,
    param_names: &[String],
    param_values: &[String],
    active_field: usize,
) {
    let field_count = param_names.len();
    let h = (8 + field_count as u16 * 2).min(area.height.saturating_sub(4));
    let w = 55.min(area.width.saturating_sub(4));
    let x = (area.width.saturating_sub(w)) / 2 + area.x;
    let y = (area.height.saturating_sub(h)) / 2 + area.y;
    let modal_area = Rect::new(x, y, w, h);

    frame.render_widget(Clear, modal_area);

    let mut lines = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            format!("  Start \"{}\"", workflow_name),
            Style::default()
                .fg(Color::Rgb(255, 175, 95))
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
    ];

    for (i, name) in param_names.iter().enumerate() {
        let is_active = i == active_field;
        let label_style = if is_active {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::Rgb(200, 200, 210))
        };
        let val = param_values.get(i).map(|s| s.as_str()).unwrap_or("");
        lines.push(Line::from(vec![
            Span::styled(format!("  {}: ", name), label_style),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  ", Style::default()),
            if is_active {
                Span::styled(
                    format!("{}{}", val, "▏"),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(
                    if val.is_empty() {
                        "—".to_string()
                    } else {
                        val.to_string()
                    },
                    Style::default().fg(Color::DarkGray),
                )
            },
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(
            "  [Enter] ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("Start  ", Style::default().fg(Color::Rgb(200, 200, 210))),
        Span::styled(
            "[Tab] ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "Next field  ",
            Style::default().fg(Color::Rgb(200, 200, 210)),
        ),
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
            " Start Workflow ",
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

fn draw_cancel_modal(frame: &mut Frame, area: Rect, instance_id: &str) {
    let h = 7.min(area.height.saturating_sub(4));
    let w = 50.min(area.width.saturating_sub(4));
    let x = (area.width.saturating_sub(w)) / 2 + area.x;
    let y = (area.height.saturating_sub(h)) / 2 + area.y;
    let modal_area = Rect::new(x, y, w, h);

    frame.render_widget(Clear, modal_area);

    let short_id = truncate(instance_id, 20);
    let lines = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            format!("  Cancel instance {}?", short_id),
            Style::default()
                .fg(Color::Yellow)
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
        .border_style(Style::default().fg(Color::Yellow))
        .style(Style::default().bg(Color::Rgb(22, 22, 30)))
        .title(Line::from(vec![Span::styled(
            " Cancel Workflow ",
            Style::default()
                .fg(Color::Yellow)
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

fn status_color(status: &str) -> Color {
    match status.to_lowercase().as_str() {
        "completed" => Color::Green,
        "running" => Color::Cyan,
        "waiting" => Color::Yellow,
        "created" => Color::Blue,
        "failed" => Color::Red,
        "cancelled" => Color::Yellow,
        "timed_out" | "timedout" => Color::Red,
        "compensated" => Color::Magenta,
        "idle" => Color::DarkGray,
        _ => Color::DarkGray,
    }
}

fn status_symbol(status: &str) -> &'static str {
    match status.to_lowercase().as_str() {
        "completed" => "✓",
        "running" => "●",
        "waiting" => "◌",
        "created" => "○",
        "failed" => "✗",
        "cancelled" => "⊘",
        "timed_out" | "timedout" => "⏱",
        "compensated" => "↩",
        _ => "·",
    }
}

pub fn is_terminal_status(status: &str) -> bool {
    is_terminal(status)
}

fn is_terminal(status: &str) -> bool {
    matches!(
        status.to_lowercase().as_str(),
        "completed" | "failed" | "cancelled" | "timed_out" | "timedout" | "compensated"
    )
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

fn format_duration_ms(ms: i64) -> String {
    if ms < 1000 {
        format!("{}ms", ms)
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else if ms < 3_600_000 {
        format!("{:.1}m", ms as f64 / 60_000.0)
    } else {
        format!("{:.1}h", ms as f64 / 3_600_000.0)
    }
}
