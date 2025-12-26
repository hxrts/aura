//! Stateful effect handlers hosted in aura-app (Layer 6).
//!
//! These handlers own runtime state (query caches, signal graphs) and therefore
//! live above the stateless Layer 3 effects implementations.

pub mod query;
pub mod reactive;
pub mod unified_handler;
