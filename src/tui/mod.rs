//! Terminal UI module — formatting toolkit and interactive dashboard.

pub mod fmt;

#[cfg(feature = "tui")]
pub mod app;

#[cfg(feature = "tui")]
pub mod log_layer;

#[cfg(feature = "tui")]
pub mod tabs;

#[cfg(feature = "tui")]
pub mod wizard;
