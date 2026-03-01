//! Tab trait and registry for the TUI dashboard.

pub mod agents;
pub mod chat;
pub mod history;
pub mod settings;
pub mod tasks;

use ratatui::Frame;
use ratatui::layout::Rect;

/// Identifies a tab in the TUI.
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
            TabId::Chat => "Chat",
            TabId::Tasks => "Tasks",
            TabId::Agents => "Agents",
            TabId::Settings => "Settings",
            TabId::History => "History",
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

    pub fn next(&self) -> TabId {
        match self {
            TabId::Chat => TabId::Tasks,
            TabId::Tasks => TabId::Agents,
            TabId::Agents => TabId::Settings,
            TabId::Settings => TabId::History,
            TabId::History => TabId::Chat,
        }
    }
}

/// Trait that each tab implements.
pub trait Tab {
    /// Render the tab content into the given area.
    fn render(&self, frame: &mut Frame, area: Rect);

    /// Handle a key event. Returns true if the event was consumed.
    fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> bool;
}
