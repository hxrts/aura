//! Guardian Authorization Handlers
//!
//! This module provides guardian-specific authorization functionality, including
//! threshold validation, recovery context verification, and guardian relationship checks.

pub mod authorization;

pub use authorization::GuardianAuthorizationHandler;
