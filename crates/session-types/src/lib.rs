//! Session Types - Unified Type System for Aura
#![allow(clippy::result_large_err)]
//!
//! This crate provides all shared types across the Aura platform, including:
//! - Session type infrastructure for type-safe distributed protocols
//! - Property types for formal verification and monitoring
//! - Trace and event types for simulation and analysis
//! - Message types for WebSocket communication
//! - Error types for unified error handling
//!
//! # Architecture
//!
//! ## Session Types
//! - **Core Traits**: `SessionProtocol`, `SessionState`, `WitnessedTransition`
//! - **Macros**: Boilerplate reduction for session type definitions
//! - **Witnesses**: Runtime evidence for distributed conditions
//! - **Rehydration**: Protocol state reconstruction from evidence
//!
//! ## Shared Types
//! - **Properties**: Formal verification properties and monitoring
//! - **Traces**: Execution traces and state snapshots
//! - **Events**: Real-time system events and notifications
//! - **Messages**: WebSocket communication protocols
//!
//! Protocol-specific definitions belong in their respective crates:
//! - Transport protocols → `aura-transport::session_types`
//! - CLI protocols → `aura-cli::session_types`
//! - Coordination protocols → `aura-coordination` (existing structure)

pub mod errors;
pub mod events;
pub mod messages;
pub mod properties;
pub mod session;
pub mod trace;

// Re-export all modules for convenience
pub use errors::*;
pub use events::*;
pub use messages::*;
pub use properties::*;
pub use session::*;
pub use trace::*;
