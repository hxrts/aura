//! Object Manifest Structure
//!
//! Implements capability-controlled metadata for stored objects with
//! separated key derivation and deterministic CBOR serialization.
//!
//! Reference: docs/040_storage.md Section 2.1

use serde::{Deserialize, Serialize};

pub type Cid = Vec<u8>;
pub type DeviceId = Vec<u8>;
pub type AccountId = Vec<u8>;
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
        let mut nonce = [0u8; 32];
        use blake3::Hasher;
        let mut hasher = Hasher::new();
        hasher.update(&root_cid);
        hasher.update(&size.to_le_bytes());
        hasher.update(&issued_at_ms.to_le_bytes());
        let hash = hasher.finalize();
        nonce.copy_from_slice(hash.as_bytes());

        Self {
            root_cid,
            size,
            chunking,
            chunk_digests: Vec::new(),
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

    pub fn compute_cid(&self) -> Cid {
        use blake3::Hasher;
        let bytes = serde_cbor::to_vec(self).expect("Manifest serialization failed");
        let mut hasher = Hasher::new();
        hasher.update(&bytes);
        hasher.finalize().as_bytes().to_vec()
    }

    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.size == 0 {
            return Err(ManifestError::InvalidSize);
        }

        if self.chunking.chunk_size < ChunkingParams::MIN_CHUNK_SIZE
            || self.chunking.chunk_size > ChunkingParams::MAX_CHUNK_SIZE
        {
            return Err(ManifestError::InvalidChunkSize);
        }

        if let Some(metadata) = &self.app_metadata {
            if metadata.len() > 4096 {
                return Err(ManifestError::MetadataTooLarge);
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChunkingParams {
    pub chunk_size: u32,
    pub num_chunks: u32,
}

impl ChunkingParams {
    pub const MIN_CHUNK_SIZE: u32 = 1 * 1024 * 1024;
    pub const MAX_CHUNK_SIZE: u32 = 4 * 1024 * 1024;
    pub const DEFAULT_CHUNK_SIZE: u32 = 1 * 1024 * 1024;

    pub fn new(total_size: u64, chunk_size: u32) -> Self {
        let chunk_size = chunk_size.clamp(Self::MIN_CHUNK_SIZE, Self::MAX_CHUNK_SIZE);
        let num_chunks = ((total_size + chunk_size as u64 - 1) / chunk_size as u64) as u32;
        Self {
            chunk_size,
            num_chunks,
        }
    }

    pub fn default_for_size(total_size: u64) -> Self {
        Self::new(total_size, Self::DEFAULT_CHUNK_SIZE)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErasureMeta {
    pub data_shards: u32,
    pub parity_shards: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeyDerivationSpec {
    pub identity_context: IdentityKeyContext,
    pub permission_context: Option<PermissionKeyContext>,
    pub derivation_path: Vec<u8>,
    pub key_version: u32,
}

impl KeyDerivationSpec {
    pub fn device_encryption(device_id: DeviceId) -> Self {
        Self {
            identity_context: IdentityKeyContext::DeviceEncryption { device_id },
            permission_context: None,
            derivation_path: vec![],
            key_version: 1,
        }
    }

    pub fn with_storage_permission(mut self, operation: String, resource: String) -> Self {
        self.permission_context = Some(PermissionKeyContext::StorageAccess {
            operation,
            resource,
        });
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum IdentityKeyContext {
    DeviceEncryption { device_id: DeviceId },
    RelationshipKeys { relationship_id: Vec<u8> },
    AccountRoot { account_id: AccountId },
    GuardianKeys { guardian_id: Vec<u8> },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PermissionKeyContext {
    StorageAccess {
        operation: String,
        resource: String,
    },
    CommunicationScope {
        operation: String,
        relationship: String,
    },
    RelayPermission {
        operation: String,
        trust_level: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AccessControl {
    CapabilityBased {
        required_permissions: Vec<Permission>,
        delegation_chain: Vec<CapabilityId>,
    },
}

impl AccessControl {
    pub fn new_capability_based(required_permissions: Vec<Permission>) -> Self {
        Self::CapabilityBased {
            required_permissions,
            delegation_chain: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Permission {
    Storage {
        operation: StorageOperation,
        resource: ResourceScope,
    },
    Communication {
        operation: CommunicationOperation,
        relationship: RelationshipScope,
    },
    Relay {
        operation: RelayOperation,
        trust_level: TrustLevel,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum StorageOperation {
    Read,
    Write,
    Delete,
    Share,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ResourceScope {
    Object { cid: Cid },
    Manifest { cid: Cid },
    AllOwnedObjects,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CommunicationOperation {
    Send,
    Receive,
    Forward,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RelationshipScope {
    Direct { peer_id: PeerId },
    OneDegree,
    TwoDegree,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RelayOperation {
    Relay,
    Store,
    Forward,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TrustLevel {
    Direct,
    OneDegree,
    TwoDegree,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StaticReplicationHint {
    pub target_peers: Vec<PeerId>,
    pub required_capability: CapabilityScope,
    pub target_replicas: u32,
    pub fallback_policy: ReplicaFallbackPolicy,
}

impl StaticReplicationHint {
    pub fn new(target_peers: Vec<PeerId>, target_replicas: u32) -> Self {
        Self {
            target_peers,
            required_capability: CapabilityScope::Storage,
            target_replicas,
            fallback_policy: ReplicaFallbackPolicy::LocalOnly,
        }
    }

    pub fn local_only() -> Self {
        Self {
            target_peers: vec![],
            required_capability: CapabilityScope::Storage,
            target_replicas: 0,
            fallback_policy: ReplicaFallbackPolicy::LocalOnly,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CapabilityScope {
    Storage,
    Communication,
    Relay,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ReplicaFallbackPolicy {
    StaticPeerList { peers: Vec<PeerId> },
    RandomSelection { min_peers: u32 },
    LocalOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThresholdSignature {
    pub signers: Vec<DeviceId>,
    pub signature_shares: Vec<Vec<u8>>,
    pub aggregated_signature: Vec<u8>,
}

impl ThresholdSignature {
    pub fn placeholder() -> Self {
        // Generate a proper test signature instead of empty vectors
        use ed25519_dalek::{SigningKey, Signer};
        
        let signing_key = SigningKey::from_bytes(&[1u8; 32]);
        let message = b"test_message_for_threshold_signature";
        let signature = signing_key.sign(message);
        
        Self {
            signers: vec![vec![1u8; 32]], // Single test signer
            signature_shares: vec![signature.to_bytes().to_vec()],
            aggregated_signature: signature.to_bytes().to_vec(),
        }
    }

    pub fn new(signers: Vec<DeviceId>, aggregated_signature: Vec<u8>) -> Self {
        Self {
            signers,
            signature_shares: vec![],
            aggregated_signature,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    InvalidSize,
    InvalidChunkSize,
    MetadataTooLarge,
    InvalidSignature,
    SerializationError,
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSize => write!(f, "Invalid manifest size"),
            Self::InvalidChunkSize => write!(f, "Invalid chunk size"),
            Self::MetadataTooLarge => write!(f, "Application metadata exceeds 4 KiB"),
            Self::InvalidSignature => write!(f, "Invalid threshold signature"),
            Self::SerializationError => write!(f, "Manifest serialization failed"),
        }
    }
}

impl std::error::Error for ManifestError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_manifest() -> ObjectManifest {
        let root_cid = vec![1u8; 32];
        let size = 5 * 1024 * 1024;
        let chunking = ChunkingParams::default_for_size(size);
        let key_derivation = KeyDerivationSpec::device_encryption(vec![2u8; 32]);
        let access_control = AccessControl::new_capability_based(vec![Permission::Storage {
            operation: StorageOperation::Read,
            resource: ResourceScope::AllOwnedObjects,
        }]);
        let replication_hint = StaticReplicationHint::local_only();
        let sig = ThresholdSignature::placeholder();

        ObjectManifest::new(
            root_cid,
            size,
            chunking,
            key_derivation,
            access_control,
            replication_hint,
            1000000,
            sig,
        )
    }

    #[test]
    fn test_manifest_creation() {
        let manifest = create_test_manifest();
        assert_eq!(manifest.size, 5 * 1024 * 1024);
        assert_eq!(manifest.version, 1);
        assert_eq!(manifest.chunking.num_chunks, 5);
    }

    #[test]
    fn test_manifest_validation() {
        let manifest = create_test_manifest();
        assert!(manifest.validate().is_ok());
    }

    #[test]
    fn test_manifest_cid_computation() {
        let manifest = create_test_manifest();
        let cid1 = manifest.compute_cid();
        let cid2 = manifest.compute_cid();
        assert_eq!(cid1, cid2);
        assert!(!cid1.is_empty());
    }

    #[test]
    fn test_chunking_params() {
        let params = ChunkingParams::default_for_size(5 * 1024 * 1024);
        assert_eq!(params.chunk_size, ChunkingParams::DEFAULT_CHUNK_SIZE);
        assert_eq!(params.num_chunks, 5);

        let params = ChunkingParams::new(10 * 1024 * 1024, 2 * 1024 * 1024);
        assert_eq!(params.chunk_size, 2 * 1024 * 1024);
        assert_eq!(params.num_chunks, 5);
    }

    #[test]
    fn test_chunking_params_bounds() {
        let params = ChunkingParams::new(1024, 100);
        assert!(params.chunk_size >= ChunkingParams::MIN_CHUNK_SIZE);

        let params = ChunkingParams::new(1024, 10 * 1024 * 1024);
        assert!(params.chunk_size <= ChunkingParams::MAX_CHUNK_SIZE);
    }

    #[test]
    fn test_key_derivation_spec() {
        let spec = KeyDerivationSpec::device_encryption(vec![1u8; 32]);
        assert_eq!(spec.key_version, 1);
        assert!(spec.permission_context.is_none());

        let spec = spec.with_storage_permission("read".to_string(), "object".to_string());
        assert!(spec.permission_context.is_some());
    }

    #[test]
    fn test_access_control() {
        let ac = AccessControl::new_capability_based(vec![Permission::Storage {
            operation: StorageOperation::Write,
            resource: ResourceScope::AllOwnedObjects,
        }]);

        match ac {
            AccessControl::CapabilityBased {
                required_permissions,
                ..
            } => {
                assert_eq!(required_permissions.len(), 1);
            }
        }
    }

    #[test]
    fn test_replication_hint() {
        let hint = StaticReplicationHint::new(vec![vec![1u8; 32], vec![2u8; 32]], 2);
        assert_eq!(hint.target_peers.len(), 2);
        assert_eq!(hint.target_replicas, 2);

        let hint = StaticReplicationHint::local_only();
        assert_eq!(hint.target_peers.len(), 0);
        assert_eq!(hint.target_replicas, 0);
    }

    #[test]
    fn test_manifest_with_metadata() {
        let mut manifest = create_test_manifest();
        manifest.app_metadata = Some(vec![0u8; 1024]);
        assert!(manifest.validate().is_ok());

        manifest.app_metadata = Some(vec![0u8; 5000]);
        assert!(matches!(
            manifest.validate(),
            Err(ManifestError::MetadataTooLarge)
        ));
    }

    #[test]
    fn test_threshold_signature() {
        let sig = ThresholdSignature::new(vec![vec![1u8; 32]], vec![0u8; 64]);
        assert_eq!(sig.signers.len(), 1);
        assert_eq!(sig.aggregated_signature.len(), 64);
    }

    #[test]
    fn test_manifest_serialization() {
        let manifest = create_test_manifest();
        let serialized = serde_cbor::to_vec(&manifest).unwrap();
        let deserialized: ObjectManifest = serde_cbor::from_slice(&serialized).unwrap();
        assert_eq!(manifest, deserialized);
    }
}
