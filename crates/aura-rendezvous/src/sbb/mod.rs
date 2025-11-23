//! Layer 5: Social Bulletin Board (SBB) - Metadata-Private Flooding
//!
//! Metadata-private message flooding through social graph relationships.
//! Enables peer discovery and rendezvous without exposing routing information.
//!
//! **Design** (per docs/110_rendezvous.md):
//! - **Controlled flooding**: TTL limits prevent network storms and bandwidth exhaustion
//! - **Duplicate detection**: Bloom filters eliminate redundant forwards (lossy but efficient)
//! - **Capability-scoped**: Only authorized peers (per capability tokens) receive envelopes
//! - **Privacy-preserving**: SBB hides routing via encryption (recipient metadata secret)
//! - **Relationship-scoped**: Floods only through social edges (guardian, peer relationships)
//!
//! **Propagation Model**:
//! Messages flood through relationship edges with decreasing TTL. Peers forward to authorized
//! relays determined by capability delegation (Biscuit tokens from aura-wot).

mod envelope;
mod flooding;
mod protocol;

pub use envelope::{EnvelopeId, RendezvousEnvelope, SbbEnvelope, SBB_MESSAGE_SIZE};
pub use flooding::{FloodResult, SbbFlooding, SbbFloodingCoordinator};
pub use protocol::current_timestamp;
