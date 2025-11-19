//! Web-of-Trust Integration for Protocol Layer
//!
//! **Layer 4 (aura-protocol)**: Effect-dependent orchestration of capability evaluation.
//!
//! This module provides effect-dependent wrappers around aura-wot's pure capability
//! evaluation logic. It adds caching, metrics, and effect system integration.

pub mod capability_evaluator;

pub use capability_evaluator::{
    CacheStats, CapabilityEvaluator, EffectSystemInterface, EffectiveCapabilitySet,
};
