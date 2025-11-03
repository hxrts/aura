//! Prelude for common imports
//!
//! This module re-exports the most commonly used types and traits for convenient importing.

// Core effect traits
pub use crate::effects::{
    NetworkEffects, StorageEffects, CryptoEffects, TimeEffects, ConsoleEffects,
    ProtocolEffects, MinimalEffects,
};

// Common error types
pub use crate::effects::{
    NetworkError, StorageError, CryptoError, TimeError,
};

// Utility types
pub use crate::effects::{
    WakeCondition, TimeoutHandle, PeerEvent, StorageStats,
    ChoreographicRole, ChoreographyEvent, ChoreographyError,
};

// Handler types
pub use crate::handlers::{
    CompositeHandler, HandlerBuilder,
};

// Middleware types
pub use crate::middleware::{
    MiddlewareStack, MiddlewareConfig, Middleware,
    TracingMiddleware, MetricsMiddleware,
    RetryMiddleware, RetryConfig,
    create_standard_stack,
};

// Runtime types
pub use crate::runtime::{
    ExecutionContext, ContextBuilder,
    SessionManager, SessionState, SessionStatus, ExecutionMode,
    EffectExecutor,
};

// External dependencies commonly used with this crate
pub use aura_types::{DeviceId, AuraError, AuraResult};
pub use uuid::Uuid;
pub use async_trait::async_trait;

// Common standard library types
pub use std::collections::HashMap;
pub use std::sync::Arc;
pub use std::time::Duration;