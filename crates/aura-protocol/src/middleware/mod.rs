//! Middleware System for Aura Protocol Handlers
//!
//! This module provides a composable middleware architecture inspired by rumpsteak's
//! ChoreoHandler middleware pattern. It allows cross-cutting concerns like observability,
//! error recovery, authorization, and effects to be layered on top of core protocol handlers.
//!
//! ## Unified Observability
//!
//! The observability middleware consolidates tracing, instrumentation, trace recording,
//! metrics, and dev console functionality into a single, efficient middleware that
//! replaces the previous separate implementations.

pub mod capability;
pub mod effects;
pub mod error_recovery;
pub mod event_watcher;
pub mod handler;
pub mod observability;
pub mod session;
pub mod stack;

#[cfg(feature = "test-utils")]
pub mod fault_injection;

// Re-export core types
pub use handler::{AuraProtocolHandler, ProtocolError, ProtocolResult};

// Re-export middleware implementations
pub use capability::CapabilityMiddleware;
pub use effects::{EffectsMiddleware, WithEffects};
pub use error_recovery::ErrorRecoveryMiddleware;
pub use event_watcher::EventWatcherMiddleware;
pub use observability::ObservabilityMiddleware;
pub use session::SessionMiddleware;
pub use stack::{MiddlewareExt, MiddlewareStackBuilder, StandardMiddlewareStack};

#[cfg(feature = "test-utils")]
pub use fault_injection::FaultInjectionMiddleware;
