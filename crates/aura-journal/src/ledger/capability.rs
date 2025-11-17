//! Capability System Types
//!
//! Implements capability-based authorization separate from tree membership (authentication).
//! Capabilities are time-limited, attenuatable tokens that grant specific permissions
//! for specific resources.

use aura_core::identifiers::DeviceId;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Timestamp type (Unix milliseconds) for tracking capability creation and expiration times
pub type Timestamp = u64;

/// Unique identifier for a capability
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CapabilityId(pub uuid::Uuid);

impl CapabilityId {
    /// Create a new capability ID.
    ///
    /// # Parameters
    /// - `id`: UUID for the capability (obtain from RandomEffects for testability)
    ///
    /// Note: Callers should obtain UUID from RandomEffects to maintain testability
    /// and consistency with the effect system architecture.
    pub fn new(id: uuid::Uuid) -> Self {
        Self(id)
    }

    /// Create from a UUID (alias for new)
    pub fn from_uuid(uuid: uuid::Uuid) -> Self {
        Self::new(uuid)
    }
}

impl fmt::Display for CapabilityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cap-{}", self.0)
    }
}

/// Resource reference for capability scope
///
/// Identifies what resource this capability grants access to.
/// Uses URI-style notation for flexibility.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ResourceRef {
    /// Resource URI (e.g., "journal://recovery/{leaf}#{epoch}")
    pub uri: String,
}

impl ResourceRef {
    /// Create a new resource reference
    pub fn new(uri: impl Into<String>) -> Self {
        Self { uri: uri.into() }
    }

    /// Create a recovery resource reference
    pub fn recovery(leaf_index: usize, epoch: u64) -> Self {
        Self {
            uri: format!("journal://recovery/{}#{}", leaf_index, epoch),
        }
    }

    /// Create a storage resource reference
    pub fn storage(path: &str) -> Self {
        Self {
            uri: format!("storage://{}", path),
        }
    }

    /// Create a relay resource reference
    pub fn relay(session_id: &str) -> Self {
        Self {
            uri: format!("relay://{}", session_id),
        }
    }

    /// Get the URI string
    pub fn as_str(&self) -> &str {
        &self.uri
    }

    /// Check if this is a recovery capability
    pub fn is_recovery(&self) -> bool {
        self.uri.starts_with("journal://recovery/")
    }

    /// Check if this is a storage capability
    pub fn is_storage(&self) -> bool {
        self.uri.starts_with("storage://")
    }

    /// Check if this is a relay capability
    pub fn is_relay(&self) -> bool {
        self.uri.starts_with("relay://")
    }
}

impl fmt::Display for ResourceRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.uri)
    }
}

impl From<String> for ResourceRef {
    fn from(uri: String) -> Self {
        Self { uri }
    }
}

impl From<&str> for ResourceRef {
    fn from(uri: &str) -> Self {
        Self {
            uri: uri.to_string(),
        }
    }
}

/// Signature for a capability
///
/// Proves that the capability was issued by an authorized device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilitySignature {
    /// Signature bytes (Ed25519)
    pub signature: Vec<u8>,
    /// Device that signed this capability
    pub signer: DeviceId,
}

impl CapabilitySignature {
    /// Create a new capability signature
    pub fn new(signature: Vec<u8>, signer: DeviceId) -> Self {
        Self { signature, signer }
    }
}

/// Capability reference
///
/// A time-limited authorization token that grants specific permissions for a specific resource.
/// Capabilities are issued as part of TreeOp records and can be revoked via tombstones.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityRef {
    /// Unique identifier for this capability
    pub id: CapabilityId,

    /// Resource this capability grants access to
    pub resource: ResourceRef,

    /// Expiration timestamp (Unix milliseconds)
    pub expires_at: Timestamp,

    /// Signature proving issuance by authorized device
    pub signature: CapabilitySignature,

    /// Optional attenuation (further restrictions)
    pub attenuation: Option<Attenuation>,
}

