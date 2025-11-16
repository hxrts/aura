//! Context Management for Handler Operations
//!
//! This module provides context management infrastructure for handler operations,
//! supporting both mutable and immutable context patterns throughout the effect system.
//!
//! ## Context Types
//!
//! - **Mutable Context**: Dynamic context that can be updated during handler operations
//!   - Session management and state tracking
//!   - Runtime configuration updates
//!   - Dynamic capability resolution
//!
//! - **Immutable Context**: Static context for pure operations
//!   - Fixed platform information
//!   - Static authentication state
//!   - Read-only configuration data
//!
//! ## Usage Patterns
//!
//! The context system supports both stateful coordination (mutable) and pure
//! functional operations (immutable), allowing handlers to choose the appropriate
//! context type for their execution model.

pub mod context;
pub mod immutable;

// Re-export from context.rs (was the original context.rs file)
pub use context::{
    AgentContext, AuraContext, ChoreographicContext, PlatformInfo,
    SimulationContext, TracingContext, MetricsContext,
};

// Re-export immutable context types with namespace prefix to avoid conflicts
pub mod immutable_types {
    pub use super::immutable::{
        AgentContext, AuraContext, AuthenticationState, ChoreographicContext,
        FaultInjectionSettings, MetricsContext, PlatformInfo,
        PropertyCheckingConfig, SessionMetadata, SimulationContext, TracingContext,
    };
}