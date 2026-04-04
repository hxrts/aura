//! # RuntimeBridge: Abstract Runtime Operations
//!
//! This module defines the `RuntimeBridge` trait, which abstracts runtime operations
//! that require system resources (networking, storage, cryptography). This enables
//! `aura-app` to remain a pure application core without direct dependencies on
//! runtime infrastructure.

#![allow(missing_docs)]

pub mod bridge_trait;
#[allow(dead_code)]
mod legacy;
pub mod offline;
pub mod types;

pub use bridge_trait::*;
pub use offline::*;
pub use types::*;
