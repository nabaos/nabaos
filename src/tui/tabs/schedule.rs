//! Schedule tab — unified view of all scheduled jobs (agents, workflows, PEA).
//!
//! Features:
//! - Job list with schedule info, last run, run count
//! - History sub-list for selected job (Enter to expand)
//! - Action keys: n create new job, d disable/enable toggle
//! - Search filtering with /
//! - New job modal with chain_id + schedule spec input

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

use super::Tab;

// ── Display types ───────────────────────────────────────────────────────────

/// Summary of a scheduled job for display.
#[derive(Clone)]
pub struct JobSummary {
    pub id: String,
    pub chain_id: String,
    pub enabled: bool,
    pub schedule_desc: String,
    pub last_run_at: Option<i64>,
    pub last_output: Option<String>,
    pub run_count: u64,
    pub created_at: i64,
}

/// A history row for a job.
#[derive(Clone)]
pub struct HistoryRow {
    pub job_id: String,
    pub chain_id: String,
    pub output: Option<String>,
    pub changed: bool,
    pub run_at: i64,
}

/// What view is active.
#[derive(Clone, PartialEq)]
pub enum ScheduleView {
    JobList,
    JobHistory(String), // job_id
}

/// Modal dialogs.
#[derive(Clone)]
pub enum ScheduleModal {
    None,
    NewJob {
        chain_id: String,
        spec: String,
        active_field: usize,
    },
    DisableConfirm {
        job_id: String,
        currently_enabled: bool,
    },
    StatusMessage {
        message: String,
        is_error: bool,
    },
}

/// Action request from tab to app.
#[derive(Clone)]
pub enum ScheduleAction {
    Create {
        chain_id: String,
        spec: String,
    },
    ToggleEnabled {
        job_id: String,
        enable: bool,
    },
    LoadHistory {
        job_id: String,
    },
}

// ── ScheduleTab ─────────────────────────────────────────────────────────────

pub struct ScheduleTab {
    pub jobs: Vec<JobSummary>,
    pub history: Vec<HistoryRow>,
    pub view: ScheduleView,
    pub state: ListState,
    pub history_state: ListState,
    pub modal: ScheduleModal,
    pub pending_action: Option<ScheduleAction>,
    pub search_query: String,
    pub searching: bool,
}

impl ScheduleTab {
    pub fn new() -> Self {
        Self {
            jobs: Vec::new(),
            history: Vec::new(),
            view: ScheduleView::JobList,
            state: ListState::default(),
            history_state: ListState::default(),
            modal: ScheduleModal::None,
            pending_action: None,
            search_query: String::new(),
            searching: false,
        }
    }

    pub fn set_jobs(&mut self, jobs: Vec<JobSummary>) {
        self.jobs = jobs;
        if !self.jobs.is_empty() && self.state.selected().is_none() {
            self.state.select(Some(0));
        }
    }

    pub fn set_history(&mut self, history: Vec<HistoryRow>) {
        self.history = history;
        if !self.history.is_empty() {
            self.history_state.select(Some(0));
        } else {
            self.history_state.select(None);
        }
    }

    pub fn selected(&self) -> Option<&JobSummary> {
        let filtered = self.filtered_jobs();
        self.state.selected().and_then(|i| filtered.into_iter().nth(i))
    }

    pub fn take_action(&mut self) -> Option<ScheduleAction> {
        self.pending_action.take()
    }

    pub fn show_status(&mut self, message: String, is_error: bool) {
        self.modal = ScheduleModal::StatusMessage { message, is_error };
    }

    fn filtered_jobs(&self) -> Vec<&JobSummary> {
        if self.search_query.is_empty() {
            self.jobs.iter().collect()
        } else {
            let q = self.search_query.to_lowercase();
            self.jobs
                .iter()
                .filter(|j| {
                    j.id.to_lowercase().contains(&q)
                        || j.chain_id.to_lowercase().contains(&q)
                        || j.schedule_desc.to_lowercase().contains(&q)
                })
                .collect()
        }
    }
}

// ── Tab implementation ──────────────────────────────────────────────────────

