//! Threshold-signed capability system
//!
//! This module provides capability tokens that are secured by Aura's threshold
//! signature system, enabling distributed authorization without single points
//! of failure.

use super::{CapabilityError, Permission, Result};
use aura_crypto::Effects;
use aura_crypto::{Ed25519Signature, Ed25519VerifyingKey};
use aura_types::DeviceId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::num::NonZeroU16;

/// Threshold participant index identifier
///
/// This is a threshold scheme participant index (1-based), distinct from the
/// general ParticipantId enum in aura-types which represents device/guardian identity.
/// This type is specifically for indexing participants in threshold signature schemes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ThresholdParticipantId(NonZeroU16);

impl ThresholdParticipantId {
    pub fn new(id: NonZeroU16) -> Self {
        Self(id)
    }

    pub fn as_u16(&self) -> u16 {
        self.0.get()
    }
}

/// Threshold signature with signers
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThresholdSignature {
    pub signature: Ed25519Signature,
    pub signers: Vec<ThresholdParticipantId>,
}

/// Public key package for threshold verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicKeyPackage {
    pub group_public: Ed25519VerifyingKey,
    pub threshold: u16,
    pub total_participants: u16,
}

/// Capability ID derived from threshold signature
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ThresholdCapabilityId(pub [u8; 32]);

impl ThresholdCapabilityId {
    /// Generate capability ID from threshold signature and permissions
    pub fn from_signature_and_permissions(
        signature: &ThresholdSignature,
        permissions: &[Permission],
        device_id: &DeviceId,
        issued_at: u64,
    ) -> Self {
        let mut hasher = aura_crypto::blake3_hasher();
        hasher.update(&aura_crypto::ed25519_signature_to_bytes(
            &signature.signature,
        ));
        for signer in &signature.signers {
            hasher.update(&signer.as_u16().to_le_bytes());
        }
        hasher.update(device_id.0.as_bytes());
        hasher.update(&issued_at.to_le_bytes());

        // Hash permissions deterministically
        let permissions_bytes =
            bincode::serialize(permissions).expect("Permission serialization should never fail");
        hasher.update(&permissions_bytes);

        Self(hasher.finalize().into())
    }
}

/// Threshold-signed capability token
///
/// A capability token that is authenticated by M-of-N threshold signatures,
/// providing distributed authorization without single points of failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdCapability {
    /// Unique capability identifier
    pub id: ThresholdCapabilityId,

    /// Target device for this capability
    pub device_id: DeviceId,

    /// Granted permissions
    pub permissions: Vec<Permission>,

    /// Threshold signature authorizing this capability
    pub authorization: ThresholdSignature,

    /// Public key package for signature verification
    pub public_key_package: PublicKeyPackage,

    /// Issuance timestamp
    pub issued_at: u64,

    /// Optional expiration timestamp
    pub expires_at: Option<u64>,

    /// Delegation chain (if this capability was delegated)
    pub delegation_chain: Vec<ThresholdCapabilityId>,
}

impl ThresholdCapability {
    /// Create a new threshold capability
    pub fn new(
        device_id: DeviceId,
        permissions: Vec<Permission>,
        authorization: ThresholdSignature,
        public_key_package: PublicKeyPackage,
        effects: &Effects,
    ) -> Result<Self> {
        let issued_at = effects
            .now()
            .map_err(|e| CapabilityError::CryptoError(format!("Failed to get time: {:?}", e)))?;

        let id = ThresholdCapabilityId::from_signature_and_permissions(
            &authorization,
            &permissions,
            &device_id,
            issued_at,
        );

        Ok(Self {
            id,
            device_id,
            permissions,
            authorization,
            public_key_package,
            issued_at,
            expires_at: None,
            delegation_chain: vec![],
        })
    }

    /// Set expiration time
    pub fn with_expiration(mut self, expires_at: u64) -> Self {
        self.expires_at = Some(expires_at);
        self
    }

    /// Set delegation chain
    pub fn with_delegation_chain(mut self, delegation_chain: Vec<ThresholdCapabilityId>) -> Self {
        self.delegation_chain = delegation_chain;
        self
    }

