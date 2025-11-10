//! Common message infrastructure
//!
//! This module contains shared message infrastructure:
//! - Generic message envelopes
//! - Error types specific to message handling

pub mod envelope;
pub mod error;

// Re-export common types
pub use envelope::*;
pub use error::*;
