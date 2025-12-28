//! LAN discovery packet formats and configuration (pure types).
//!
//! The UDP socket implementation is provided via `UdpEffects` (Layer 3) and wired by
//! the runtime (Layer 6) so Layer 5 remains runtime/OS-agnostic and simulatable.
//! This module intentionally contains **no** `tokio` or `std::net` usage.

use crate::facts::{RendezvousDescriptor, TransportHint};
use aura_core::identifiers::AuthorityId;
use serde::{Deserialize, Serialize};

// =============================================================================
// CONSTANTS
// =============================================================================

/// Default UDP port for LAN discovery.
pub const DEFAULT_LAN_PORT: u16 = 19433;

/// Default broadcast interval in milliseconds.
pub const DEFAULT_ANNOUNCE_INTERVAL_MS: u64 = 5000;

/// Maximum packet size for UDP broadcast.
pub const MAX_PACKET_SIZE: usize = 1400;

/// Protocol magic bytes to identify Aura LAN discovery packets.
pub const MAGIC_BYTES: &[u8; 4] = b"AURA";

/// Protocol version.
pub const PROTOCOL_VERSION: u8 = 1;

// =============================================================================
// CONFIGURATION
// =============================================================================

/// Configuration for LAN discovery.
#[derive(Debug, Clone)]
pub struct LanDiscoveryConfig {
    /// UDP port for discovery.
    pub port: u16,
    /// Interval between announcements in milliseconds.
    pub announce_interval_ms: u64,
    /// Whether LAN discovery is enabled.
    pub enabled: bool,
    /// Bind address (e.g. "0.0.0.0").
    pub bind_addr: String,
    /// Broadcast address (e.g. "255.255.255.255").
    pub broadcast_addr: String,
}

impl Default for LanDiscoveryConfig {
    fn default() -> Self {
        Self {
            port: DEFAULT_LAN_PORT,
            announce_interval_ms: DEFAULT_ANNOUNCE_INTERVAL_MS,
            enabled: true,
            bind_addr: "0.0.0.0".to_string(),
            broadcast_addr: "255.255.255.255".to_string(),
        }
    }
}

// =============================================================================
// PACKET TYPES
// =============================================================================

/// LAN discovery packet sent via UDP broadcast.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanDiscoveryPacket {
    /// Protocol version.
    pub version: u8,
    /// Authority announcing presence.
    pub authority_id: AuthorityId,
    /// Transport descriptor for connecting.
    pub descriptor: RendezvousDescriptor,
    /// Timestamp (ms since epoch).
    pub timestamp_ms: u64,
}

impl LanDiscoveryPacket {
    /// Create a new discovery packet.
    pub fn new(
        authority_id: AuthorityId,
        descriptor: RendezvousDescriptor,
        timestamp_ms: u64,
    ) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            authority_id,
            descriptor,
            timestamp_ms,
        }
    }

    /// Serialize packet with magic header.
    pub fn to_bytes(&self) -> Option<Vec<u8>> {
        let json = serde_json::to_vec(self).ok()?;
        let mut bytes = Vec::with_capacity(MAGIC_BYTES.len() + json.len());
        bytes.extend_from_slice(MAGIC_BYTES);
        bytes.extend(json);
        Some(bytes)
    }

    /// Deserialize packet, validating magic header and version.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < MAGIC_BYTES.len() {
            return None;
        }
        if &bytes[..MAGIC_BYTES.len()] != MAGIC_BYTES {
            return None;
        }
        let json = &bytes[MAGIC_BYTES.len()..];
        let packet: LanDiscoveryPacket = serde_json::from_slice(json).ok()?;
        if packet.version != PROTOCOL_VERSION {
            return None;
        }
        Some(packet)
    }
}

/// Peer discovered via LAN broadcast.
#[derive(Debug, Clone)]
pub struct DiscoveredPeer {
    /// Authority ID discovered.
    pub authority_id: AuthorityId,
    /// Transport descriptor received from the peer.
    pub descriptor: RendezvousDescriptor,
    /// Source address string of the UDP packet (best-effort).
    pub source_addr: String,
    /// Timestamp when discovered (ms since epoch).
    pub discovered_at_ms: u64,
}

impl DiscoveredPeer {
    /// Create a new discovered peer record.
    pub fn new(
        authority_id: AuthorityId,
        descriptor: RendezvousDescriptor,
        source_addr: String,
        discovered_at_ms: u64,
    ) -> Self {
        Self {
            authority_id,
            descriptor,
            source_addr,
            discovered_at_ms,
        }
    }

    /// Get transport hints from the descriptor.
    pub fn transport_hints(&self) -> &[TransportHint] {
        &self.descriptor.transport_hints
    }
}
