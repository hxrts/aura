//! Prelude for common imports
//!
//! This module re-exports the most commonly used types and traits for convenient importing.

// Core effect traits
pub use crate::effects::{
    NetworkEffects, StorageEffects, CryptoEffects, TimeEffects, ConsoleEffects,
    LedgerEffects, RandomEffects, ChoreographicEffects, JournalEffects,
    Effects, ProtocolEffects, MinimalEffects,
};

// Common error types
pub use crate::effects::{
    NetworkError, StorageError, LedgerError, 
    ChoreographyError,
};

// Utility types
pub use crate::effects::{
    WakeCondition, ChoreographicRole, ChoreographyEvent, ChoreographyMetrics,
    ConsoleEvent, NetworkAddress, StorageLocation,
};

// Handler types
pub use crate::handlers::{
    CompositeHandler, AuraHandlerFactory, AuraHandler,
    ExecutionMode, EffectType,
};

// Middleware types
pub use crate::middleware::{
    MiddlewareStack, LoggingMiddleware, MetricsMiddleware,
    AuraMiddleware,
};

// Context types
pub use crate::handlers::{
    AuraContext, MiddlewareContext,
};


// External dependencies commonly used with this crate
pub use aura_types::{DeviceId, AuraError, AuraResult};
pub use uuid::Uuid;
pub use async_trait::async_trait;

// Common standard library types
pub use std::collections::HashMap;
pub use std::sync::Arc;
pub use std::time::Duration;