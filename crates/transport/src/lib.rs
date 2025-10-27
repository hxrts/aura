//! Pluggable transport layer with presence ticket enforcement
//!
//! This crate provides a modular transport system for Aura, organized into several
//! logical layers:
//!
//! - **Core**: Fundamental transport abstractions and implementations
//! - **Protocols**: Specific transport protocol implementations (HTTPS, SBB)
//! - **Infrastructure**: Supporting components (envelopes, presence, peer discovery)
//! - **Session Types**: Session type definitions for compile-time safety
//! - **Testing**: Test utilities and stub implementations

#![allow(warnings, clippy::all)]

// ========== Core Types (kept at root for compatibility) ==========
pub mod error;
pub mod types;

// ========== Module Organization ==========
pub mod core;
pub mod infrastructure;
pub mod protocols;
pub mod session_types;
pub mod testing;

// ========== Re-exports for compatibility ==========

// Core transport functionality
pub use core::*;

// Protocol implementations
pub use protocols::*;

// Infrastructure components
pub use infrastructure::*;

// Session types
pub use session_types::*;

// Testing utilities
pub use testing::*;

// Root-level types
pub use error::*;
pub use types::*;
