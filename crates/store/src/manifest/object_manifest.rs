//! Object Manifest Structure
//!
//! Implements capability-controlled metadata for stored objects with
//! separated key derivation and deterministic CBOR serialization.
//!
//! Reference: docs/040_storage.md Section 2.1

use serde::{Deserialize, Serialize};

// Import shared types from aura-types
pub use aura_types::{AccountId, Cid, DeviceId, DeviceIdExt};

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
        use aura_crypto::blake3_hash_chunks;
        let nonce = blake3_hash_chunks(&[
            root_cid.as_str().as_bytes(),
            &size.to_le_bytes(),
        ]);

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

impl ThresholdSignature {
    /// Create a placeholder threshold signature for testing purposes
    ///
    /// **WARNING: This creates a fake signature with no cryptographic security.**
    /// Use only for testing. For production, use `from_frost()` with real FROST signatures.
    pub fn placeholder() -> Self {
        use aura_crypto::Effects;

        Self {
            threshold: 2,
            signature_shares: vec![
                SignatureShare {
                    device_id: DeviceId::new_with_effects(&Effects::test()),
                    share: vec![0u8; 32], // Fake signature share
                },
                SignatureShare {
                    device_id: DeviceId::new_with_effects(&Effects::test()),
                    share: vec![1u8; 32], // Fake signature share
                },
            ],
        }
    }

    /// Create a real threshold signature from FROST signature shares
    ///
    /// This method should be used in production to create actual cryptographically
    /// secure threshold signatures from FROST protocol outputs.
    ///
    /// # Arguments
    ///
    /// * `threshold` - The minimum number of shares required (M in M-of-N)
    /// * `shares` - Vector of (device_id, signature_share) pairs from FROST
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // After running FROST threshold signing protocol:
    /// let frost_shares = vec![
    ///     (device_id_1, frost_signature_share_1),
    ///     (device_id_2, frost_signature_share_2),
    /// ];
    /// let threshold_sig = ThresholdSignature::from_frost(2, frost_shares);
    /// ```
    pub fn from_frost(threshold: u32, shares: Vec<(DeviceId, Vec<u8>)>) -> Self {
        let signature_shares = shares
            .into_iter()
            .map(|(device_id, share)| SignatureShare { device_id, share })
            .collect();

        Self {
            threshold,
            signature_shares,
        }
    }

    /// Verify the threshold signature against provided data
    ///
    /// **Note: This is a placeholder implementation.**
    /// Real verification would use FROST verification algorithms.
    pub fn verify(&self, _data: &[u8], _public_keys: &[(DeviceId, Vec<u8>)]) -> bool {
        // Placeholder verification - always returns true
        // In production, this would:
        // 1. Verify each signature share against its corresponding public key
        // 2. Check that we have at least `threshold` valid shares
        // 3. Aggregate the shares and verify against the group public key
        !self.signature_shares.is_empty() && self.signature_shares.len() >= self.threshold as usize
    }

    /// Get the number of signature shares
    pub fn share_count(&self) -> usize {
        self.signature_shares.len()
    }

    /// Check if this signature meets the threshold requirement
    pub fn meets_threshold(&self) -> bool {
        self.signature_shares.len() >= self.threshold as usize
    }

    /// Get the device IDs of signers
    pub fn signers(&self) -> Vec<&DeviceId> {
        self.signature_shares.iter().map(|s| &s.device_id).collect()
    }

    /// Create a threshold signature from a completed FROST session
    ///
    /// This method converts the output of a completed FROST session from the
    /// coordination layer into a ThresholdSignature for storage manifests.
    ///
    /// # Example Integration with Coordination Layer
    ///
    /// ```rust,ignore
    /// use aura_coordination::FrostSession;
    ///
    /// // After running a complete FROST session:
    /// let mut frost_session = FrostSession::new(session_id, message, threshold, key_share);
    /// // ... run FROST protocol rounds ...
    /// let aggregated_signature = frost_session.aggregate_signature()?;
    ///
    /// // Convert to storage ThresholdSignature:
    /// let device_shares: Vec<(DeviceId, Vec<u8>)> = frost_session
    ///     .signature_shares
    ///     .iter()
    ///     .map(|(id, share)| {
    ///         // Convert frost::Identifier back to DeviceId
    ///         let device_id = id.to_bytes().to_vec();
    ///         let share_bytes = share.serialize(); // Serialize FROST share
    ///         (device_id, share_bytes)
    ///     })
    ///     .collect();
    ///
    /// let threshold_sig = ThresholdSignature::from_frost_session(
    ///     threshold as u32,
    ///     device_shares,
    ///     aggregated_signature.to_bytes().to_vec(),
    /// );
    /// ```
    pub fn from_frost_session(
        threshold: u32,
        device_shares: Vec<(DeviceId, Vec<u8>)>,
        _aggregated_signature: Vec<u8>, // Store the final signature if needed
    ) -> Self {
        Self::from_frost(threshold, device_shares)
    }
}
