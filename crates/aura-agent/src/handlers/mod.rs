//! Agent Handlers - Domain-Specific Effect Handlers
//!
//! This module contains domain-specific handlers that implement multi-party
//! protocols and workflows using shared utilities.

pub mod auth;
pub mod invitation;
pub mod ota;
pub mod recovery;
pub mod sessions;
pub mod shared;
pub mod storage;

pub use shared::{HandlerContext, HandlerUtilities};