    /// Verify the threshold signature on this capability
    pub fn verify_signature(&self) -> Result<()> {
        // Create the signed payload
        let payload = self.create_signing_payload();

        // Verify threshold signature
        aura_crypto::ed25519_verify(
            &self.public_key_package.group_public,
            &payload,
            &self.authorization.signature,
        )
        .map_err(|e| {
            CapabilityError::CryptoError(format!("Signature verification failed: {:?}", e))
        })?;

        // Verify threshold was met
        if self.authorization.signers.len() < self.public_key_package.threshold as usize {
            return Err(CapabilityError::CryptoError(
                "Insufficient signers for threshold".to_string(),
            ));
        }

        // Verify all signers are valid participants (simplified for now)
        // In production, this would check against verifying shares
        for signer in &self.authorization.signers {
            if signer.as_u16() > self.public_key_package.total_participants {
                return Err(CapabilityError::CryptoError(format!(
                    "Invalid signer: {:?}",
                    signer
                )));
            }
        }

        Ok(())
    }

    /// Check if capability is expired
    pub fn is_expired(&self, current_time: u64) -> bool {
        if let Some(expires_at) = self.expires_at {
            current_time >= expires_at
        } else {
            false
        }
    }

    /// Check if capability grants specific permission
    pub fn grants_permission(&self, required: &Permission) -> bool {
        self.permissions
            .iter()
            .any(|granted| permission_satisfies(granted, required))
    }

    /// Get capability authority level (number of signers)
    pub fn authority_level(&self) -> usize {
        self.authorization.signers.len()
    }

    /// Check if capability was signed by specific participant
    pub fn signed_by(&self, participant: &ThresholdParticipantId) -> bool {
        self.authorization.signers.contains(participant)
    }

    /// Create signing payload for verification
    fn create_signing_payload(&self) -> Vec<u8> {
        let mut payload = Vec::new();

        // Include device ID
        payload.extend_from_slice(self.device_id.0.as_bytes());

        // Include permissions (deterministic serialization)
        let permissions_bytes = bincode::serialize(&self.permissions)
            .expect("Permission serialization should never fail");
        payload.extend_from_slice(&permissions_bytes);

        // Include timestamp
        payload.extend_from_slice(&self.issued_at.to_le_bytes());

        // Include expiration if present
        if let Some(expires_at) = self.expires_at {
            payload.extend_from_slice(&expires_at.to_le_bytes());
        }

        // Include delegation chain
        for parent_id in &self.delegation_chain {
            payload.extend_from_slice(&parent_id.0);
        }

        payload
    }
}

/// Check if granted permission satisfies required permission
fn permission_satisfies(granted: &Permission, required: &Permission) -> bool {
    match (granted, required) {
        (
            Permission::Storage {
                operation: granted_op,
                resource: granted_res,
            },
            Permission::Storage {
                operation: required_op,
                resource: required_res,
            },
        ) => granted_op == required_op && resource_matches(granted_res, required_res),
        (
            Permission::Communication {
                operation: granted_op,
                relationship: granted_rel,
            },
            Permission::Communication {
                operation: required_op,
                relationship: required_rel,
            },
        ) => granted_op == required_op && resource_matches(granted_rel, required_rel),
        (
            Permission::Relay {
                operation: granted_op,
                trust_level: granted_trust,
            },
            Permission::Relay {
                operation: required_op,
                trust_level: required_trust,
            },
        ) => granted_op == required_op && trust_level_sufficient(granted_trust, required_trust),
        _ => false,
    }
}

/// Check if granted resource pattern matches required resource
fn resource_matches(granted: &str, required: &str) -> bool {
    if granted == "*" {
        return true;
    }

    if granted.ends_with("/*") {
        let prefix = &granted[..granted.len() - 1]; // Remove "*", keep "/"
        return required.starts_with(prefix);
    }

    granted == required
}

/// Check if granted trust level is sufficient for required trust level
fn trust_level_sufficient(granted: &str, required: &str) -> bool {
    // Define trust level hierarchy
    let trust_levels = ["basic", "elevated", "admin"];

    let granted_level = trust_levels.iter().position(|&level| level == granted);
    let required_level = trust_levels.iter().position(|&level| level == required);

    match (granted_level, required_level) {
        (Some(granted_idx), Some(required_idx)) => granted_idx >= required_idx,
        _ => granted == required, // Fallback to exact match for unknown levels
    }
}

/// Threshold capability manager
///
/// Manages threshold-signed capabilities with clean, secure interfaces.
#[derive(Debug, Clone)]
pub struct ThresholdCapabilityManager {
    /// Active capabilities indexed by device
    capabilities: std::collections::BTreeMap<DeviceId, Vec<ThresholdCapability>>,

