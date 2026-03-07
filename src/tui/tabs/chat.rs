//! Chat tab — query input + response history with visual message styling.
//!
//! Features:
//! - Basic markdown rendering (**bold**, `inline code`, ```code blocks```)
//! - Persistent history loading via push_*_silent()
//! - Ctrl+L to clear visible chat
//! - Ctrl+A / Ctrl+E for cursor movement
//! - Live elapsed timer during loading

use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;

use super::Tab;

const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// A single message in the chat history.
#[derive(Clone)]
pub struct ChatMessage {
    pub role: String,       // "user", "agent", or "system"
    pub text: String,
    pub cost_label: String, // e.g. "cached · $0.00" or "llm · $0.003"
    pub timestamp: String,
}

pub struct ChatTab {
    pub messages: Vec<ChatMessage>,
    pub input: String,
    pub cursor_pos: usize,
    pub scroll_offset: usize,
    pub is_loading: bool,
    pub spinner_tick: usize,
    pub loading_start: Option<Instant>,
    input_history: Vec<String>,
    history_index: Option<usize>,
}

impl ChatTab {
    pub fn new() -> Self {
        Self {
            messages: vec![ChatMessage {
                role: "system".to_string(),
                text: "Welcome to NabaOS. Type a query and press Enter.".to_string(),
                cost_label: String::new(),
                timestamp: String::new(),
            }],
            input: String::new(),
            cursor_pos: 0,
            scroll_offset: 0,
            is_loading: false,
            spinner_tick: 0,
            loading_start: None,
            input_history: Vec::new(),
            history_index: None,
        }
    }

    /// Add a message from the user.
    pub fn push_user(&mut self, text: String) {
        self.input_history.push(text.clone());
        self.history_index = None;
        self.messages.push(ChatMessage {
            role: "user".to_string(),
            text,
            cost_label: String::new(),
            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
        });
        self.scroll_to_bottom();
    }

    /// Add a response from the agent.
    pub fn push_agent(&mut self, text: String, cost_label: String) {
        self.messages.push(ChatMessage {
            role: "agent".to_string(),
            text,
            cost_label,
            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
        });
        self.is_loading = false;
        self.loading_start = None;
        self.scroll_to_bottom();
    }

    /// Add a historical user message (no scroll, no input_history push).
    pub fn push_user_silent(&mut self, text: String, timestamp_ms: i64) {
        let ts = chrono::DateTime::from_timestamp_millis(timestamp_ms)
            .map(|dt| dt.with_timezone(&chrono::Local).format("%H:%M:%S").to_string())
            .unwrap_or_default();
        self.messages.push(ChatMessage {
            role: "user".to_string(),
            text,
            cost_label: String::new(),
            timestamp: ts,
        });
    }

    /// Add a historical agent message (no scroll, no loading state change).
    pub fn push_agent_silent(&mut self, text: String, timestamp_ms: i64) {
        let ts = chrono::DateTime::from_timestamp_millis(timestamp_ms)
            .map(|dt| dt.with_timezone(&chrono::Local).format("%H:%M:%S").to_string())
            .unwrap_or_default();
        self.messages.push(ChatMessage {
            role: "agent".to_string(),
            text,
            cost_label: String::new(),
            timestamp: ts,
        });
    }

    /// Returns the current input text (for the app to process).
    pub fn take_input(&mut self) -> String {
        self.cursor_pos = 0;
        std::mem::take(&mut self.input)
    }

    /// Advance spinner animation.
    pub fn tick(&mut self) {
        if self.is_loading {
            self.spinner_tick = self.spinner_tick.wrapping_add(1);
        }
    }

    /// Set loading state with timer.
    pub fn set_loading(&mut self, loading: bool) {
        self.is_loading = loading;
        if loading {
            self.loading_start = Some(Instant::now());
        } else {
            self.loading_start = None;
        }
    }

    /// Return the text of the most recent user message, if any.
    pub fn last_user_message(&self) -> Option<String> {
        self.messages
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .map(|m| m.text.clone())
    }

