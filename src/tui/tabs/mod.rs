//! Tab definitions and shared Tab trait.

use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::Frame;

/// Identifies each tab in the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabId {
    Chat,
    Agents,
    Workflows,
    Resources,
    Pea,
    Schedule,
    Settings,
    History,
}

impl TabId {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Chat => "Chat",
            Self::Agents => "Agents",
            Self::Workflows => "Workflows",
            Self::Resources => "Resources",
            Self::Pea => "PEA",
            Self::Schedule => "Schedule",
            Self::Settings => "Settings",
            Self::History => "History",
        }
    }

    pub fn all() -> &'static [TabId] {
        &[
            TabId::Chat,
            TabId::Agents,
            TabId::Workflows,
            TabId::Resources,
            TabId::Pea,
            TabId::Schedule,
            TabId::Settings,
            TabId::History,
        ]
    }

    pub fn index(&self) -> usize {
        match self {
            Self::Chat => 0,
            Self::Agents => 1,
            Self::Workflows => 2,
            Self::Resources => 3,
            Self::Pea => 4,
            Self::Schedule => 5,
            Self::Settings => 6,
            Self::History => 7,
        }
    }

    pub fn from_index(i: usize) -> Self {
        match i {
            0 => Self::Chat,
            1 => Self::Agents,
            2 => Self::Workflows,
            3 => Self::Resources,
            4 => Self::Pea,
            5 => Self::Schedule,
            6 => Self::Settings,
            7 => Self::History,
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

    /// Status-bar hints for each tab.
    pub fn hints(&self) -> &'static str {
        match self {
            Self::Chat => "[Enter] send  [PgUp/Dn] scroll  [Ctrl+L] clear",
            Self::Agents => "[Enter] detail  [i] install  [s] start/stop  [/] search",
            Self::Workflows => "[Enter] detail  [n] new  [c] cancel",
            Self::Resources => "[Enter] detail  [r] register  [d] delete",
            Self::Pea => "[Enter] detail  [n] new objective  [p] pause  [x] cancel",
            Self::Schedule => "[Enter] history  [n] new  [d] disable/enable  [/] search",
            Self::Settings => "[Enter] edit  [r] reload",
            Self::History => "[Enter] detail  [/] search",
        }
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
pub mod resources;
pub mod schedule;
pub mod settings;
pub mod tasks;
pub mod workflows;
