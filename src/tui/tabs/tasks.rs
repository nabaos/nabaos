//! PEA tab — objective list, task tree, belief inspector, and budget tracking.
//!
//! Features:
//! - Objective list with budget bars and status indicators
//! - Task tree in drilldown view with dependency visualization
//! - Belief inspector with confidence bars
//! - Create objective modal (description + budget input)
//! - Pause/resume/cancel actions with confirmations
//! - Milestone progress tracking

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

use super::Tab;

// ── Display types ───────────────────────────────────────────────────────────

/// Displayable objective summary.
#[derive(Clone)]
pub struct ObjectiveSummary {
    pub id: String,
    pub description: String,
    pub status: String,
    pub spent: f64,
    pub budget: f64,
    pub progress_score: f64,
    pub task_count: usize,
    pub completed_tasks: usize,
    pub milestone_count: usize,
    pub milestones_achieved: usize,
    pub budget_strategy: String,
    pub beliefs: Vec<(String, f64)>, // (key, confidence)
    pub created_at: u64,
}

/// Displayable task in the task tree.
#[derive(Clone)]
pub struct TaskSummary {
    pub id: String,
    pub description: String,
    pub status: String,
    pub task_type: String,
    pub depends_on: Vec<String>,
    pub parent_task_id: Option<String>,
    pub depth: usize, // nesting depth for indentation
    pub capability: Option<String>,
    pub retry_count: i32,
    pub max_retries: i32,
}

/// What view is active.
#[derive(Clone, PartialEq)]
pub enum PeaView {
    /// Top-level objective list.
    Objectives,
    /// Task tree for a selected objective.
    Tasks(String), // objective_id
}

/// Modal dialogs.
#[derive(Clone)]
pub enum PeaModal {
    None,
    /// Create new objective.
    CreateObjective {
        description: String,
        budget: String,
        active_field: usize, // 0=description, 1=budget
    },
    /// Confirm pause/resume/cancel.
    ActionConfirm {
        objective_id: String,
        action: String, // "pause", "resume", "cancel"
    },
    /// Status message.
    StatusMessage {
        message: String,
        is_error: bool,
    },
}

/// Action request from tab to app.
#[derive(Clone)]
pub enum PeaAction {
    Create {
        description: String,
        budget_usd: f64,
    },
    Pause {
        objective_id: String,
    },
    Resume {
        objective_id: String,
    },
    Cancel {
        objective_id: String,
    },
    LoadTasks {
        objective_id: String,
    },
}

// ── TasksTab (PEA) ─────────────────────────────────────────────────────────

pub struct TasksTab {
    pub objectives: Vec<ObjectiveSummary>,
    pub tasks: Vec<TaskSummary>,
    pub state: ListState,
    pub task_state: ListState,
    pub view: PeaView,
    pub modal: PeaModal,
    pub pending_action: Option<PeaAction>,
}

impl TasksTab {
    pub fn new() -> Self {
        Self {
            objectives: Vec::new(),
            tasks: Vec::new(),
            state: ListState::default(),
            task_state: ListState::default(),
            view: PeaView::Objectives,
            modal: PeaModal::None,
            pending_action: None,
        }
    }

    /// Replace objectives list (called on refresh).
    pub fn set_objectives(&mut self, objectives: Vec<ObjectiveSummary>) {
        self.objectives = objectives;
        if !self.objectives.is_empty() && self.state.selected().is_none() {
            self.state.select(Some(0));
        }
    }

    pub fn set_tasks(&mut self, tasks: Vec<TaskSummary>) {
        self.tasks = tasks;
        if !self.tasks.is_empty() {
            self.task_state.select(Some(0));
        } else {
            self.task_state.select(None);
        }
    }

    /// Get the currently selected objective, if any.
    pub fn selected(&self) -> Option<&ObjectiveSummary> {
        self.state.selected().and_then(|i| self.objectives.get(i))
    }

    /// Get the currently selected task, if any.
    pub fn selected_task(&self) -> Option<&TaskSummary> {
        self.task_state.selected().and_then(|i| self.tasks.get(i))
    }

    /// Take any pending action.
    pub fn take_action(&mut self) -> Option<PeaAction> {
        self.pending_action.take()
    }

    /// Show a status message.
    pub fn show_status(&mut self, message: String, is_error: bool) {
        self.modal = PeaModal::StatusMessage { message, is_error };
    }
}

// ── Tab implementation ──────────────────────────────────────────────────────

