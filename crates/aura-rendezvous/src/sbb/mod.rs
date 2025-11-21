//! Social Bulletin Board (SBB) Implementation
//!
//! This module implements controlled flooding through the social graph for peer discovery.
//! SBB enables envelope propagation through friend/guardian relationships with TTL limits,
//! duplicate detection, and capability enforcement.

mod envelope;
mod flooding;
mod protocol;

pub use envelope::{EnvelopeId, RendezvousEnvelope, SbbEnvelope, SBB_MESSAGE_SIZE};
pub use flooding::{FloodResult, SbbFlooding, SbbFloodingCoordinator};
pub use protocol::current_timestamp;
