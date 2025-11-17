//! Backwards-compatibility shim for immutable context paths.
//!
//! Older modules referenced `crate::handlers::context_immutable::*`. The
//! canonical implementation now lives under `handlers::context::immutable`.
//! This module simply re-exports those types to avoid touching every caller.

pub use super::context::immutable::{
    AgentContext, AuraContext, AuthenticationState, ChoreographicContext, FaultInjectionSettings,
    MetricsContext, PlatformInfo, PropertyCheckingConfig, SessionMetadata, SimulationContext,
    TracingContext,
};
