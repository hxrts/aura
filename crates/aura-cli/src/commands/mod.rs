//! Layer 7: CLI Command Definitions - User-Facing Interface
//!
//! User-facing commands grouped by domain: **amp** (scenarios), **authority** (inspection),
//! **context** (relational context management).
//!
//! **Integration** (per docs/001_system_architecture.md):
//! Commands drive aura-agent (Layer 6) effect system via CLI handlers (aura-cli/handlers).
//! Messages flow through guards (aura-protocol/guards) for authorization and flow control.

pub mod amp;
pub mod authority;
pub mod chat;
pub mod context;
pub mod demo;

pub use amp::AmpAction;
pub use authority::AuthorityCommands;
pub use chat::ChatCommands;
pub use context::ContextAction;
pub use demo::DemoCommands;
