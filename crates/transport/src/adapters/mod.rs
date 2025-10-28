//! Transport Protocol Adapters
//!
//! This module contains concrete implementations that adapt specific transport
//! protocols to the transport abstraction layer.

pub mod https_relay;
pub mod memory;
pub mod noise_tcp;
pub mod simple_tcp;

// Re-export protocol adapters
pub use https_relay::*;
pub use memory::*;
pub use noise_tcp::*;
pub use simple_tcp::*;
