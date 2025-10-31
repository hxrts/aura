//! Transport Protocol Adapters
//!
//! This module contains concrete implementations that adapt specific transport
//! protocols to the transport abstraction layer.
//!
//! ## Available Adapters
//!
//! - `memory` - In-memory transport for testing and development
//! - `https_relay` - HTTPS relay transport for NAT traversal
//! - `noise_tcp` - Direct P2P transport with Noise protocol encryption
//! - `simple_tcp` - Unencrypted TCP transport for testing only

pub mod https_relay;
/// Memory-based transport adapter for testing
pub mod memory;
pub mod noise_tcp;
pub mod simple_tcp;

// Re-export protocol adapters
pub use https_relay::*;
pub use memory::*;
pub use noise_tcp::*;
pub use simple_tcp::*;
