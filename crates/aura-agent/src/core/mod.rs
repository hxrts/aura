//! Agent Core - Public API, Config, and Types
//!
//! This module contains the minimal public API surface for the agent runtime.
//! It exposes only the constructors, config types, and operation entrypoints
//! needed by consumers, keeping everything else crate-private.

pub mod api;
pub mod config;
pub mod context;
pub mod error;

pub use api::{AgentBuilder, AuraAgent};
pub use config::AgentConfig;
pub use context::AuthorityContext;
pub use error::{AgentError, AgentResult};
