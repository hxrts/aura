//! Agent Error Types
//!
//! Unified error handling for the agent runtime.

use aura_core::AuraError;

/// Agent-specific error types
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    /// Configuration error
    #[error("Agent configuration error: {0}")]
    Config(String),

    /// Runtime error
    #[error("Agent runtime error: {0}")]
    Runtime(String),

    /// Authority context error
    #[error("Authority context error: {0}")]
    Context(String),

    /// Effect system error
    #[error("Effect system error: {0}")]
    Effects(String),

    /// Choreography error
    #[error("Choreography error: {0}")]
    Choreography(String),

    /// Underlying Aura error
    #[error("Aura error: {0}")]
    Aura(#[from] AuraError),
}

/// Agent result type
pub type AgentResult<T> = std::result::Result<T, AgentError>;

impl AgentError {
    /// Create a configuration error
    pub fn config(msg: impl Into<String>) -> Self {
        Self::Config(msg.into())
    }

    /// Create a runtime error
    pub fn runtime(msg: impl Into<String>) -> Self {
        Self::Runtime(msg.into())
    }

    /// Create a context error
    pub fn context(msg: impl Into<String>) -> Self {
        Self::Context(msg.into())
    }

    /// Create an effects error
    pub fn effects(msg: impl Into<String>) -> Self {
        Self::Effects(msg.into())
    }

    /// Create a choreography error
    pub fn choreography(msg: impl Into<String>) -> Self {
        Self::Choreography(msg.into())
    }
}
