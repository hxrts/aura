//! Agent-Specific Handler Implementations
//!
//! This module contains handler implementations for agent-specific effects.
//! These handlers compose core system effects (from aura-protocol) into 
//! higher-level device workflows and capabilities.
//!
//! Each handler focuses on a specific area of device functionality:
//! 
//! - `auth`: Authentication and biometric operations

pub mod auth;

// Re-export all handler implementations
pub use auth::AuthenticationHandler;