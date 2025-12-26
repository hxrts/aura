//! Aura bridge prelude.
//!
//! Curated re-exports for handler bridge usage.

#[allow(deprecated)]
pub use crate::bridges::{TypedHandlerBridge, UnifiedAuraHandlerBridge, UnifiedHandlerBridgeFactory};

/// Composite effect requirements for bridge usage.
pub trait BridgeEffects: Send + Sync {}

impl<T> BridgeEffects for T where T: Send + Sync {}
