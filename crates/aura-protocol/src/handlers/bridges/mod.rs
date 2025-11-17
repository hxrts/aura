//! Bridge Adapters for Handler Integration
//!
//! This module contains bridge adapters that connect different handler systems
//! and provide integration points between various effect handling approaches.
//!
//! ## Bridge Types
//!
//! - **Typed Bridge**: Type-safe bridging between specific handler interfaces
//!   - Preserves type information during handler composition
//!   - Enables compile-time verification of handler compatibility
//!   - Used for strongly-typed effect system integration
//!
//! - **Unified Bridge**: Dynamic bridging for heterogeneous handler systems
//!   - Type-erased bridging for maximum flexibility
//!   - Runtime handler discovery and adaptation
//!   - Used for plugin systems and dynamic handler loading
//!
//! ## Design Principles
//!
//! Bridge adapters follow the adapter pattern to connect incompatible interfaces
//! while preserving the algebraic properties of the effect system. They enable
//! seamless integration between different handler implementations without
//! requiring changes to the core handler interfaces.

pub mod typed_bridge;
pub mod unified_bridge;

pub use typed_bridge::TypedHandlerBridge;
pub use unified_bridge::{UnifiedAuraHandlerBridge, UnifiedHandlerBridgeFactory};
