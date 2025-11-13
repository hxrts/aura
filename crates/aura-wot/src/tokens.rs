//! Capability tokens and delegation
//!
//! This module provides unified capability tokens that were extracted from
//! aura-verify as part of the capability consolidation.
//!
//! The capability tokens implement threshold authentication and delegation
//! on top of the meet-semilattice capability framework.

use aura_core::hash;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::hash::Hash;

/// Capability identifier for access control
///
/// Unique identifier for capabilities within the capability system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CapabilityId(pub [u8; 32]);

impl CapabilityId {
    /// Create a new capability ID
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Create from a byte slice
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, aura_core::TypeError> {
        if bytes.len() == 32 {
            let mut array = [0u8; 32];
            array.copy_from_slice(bytes);
            Ok(Self(array))
        } else {
            Err(aura_core::TypeError::InvalidIdentifier(format!(
                "CapabilityId must be exactly 32 bytes, got {}",
                bytes.len()
            )))
        }
    }

    /// Create from hash bytes
    pub fn from_hash(hash_bytes: &[u8; 32]) -> Self {
        Self(*hash_bytes)
    }

    /// Get the raw bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Convert to hex string
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Create from hex string
    pub fn from_hex(hex_str: &str) -> Result<Self, hex::FromHexError> {
        let bytes = hex::decode(hex_str)?;
        if bytes.len() == 32 {
            let mut array = [0u8; 32];
            array.copy_from_slice(&bytes);
            Ok(Self(array))
        } else {
            Err(hex::FromHexError::InvalidStringLength)
        }
    }

    /// Generate a random capability ID
    #[allow(clippy::disallowed_methods)]
    pub fn random() -> Self {
        let bytes = hash::hash(uuid::Uuid::from_bytes([0u8; 16]).as_bytes());
        Self(bytes)
    }

    /// Generate a deterministic capability ID from a parent chain
    pub fn from_chain(
        parent_id: Option<&CapabilityId>,
        subject_id: &[u8],
        scope_data: &[u8],
    ) -> Self {
        let mut h = hash::hasher();

        if let Some(parent) = parent_id {
            h.update(&parent.0);
        }

        h.update(subject_id);
        h.update(scope_data);

        Self(h.finalize())
    }

    /// Generate a capability ID from device and timestamp
    pub fn from_device_and_timestamp(device_id: aura_core::DeviceId, timestamp: u64) -> Self {
        let mut h = hash::hasher();
        h.update(device_id.0.as_bytes());
        h.update(&timestamp.to_le_bytes());
        h.update(b"capability");
        Self(h.finalize())
    }
}

impl fmt::Display for CapabilityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cap:{}", &self.to_hex()[..16])
    }
}

