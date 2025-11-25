//! System trait stubs
//!
//! Placeholder for system-level traits that define interfaces
//! for runtime components in the authority-centric architecture.

/// Stub system traits
#[allow(dead_code)] // Part of future system component API
pub trait SystemComponent {
    fn initialize(&mut self) -> Result<(), SystemError>;
    fn shutdown(&mut self) -> Result<(), SystemError>;
}

#[derive(Debug, thiserror::Error)]
#[allow(dead_code)] // Part of future system error handling
pub enum SystemError {
    #[error("Initialization failed: {reason}")]
    InitializationFailed { reason: String },
    #[error("Shutdown failed: {reason}")]
    ShutdownFailed { reason: String },
}
