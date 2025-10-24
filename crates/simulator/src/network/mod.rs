//! Network simulation components
//!
//! This module provides network fabric simulation including
//! latency, partitions, and message delivery control.

pub mod fabric;
pub mod transport;

pub use fabric::*;
pub use transport::*;
