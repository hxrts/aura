#![deny(clippy::dbg_macro)]
#![deny(clippy::todo)]
//! # Aura Bridge - Layer 4: Handler/Effector Bridges

pub mod bridges;
pub mod prelude;

#[allow(deprecated)]
pub use bridges::{TypedHandlerBridge, UnifiedAuraHandlerBridge, UnifiedHandlerBridgeFactory};
