//! Layer 7: CLI Argument Parsing - User-Facing Interface
//!
//! **Responsibility**: This module defines Clap command-line argument structures only.
//! It does NOT contain implementation logic.
//!
//! **Separation of Concerns**:
//! - `cli/` - Argument parsing (Clap definitions) - THIS MODULE
//! - `handlers/` - Implementation logic (effect calls, business logic)
//!
//! User-facing argument groups by domain: **amp** (scenarios), **authority** (inspection),
//! **context** (relational context management).
//!
//! **Integration Flow** (per docs/001_system_architecture.md):
//! CLI Args → Handlers → Effects → Facts → Views → UI
//!
//! Commands drive aura-agent (Layer 6) effect system via CLI handlers (aura-terminal/handlers).
//! Messages flow through guards (aura-protocol/guards) for authorization and flow control.

pub mod amp;
pub mod authority;
pub mod bpaf_commands;
pub mod bpaf_init;
pub mod bpaf_node;
pub mod bpaf_status;
pub mod chat;
pub mod context;
#[cfg(feature = "development")]
pub mod demo;
pub mod sync;
#[cfg(feature = "terminal")]
pub mod tui;

pub use amp::AmpAction;
pub use authority::AuthorityCommands;
pub use chat::ChatCommands;
pub use context::ContextAction;
#[cfg(feature = "development")]
pub use demo::DemoCommands;
pub use sync::SyncAction;
#[cfg(feature = "terminal")]
pub use tui::TuiArgs;
