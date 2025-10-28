//! SBB (Social Backup and Broadcasting) Protocol Suite
//!
//! This module contains the implementation of the SBB protocol family,
//! including gossip, publisher, and recognizer components.

pub mod gossip;
pub mod publisher;
pub mod recognizer;

// Re-export SBB protocol components
pub use gossip::*;
pub use publisher::*;
pub use recognizer::*;
