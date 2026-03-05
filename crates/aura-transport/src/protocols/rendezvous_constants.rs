//! Rendezvous protocol constants shared across transport messages.

/// Rendezvous protocol version for authentication payloads.
pub const RENDEZVOUS_PROTOCOL_VERSION: u8 = 1;

/// Payload discriminator for rendezvous messages.
pub const PROTOCOL_RENDEZVOUS: &str = "rendezvous";
