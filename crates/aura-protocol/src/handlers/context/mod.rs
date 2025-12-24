//! Context Management for Handler Operations
//!
//! This module provides the unified context infrastructure for handler operations,
//! supporting pure functional operations with immutable, thread-safe context.
//!
//! The `AuraContext` type flows through all handler operations without mutation,
//! ensuring thread-safe access. All modifications return new instances rather
//! than mutating in place.

pub mod immutable;

// Re-export context types for use throughout the handler system
pub use immutable::{
    AgentContext, AuraContext, AuthenticationState, ChoreographicContext, FaultInjectionSettings,
    MetricsContext, PlatformInfo, PropertyCheckingConfig, SessionMetadata, SimulationContext,
    TracingContext,
};