impl CapabilityRef {
    /// Create a new capability reference
    pub fn new(
        id: CapabilityId,
        resource: ResourceRef,
        expires_at: Timestamp,
        signature: CapabilitySignature,
    ) -> Self {
        Self {
            id,
            resource,
            expires_at,
            signature,
            attenuation: None,
        }
    }

    /// Create a capability with attenuation
    pub fn with_attenuation(mut self, attenuation: Attenuation) -> Self {
        self.attenuation = Some(attenuation);
        self
    }

    /// Check if this capability has expired
    pub fn is_expired(&self, current_time: Timestamp) -> bool {
        current_time >= self.expires_at
    }

    /// Check if this capability is valid at the given time
    pub fn is_valid_at(&self, timestamp: Timestamp) -> bool {
        !self.is_expired(timestamp)
    }

    /// Get time until expiration (returns 0 if already expired)
    pub fn time_until_expiration(&self, current_time: Timestamp) -> Timestamp {
        if self.is_expired(current_time) {
            0
        } else {
            self.expires_at - current_time
        }
    }
}

/// Attenuation for capability
///
/// Additional restrictions that can be applied to a capability when delegating it.
/// Attenuations can only further restrict, never expand, the original capability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attenuation {
    /// Maximum number of uses (None = unlimited)
    pub max_uses: Option<u32>,

    /// Restricted to specific operations
    pub allowed_operations: Option<Vec<String>>,

    /// Further restricted expiration time
    pub restricted_expires_at: Option<Timestamp>,

    /// Additional metadata
    pub metadata: std::collections::BTreeMap<String, String>,
}

impl Attenuation {
    /// Create a new attenuation
    pub fn new() -> Self {
        Self {
            max_uses: None,
            allowed_operations: None,
            restricted_expires_at: None,
            metadata: std::collections::BTreeMap::new(),
        }
    }

    /// Set maximum uses
    pub fn with_max_uses(mut self, max_uses: u32) -> Self {
        self.max_uses = Some(max_uses);
        self
    }

    /// Set allowed operations
    pub fn with_operations(mut self, operations: Vec<String>) -> Self {
        self.allowed_operations = Some(operations);
        self
    }

    /// Set restricted expiration
    pub fn with_expiration(mut self, expires_at: Timestamp) -> Self {
        self.restricted_expires_at = Some(expires_at);
        self
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }
}