impl Tab for ScheduleTab {
    fn render(&self, frame: &mut Frame, area: Rect) {
        if self.jobs.is_empty() {
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(Line::from(vec![Span::styled(
                    " Schedule ",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                )]));
            let content = Paragraph::new(vec![
                Line::from(""),
                Line::from(""),
                Line::from(vec![Span::styled(
                    "  No scheduled jobs",
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
                        " to create a new scheduled job",
                        Style::default().fg(Color::DarkGray),
                    ),
                ]),
            ])
            .block(block);
            frame.render_widget(content, area);
            return;
        }

        match &self.view {
            ScheduleView::JobList => self.render_job_list(frame, area),
            ScheduleView::JobHistory(_) => self.render_history(frame, area),
        }

        // Modal overlay
        match &self.modal {
            ScheduleModal::None => {}
            ScheduleModal::NewJob {
                chain_id,
                spec,
                active_field,
            } => {
                draw_new_job_modal(frame, area, chain_id, spec, *active_field);
            }
            ScheduleModal::DisableConfirm {
                job_id,
                currently_enabled,
            } => {
                draw_toggle_modal(frame, area, job_id, *currently_enabled);
            }
            ScheduleModal::StatusMessage { message, is_error } => {
                draw_status_modal(frame, area, message, *is_error);
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        // Search mode intercepts
        if self.searching {
            match key.code {
                KeyCode::Esc => {
                    self.searching = false;
                    self.search_query.clear();
                }
                KeyCode::Enter => {
                    self.searching = false;
                }
                KeyCode::Backspace => {
                    self.search_query.pop();
                }
                KeyCode::Char(c) => {
                    self.search_query.push(c);
                }
                _ => {}
            }
            // Reset selection after filter change
            let count = self.filtered_jobs().len();
            if count > 0 {
                let sel = self.state.selected().unwrap_or(0).min(count - 1);
                self.state.select(Some(sel));
            } else {
                self.state.select(None);
            }
            return true;
        }

        // Modal intercepts all keys
        match &mut self.modal {
            ScheduleModal::NewJob {
                chain_id,
                spec,
                active_field,
            } => {
                match key.code {
                    KeyCode::Tab | KeyCode::Down => {
                        *active_field = (*active_field + 1) % 2;
                    }
                    KeyCode::Up => {
                        *active_field = if *active_field == 0 { 1 } else { 0 };
                    }
                    KeyCode::Enter => {
                        let cid = chain_id.clone();
                        let sp = spec.clone();
                        self.modal = ScheduleModal::None;
                        if !cid.is_empty() && !sp.is_empty() {
                            self.pending_action =
                                Some(ScheduleAction::Create { chain_id: cid, spec: sp });
                        }
                    }
                    KeyCode::Esc => {
                        self.modal = ScheduleModal::None;
                    }
                    KeyCode::Backspace => {
                        let field = if *active_field == 0 {
                            chain_id
                        } else {
                            spec
                        };
                        field.pop();
                    }
                    KeyCode::Char(c) => {
                        let field = if *active_field == 0 {
                            chain_id
                        } else {
                            spec
                        };
                        field.push(c);
                    }
                    _ => {}
                }
                return true;
            }
            ScheduleModal::DisableConfirm {
                job_id,
                currently_enabled,
            } => {
                match key.code {
                    KeyCode::Char('y') | KeyCode::Enter => {
                        let id = job_id.clone();
                        let enable = !*currently_enabled;
                        self.modal = ScheduleModal::None;
                        self.pending_action =
                            Some(ScheduleAction::ToggleEnabled { job_id: id, enable });
                    }
                    _ => {
                        self.modal = ScheduleModal::None;
                    }
                }
                return true;
            }
            ScheduleModal::StatusMessage { .. } => {
                self.modal = ScheduleModal::None;
                return true;
            }
            ScheduleModal::None => {}
        }

        match &self.view {
            ScheduleView::JobList => self.handle_key_jobs(key),
            ScheduleView::JobHistory(_) => self.handle_key_history(key),
        }
    }
}

// ── Rendering helpers ───────────────────────────────────────────────────────

impl ScheduleTab {
    fn render_job_list(&self, frame: &mut Frame, area: Rect) {
        let filtered = self.filtered_jobs();
        let avail = area.width.saturating_sub(6) as usize;
        let status_w = 3;
        let id_w = 22.min(avail / 4);
        let sched_w = 20.min(avail / 4);
        let last_w = 12;
        let runs_w = 10;
        let chain_w = avail.saturating_sub(status_w + id_w + sched_w + last_w + runs_w + 5);

        let items: Vec<ListItem> = filtered
            .iter()
            .map(|j| {
                let icon = if j.enabled { "●" } else { "◌" };
                let icon_color = if j.enabled {
                    Color::Green
                } else {
                    Color::DarkGray
                };

                let last_run = match j.last_run_at {
                    Some(ts) => format_relative_time(ts),
                    None => "never".to_string(),
                };

                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!(" {} ", icon),
                        Style::default().fg(icon_color),
                    ),
                    Span::styled(
                        format!("{:<width$} ", truncate(&j.id, id_w), width = id_w),
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{:<width$} ", truncate(&j.schedule_desc, sched_w), width = sched_w),
                        Style::default().fg(Color::Rgb(255, 175, 95)),
                    ),
                    Span::styled(
                        format!("Last: {:<width$} ", truncate(&last_run, last_w), width = last_w),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(
                        format!("Runs: {:<width$} ", j.run_count, width = runs_w),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(
                        format!("chain: {}", truncate(&j.chain_id, chain_w)),
                        Style::default().fg(Color::Rgb(100, 100, 120)),
                    ),
                ]))
            })
            .collect();

        let mut title_spans = vec![
            Span::styled(
                " Schedule ",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("({}) ", filtered.len()),
                Style::default().fg(Color::DarkGray),
            ),
        ];

        if !self.search_query.is_empty() {
            title_spans.push(Span::styled(
                format!("[/{}] ", self.search_query),
                Style::default().fg(Color::Yellow),
            ));
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(Line::from(title_spans));

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

    fn render_history(&self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(5),
        ])
        .split(area);

        let job_id = if let ScheduleView::JobHistory(ref id) = self.view {
            id.as_str()
        } else {
            ""
        };

        let breadcrumb = Line::from(vec![
            Span::styled(" <- ", Style::default().fg(Color::Cyan)),
            Span::styled("Schedule", Style::default().fg(Color::DarkGray)),
            Span::styled(" / ", Style::default().fg(Color::Rgb(60, 60, 80))),
            Span::styled(
                job_id.to_string(),
                Style::default()
                    .fg(Color::Rgb(255, 175, 95))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  ({} runs)", self.history.len()),
                Style::default().fg(Color::DarkGray),
            ),
        ]);
        frame.render_widget(Paragraph::new(breadcrumb), chunks[0]);

        if self.history.is_empty() {
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(Line::from(vec![Span::styled(
                    " History ",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                )]));
            let content = Paragraph::new(vec![
                Line::from(""),
                Line::from(vec![Span::styled(
                    "  No history yet",
                    Style::default().fg(Color::DarkGray),
                )]),
            ])
            .block(block);
            frame.render_widget(content, chunks[1]);
            return;
        }

        let avail = chunks[1].width.saturating_sub(6) as usize;
        let time_w = 16;
        let changed_w = 4;
        let output_w = avail.saturating_sub(time_w + changed_w + 2);

        let items: Vec<ListItem> = self
            .history
            .iter()
            .map(|h| {
                let time_str = format_relative_time(h.run_at);
                let changed_icon = if h.changed { "~" } else { "=" };
                let changed_color = if h.changed {
                    Color::Yellow
                } else {
                    Color::DarkGray
                };
                let output = h
                    .output
                    .as_deref()
                    .unwrap_or("(no output)")
                    .lines()
                    .next()
                    .unwrap_or("(no output)");

                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!(" {} ", changed_icon),
                        Style::default().fg(changed_color),
                    ),
                    Span::styled(
                        format!("{:<width$} ", truncate(&time_str, time_w), width = time_w),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(
                        truncate(output, output_w),
                        Style::default().fg(Color::White),
                    ),
                ]))
            })
            .collect();

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(Line::from(vec![
                Span::styled(
                    " History ",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("({}) ", self.history.len()),
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

        frame.render_stateful_widget(list, chunks[1], &mut self.history_state.clone());
    }

    fn handle_key_jobs(&mut self, key: KeyEvent) -> bool {
        let filtered_len = self.filtered_jobs().len();
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if filtered_len > 0 {
                    let i = self.state.selected().unwrap_or(0);
                    self.state.select(Some((i + 1) % filtered_len));
                }
                true
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if filtered_len > 0 {
                    let i = self.state.selected().unwrap_or(0);
                    self.state
                        .select(Some(i.checked_sub(1).unwrap_or(filtered_len - 1)));
                }
                true
            }
            KeyCode::Enter => {
                if let Some(job) = self.selected().cloned() {
                    let job_id = job.id.clone();
                    self.view = ScheduleView::JobHistory(job_id.clone());
                    self.pending_action = Some(ScheduleAction::LoadHistory { job_id });
                }
                true
            }
            KeyCode::Char('n') => {
                self.modal = ScheduleModal::NewJob {
                    chain_id: String::new(),
                    spec: String::new(),
                    active_field: 0,
                };
                true
            }
            KeyCode::Char('d') => {
                if let Some(job) = self.selected().cloned() {
                    self.modal = ScheduleModal::DisableConfirm {
                        job_id: job.id.clone(),
                        currently_enabled: job.enabled,
                    };
                }
                true
            }
            KeyCode::Char('/') => {
                self.searching = true;
                self.search_query.clear();
                true
            }
            _ => false,
        }
    }

    fn handle_key_history(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                let len = self.history.len();
                if len > 0 {
                    let i = self.history_state.selected().unwrap_or(0);
                    self.history_state.select(Some((i + 1) % len));
                }
                true
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let len = self.history.len();
                if len > 0 {
                    let i = self.history_state.selected().unwrap_or(0);
                    self.history_state
                        .select(Some(i.checked_sub(1).unwrap_or(len - 1)));
                }
                true
            }
            KeyCode::Esc | KeyCode::Backspace => {
                self.view = ScheduleView::JobList;
                self.history.clear();
                self.history_state.select(None);
                true
            }
            _ => false,
        }
    }
}

