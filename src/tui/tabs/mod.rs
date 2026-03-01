//! Tab definitions and shared Tab trait.

use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::Frame;

/// Identifies each tab in the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabId {
    Chat,
    Tasks,
    Agents,
    Settings,
    History,
}

impl TabId {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Chat => "Chat",
            Self::Tasks => "Tasks",
            Self::Agents => "Agents",
            Self::Settings => "Settings",
            Self::History => "History",
        }
    }

    pub fn all() -> &'static [TabId] {
        &[
            TabId::Chat,
            TabId::Tasks,
            TabId::Agents,
            TabId::Settings,
            TabId::History,
        ]
    }

    pub fn index(&self) -> usize {
        match self {
            Self::Chat => 0,
            Self::Tasks => 1,
            Self::Agents => 2,
            Self::Settings => 3,
            Self::History => 4,
        }
    }

    pub fn from_index(i: usize) -> Self {
        match i {
            0 => Self::Chat,
            1 => Self::Tasks,
            2 => Self::Agents,
            3 => Self::Settings,
            4 => Self::History,
            _ => Self::Chat,
        }
    }

    pub fn next(&self) -> Self {
        Self::from_index((self.index() + 1) % Self::all().len())
    }

    pub fn prev(&self) -> Self {
        let len = Self::all().len();
        Self::from_index((self.index() + len - 1) % len)
    }
}

/// Shared trait for all tabs.
pub trait Tab {
    fn render(&self, frame: &mut Frame, area: Rect);
    fn handle_key(&mut self, key: KeyEvent) -> bool;
}

pub mod agents;
pub mod chat;
pub mod history;
pub mod settings;
pub mod tasks;
