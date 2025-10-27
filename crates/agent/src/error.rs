//! Agent errors - using unified error system
//!
//! This module completely replaces the old agent error handling with
//! the unified Aura error system, providing agent-specific constructors.

// Re-export unified error system
pub use aura_errors::{AuraError, ErrorCode, ErrorSeverity, Result};

// Type aliases for backward compatibility
pub type AgentError = AuraError;
pub type ProtocolError = AuraError;
pub type CryptoError = AuraError; 
pub type DataError = AuraError;
pub type InfrastructureError = AuraError;
pub type CapabilityError = AuraError;
pub type SystemError = AuraError;

// Agent-specific error constructors are provided by the aura-errors crate
// The AgentError type alias points to AuraError which has all the constructors