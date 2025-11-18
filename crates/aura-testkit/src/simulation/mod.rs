//! Multi-device & protocol simulation testing
//!
//! This module provides infrastructure for testing distributed protocols that require
//! coordination across multiple simulated devices. It handles choreographic protocols,
//! network simulation, and inter-device communication.

pub mod choreography;
pub mod network;
pub mod transport;

pub use choreography::*;
pub use network::*;
pub use transport::*;