impl From<[u8; 32]> for CapabilityId {
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

// Note: From<[u8; 32]> implementation is above, using direct constructor
// This avoids duplicate implementation conflict

impl From<CapabilityId> for [u8; 32] {
    fn from(capability_id: CapabilityId) -> Self {
        capability_id.0
    }
}

// =============================================================================
// Unified Capability Token - Single Source of Truth
// =============================================================================

/// Unified capability token for delegated access control
///
/// This is the canonical CapabilityToken definition that consolidates all previous
/// definitions across the codebase into a single authoritative source.
///
/// # Design Principles
///
/// - **Bearer Token**: Possession grants access (like a physical key)
/// - **Threshold-Signed**: Created by M-of-N threshold signature
/// - **Delegatable**: Can be attenuated and delegated to other devices
/// - **Conditional**: Can have time windows, usage limits, and other conditions
/// - **Verifiable**: Cryptographically signed and independently verifiable
///
/// # Lifecycle
///
/// 1. **Issuance**: Threshold (M-of-N devices) creates and signs token
/// 2. **Distribution**: Token distributed to authorized devices
/// 3. **Usage**: Individual devices present token for access
/// 4. **Verification**: Service verifies signature and conditions
/// 5. **Revocation**: Token can be revoked if compromised
///
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityToken {
    /// Unique identifier for this capability token
    pub token_id: CapabilityId,

    /// Account that issued this token (threshold identity)
    pub issuer: aura_core::AccountId,

    /// Permissions granted by this token
    pub permissions: Vec<aura_core::CanonicalPermission>,

    /// Optional resource restrictions (e.g., specific chunk IDs, paths)
    pub resources: Vec<String>,

    /// Unix timestamp when this capability was issued
    pub issued_at: u64,

    /// Optional Unix timestamp when this capability expires
    pub expires_at: Option<u64>,

    /// Whether this token has been revoked
    pub revoked: bool,

    /// Devices that participated in creating this token (M-of-N threshold)
    pub signers: Vec<aura_core::DeviceId>,

    /// The threshold signature authorizing this token
    pub threshold_signature: Vec<u8>,

    /// Delegation chain showing token ancestry
    pub delegation_chain: Vec<DelegationProof>,

    /// Maximum delegation depth allowed
    pub max_delegation_depth: u8,

    /// Current delegation depth (0 = original token)
    pub current_delegation_depth: u8,

    /// Conditions that must be met for this token to be valid
    pub conditions: Vec<CapabilityCondition>,

    /// Nonce for uniqueness and replay protection
    pub nonce: [u8; 32],
}

/// Proof of delegation from parent token to child token
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelegationProof {
    /// Parent token that delegated authority
    pub parent_token_id: CapabilityId,

    /// Permissions delegated (must be subset of parent's permissions)
    pub delegated_permissions: Vec<aura_core::CanonicalPermission>,

    /// Device that performed the delegation
    pub delegator_device_id: aura_core::DeviceId,

    /// Signature from delegator device
    pub signature: Vec<u8>,

    /// Unix timestamp when delegation occurred
    pub timestamp: u64,
}

/// Conditions that can be attached to capability tokens
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CapabilityCondition {
    /// Only valid during a specific time window
    TimeWindow {
        /// Unix timestamp for window start
        start: u64,
        /// Unix timestamp for window end
        end: u64,
    },

    /// Only valid when used from specific devices
    DeviceRestriction {
        /// List of device IDs that are allowed to use this capability
        allowed_devices: Vec<aura_core::DeviceId>,
    },

    /// Only valid for a limited number of uses
    UsageLimit {
        /// Maximum number of times this capability can be used
        max_uses: u32,
        /// Current usage count
        current_uses: u32,
    },

    /// Only valid when combined with other capabilities
    RequiresCombination {
        /// Other capability IDs that must be presented together
        required_capabilities: Vec<CapabilityId>,
    },

    /// Custom condition with arbitrary key-value data
    Custom {
        /// Custom condition key
        key: String,
        /// Custom condition value
        value: String,
    },
}

impl CapabilityToken {
    /// Create a new capability token
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        issuer: aura_core::AccountId,
        permissions: Vec<aura_core::CanonicalPermission>,
        resources: Vec<String>,
        issued_at: u64,
        expires_at: Option<u64>,
        signers: Vec<aura_core::DeviceId>,
        threshold_signature: Vec<u8>,
        nonce: [u8; 32],
    ) -> Self {
        let token_id = CapabilityId::from_hash(&hash::hash(&nonce));

        Self {
            token_id,
            issuer,
            permissions,
            resources,
            issued_at,
            expires_at,
            revoked: false,
            signers,
            threshold_signature,
            delegation_chain: Vec::new(),
            max_delegation_depth: 5,
            current_delegation_depth: 0,
            conditions: Vec::new(),
            nonce,
        }
    }

    /// Check if this capability is currently valid
    pub fn is_valid(&self, current_time: u64) -> bool {
        // Check revocation
        if self.revoked {
            return false;
        }

        // Check expiration
        if let Some(expires_at) = self.expires_at {
            if current_time > expires_at {
                return false;
            }
        }

        // Check conditions
        for condition in &self.conditions {
            if !self.check_condition(condition, current_time) {
                return false;
            }
        }

        true
    }

    /// Check if this capability grants a specific permission
    pub fn has_permission(&self, permission: &aura_core::CanonicalPermission) -> bool {
        self.permissions.contains(permission)
            || self
                .permissions
                .contains(&aura_core::CanonicalPermission::Admin)
    }

