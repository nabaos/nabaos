#![cfg_attr(not(test), deny(unsafe_code))]
#![allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::large_enum_variant,
    clippy::collapsible_if,
    clippy::collapsible_match,
    clippy::manual_strip,
    clippy::regex_creation_in_loops,
    clippy::erasing_op,
    clippy::new_without_default,
    clippy::incompatible_msrv,
    clippy::manual_clamp,
    clippy::should_implement_trait,
    clippy::if_same_then_else,
    clippy::approx_constant,
    clippy::field_reassign_with_default,
    clippy::unnecessary_get_then_check
)]

pub mod agent_os;
pub mod browser;
pub mod cache;
pub mod chain;
pub mod channels;
pub mod collaboration;
pub mod config_migration;
pub mod core;
pub mod deep_agent;
pub mod export;
pub mod knowledge;
pub mod llm_router;
pub mod mcp;
pub mod media;
pub mod memory;
pub mod meta_agent;
pub mod modules;
pub mod pea;
pub mod persona;
pub mod providers;
pub mod resource;
pub mod runtime;
pub mod security;
pub mod swarm;
pub mod tui;
pub mod viz;
pub mod w5h2;

#[cfg(feature = "watcher")]
pub mod watcher;
