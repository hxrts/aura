//! Layer 4: Core Handler Infrastructure - Type Erasure for Protocol Integration
//!
//! Provides type-erased handler traits for dynamic dispatch and integration with
//! the protocol layer's guard chain and multi-party coordination systems.
//!
//! **Remaining Components**:
//! - **Erased**: Type-erased handler traits for dynamic dispatch; enables runtime polymorphism
//!
//! **Moved to aura-composition (Layer 3)**:
//! - **Composite**: Multi-handler composition with delegation patterns
//! - **Factory**: Handler construction with configuration and builder patterns
//! - **Registry**: Handler registration and lookup mechanisms
//!
//! **Design Pattern** (per docs/106_effect_system_and_runtime.md):
//! - Handlers implement effect traits (Layer 1 interfaces)
//! - aura-composition provides registry and composition (Layer 3)
//! - This module provides protocol integration (Layer 4)
//! - Type-erasing enables plugin systems and dynamic handler loading
//!
//! **Integration**: Works with guard chain (aura-protocol/guards) to enforce authorization,
//! flow budgets, and privacy at message entry points

pub mod erased;

pub use erased::{AuraHandler, BoxedHandler, HandlerUtils};
