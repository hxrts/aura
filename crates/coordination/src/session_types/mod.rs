//! Session type infrastructure for coordination protocols
//!
//! This module contains shared session type infrastructure and utilities
//! used across distributed coordination protocols.

pub mod agent;
pub mod context;
pub mod context_safe;
pub mod frost;
pub mod frost_safe;
pub mod local_transitions;
pub mod session_errors;
pub mod wrapper;

// Re-export shared session type infrastructure
pub use agent::*;
pub use context::*;
pub use context_safe::*;
pub use frost::*;
pub use frost_safe::*;
pub use local_transitions::*;
pub use session_errors::{AgentSessionError, ContextSessionError, FrostSessionError};
pub use wrapper::{SessionProtocol, SessionTypedProtocol};

// Re-export aura-types session infrastructure
pub use aura_types::session_core::{witnesses, SessionState};
