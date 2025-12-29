//! Flood Module - Rendezvous packet flooding
//!
//! This module implements the flooding layer of Aura's progressive disclosure
//! model. Rendezvous packets are flooded through the social topology to reach
//! potential recipients without revealing sender or recipient identity.
//!
//! # Design
//!
//! **Opaque flooding**: Packets are encrypted to the recipient's public key.
//! Relay nodes see only fixed-size opaque blobs and cannot determine content,
//! sender, or intended recipient.
//!
//! **TTL-based propagation**: Packets have a time-to-live that's decremented
//! at each hop. This limits the flood scope while ensuring coverage.
//!
//! **Deduplication**: Each packet has a unique nonce. Nodes track seen nonces
//! to prevent amplification from retransmitted packets.
//!
//! **Social topology routing**: Packets propagate along social relationships
//! (home peers, then neighborhood peers) for natural flood boundaries.
//!
//! # Example
//!
//! ```ignore
//! use aura_rendezvous::flood::{FloodPropagation, PacketBuilder};
//! use aura_social::SocialTopology;
//!
//! let flood = FloodPropagation::new(local_authority, keypair);
//!
//! // Create and encrypt a packet
//! let packet = PacketBuilder::new()
//!     .with_payload(rendezvous_data)
//!     .encrypt_to(&recipient_public_key)?;
//!
//! // Flood through topology
//! let targets = flood.flood_targets(&topology);
//! ```

mod packet;
mod propagation;

pub use packet::{DecryptedPayload, PacketBuilder, PacketCrypto};
pub use propagation::{FloodPropagation, SeenNonces};
