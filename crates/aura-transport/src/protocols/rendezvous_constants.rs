//! Rendezvous protocol constants shared across transport messages.

/// Rendezvous protocol version for authentication payloads.
pub const RENDEZVOUS_PROTOCOL_VERSION: u8 = 1;

/// Payload discriminator for rendezvous messages.
pub const PROTOCOL_RENDEZVOUS: &str = "rendezvous";

/// Transport descriptor metadata keys for rendezvous negotiation.
pub const META_ALPN: &str = "alpn";
pub const META_UFRAG: &str = "ufrag";
pub const META_PWD: &str = "pwd";
pub const META_CANDIDATES: &str = "candidates";
pub const META_ONION: &str = "onion";
pub const META_SERVICE_UUID: &str = "service_uuid";