// ── Modal rendering ─────────────────────────────────────────────────────────

fn draw_new_job_modal(
    frame: &mut Frame,
    area: Rect,
    chain_id: &str,
    spec: &str,
    active_field: usize,
) {
    let h = 14.min(area.height.saturating_sub(4));
    let w = 55.min(area.width.saturating_sub(4));
    let x = (area.width.saturating_sub(w)) / 2 + area.x;
    let y = (area.height.saturating_sub(h)) / 2 + area.y;
    let modal_area = Rect::new(x, y, w, h);

    frame.render_widget(Clear, modal_area);

    let fields = ["Chain ID", "Schedule"];
    let values = [chain_id, spec];

    let mut lines = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            "  New Scheduled Job",
            Style::default()
                .fg(Color::Rgb(255, 175, 95))
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
    ];

    for (i, name) in fields.iter().enumerate() {
        let is_active = i == active_field;
        let label_style = if is_active {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::Rgb(200, 200, 210))
        };
        let val = values[i];
        lines.push(Line::from(vec![Span::styled(
            format!("  {}: ", name),
            label_style,
        )]));
        lines.push(Line::from(vec![
            Span::styled("  ", Style::default()),
            if is_active {
                Span::styled(
                    format!("{}\u{258f}", val),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(
                    if val.is_empty() {
                        "\u{2014}".to_string()
                    } else {
                        val.to_string()
                    },
                    Style::default().fg(Color::DarkGray),
                )
            },
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        "  e.g. \"every 10m\" or \"cron:0 8 * * *\"",
        Style::default().fg(Color::Rgb(80, 80, 100)),
    )]));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
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
    ]));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(255, 175, 95)))
        .style(Style::default().bg(Color::Rgb(22, 22, 30)))
        .title(Line::from(vec![Span::styled(
            " New Scheduled Job ",
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

fn draw_toggle_modal(frame: &mut Frame, area: Rect, job_id: &str, currently_enabled: bool) {
    let h = 7.min(area.height.saturating_sub(4));
    let w = 50.min(area.width.saturating_sub(4));
    let x = (area.width.saturating_sub(w)) / 2 + area.x;
    let y = (area.height.saturating_sub(h)) / 2 + area.y;
    let modal_area = Rect::new(x, y, w, h);

    frame.render_widget(Clear, modal_area);

    let action = if currently_enabled {
        "Disable"
    } else {
        "Enable"
    };
    let short_id = truncate(job_id, 20);
    let lines = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            format!("  {} job {}?", action, short_id),
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
            format!(" {} Job ", action),
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
    let icon = if is_error { "\u{2717}" } else { "\u{2713}" };

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

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else if max <= 1 {
        "\u{2026}".to_string()
    } else {
        format!("{}\u{2026}", &s[..max - 1])
    }
}

fn format_relative_time(ts_ms: i64) -> String {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let diff_secs = (now_ms - ts_ms) / 1000;
    if diff_secs < 0 {
        "in the future".to_string()
    } else if diff_secs < 60 {
        format!("{}s ago", diff_secs)
    } else if diff_secs < 3600 {
        format!("{}m ago", diff_secs / 60)
    } else if diff_secs < 86400 {
        format!("{}h ago", diff_secs / 3600)
    } else {
        format!("{}d ago", diff_secs / 86400)
    }
}
