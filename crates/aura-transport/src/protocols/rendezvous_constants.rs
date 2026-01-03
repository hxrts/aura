//! Rendezvous protocol constants shared across transport messages.

/// Rendezvous protocol version for authentication payloads.
pub const RENDEZVOUS_PROTOCOL_VERSION: u8 = 1;

/// Payload discriminator for rendezvous messages.
pub const PROTOCOL_RENDEZVOUS: &str = "rendezvous";

/// Transport descriptor metadata keys for rendezvous negotiation.
#[allow(dead_code)]
pub const META_ALPN: &str = "alpn";
#[allow(dead_code)]
pub const META_UFRAG: &str = "ufrag";
#[allow(dead_code)]
pub const META_PWD: &str = "pwd";
#[allow(dead_code)]
pub const META_CANDIDATES: &str = "candidates";
#[allow(dead_code)]
pub const META_ONION: &str = "onion";
#[allow(dead_code)]
pub const META_SERVICE_UUID: &str = "service_uuid";
