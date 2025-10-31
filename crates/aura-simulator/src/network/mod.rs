//! Network simulation components
//!
//! This module provides network fabric simulation including
//! latency, partitions, and message delivery control.

pub mod fabric;
// pub mod transport;  // Temporarily disabled due to aura-transport compilation issues

pub use fabric::*;
// pub use transport::*;
