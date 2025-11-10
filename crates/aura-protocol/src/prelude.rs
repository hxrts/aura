//! Prelude for common imports
//!
//! This module re-exports the most commonly used types and traits for convenient importing.

// Core effect traits
pub use crate::effects::{
    AuraEffects, ChoreographicEffects, ConsoleEffects, CryptoEffects, JournalEffects, LedgerEffects,
    NetworkEffects, RandomEffects, StorageEffects, TimeEffects, TreeEffects,
};

// Common error types
pub use crate::effects::{ChoreographyError, LedgerError, NetworkError, StorageError};

// Utility types
pub use crate::effects::{
    ChoreographicRole, ChoreographyEvent, ChoreographyMetrics, ConsoleEvent, NetworkAddress,
    StorageLocation, WakeCondition,
};

// Handler types
pub use crate::handlers::{
    AuraHandler, AuraHandlerFactory, CompositeHandler, EffectType, ExecutionMode,
};

// Middleware types
pub use crate::middleware::{MiddlewareContext, MiddlewareError, MiddlewareResult};

// Context types
pub use crate::handlers::AuraContext;

// External dependencies commonly used with this crate
pub use async_trait::async_trait;
pub use aura_core::{AuraError, AuraResult, DeviceId};
pub use uuid::Uuid;

// Common standard library types
pub use std::collections::HashMap;
pub use std::sync::Arc;
pub use std::time::Duration;