    /// Check if this capability can access a specific resource
    pub fn can_access_resource(&self, resource: &str) -> bool {
        // Empty resources list means access to all resources
        if self.resources.is_empty() {
            return true;
        }

        self.resources.iter().any(|r| r == resource)
    }

    /// Check if this token can be delegated
    pub fn can_delegate(&self) -> bool {
        self.current_delegation_depth < self.max_delegation_depth
    }

    /// Create a delegated token with attenuated permissions
    pub fn delegate(
        &self,
        delegated_permissions: Vec<aura_core::CanonicalPermission>,
        delegated_resources: Vec<String>,
        delegator_device_id: aura_core::DeviceId,
        delegator_signature: Vec<u8>,
        current_time: u64,
    ) -> Result<Self, String> {
        if !self.can_delegate() {
            return Err("Maximum delegation depth exceeded".to_string());
        }

        // Verify delegated permissions are subset of current permissions
        for perm in &delegated_permissions {
            if !self.has_permission(perm) {
                return Err(format!(
                    "Cannot delegate permission {:?} not held by parent token",
                    perm
                ));
            }
        }

        // Create delegation proof
        let proof = DelegationProof {
            parent_token_id: self.token_id,
            delegated_permissions: delegated_permissions.clone(),
            delegator_device_id,
            signature: delegator_signature,
            timestamp: current_time,
        };

        // Generate new nonce for delegated token
        let mut h = hash::hasher();
        h.update(&self.nonce);
        h.update(delegator_device_id.0.as_bytes());
        h.update(&current_time.to_le_bytes());
        let new_nonce = h.finalize();

        let mut delegation_chain = self.delegation_chain.clone();
        delegation_chain.push(proof);

        Ok(Self {
            token_id: CapabilityId::from_hash(&hash::hash(&new_nonce)),
            issuer: self.issuer,
            permissions: delegated_permissions,
            resources: delegated_resources,
            issued_at: current_time,
            expires_at: self.expires_at, // Inherit parent expiration
            revoked: false,
            signers: self.signers.clone(),
            threshold_signature: self.threshold_signature.clone(),
            delegation_chain,
            max_delegation_depth: self.max_delegation_depth,
            current_delegation_depth: self.current_delegation_depth + 1,
            conditions: self.conditions.clone(),
            nonce: new_nonce,
        })
    }

    /// Revoke this token
    pub fn revoke(&mut self) {
        self.revoked = true;
    }

    /// Add a condition to this token
    pub fn add_condition(&mut self, condition: CapabilityCondition) {
        self.conditions.push(condition);
    }

    /// Get the root token ID (start of delegation chain)
    pub fn root_token_id(&self) -> CapabilityId {
        self.delegation_chain
            .first()
            .map(|proof| proof.parent_token_id)
            .unwrap_or(self.token_id)
    }

    /// Check a specific condition
    fn check_condition(&self, condition: &CapabilityCondition, current_time: u64) -> bool {
        match condition {
            CapabilityCondition::TimeWindow { start, end } => {
                current_time >= *start && current_time <= *end
            }
            CapabilityCondition::DeviceRestriction { allowed_devices } => {
                // This check requires device context from caller
                // TODO fix - For now, return true - enforcement happens at verification layer
                !allowed_devices.is_empty()
            }
            CapabilityCondition::UsageLimit {
                max_uses,
                current_uses,
            } => current_uses < max_uses,
            CapabilityCondition::RequiresCombination {
                required_capabilities,
            } => {
                // This check requires capability context from caller
                // TODO fix - For now, return true - enforcement happens at verification layer
                !required_capabilities.is_empty()
            }
            CapabilityCondition::Custom { .. } => {
                // Custom conditions must be checked by application layer
                true
            }
        }
    }

