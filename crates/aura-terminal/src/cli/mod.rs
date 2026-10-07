//! Layer 7: CLI Argument Parsing - User-Facing Interface
//!
//! **Responsibility**: bpaf argument parsers only, no implementation logic.
//!
//! Account commands parse straight into a typed [`crate::command::Request`];
//! the shared command model (`crate::command`) executes it through the
//! `aura_app::ui::workflows` the TUI and web use, and renders the typed
//! response. Offline tools (`init`, `threshold`, `replay`) and long-running
//! modes (`tui`, `sync daemon`) have their own argument types.

pub mod chat;
pub mod commands;
#[cfg(feature = "development")]
pub mod demo;
pub mod init;

pub mod requests;
pub mod sync;
#[cfg(feature = "terminal")]
pub mod tui;

#[cfg(feature = "development")]
pub use demo::DemoCommands;
pub use sync::SyncDaemonArgs;
#[cfg(feature = "terminal")]
pub use tui::TuiArgs;
