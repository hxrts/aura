#![allow(
    missing_docs,
    unused_variables,
    clippy::unwrap_used,
    clippy::expect_used,
    dead_code,
    clippy::match_like_matches_macro,
    clippy::type_complexity,
    clippy::while_let_loop,
    clippy::redundant_closure,
    clippy::large_enum_variant,
    clippy::unused_unit,
    clippy::get_first,
    clippy::single_range_in_vec_init,
    clippy::disallowed_methods,
    deprecated
)]
//! Layer 4: Bridge Adapters - Type-Safe & Dynamic Handler Integration
//!
//! Bridge adapters connecting different handler systems with type-safe or dynamic bridging.
//! Enables seamless integration between handler implementations without core interface changes.
//!
//! **Bridge Types** (per docs/001_system_architecture.md):
//! - **TypedHandlerBridge**: Type-safe bridging between specific handler interfaces
//!   - Preserves type information during handler composition
//!   - Enables compile-time verification of handler compatibility
//!   - Used for strongly-typed effect system integration (static dispatch)
//!
//! - **UnifiedAuraHandlerBridge**: Dynamic bridging for heterogeneous handler systems
//!   - Type-erased bridging for maximum flexibility (runtime dispatch)
//!   - Enables handler discovery and adaptation at runtime
//!   - Used for plugin systems and dynamic handler loading
//!
//! **Design Principle** (per docs/106_effect_system_and_runtime.md):
//! Bridge adapters follow adapter pattern to connect incompatible interfaces while
//! preserving algebraic properties of effect system. Enable seamless integration
//! between production handlers, mocks, and simulation handlers without changes to
//! core handler interfaces or effect trait definitions.

pub mod typed_bridge;
pub mod unified_bridge;
pub mod config;

pub use typed_bridge::TypedHandlerBridge;
pub use unified_bridge::{UnifiedAuraHandlerBridge, UnifiedHandlerBridgeFactory};
pub use config::BridgeRuntimeConfig;
