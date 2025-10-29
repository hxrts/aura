//! Coordination errors - using unified error system

// Re-export unified error system
pub use aura_types::{AuraError, AuraResult as Result, ErrorCode, ErrorSeverity};

// Type alias for backward compatibility during transition
pub type CoordinationError = AuraError;
