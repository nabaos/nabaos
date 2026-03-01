//! History tab — past queries with tier, cost, and latency.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use super::Tab;

/// A historical query entry.
#[derive(Clone)]
pub struct HistoryEntry {
    pub timestamp: String,
    pub query: String,
    pub tier: String,
    pub cost: f64,
    pub latency_ms: f64,
}

pub struct HistoryTab {
    pub entries: Vec<HistoryEntry>,
    pub state: ListState,
}

impl HistoryTab {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            state: ListState::default(),
        }
    }

    pub fn push(&mut self, entry: HistoryEntry) {
        self.entries.insert(0, entry); // newest first
        if self.state.selected().is_none() && !self.entries.is_empty() {
            self.state.select(Some(0));
        }
    }
}

impl Tab for HistoryTab {
    fn render(&self, frame: &mut Frame, area: Rect) {
        if self.entries.is_empty() {
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
                Line::from(""),
                Line::from(vec![Span::styled(
                    "  No queries yet",
                    Style::default().fg(Color::DarkGray),
                )]),
                Line::from(""),
                Line::from(vec![Span::styled(
                    "  Send a query in the Chat tab to see history here",
                    Style::default().fg(Color::DarkGray),
                )]),
            ])
            .block(block);
            frame.render_widget(content, area);
            return;
        }

        let avail = area.width.saturating_sub(4) as usize;

        let items: Vec<ListItem> = self
            .entries
            .iter()
            .map(|e| {
                let tier_color = tier_to_color(&e.tier);
                let tier_label = tier_to_label(&e.tier);
                let query_w = avail.saturating_sub(35);
                let query = if e.query.len() > query_w && query_w > 1 {
                    format!("{}…", &e.query[..query_w - 1])
                } else {
                    e.query.clone()
                };

                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("  {} ", e.timestamp),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(
                        format!("{:<width$} ", query, width = query_w),
                        Style::default().fg(Color::White),
                    ),
                    Span::styled(
                        format!("{:<8}", tier_label),
                        Style::default().fg(tier_color),
                    ),
                    Span::styled(
                        format!("{:>8} ", format_cost(e.cost)),
                        Style::default().fg(if e.cost == 0.0 {
                            Color::Green
                        } else {
                            Color::Yellow
                        }),
                    ),
                    Span::styled(format_latency(e.latency_ms), Style::default().fg(Color::DarkGray)),
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
                    format!("({}) ", self.entries.len()),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));

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
            KeyCode::Down | KeyCode::Char('j') => {
                let len = self.entries.len();
                if len > 0 {
                    let i = self.state.selected().unwrap_or(0);
                    self.state.select(Some((i + 1) % len));
                }
                true
            }
            KeyCode::Up | KeyCode::Char('k') => {
                let len = self.entries.len();
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

fn tier_to_color(tier: &str) -> Color {
    if tier.contains("Fingerprint") {
        Color::Green
    } else if tier.contains("Cache") || tier.contains("Bert") {
        Color::Cyan
    } else if tier.contains("Cheap") {
        Color::Yellow
    } else if tier.contains("Deep") {
        Color::Magenta
    } else if tier.contains("Blocked") {
        Color::Red
    } else {
        Color::DarkGray
    }
}

fn tier_to_label(tier: &str) -> &str {
    if tier.contains("Fingerprint") {
        "cache"
    } else if tier.contains("Cache") || tier.contains("Bert") {
        "cache"
    } else if tier.contains("Cheap") {
        "llm"
    } else if tier.contains("Deep") {
        "agent"
    } else if tier.contains("Blocked") {
        "blocked"
    } else {
        "unknown"
    }
}

fn format_cost(usd: f64) -> String {
    if usd == 0.0 {
        "$0.00".to_string()
    } else if usd < 0.01 {
        format!("${:.4}", usd)
    } else {
        format!("${:.2}", usd)
    }
}

fn format_latency(ms: f64) -> String {
    if ms < 1.0 {
        format!("{:.1}ms", ms)
    } else if ms < 1000.0 {
        format!("{:.0}ms", ms)
    } else {
        format!("{:.1}s", ms / 1000.0)
    }
}