impl Tab for TasksTab {
    fn render(&self, frame: &mut Frame, area: Rect) {
        if self.objectives.is_empty() && matches!(self.view, PeaView::Objectives) {
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(Line::from(vec![Span::styled(
                    " PEA Objectives ",
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
                    Span::styled("  Press ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        "n",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        " to create a new objective",
                        Style::default().fg(Color::DarkGray),
                    ),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("  Or use: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        "nabaos pea start \"your goal\" --budget 1.0",
                        Style::default().fg(Color::Cyan),
                    ),
                ]),
            ])
            .block(block);
            frame.render_widget(content, area);
            self.render_modal(frame, area);
            return;
        }

        match &self.view {
            PeaView::Objectives => self.render_objectives(frame, area),
            PeaView::Tasks(_) => self.render_tasks(frame, area),
        }

        self.render_modal(frame, area);
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        // Modal intercepts all keys
        match &mut self.modal {
            PeaModal::CreateObjective {
                description,
                budget,
                active_field,
            } => {
                match key.code {
                    KeyCode::Tab | KeyCode::Down => {
                        *active_field = (*active_field + 1) % 2;
                    }
                    KeyCode::Up => {
                        *active_field = active_field.checked_sub(1).unwrap_or(1);
                    }
                    KeyCode::Enter => {
                        let desc = description.clone();
                        let budget_val: f64 = budget.parse().unwrap_or(1.0);
                        if !desc.is_empty() && budget_val > 0.0 {
                            self.modal = PeaModal::None;
                            self.pending_action = Some(PeaAction::Create {
                                description: desc,
                                budget_usd: budget_val,
                            });
                        }
                    }
                    KeyCode::Esc => {
                        self.modal = PeaModal::None;
                    }
                    KeyCode::Backspace => {
                        if *active_field == 0 {
                            description.pop();
                        } else {
                            budget.pop();
                        }
                    }
                    KeyCode::Char(c) => {
                        if *active_field == 0 {
                            description.push(c);
                        } else if c.is_ascii_digit() || c == '.' {
                            budget.push(c);
                        }
                    }
                    _ => {}
                }
                return true;
            }
            PeaModal::ActionConfirm {
                objective_id,
                action,
            } => {
                match key.code {
                    KeyCode::Char('y') | KeyCode::Enter => {
                        let id = objective_id.clone();
                        let act = action.clone();
                        self.modal = PeaModal::None;
                        self.pending_action = Some(match act.as_str() {
                            "pause" => PeaAction::Pause { objective_id: id },
                            "resume" => PeaAction::Resume { objective_id: id },
                            "cancel" => PeaAction::Cancel { objective_id: id },
                            _ => return true,
                        });
                    }
                    _ => {
                        self.modal = PeaModal::None;
                    }
                }
                return true;
            }
            PeaModal::StatusMessage { .. } => {
                self.modal = PeaModal::None;
                return true;
            }
            PeaModal::None => {}
        }

        match &self.view {
            PeaView::Objectives => self.handle_key_objectives(key),
            PeaView::Tasks(_) => self.handle_key_tasks(key),
        }
    }
}

// ── Rendering helpers ───────────────────────────────────────────────────────

