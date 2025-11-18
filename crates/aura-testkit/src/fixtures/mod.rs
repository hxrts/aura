//! Composed test scenarios & fixtures
//!
//! This module provides reusable, semantic test fixtures that combine the lower-level
//! builders into complete test scenarios. These fixtures eliminate boilerplate and provide
//! convenient setup for common testing patterns.

pub mod biscuit;
pub mod cleanup;
pub mod common;
pub mod protocol;

pub use biscuit::*;
pub use cleanup::*;
pub use common::*;
pub use protocol::*;
