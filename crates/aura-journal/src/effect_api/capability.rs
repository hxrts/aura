//! Capability System Types
//!
//! Implements capability-based authorization separate from tree membership (authentication).
//! Capabilities are time-limited, attenuatable tokens that grant specific permissions
//! for specific resources.

use super::journal_types::uuid_newtype;
use aura_core::types::identifiers::DeviceId;
use serde::{Deserialize, Serialize};

/// Import unified time types from aura-core
use aura_core::time::TimeStamp;

uuid_newtype!(CapabilityId, "cap-", "Unique identifier for a capability");

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
            uri: format!("journal://recovery/{leaf_index}#{epoch}"),
        }
    }

    /// Create a storage resource reference
    pub fn storage(path: &str) -> Self {
        Self {
            uri: format!("storage://{path}"),
        }
    }

    /// Create a relay resource reference
    pub fn relay(session_id: &str) -> Self {
        Self {
            uri: format!("relay://{session_id}"),
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

impl std::fmt::Display for ResourceRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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
/// Capabilities are issued as part of TreeOp records and can be revoked via retractions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityRef {
    /// Unique identifier for this capability
    pub id: CapabilityId,

    /// Resource this capability grants access to
    pub resource: ResourceRef,

    /// Expiration timestamp (Unix milliseconds)
    pub expires_at: TimeStamp,

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
        expires_at: TimeStamp,
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
    pub fn is_expired(&self, current_time: &TimeStamp) -> bool {
        use aura_core::time::{OrderingPolicy, TimeOrdering};
        matches!(
            current_time.compare(&self.expires_at, OrderingPolicy::DeterministicTieBreak),
            TimeOrdering::After
        )
    }

    /// Check if this capability is valid at the given time
    pub fn is_valid_at(&self, timestamp: &TimeStamp) -> bool {
        !self.is_expired(timestamp)
    }

    /// Get time until expiration (returns 0 if already expired)
    pub fn time_until_expiration(&self, current_time: &TimeStamp) -> Option<u64> {
        use aura_core::time::{OrderingPolicy, TimeOrdering};
        match (current_time, &self.expires_at) {
            (TimeStamp::PhysicalClock(now), TimeStamp::PhysicalClock(exp)) => {
                match current_time.compare(&self.expires_at, OrderingPolicy::DeterministicTieBreak)
                {
                    TimeOrdering::After | TimeOrdering::Concurrent => Some(0),
                    _ => Some(exp.ts_ms.saturating_sub(now.ts_ms)),
                }
            }
            // For non-physical clocks, return None to force callers to supply a physical time
            _ => None,
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
    pub restricted_expires_at: Option<TimeStamp>,

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
    pub fn with_expiration(mut self, expires_at: TimeStamp) -> Self {
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
    pub guardian_threshold: u32,

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
        guardian_threshold: u32,
        expires_at: TimeStamp,
        leaf_index: u32,
        epoch: u64,
        signature: CapabilitySignature,
    ) -> Self {
        let capability = CapabilityRef::new(
            capability_id,
            ResourceRef::recovery(leaf_index as usize, epoch),
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
    pub fn is_valid(&self, current_time: &TimeStamp) -> bool {
        // Check expiration
        if self.capability.is_expired(current_time) {
            return false;
        }

        // Check guardian threshold
        if self.issuing_guardians.len() < self.guardian_threshold as usize {
            return false;
        }

        true
    }

    /// Check if this capability has sufficient guardian consent
    pub fn has_guardian_quorum(&self) -> bool {
        self.issuing_guardians.len() >= self.guardian_threshold as usize
    }
}
