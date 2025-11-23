//! Common protocol constants
//!
//! Constants used across the Aura protocol for version negotiation,
//! wire format compatibility, and other shared values.

/// Current wire format version for Aura protocol messages
///
/// This version is used in all message envelopes to ensure compatibility
/// between different protocol implementations and versions.
///
/// Increment this when making wire format changes that require
/// coordination across the distributed system.
pub const WIRE_FORMAT_VERSION: u16 = 1;