//! Chat tab — query input + response history with visual message styling.

use crossterm::event::{KeyCode, KeyEvent};
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
    pub scroll_offset: usize,
    pub is_loading: bool,
    pub spinner_tick: usize,
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
            scroll_offset: 0,
            is_loading: false,
            spinner_tick: 0,
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
        self.scroll_to_bottom();
    }

    /// Returns the current input text (for the app to process).
    pub fn take_input(&mut self) -> String {
        std::mem::take(&mut self.input)
    }

    /// Advance spinner animation.
    pub fn tick(&mut self) {
        if self.is_loading {
            self.spinner_tick = self.spinner_tick.wrapping_add(1);
        }
    }

    fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    fn build_items(&self, width: usize) -> Vec<ListItem<'_>> {
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
                    // Body
                    for line in wrap_text(&msg.text, wrap_w) {
                        items.push(ListItem::new(Line::from(vec![
                            Span::raw("    "),
                            Span::styled(line, Style::default().fg(Color::White)),
                        ])));
                    }
                    items.push(ListItem::new(Line::from("")));
                }
                _ => {}
            }
        }

        // Loading spinner
        if self.is_loading {
            let frame = self.spinner_tick % SPINNER.len();
            items.push(ListItem::new(Line::from(vec![
                Span::raw("    "),
                Span::styled(
                    format!("{} ", SPINNER[frame]),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    "Thinking...",
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
            let cursor_x = chunks[1].x + 1 + self.input.len() as u16;
            let cursor_y = chunks[1].y + 1;
            frame.set_cursor_position((cursor_x.min(chunks[1].right().saturating_sub(2)), cursor_y));
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        if self.is_loading {
            return false;
        }
        match key.code {
            KeyCode::Char(c) => {
                self.input.push(c);
                self.history_index = None;
                true
            }
            KeyCode::Backspace => {
                self.input.pop();
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
                }
                true
            }
            KeyCode::Down => {
                if let Some(idx) = self.history_index {
                    if idx + 1 < self.input_history.len() {
                        self.history_index = Some(idx + 1);
                        self.input = self.input_history[idx + 1].clone();
                    } else {
                        self.history_index = None;
                        self.input.clear();
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

/// Word-wrap text to fit within max_width columns.
fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            lines.push(String::new());
            continue;
        }
        let words: Vec<&str> = paragraph.split_whitespace().collect();
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
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}