    /// Clear visible chat messages.
    pub fn clear(&mut self) {
        self.messages.clear();
        self.messages.push(ChatMessage {
            role: "system".to_string(),
            text: "Chat cleared. Type a query and press Enter.".to_string(),
            cost_label: String::new(),
            timestamp: String::new(),
        });
        self.scroll_offset = 0;
    }

    fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    fn build_items(&self, width: usize) -> Vec<ListItem<'static>> {
        let wrap_w = width.saturating_sub(6);
        let mut items = Vec::new();

        for msg in &self.messages {
            match msg.role.as_str() {
                "system" => {
                    items.push(ListItem::new(Line::from(vec![
                        Span::styled(
                            format!("  {}", msg.text),
                            Style::default()
                                .fg(Color::DarkGray)
                                .add_modifier(Modifier::ITALIC),
                        ),
                    ])));
                    items.push(ListItem::new(Line::from("")));
                }
                "user" => {
                    // Header: ▍ You · timestamp
                    items.push(ListItem::new(Line::from(vec![
                        Span::styled("  ▍ ", Style::default().fg(Color::Cyan)),
                        Span::styled(
                            "You",
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            format!("  {}", msg.timestamp),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ])));
                    // Body with cyan border
                    for line in wrap_text(&msg.text, wrap_w) {
                        items.push(ListItem::new(Line::from(vec![
                            Span::styled("  ▍ ", Style::default().fg(Color::Cyan)),
                            Span::styled(line, Style::default().fg(Color::White)),
                        ])));
                    }
                    items.push(ListItem::new(Line::from("")));
                }
                "agent" => {
                    // Header: NabaOS · timestamp · cost badge
                    let mut header = vec![
                        Span::raw("    "),
                        Span::styled(
                            "NabaOS",
                            Style::default()
                                .fg(Color::Magenta)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ];
                    if !msg.timestamp.is_empty() {
                        header.push(Span::styled(
                            format!("  {}", msg.timestamp),
                            Style::default().fg(Color::DarkGray),
                        ));
                    }
                    if !msg.cost_label.is_empty() {
                        let color =
                            if msg.cost_label.contains("cached") || msg.cost_label.contains("$0.00")
                            {
                                Color::Green
                            } else if msg.cost_label.contains("error") {
                                Color::Red
                            } else {
                                Color::Yellow
                            };
                        header.push(Span::styled(
                            format!("  {}", msg.cost_label),
                            Style::default().fg(color),
                        ));
                    }
                    items.push(ListItem::new(Line::from(header)));
                    // Body with markdown rendering
                    render_markdown_lines(&msg.text, wrap_w, &mut items);
                    items.push(ListItem::new(Line::from("")));
                }
                _ => {}
            }
        }

        // Loading spinner with elapsed timer
        if self.is_loading {
            let frame = self.spinner_tick % SPINNER.len();
            let elapsed = self
                .loading_start
                .map(|s| s.elapsed().as_secs_f64())
                .unwrap_or(0.0);
            items.push(ListItem::new(Line::from(vec![
                Span::raw("    "),
                Span::styled(
                    format!("{} ", SPINNER[frame]),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    format!("Thinking... ({:.1}s)", elapsed),
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                ),
            ])));
        }

        items
    }
}

impl Tab for ChatTab {
    fn render(&self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::vertical([
            Constraint::Min(5),    // messages
            Constraint::Length(3), // input
        ])
        .split(area);

        // Messages
        let width = chunks[0].width.saturating_sub(2) as usize;
        let items = self.build_items(width);
        let total = items.len();
        let visible_height = chunks[0].height.saturating_sub(2) as usize;

        // Auto-scroll to bottom, with manual scroll offset
        let skip = if self.scroll_offset == 0 {
            total.saturating_sub(visible_height)
        } else {
            total
                .saturating_sub(visible_height)
                .saturating_sub(self.scroll_offset)
        };

        let visible_items: Vec<ListItem> = items.into_iter().skip(skip).collect();

        let msg_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(Line::from(vec![
                Span::styled(
                    " Messages ",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("({}) ", self.messages.len()),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
        let list = List::new(visible_items).block(msg_block);
        frame.render_widget(list, chunks[0]);

        // Input
        let input_style = if self.is_loading {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::Cyan)
        };
        let input_block = Block::default()
            .borders(Borders::ALL)
            .border_style(input_style)
            .title(Line::from(vec![Span::styled(
                " ❯ ",
                input_style.add_modifier(Modifier::BOLD),
            )]));
        let input_para = Paragraph::new(self.input.as_str())
            .block(input_block)
            .style(Style::default().fg(Color::White))
            .wrap(Wrap { trim: false });
        frame.render_widget(input_para, chunks[1]);

        // Cursor position
        if !self.is_loading {
            let cursor_x = chunks[1].x + 1 + self.cursor_pos as u16;
            let cursor_y = chunks[1].y + 1;
            frame.set_cursor_position((cursor_x.min(chunks[1].right().saturating_sub(2)), cursor_y));
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        if self.is_loading {
            return false;
        }
        match key.code {
            // Ctrl+L: clear chat
            KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.clear();
                true
            }
            // Ctrl+A: cursor to start
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.cursor_pos = 0;
                true
            }
            // Ctrl+E: cursor to end
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.cursor_pos = self.input.len();
                true
            }
            KeyCode::Home => {
                self.cursor_pos = 0;
                true
            }
            KeyCode::End => {
                self.cursor_pos = self.input.len();
                true
            }
            KeyCode::Left => {
                self.cursor_pos = self.cursor_pos.saturating_sub(1);
                true
            }
            KeyCode::Right => {
                if self.cursor_pos < self.input.len() {
                    self.cursor_pos += 1;
                }
                true
            }
            KeyCode::Char(c) => {
                self.input.insert(self.cursor_pos, c);
                self.cursor_pos += 1;
                self.history_index = None;
                true
            }
            KeyCode::Backspace => {
                if self.cursor_pos > 0 {
                    self.input.remove(self.cursor_pos - 1);
                    self.cursor_pos -= 1;
                }
                true
            }
            KeyCode::Delete => {
                if self.cursor_pos < self.input.len() {
                    self.input.remove(self.cursor_pos);
                }
                true
            }
            KeyCode::Up => {
                // Input history navigation
                if !self.input_history.is_empty() {
                    let idx = match self.history_index {
                        Some(i) => i.saturating_sub(1),
                        None => self.input_history.len() - 1,
                    };
                    self.history_index = Some(idx);
                    self.input = self.input_history[idx].clone();
                    self.cursor_pos = self.input.len();
                }
                true
            }
            KeyCode::Down => {
                if let Some(idx) = self.history_index {
                    if idx + 1 < self.input_history.len() {
                        self.history_index = Some(idx + 1);
                        self.input = self.input_history[idx + 1].clone();
                        self.cursor_pos = self.input.len();
                    } else {
                        self.history_index = None;
                        self.input.clear();
                        self.cursor_pos = 0;
                    }
                }
                true
            }
            KeyCode::PageUp => {
                self.scroll_offset = self.scroll_offset.saturating_add(5);
                true
            }
            KeyCode::PageDown => {
                self.scroll_offset = self.scroll_offset.saturating_sub(5);
                true
            }
            _ => false,
        }
    }
}

// ── Markdown rendering helpers ──────────────────────────────────────────────

/// Render agent response text with basic markdown into ListItems.
/// All spans are owned (String) to avoid lifetime issues with wrapped text.
fn render_markdown_lines(text: &str, wrap_w: usize, items: &mut Vec<ListItem<'static>>) {
    let mut in_code_block = false;

    for paragraph in text.split('\n') {
        // Code block toggle
        if paragraph.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
            items.push(ListItem::new(Line::from(vec![
                Span::raw("    "),
                Span::styled(
                    "─".repeat(wrap_w.min(40)),
                    Style::default().fg(Color::DarkGray),
                ),
            ])));
            continue;
        }

        if in_code_block {
            // Inside code block — render with distinct styling
            let code_line = if paragraph.len() > wrap_w {
                paragraph[..wrap_w].to_string()
            } else {
                paragraph.to_string()
            };
            items.push(ListItem::new(Line::from(vec![
                Span::raw("    "),
                Span::styled(
                    format!(" {} ", code_line),
                    Style::default()
                        .fg(Color::Rgb(180, 220, 180))
                        .bg(Color::Rgb(30, 30, 40)),
                ),
            ])));
        } else {
            // Normal text — wrap and apply inline markdown
            for line in wrap_text(paragraph, wrap_w) {
                let spans = parse_inline_markdown(&line);
                let mut full_line: Vec<Span<'static>> = vec![Span::raw("    ")];
                full_line.extend(spans);
                items.push(ListItem::new(Line::from(full_line)));
            }
        }
    }
}

/// Parse inline markdown: **bold** and `inline code`. Returns owned spans.
fn parse_inline_markdown(text: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut pos = 0;
    let bytes = text.as_bytes();
    let len = bytes.len();

    while pos < len {
        // Find the next markdown marker
        let bold_pos = text[pos..].find("**").map(|p| pos + p);
        let code_pos = text[pos..].find('`').map(|p| pos + p);

        let next = match (bold_pos, code_pos) {
            (Some(b), Some(c)) => {
                if b <= c {
                    Some(("**", b))
                } else {
                    Some(("`", c))
                }
            }
            (Some(b), None) => Some(("**", b)),
            (None, Some(c)) => Some(("`", c)),
            (None, None) => None,
        };

        match next {
            None => {
                if pos < len {
                    spans.push(Span::styled(
                        text[pos..].to_string(),
                        Style::default().fg(Color::White),
                    ));
                }
                break;
            }
            Some(("**", marker_pos)) => {
                if marker_pos > pos {
                    spans.push(Span::styled(
                        text[pos..marker_pos].to_string(),
                        Style::default().fg(Color::White),
                    ));
                }
                let after_start = marker_pos + 2;
                if let Some(end) = text[after_start..].find("**") {
                    spans.push(Span::styled(
                        text[after_start..after_start + end].to_string(),
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ));
                    pos = after_start + end + 2;
                } else {
                    spans.push(Span::styled(
                        text[marker_pos..].to_string(),
                        Style::default().fg(Color::White),
                    ));
                    break;
                }
            }
            Some(("`", marker_pos)) => {
                if marker_pos > pos {
                    spans.push(Span::styled(
                        text[pos..marker_pos].to_string(),
                        Style::default().fg(Color::White),
                    ));
                }
                let after_start = marker_pos + 1;
                if let Some(end) = text[after_start..].find('`') {
                    spans.push(Span::styled(
                        format!(" {} ", &text[after_start..after_start + end]),
                        Style::default()
                            .fg(Color::Rgb(180, 220, 180))
                            .bg(Color::Rgb(30, 30, 40)),
                    ));
                    pos = after_start + end + 1;
                } else {
                    spans.push(Span::styled(
                        text[marker_pos..].to_string(),
                        Style::default().fg(Color::White),
                    ));
                    break;
                }
            }
            _ => unreachable!(),
        }
    }

    if spans.is_empty() {
        spans.push(Span::styled(String::new(), Style::default().fg(Color::White)));
    }

    spans
}

/// Word-wrap text to fit within max_width columns.
pub fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    if text.is_empty() {
        lines.push(String::new());
        return lines;
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        lines.push(String::new());
        return lines;
    }
    let mut current_line = String::new();
    for word in words {
        if current_line.is_empty() {
            current_line = word.to_string();
        } else if current_line.len() + 1 + word.len() <= max_width {
            current_line.push(' ');
            current_line.push_str(word);
        } else {
            lines.push(current_line);
            current_line = word.to_string();
        }
    }
    if !current_line.is_empty() {
        lines.push(current_line);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_last_user_message_empty() {
        let chat = ChatTab::new();
        assert!(chat.last_user_message().is_none());
    }

    #[test]
    fn test_last_user_message_returns_most_recent() {
        let mut chat = ChatTab::new();
        chat.push_user("first query".into());
        chat.push_agent("response 1".into(), "$0.01".into());
        chat.push_user("second query".into());
        chat.push_agent("response 2".into(), "$0.02".into());
        assert_eq!(chat.last_user_message().unwrap(), "second query");
    }
}
