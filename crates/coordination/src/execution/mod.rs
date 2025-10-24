//! Protocol execution infrastructure
//!
//! This module provides the infrastructure for executing choreographic protocols:
//! - `ProtocolContext` - Execution environment with local projection
//! - `TimeSource` - Time abstraction for simulation vs production
//! - Core types for protocol instructions and results
//! - Helper abstractions for reducing choreography boilerplate

pub mod context;
pub mod helpers;
pub mod time;
pub mod types;

pub use context::ProtocolContext;
pub use helpers::*;
pub use time::*;
pub use types::*;
