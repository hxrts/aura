//! Shared semantic scenario contract for harness, simulator, and verification flows.
//!
//! This contract describes scenario actions and expectations without embedding
//! renderer-specific details such as PTY key sequences or DOM selectors.

#![allow(missing_docs)] // Shared semantic contract - expanded incrementally during migration.

pub mod actions;
pub mod expectations;
pub mod submission;
pub mod values;

pub use actions::*;
pub use expectations::*;
pub use submission::*;
pub use values::*;
