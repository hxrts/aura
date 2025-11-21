//! CLI command definitions grouped by domain.

pub mod amp;
pub mod authority;
pub mod context;

pub use amp::AmpAction;
pub use authority::AuthorityCommands;
pub use context::ContextAction;
