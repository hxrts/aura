//! Web-of-Trust Integration for Protocol Layer
//!
//! **Layer 4 (aura-protocol)**: Effect-dependent orchestration of capability evaluation.
//!
//! This module provides effect-dependent wrappers around aura-wot's pure capability
//! evaluation logic. It adds caching, metrics, and effect system integration.

// pub mod capability_evaluator; // Disabled - needs Capability type rewrite

// Temporary placeholder trait to avoid breaking imports
pub trait EffectSystemInterface {
    /// Get the device ID for this effect system
    fn device_id(&self) -> aura_core::DeviceId;

    /// Query metadata from the effect system
    fn get_metadata(&self, key: &str) -> Option<String>;
}

// pub use capability_evaluator::{
//     CacheStats, CapabilityEvaluator, EffectSystemInterface, EffectiveCapabilitySet,
// };
