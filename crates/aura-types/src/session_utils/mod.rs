//! Session Utilities - Supporting Types for Aura
#![allow(clippy::result_large_err)]
//!
//! This module provides utility types across the Aura platform, including:
//! - Property types for formal verification and monitoring
//! - Trace and event types for simulation and analysis
//! - Message types for WebSocket communication
//! - Error types for unified error handling
//!
//! Core session type infrastructure is in the `session_core` module.
//!
//! # Architecture
//!
//! ## Shared Types
//! - **Properties**: Formal verification properties and monitoring
//! - **Traces**: Execution traces and state snapshots
//! - **Events**: Real-time system events and notifications
//! - **Messages**: WebSocket communication protocols
//! - **Errors**: Unified error types for session operations
//!
//! Protocol-specific definitions belong in their respective crates:
//! - Transport protocols → `aura-transport::session_types`
//! - CLI protocols → `aura-cli::session_types`
//! - Coordination protocols → `aura-coordination` (existing structure)

pub mod errors;
pub mod events;
pub mod messages;
pub mod properties;
pub mod trace;

// Re-export all modules for convenience
pub use errors::*;
pub use events::*;
pub use messages::*;
pub use properties::*;
pub use trace::*;
