//! Aura CLI Library
//!
//! This module provides the core functionality for the Aura CLI,
//! including command-line interface components and session types.
//!
//! NOTE: Temporarily simplified - session types disabled

/// Command handlers for CLI operations
pub mod commands;

/// Configuration management for the CLI
pub mod config;
// Temporarily disabled - has compilation errors
// pub mod session_types;

// Re-export common types for convenience
// Temporarily disabled - session types module disabled
// pub use session_types::*;