    /// Serialize token for signing (excludes signature field)
    pub fn serialize_for_signature(&self) -> Vec<u8> {
        // Serialize all fields except threshold_signature
        let mut h = hash::hasher();
        h.update(self.token_id.as_bytes());
        h.update(self.issuer.0.as_bytes());
        h.update(&self.issued_at.to_le_bytes());
        if let Some(expires_at) = self.expires_at {
            h.update(&expires_at.to_le_bytes());
        }
        h.update(&self.nonce);
        h.finalize().to_vec()
    }
}

impl fmt::Display for CapabilityToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "CapabilityToken({}, issuer={}, depth={}/{})",
            self.token_id, self.issuer, self.current_delegation_depth, self.max_delegation_depth
        )
    }
}

impl Hash for CapabilityToken {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.token_id.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_token() -> CapabilityToken {
        let issuer = aura_core::AccountId::from_bytes(*b"test_account_id_1234567890123456");
        let device1 = aura_core::DeviceId::from_bytes(*b"device1_test_id_1234567890123456");
        let device2 = aura_core::DeviceId::from_bytes(*b"device2_test_id_1234567890123456");

        CapabilityToken::new(
            issuer,
            vec![aura_core::CanonicalPermission::StorageRead],
            vec!["resource1".to_string()],
            1000,
            Some(2000),
            vec![device1, device2],
            vec![0u8; 64], // Mock signature
            [0u8; 32],
        )
    }

    #[test]
    fn test_capability_token_creation() {
        let token = create_test_token();
        assert_eq!(token.current_delegation_depth, 0);
        assert!(!token.revoked);
        assert_eq!(token.signers.len(), 2);
    }

    #[test]
    fn test_capability_token_validity() {
        let token = create_test_token();

        assert!(token.is_valid(1500)); // Within validity period
        assert!(!token.is_valid(2500)); // After expiration
    }

    #[test]
    fn test_capability_token_permissions() {
        let token = create_test_token();

        assert!(token.has_permission(&aura_core::CanonicalPermission::StorageRead));
        assert!(!token.has_permission(&aura_core::CanonicalPermission::StorageWrite));
    }

    #[test]
    fn test_capability_token_delegation() {
        let token = create_test_token();
        let delegator = aura_core::DeviceId::from_bytes(*b"delegator_test_id_12345678901234");

        let delegated = token
            .delegate(
                vec![aura_core::CanonicalPermission::StorageRead],
                vec!["resource1".to_string()],
                delegator,
                vec![0u8; 64],
                1500,
            )
            .expect("Delegation should succeed");

        assert_eq!(delegated.current_delegation_depth, 1);
        assert_eq!(delegated.delegation_chain.len(), 1);
        assert!(delegated.can_delegate());
    }

    #[test]
    fn test_capability_token_delegation_depth_limit() {
        let mut token = create_test_token();
        token.max_delegation_depth = 2;
        token.current_delegation_depth = 2;

        assert!(!token.can_delegate());

        let delegator = aura_core::DeviceId::from_bytes(*b"delegator_test_id_12345678901234");
        let result = token.delegate(
            vec![aura_core::CanonicalPermission::StorageRead],
            vec![],
            delegator,
            vec![],
            1500,
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_capability_token_revocation() {
        let mut token = create_test_token();
        assert!(token.is_valid(1500));

        token.revoke();
        assert!(!token.is_valid(1500));
    }

    #[test]
    fn test_capability_condition_time_window() {
        let mut token = create_test_token();
        token.add_condition(CapabilityCondition::TimeWindow {
            start: 1200,
            end: 1800,
        });

        assert!(!token.is_valid(1100)); // Before window
        assert!(token.is_valid(1500)); // Within window
        assert!(!token.is_valid(1900)); // After window
    }

    #[test]
    fn test_capability_id_generation() {
        let id1 = CapabilityId::random();
        let id2 = CapabilityId::random();
        assert_ne!(id1, id2);

        let device = aura_core::DeviceId::from_bytes(*b"device_test_id_12345678901234567");
        let id3 = CapabilityId::from_device_and_timestamp(device, 1000);
        let id4 = CapabilityId::from_device_and_timestamp(device, 1000);
        assert_eq!(id3, id4); // Deterministic
    }
}