    /// Revoked capability IDs
    revoked: BTreeSet<ThresholdCapabilityId>,

    /// Trusted public key packages for signature verification
    trusted_key_packages: std::collections::BTreeMap<String, PublicKeyPackage>,
}

impl ThresholdCapabilityManager {
    /// Create new threshold capability manager
    pub fn new() -> Self {
        Self {
            capabilities: std::collections::BTreeMap::new(),
            revoked: BTreeSet::new(),
            trusted_key_packages: std::collections::BTreeMap::new(),
        }
    }

    /// Register a trusted public key package
    pub fn register_key_package(&mut self, name: String, key_package: PublicKeyPackage) {
        self.trusted_key_packages.insert(name, key_package);
    }

    /// Grant threshold capability
    pub fn grant_capability(&mut self, capability: ThresholdCapability) -> Result<()> {
        // Verify signature before storing
        capability.verify_signature()?;

        // Check if capability is already revoked
        if self.revoked.contains(&capability.id) {
            return Err(CapabilityError::AuthorizationError(
                "Cannot grant revoked capability".to_string(),
            ));
        }

        // Store capability
        self.capabilities
            .entry(capability.device_id)
            .or_default()
            .push(capability);

        Ok(())
    }

    /// Verify permission for device
    pub fn verify_permission(
        &self,
        device_id: &DeviceId,
        permission: &Permission,
        current_time: u64,
    ) -> Result<&ThresholdCapability> {
        let capabilities = self.capabilities.get(device_id).ok_or_else(|| {
            CapabilityError::AuthorizationError("No capabilities found for device".to_string())
        })?;

        // Find valid capability that grants the permission
        for capability in capabilities {
            // Skip expired capabilities
            if capability.is_expired(current_time) {
                continue;
            }

            // Skip revoked capabilities
            if self.revoked.contains(&capability.id) {
                continue;
            }

            // Check if capability grants the permission
            if capability.grants_permission(permission) {
                return Ok(capability);
            }
        }

        Err(CapabilityError::AuthorizationError(
            "No valid capability grants this permission".to_string(),
        ))
    }

    /// Revoke capability
    pub fn revoke_capability(&mut self, capability_id: ThresholdCapabilityId) {
        self.revoked.insert(capability_id);
    }

    /// Get all capabilities for device
    pub fn get_capabilities(&self, device_id: &DeviceId) -> Vec<&ThresholdCapability> {
        self.capabilities
            .get(device_id)
            .map(|caps| caps.iter().collect())
            .unwrap_or_default()
    }

    /// Clean up expired and revoked capabilities
    pub fn cleanup(&mut self, current_time: u64) {
        for capabilities in self.capabilities.values_mut() {
            capabilities
                .retain(|cap| !cap.is_expired(current_time) && !self.revoked.contains(&cap.id));
        }
    }

    /// Get capability statistics
    pub fn stats(&self) -> CapabilityStats {
        let total_capabilities = self.capabilities.values().map(|caps| caps.len()).sum();
        let revoked_count = self.revoked.len();
        let device_count = self.capabilities.len();

        CapabilityStats {
            total_capabilities,
            revoked_count,
            device_count,
            trusted_key_packages: self.trusted_key_packages.len(),
        }
    }
}

impl Default for ThresholdCapabilityManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Capability manager statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityStats {
    pub total_capabilities: usize,
    pub revoked_count: usize,
    pub device_count: usize,
    pub trusted_key_packages: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_crypto::Ed25519SigningKey;
    use uuid::Uuid;

    fn test_effects() -> Effects {
        Effects::for_test("threshold_capabilities_test")
    }

    fn test_device_id() -> DeviceId {
        DeviceId(Uuid::new_v4())
    }

    fn mock_threshold_signature() -> ThresholdSignature {
        let signature = aura_crypto::Ed25519Signature::from_bytes(&[0u8; 64]);
        let signers = vec![
            ThresholdParticipantId::new(NonZeroU16::new(1).unwrap()),
            ThresholdParticipantId::new(NonZeroU16::new(2).unwrap()),
        ];

        ThresholdSignature { signature, signers }
    }

    fn mock_public_key_package() -> PublicKeyPackage {
        let signing_key = aura_crypto::Ed25519SigningKey::from_bytes(&[1u8; 32]);
        let group_public = signing_key.verifying_key();

        PublicKeyPackage {
            group_public,
            threshold: 2,
            total_participants: 3,
        }
    }