impl TasksTab {
    fn render_objectives(&self, frame: &mut Frame, area: Rect) {
        let avail = area.width.saturating_sub(6) as usize;
        let budget_w = 28; // bar + cost display
        let desc_w = avail.saturating_sub(budget_w + 2);

        let items: Vec<ListItem> = self
            .objectives
            .iter()
            .map(|obj| {
                let (symbol, color) = objective_status_icon(&obj.status);

                let frac = if obj.budget > 0.0 {
                    (obj.spent / obj.budget).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let bar_w: usize = 10;
                let filled = (frac * bar_w as f64).round() as usize;
                let empty = bar_w.saturating_sub(filled);
                let bar = format!("{}{}", "█".repeat(filled), "░".repeat(empty));

                let desc = truncate(&obj.description, desc_w);

                let task_info = if obj.task_count > 0 {
                    format!(" {}/{}", obj.completed_tasks, obj.task_count)
                } else {
                    String::new()
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
                    Span::styled(task_info, Style::default().fg(Color::Rgb(100, 100, 120))),
                ]))
            })
            .collect();

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(Line::from(vec![
                Span::styled(
                    " PEA Objectives ",
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

    fn render_tasks(&self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::vertical([
            Constraint::Length(1), // breadcrumb
            Constraint::Min(5),   // task tree
        ])
        .split(area);

        // Breadcrumb
        let obj_desc = if let PeaView::Tasks(ref obj_id) = self.view {
            self.objectives
                .iter()
                .find(|o| o.id == *obj_id)
                .map(|o| truncate(&o.description, 30))
                .unwrap_or_else(|| truncate(obj_id, 12))
        } else {
            String::new()
        };
        let breadcrumb = Line::from(vec![
            Span::styled(" ← ", Style::default().fg(Color::Cyan)),
            Span::styled("Objectives", Style::default().fg(Color::DarkGray)),
            Span::styled(" / ", Style::default().fg(Color::Rgb(60, 60, 80))),
            Span::styled(
                obj_desc,
                Style::default()
                    .fg(Color::Rgb(255, 175, 95))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  ({} tasks)", self.tasks.len()),
                Style::default().fg(Color::DarkGray),
            ),
        ]);
        frame.render_widget(Paragraph::new(breadcrumb), chunks[0]);

        if self.tasks.is_empty() {
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(Line::from(vec![Span::styled(
                    " Task Tree ",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                )]));
            let content = Paragraph::new(vec![
                Line::from(""),
                Line::from(vec![Span::styled(
                    "  No tasks decomposed yet",
                    Style::default().fg(Color::DarkGray),
                )]),
                Line::from(""),
                Line::from(vec![Span::styled(
                    "  Tasks are generated when the PEA engine ticks",
                    Style::default().fg(Color::DarkGray),
                )]),
            ])
            .block(block);
            frame.render_widget(content, chunks[1]);
            return;
        }

        let avail = chunks[1].width.saturating_sub(6) as usize;

        let items: Vec<ListItem> = self
            .tasks
            .iter()
            .map(|t| {
                let (sym, color) = task_status_icon(&t.status);
                let indent = "  ".repeat(t.depth);
                let connector = if t.depth > 0 { "├─ " } else { "" };
                let desc_max = avail.saturating_sub(indent.len() + connector.len() + 4);
                let desc = truncate(&t.description, desc_max);

                let type_tag = match t.task_type.as_str() {
                    "Compound" => " ⊞",
                    _ => "",
                };

                ListItem::new(Line::from(vec![
                    Span::styled(format!("{} ", sym), Style::default().fg(color)),
                    Span::styled(
                        format!("{}{}", indent, connector),
                        Style::default().fg(Color::Rgb(60, 60, 80)),
                    ),
                    Span::styled(
                        desc,
                        Style::default().fg(if t.status == "Running" {
                            Color::White
                        } else if t.status == "Completed" {
                            Color::Rgb(100, 100, 120)
                        } else {
                            Color::Rgb(200, 200, 210)
                        }),
                    ),
                    Span::styled(
                        type_tag.to_string(),
                        Style::default().fg(Color::Rgb(100, 100, 120)),
                    ),
                ]))
            })
            .collect();

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(Line::from(vec![
                Span::styled(
                    " Task Tree ",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("({}) ", self.tasks.len()),
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

        frame.render_stateful_widget(list, chunks[1], &mut self.task_state.clone());
    }

    fn render_modal(&self, frame: &mut Frame, area: Rect) {
        match &self.modal {
            PeaModal::None => {}
            PeaModal::CreateObjective {
                description,
                budget,
                active_field,
            } => {
                draw_create_modal(frame, area, description, budget, *active_field);
            }
            PeaModal::ActionConfirm {
                action,
                ..
            } => {
                draw_action_confirm_modal(frame, area, action);
            }
            PeaModal::StatusMessage { message, is_error } => {
                draw_status_modal(frame, area, message, *is_error);
            }
        }
    }

    fn handle_key_objectives(&mut self, key: KeyEvent) -> bool {
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
            // Enter — view task tree
            KeyCode::Enter => {
                if let Some(obj) = self.selected() {
                    let obj_id = obj.id.clone();
                    self.view = PeaView::Tasks(obj_id.clone());
                    self.pending_action = Some(PeaAction::LoadTasks {
                        objective_id: obj_id,
                    });
                }
                true
            }
            // n — create new objective
            KeyCode::Char('n') => {
                self.modal = PeaModal::CreateObjective {
                    description: String::new(),
                    budget: "1.00".to_string(),
                    active_field: 0,
                };
                true
            }
            // p — pause/resume
            KeyCode::Char('p') => {
                if let Some(obj) = self.selected() {
                    let action = if obj.status == "active" {
                        "pause"
                    } else if obj.status == "paused" {
                        "resume"
                    } else {
                        return true;
                    };
                    self.modal = PeaModal::ActionConfirm {
                        objective_id: obj.id.clone(),
                        action: action.to_string(),
                    };
                }
                true
            }
            // x — cancel
            KeyCode::Char('x') => {
                if let Some(obj) = self.selected() {
                    if obj.status == "active" || obj.status == "paused" {
                        self.modal = PeaModal::ActionConfirm {
                            objective_id: obj.id.clone(),
                            action: "cancel".to_string(),
                        };
                    }
                }
                true
            }
            _ => false,
        }
    }

    fn handle_key_tasks(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                let len = self.tasks.len();
                if len > 0 {
                    let i = self.task_state.selected().unwrap_or(0);
                    self.task_state.select(Some((i + 1) % len));
                }
                true
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let len = self.tasks.len();
                if len > 0 {
                    let i = self.task_state.selected().unwrap_or(0);
                    self.task_state
                        .select(Some(i.checked_sub(1).unwrap_or(len - 1)));
                }
                true
            }
            // Esc / Backspace — back to objectives
            KeyCode::Esc | KeyCode::Backspace => {
                self.view = PeaView::Objectives;
                self.tasks.clear();
                self.task_state.select(None);
                true
            }
            _ => false,
        }
    }
}

// ── Modal rendering ─────────────────────────────────────────────────────────

fn draw_create_modal(
    frame: &mut Frame,
    area: Rect,
    description: &str,
    budget: &str,
    active_field: usize,
) {
    let h = 12.min(area.height.saturating_sub(4));
    let w = 55.min(area.width.saturating_sub(4));
    let x = (area.width.saturating_sub(w)) / 2 + area.x;
    let y = (area.height.saturating_sub(h)) / 2 + area.y;
    let modal_area = Rect::new(x, y, w, h);

    frame.render_widget(Clear, modal_area);

    let desc_style = if active_field == 0 {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::Rgb(200, 200, 210))
    };
    let budget_style = if active_field == 1 {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::Rgb(200, 200, 210))
    };

    let lines = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            "  New Objective",
            Style::default()
                .fg(Color::Rgb(255, 175, 95))
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![Span::styled("  Description:", desc_style)]),
        Line::from(vec![
            Span::styled("  ", Style::default()),
            if active_field == 0 {
                Span::styled(
                    format!("{}▏", description),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(
                    if description.is_empty() {
                        "—".to_string()
                    } else {
                        description.to_string()
                    },
                    Style::default().fg(Color::DarkGray),
                )
            },
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Budget: $", budget_style),
            if active_field == 1 {
                Span::styled(
                    format!("{}▏", budget),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(budget.to_string(), Style::default().fg(Color::DarkGray))
            },
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "  [Enter] ",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("Create  ", Style::default().fg(Color::Rgb(200, 200, 210))),
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
        ]),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(255, 175, 95)))
        .style(Style::default().bg(Color::Rgb(22, 22, 30)))
        .title(Line::from(vec![Span::styled(
            " Create Objective ",
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

fn draw_action_confirm_modal(frame: &mut Frame, area: Rect, action: &str) {
    let h = 7.min(area.height.saturating_sub(4));
    let w = 45.min(area.width.saturating_sub(4));
    let x = (area.width.saturating_sub(w)) / 2 + area.x;
    let y = (area.height.saturating_sub(h)) / 2 + area.y;
    let modal_area = Rect::new(x, y, w, h);

    frame.render_widget(Clear, modal_area);

    let action_cap = capitalize(action);
    let border_color = if action == "cancel" {
        Color::Red
    } else {
        Color::Yellow
    };

    let lines = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            format!("  {} this objective?", action_cap),
            Style::default()
                .fg(border_color)
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
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(Color::Rgb(22, 22, 30)))
        .title(Line::from(vec![Span::styled(
            format!(" {} Objective ", action_cap),
            Style::default()
                .fg(border_color)
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

fn objective_status_icon(status: &str) -> (&'static str, Color) {
    match status {
        "active" => ("●", Color::Cyan),
        "completed" => ("✓", Color::Green),
        "failed" => ("✗", Color::Red),
        "paused" => ("◌", Color::Yellow),
        _ => ("○", Color::DarkGray),
    }
}

pub fn task_status_icon(status: &str) -> (&'static str, Color) {
    match status {
        "Pending" => ("○", Color::DarkGray),
        "Ready" => ("◌", Color::Blue),
        "Running" => ("●", Color::Cyan),
        "Completed" => ("✓", Color::Green),
        "Failed" => ("✗", Color::Red),
        "Blocked" => ("⊘", Color::Yellow),
        "Skipped" => ("—", Color::DarkGray),
        _ => ("·", Color::DarkGray),
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

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().to_string() + c.as_str(),
    }
}