impl Default for Attenuation {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::disallowed_methods)]
    fn test_capability_id_creation() {
        let id1 = CapabilityId::new(uuid::Uuid::new_v4());
        let id2 = CapabilityId::new(uuid::Uuid::new_v4());
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_resource_ref_recovery() {
        let resource = ResourceRef::recovery(0, 42);
        assert!(resource.is_recovery());
        assert!(!resource.is_storage());
        assert!(resource.as_str().contains("42"));
    }

    #[test]
    fn test_resource_ref_storage() {
        let resource = ResourceRef::storage("/backup/data");
        assert!(resource.is_storage());
        assert!(!resource.is_recovery());
        assert!(resource.as_str().contains("/backup/data"));
    }

    #[test]
    fn test_resource_ref_relay() {
        let resource = ResourceRef::relay("session-123");
        assert!(resource.is_relay());
        assert!(!resource.is_storage());
        assert!(resource.as_str().contains("session-123"));
    }

    #[test]
    #[allow(clippy::disallowed_methods)]
    fn test_capability_ref_expiration() {
        let cap = CapabilityRef::new(
            CapabilityId::new(uuid::Uuid::new_v4()),
            ResourceRef::recovery(0, 1),
            1000,
            CapabilitySignature::new(vec![0u8; 64], DeviceId(uuid::Uuid::from_bytes([1u8; 16]))),
        );

        assert!(!cap.is_expired(500));
        assert!(cap.is_expired(1000));
        assert!(cap.is_expired(1500));
    }

    #[test]
    #[allow(clippy::disallowed_methods)]
    fn test_capability_ref_time_until_expiration() {
        let cap = CapabilityRef::new(
            CapabilityId::new(uuid::Uuid::new_v4()),
            ResourceRef::recovery(0, 1),
            1000,
            CapabilitySignature::new(vec![0u8; 64], DeviceId(uuid::Uuid::from_bytes([1u8; 16]))),
        );

        assert_eq!(cap.time_until_expiration(500), 500);
        assert_eq!(cap.time_until_expiration(1000), 0);
        assert_eq!(cap.time_until_expiration(1500), 0);
    }

    #[test]
    #[allow(clippy::disallowed_methods)]
    fn test_capability_ref_with_attenuation() {
        let attenuation = Attenuation::new()
            .with_max_uses(5)
            .with_operations(vec!["read".to_string()]);

        let cap = CapabilityRef::new(
            CapabilityId::new(uuid::Uuid::new_v4()),
            ResourceRef::storage("/data"),
            1000,
            CapabilitySignature::new(vec![0u8; 64], DeviceId(uuid::Uuid::from_bytes([1u8; 16]))),
        )
        .with_attenuation(attenuation);

        assert!(cap.attenuation.is_some());
        let att = cap.attenuation.unwrap();
        assert_eq!(att.max_uses, Some(5));
        assert_eq!(att.allowed_operations, Some(vec!["read".to_string()]));
    }

    #[test]
    fn test_attenuation_builder() {
        let attenuation = Attenuation::new()
            .with_max_uses(10)
            .with_operations(vec!["read".to_string(), "write".to_string()])
            .with_expiration(5000)
            .with_metadata("purpose".to_string(), "testing".to_string());

        assert_eq!(attenuation.max_uses, Some(10));
        assert_eq!(attenuation.restricted_expires_at, Some(5000));
        assert_eq!(attenuation.metadata.get("purpose").unwrap(), "testing");
    }

    #[test]
    fn test_resource_ref_from_string() {
        let resource: ResourceRef = "custom://resource".into();
        assert_eq!(resource.as_str(), "custom://resource");
    }

    #[test]
    fn test_capability_signature() {
        let sig =
            CapabilitySignature::new(vec![0u8; 64], DeviceId(uuid::Uuid::from_bytes([0u8; 16])));
        assert_eq!(sig.signature.len(), 64);
    }
}

/// Recovery-specific capability types
///
/// Recovery capabilities are special time-limited capabilities issued by guardian
/// quorums to allow device recovery/rekey operations.
///
/// Recovery capability issued by guardians
///
/// Allows a device to perform recovery operations. Has short TTL and
/// requires threshold guardian consent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryCapability {
    /// Base capability reference
    pub capability: CapabilityRef,

    /// Device being recovered
    pub target_device: DeviceId,

    /// Guardians who issued this capability
    pub issuing_guardians: Vec<DeviceId>,

    /// Threshold of guardians required
    pub guardian_threshold: usize,

    /// Purpose of recovery (for audit trail)
    pub recovery_reason: String,
}

impl RecoveryCapability {
    /// Create a new recovery capability.
    ///
    /// # Arguments
    ///
    /// * `capability_id` - Unique identifier for the capability (obtain from RandomEffects for testability)
    /// * `target_device` - Device being recovered
    /// * `issuing_guardians` - Guardians issuing this capability
    /// * `guardian_threshold` - Number of guardians required
    /// * `expires_at` - Expiration time (typically short TTL like 1 hour)
    /// * `leaf_index` - Leaf index of recovering device
    /// * `epoch` - Current epoch
    /// * `signature` - Threshold signature from guardians
    ///
    /// Note: Callers should obtain capability_id from RandomEffects to maintain testability
    /// and consistency with the effect system architecture.
    pub fn new(
        capability_id: CapabilityId,
        target_device: DeviceId,
        issuing_guardians: Vec<DeviceId>,
        guardian_threshold: usize,
        expires_at: Timestamp,
        leaf_index: usize,
        epoch: u64,
        signature: CapabilitySignature,
    ) -> Self {
        let capability = CapabilityRef::new(
            capability_id,
            ResourceRef::recovery(leaf_index, epoch),
            expires_at,
            signature,
        );

        Self {
            capability,
            target_device,
            issuing_guardians,
            guardian_threshold,
            recovery_reason: "Device recovery".to_string(),
        }
    }

