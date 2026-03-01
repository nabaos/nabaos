//! Chat tab — query input + response history.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;

use super::Tab;

/// A single message in the chat history.
#[derive(Clone)]
pub struct ChatMessage {
    pub role: String,       // "user" or "agent"
    pub text: String,
    pub cost_label: String, // e.g. "[cached · $0.00]" or "[llm · $0.003]"
    pub timestamp: String,
}

pub struct ChatTab {
    pub messages: Vec<ChatMessage>,
    pub input: String,
    pub scroll: u16,
}

impl ChatTab {
    pub fn new() -> Self {
        Self {
            messages: vec![ChatMessage {
                role: "agent".to_string(),
                text: "Welcome to NabaOS. Type a query below and press Enter.".to_string(),
                cost_label: String::new(),
                timestamp: String::new(),
            }],
            input: String::new(),
            scroll: 0,
        }
    }

    /// Add a message from the user.
    pub fn push_user(&mut self, text: String) {
        self.messages.push(ChatMessage {
            role: "user".to_string(),
            text,
            cost_label: String::new(),
            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
        });
    }

    /// Add a response from the agent.
    pub fn push_agent(&mut self, text: String, cost_label: String) {
        self.messages.push(ChatMessage {
            role: "agent".to_string(),
            text,
            cost_label,
            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
        });
    }

    /// Returns the current input text (for the app to process).
    pub fn take_input(&mut self) -> String {
        std::mem::take(&mut self.input)
    }
}

impl Tab for ChatTab {
    fn render(&self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::vertical([
            Constraint::Min(5),    // message history
            Constraint::Length(3), // input
        ])
        .split(area);

        // Message history
        let items: Vec<ListItem> = self
            .messages
            .iter()
            .map(|msg| {
                let (prefix, style) = if msg.role == "user" {
                    ("> ", Style::default().fg(Color::Cyan))
                } else {
                    ("  ", Style::default().fg(Color::White))
                };
                let mut spans = vec![Span::styled(prefix, style), Span::styled(&msg.text, style)];
                if !msg.cost_label.is_empty() {
                    spans.push(Span::styled(
                        format!("  {}", msg.cost_label),
                        Style::default().fg(Color::DarkGray),
                    ));
                }
                ListItem::new(Line::from(spans))
            })
            .collect();

        let messages_block = Block::default()
            .borders(Borders::ALL)
            .title(" Messages ");
        let list = List::new(items).block(messages_block);
        frame.render_widget(list, chunks[0]);

        // Input area
        let input_block = Block::default().borders(Borders::ALL).title(" > ");
        let input_para = Paragraph::new(self.input.as_str())
            .block(input_block)
            .wrap(Wrap { trim: false });
        frame.render_widget(input_para, chunks[1]);

        // Set cursor position
        let cursor_x = chunks[1].x + 1 + self.input.len() as u16;
        let cursor_y = chunks[1].y + 1;
        frame.set_cursor_position((cursor_x.min(chunks[1].right() - 2), cursor_y));
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char(c) => {
                self.input.push(c);
                true
            }
            KeyCode::Backspace => {
                self.input.pop();
                true
            }
            KeyCode::Up => {
                self.scroll = self.scroll.saturating_add(1);
                true
            }
            KeyCode::Down => {
                self.scroll = self.scroll.saturating_sub(1);
                true
            }
            _ => false,
        }
    }
}
