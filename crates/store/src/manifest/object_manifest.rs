//! Object Manifest Structure
//!
//! Implements capability-controlled metadata for stored objects with
//! separated key derivation and deterministic CBOR serialization.
//!
//! Reference: docs/040_storage.md Section 2.1

use serde::{Deserialize, Serialize};

// Import shared types from aura-types
pub use aura_types::{AccountId, Cid, DeviceId};

pub type PeerId = Vec<u8>;
pub type CapabilityId = Vec<u8>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ObjectManifest {
    pub root_cid: Cid,
    pub size: u64,
    pub chunking: ChunkingParams,
    pub chunk_digests: Vec<[u8; 32]>,
    pub erasure: Option<ErasureMeta>,
    pub context_id: Option<[u8; 32]>,
    pub app_metadata: Option<Vec<u8>>,
    pub key_derivation: KeyDerivationSpec,
    pub access_control: AccessControl,
    pub replication_hint: StaticReplicationHint,
    pub version: u64,
    pub prev_manifest: Option<Cid>,
    pub issued_at_ms: u64,
    pub nonce: [u8; 32],
    pub sig: ThresholdSignature,
}

impl ObjectManifest {
    pub fn new(
        root_cid: Cid,
        size: u64,
        chunking: ChunkingParams,
        key_derivation: KeyDerivationSpec,
        access_control: AccessControl,
        replication_hint: StaticReplicationHint,
        issued_at_ms: u64,
        sig: ThresholdSignature,
    ) -> Self {
        use blake3::Hasher;
        let mut hasher = Hasher::new();
        hasher.update(root_cid.as_str().as_bytes());
        hasher.update(&size.to_le_bytes());
        let hash = hasher.finalize();
        let nonce = *hash.as_bytes();

        Self {
            root_cid,
            size,
            chunking,
            chunk_digests: vec![],
            erasure: None,
            context_id: None,
            app_metadata: None,
            key_derivation,
            access_control,
            replication_hint,
            version: 1,
            prev_manifest: None,
            issued_at_ms,
            nonce,
            sig,
        }
    }

    pub fn with_chunk_digests(mut self, digests: Vec<[u8; 32]>) -> Self {
        self.chunk_digests = digests;
        self
    }

    pub fn with_erasure_meta(mut self, erasure: ErasureMeta) -> Self {
        self.erasure = Some(erasure);
        self
    }

    pub fn with_app_metadata(mut self, metadata: Vec<u8>) -> Self {
        self.app_metadata = Some(metadata);
        self
    }

    pub fn with_context_id(mut self, context_id: [u8; 32]) -> Self {
        self.context_id = Some(context_id);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChunkingParams {
    pub chunk_size: u32,
    pub algorithm: String,
}

impl ChunkingParams {
    pub fn default_for_size(size: u64) -> Self {
        let chunk_size = if size < 1024 * 1024 {
            256 * 1024
        } else if size < 100 * 1024 * 1024 {
            1024 * 1024
        } else {
            4 * 1024 * 1024
        };

        Self {
            chunk_size: chunk_size as u32,
            algorithm: "blake3".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErasureMeta {
    pub codec: String,
    pub required_fragments: u32,
    pub total_fragments: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeyDerivationSpec {
    pub algorithm: String,
    pub domain: Vec<u8>,
    pub context: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccessControl {
    pub resource_scope: ResourceScope,
    pub required_permissions: Vec<Permission>,
    pub delegation_allowed: bool,
    pub max_delegation_depth: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ResourceScope {
    StorageObject { account_id: AccountId },
    AccountStorage { account_id: AccountId },
    DeviceStorage { device_id: DeviceId },
    Public,
    // Legacy variants for backward compatibility
    AllOwnedObjects,
    Object { cid: String },
    Manifest { cid: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Permission {
    pub operation: StorageOperation,
    pub resource: ResourceScope,
    pub grant_time: u64,
    pub expiry: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum StorageOperation {
    Store,
    Retrieve,
    Delete,
    Read,
    Write,
    List,
    GetMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StaticReplicationHint {
    pub desired_replicas: u32,
    pub peer_preferences: Option<Vec<PeerId>>,
    // Additional fields for replicator compatibility
    pub target_peers: Vec<PeerId>,
    pub target_replicas: u32,
    pub fallback_policy: ReplicaFallbackPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ReplicaFallbackPolicy {
    LocalOnly,
    StaticPeerList { peers: Vec<PeerId> },
    RandomSelection { min_peers: usize },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThresholdSignature {
    pub threshold: u32,
    pub signature_shares: Vec<SignatureShare>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignatureShare {
    pub device_id: DeviceId,
    pub share: Vec<u8>,
}
