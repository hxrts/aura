//! Coordination errors - using unified error system

// Re-export unified error system
pub use aura_errors::{AuraError, ErrorCode, ErrorSeverity, Result};

// Type alias for backward compatibility during transition
pub type CoordinationError = AuraError;
