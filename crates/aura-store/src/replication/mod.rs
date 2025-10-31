//! Replication Domain
//!
//! This domain manages data durability through replication:
//! - **Replica Placement**: Unified strategy abstraction for choosing replica locations
//! - **Static Replication**: Replication to predetermined peers
//! - **Social Replication**: Trust-based peer selection via SSB relationships
//! - **Verification**: Proof-of-storage challenges to verify replicas
//!
//! # Module Organization
//!
//! Components are organized with prefixes to indicate their subsystem:
//! - `placement.rs` - Replica placement strategy abstraction
//! - `static_replication.rs` - Static replication to predetermined peers
//! - `social_*.rs` - Trust-based peer selection via SSB relationships
//!   - `social_storage.rs` - SSB integration
//!   - `social_peer_selection.rs` - Trust-based peer choice
//!   - `social_trust_scoring.rs` - Reputation computation
//! - `verification_*.rs` - Proof-of-storage challenges
//!   - `verification_challenge.rs` - Challenge construction
//!   - `verification_proof.rs` - Proof verification

pub mod placement;
pub mod social_peer_selection;
pub mod social_storage;
pub mod social_trust_scoring;
pub mod static_replication;
pub mod verification_challenge;
pub mod verification_proof;

// Re-exports for convenient access
pub use placement::{
    PlacementConfig, PlacementResult, PlacementStatus, PlacementStrategy, ReplicaPlacementEngine,
};
pub use social_peer_selection::PeerSelector;
pub use social_storage::{StorageCapabilityAnnouncement, StorageOperation, TrustLevel};
pub use social_trust_scoring::TrustScore;
pub use static_replication::{ReplicaPlacement, StaticReplicationConfig, StaticReplicator};
pub use verification_challenge::{Challenge, ProofResponse, ReplicaTag};
pub use verification_proof::{ProofOfStorageVerifier, VerificationResult};