    /// Set the recovery reason
    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.recovery_reason = reason.into();
        self
    }

    /// Check if this recovery capability is valid
    pub fn is_valid(&self, current_time: Timestamp) -> bool {
        // Check expiration
        if self.capability.is_expired(current_time) {
            return false;
        }

        // Check guardian threshold
        if self.issuing_guardians.len() < self.guardian_threshold {
            return false;
        }

        true
    }

    /// Check if this capability has sufficient guardian consent
    pub fn has_guardian_quorum(&self) -> bool {
        self.issuing_guardians.len() >= self.guardian_threshold
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;

    #[test]
    #[allow(clippy::disallowed_methods)]
    fn test_recovery_capability_creation() {
        let target = DeviceId(uuid::Uuid::from_bytes([2u8; 16]));
        let guardians = vec![
            DeviceId(uuid::Uuid::from_bytes([3u8; 16])),
            DeviceId(uuid::Uuid::from_bytes([4u8; 16])),
        ];
        let sig =
            CapabilitySignature::new(vec![0u8; 64], DeviceId(uuid::Uuid::from_bytes([0u8; 16])));

        let recovery_cap = RecoveryCapability::new(
            CapabilityId::new(uuid::Uuid::new_v4()),
            target,
            guardians.clone(),
            2,
            10000,
            0,
            1,
            sig,
        );

        assert_eq!(recovery_cap.target_device, target);
        assert_eq!(recovery_cap.issuing_guardians.len(), 2);
        assert_eq!(recovery_cap.guardian_threshold, 2);
        assert!(recovery_cap.has_guardian_quorum());
    }

    #[test]
    #[allow(clippy::disallowed_methods)]
    fn test_recovery_capability_expiration() {
        let sig =
            CapabilitySignature::new(vec![0u8; 64], DeviceId(uuid::Uuid::from_bytes([0u8; 16])));
        let recovery_cap = RecoveryCapability::new(
            CapabilityId::new(uuid::Uuid::new_v4()),
            DeviceId(uuid::Uuid::from_bytes([5u8; 16])),
            vec![
                DeviceId(uuid::Uuid::from_bytes([6u8; 16])),
                DeviceId(uuid::Uuid::from_bytes([7u8; 16])),
            ],
            2,
            1000,
            0,
            1,
            sig,
        );

        assert!(recovery_cap.is_valid(500));
        assert!(!recovery_cap.is_valid(1500));
    }

    #[test]
    #[allow(clippy::disallowed_methods)]
    fn test_recovery_capability_insufficient_guardians() {
        let sig =
            CapabilitySignature::new(vec![0u8; 64], DeviceId(uuid::Uuid::from_bytes([0u8; 16])));
        let recovery_cap = RecoveryCapability::new(
            CapabilityId::new(uuid::Uuid::new_v4()),
            DeviceId(uuid::Uuid::from_bytes([8u8; 16])),
            vec![DeviceId(uuid::Uuid::from_bytes([9u8; 16]))], // Only 1 guardian
            2,                                                 // But need 2
            10000,
            0,
            1,
            sig,
        );

        assert!(!recovery_cap.has_guardian_quorum());
        assert!(!recovery_cap.is_valid(500)); // Invalid due to insufficient guardians
    }

    #[test]
    #[allow(clippy::disallowed_methods)]
    fn test_recovery_capability_with_reason() {
        let sig =
            CapabilitySignature::new(vec![0u8; 64], DeviceId(uuid::Uuid::from_bytes([0u8; 16])));
        let recovery_cap = RecoveryCapability::new(
            CapabilityId::new(uuid::Uuid::new_v4()),
            DeviceId(uuid::Uuid::from_bytes([10u8; 16])),
            vec![
                DeviceId(uuid::Uuid::from_bytes([11u8; 16])),
                DeviceId(uuid::Uuid::from_bytes([12u8; 16])),
            ],
            2,
            10000,
            0,
            1,
            sig,
        )
        .with_reason("Lost device, need to rekey");

        assert_eq!(recovery_cap.recovery_reason, "Lost device, need to rekey");
    }
}