    #[test]
    fn test_capability_creation() {
        let device_id = test_device_id();
        let permissions = vec![Permission::Storage {
            operation: super::super::StorageOperation::Read,
            resource: "test/*".to_string(),
        }];
        let authorization = mock_threshold_signature();
        let public_key_package = mock_public_key_package();
        let effects = test_effects();

        let capability = ThresholdCapability::new(
            device_id,
            permissions.clone(),
            authorization,
            public_key_package,
            &effects,
        );

        assert!(capability.is_ok());
        let cap = capability.unwrap();
        assert_eq!(cap.device_id, device_id);
        assert_eq!(cap.permissions, permissions);
        assert!(cap.expires_at.is_none());
    }

    #[test]
    fn test_capability_expiration() {
        let device_id = test_device_id();
        let permissions = vec![Permission::Storage {
            operation: super::super::StorageOperation::Read,
            resource: "test/*".to_string(),
        }];
        let authorization = mock_threshold_signature();
        let public_key_package = mock_public_key_package();
        let effects = test_effects();

        let current_time = effects.now().unwrap();
        let capability = ThresholdCapability::new(
            device_id,
            permissions,
            authorization,
            public_key_package,
            &effects,
        )
        .unwrap()
        .with_expiration(current_time + 1000);

        assert!(!capability.is_expired(current_time));
        assert!(!capability.is_expired(current_time + 500));
        assert!(capability.is_expired(current_time + 1000));
        assert!(capability.is_expired(current_time + 2000));
    }

    #[test]
    fn test_permission_matching() {
        let granted = Permission::Storage {
            operation: super::super::StorageOperation::Read,
            resource: "test/*".to_string(),
        };

        let required_match = Permission::Storage {
            operation: super::super::StorageOperation::Read,
            resource: "test/file.txt".to_string(),
        };

        let required_no_match = Permission::Storage {
            operation: super::super::StorageOperation::Write,
            resource: "test/file.txt".to_string(),
        };

        assert!(permission_satisfies(&granted, &required_match));
        assert!(!permission_satisfies(&granted, &required_no_match));
    }

    #[test]
    fn test_resource_matching() {
        assert!(resource_matches("*", "anything"));
        assert!(resource_matches("test/*", "test/file.txt"));
        assert!(resource_matches("test/*", "test/subdir/file.txt"));
        assert!(!resource_matches("test/*", "other/file.txt"));
        assert!(resource_matches("exact", "exact"));
        assert!(!resource_matches("exact", "different"));
    }

    #[test]
    fn test_trust_level_hierarchy() {
        assert!(trust_level_sufficient("admin", "basic"));
        assert!(trust_level_sufficient("admin", "elevated"));
        assert!(trust_level_sufficient("admin", "admin"));
        assert!(trust_level_sufficient("elevated", "basic"));
        assert!(trust_level_sufficient("elevated", "elevated"));
        assert!(!trust_level_sufficient("elevated", "admin"));
        assert!(trust_level_sufficient("basic", "basic"));
        assert!(!trust_level_sufficient("basic", "elevated"));
        assert!(!trust_level_sufficient("basic", "admin"));
    }

    #[test]
    fn test_manager_operations() {
        let manager = ThresholdCapabilityManager::new();
        let device_id = test_device_id();
        let permission = Permission::Storage {
            operation: super::super::StorageOperation::Read,
            resource: "test/file.txt".to_string(),
        };
        let effects = test_effects();
        let current_time = effects.now().unwrap();

        // Should fail initially (no capabilities)
        assert!(manager
            .verify_permission(&device_id, &permission, current_time)
            .is_err());

        // Grant capability (this would fail signature verification in practice)
        // For testing, we'll create a capability directly
        let _capability = ThresholdCapability::new(
            device_id,
            vec![Permission::Storage {
                operation: super::super::StorageOperation::Read,
                resource: "test/*".to_string(),
            }],
            mock_threshold_signature(),
            mock_public_key_package(),
            &effects,
        )
        .unwrap();

        // We can't test grant_capability because it requires valid signatures,
        // but we can test the manager structure
        assert_eq!(manager.stats().total_capabilities, 0);
        assert_eq!(manager.get_capabilities(&device_id).len(), 0);
    }
}
