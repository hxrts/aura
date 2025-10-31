//! Manifest Domain
//!
//! This domain manages object metadata and capability definitions that serve as
//! the source of truth for all storage operations:
//!
//! - **Object Manifest**: Complete metadata structure for stored objects
//! - **Access Control**: Capability and permission definitions for access control
//! - **Key Derivation**: Specifications for deriving encryption keys via DKD
//! - **Replication Hints**: Static peer replication preferences
//! - **Threshold Signatures**: Multi-signature validation for authenticity
//!
//! # Manifest as Contract
//!
//! Each `ObjectManifest` acts as a cryptographic contract that specifies:
//! - **What**: The object's content hash (Cid) and size
//! - **How**: Chunking parameters, encryption specifications, erasure coding params
//! - **Who**: Access control matrix with capabilities and permissions
//! - **Where**: Preferred replica locations via static replication hints
//!
//! Manifests are **threshold-signed** by the account threshold key, ensuring:
//! - No single device can unilaterally change object metadata
//! - Changes are auditable through the journal ledger
//! - Replicas can verify manifest authenticity independently
//!
//! # Integration Points
//!
//! - **Content Domain**: Chunking/encryption parameters guide content processing
//! - **Access Control Domain**: Capabilities in manifest define who can access
//! - **Replication Domain**: Hints inform replica placement strategy
//! - **Journal Domain**: All manifest updates are ledger events (threshold-signed)
//!
//! # Future Enhancements
//!
//! - Manifest builder pattern for construction and validation
//! - Version management for manifest evolution
//! - Capability delegation chains in manifest
//! - Storage quota enforcement via manifest metadata

pub mod object_manifest;

pub use object_manifest::{
    AccessControl, AccountId, CapabilityId, ChunkingParams, Cid, DeviceId, ErasureMeta,
    KeyDerivationSpec, ObjectManifest, PeerId, Permission, ReplicaFallbackPolicy, ResourceScope,
    SignatureShare, StaticReplicationHint, StorageOperation, ThresholdSignature,
};
