//! Shared types for guardian operations.

use aura_core::frost::ThresholdSignature;
use aura_core::{identifiers::GuardianId, AccountId, DeviceId, TrustLevel};
use serde::{Deserialize, Serialize};

/// Metadata describing a guardian that can participate in recovery.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GuardianProfile {
    /// Stable guardian identifier (journal namespace)
    pub guardian_id: GuardianId,
    /// Device that will receive recovery traffic
    pub device_id: DeviceId,
    /// Human readable label for operator UX
    pub label: String,
    /// Trust level attached to this guardian edge
    pub trust_level: TrustLevel,
    /// Cooldown (seconds) enforced between approvals from this guardian
    pub cooldown_secs: u64,
}

impl GuardianProfile {
    /// Helper constructor with default cooldown (15m) and High trust.
    pub fn new(guardian_id: GuardianId, device_id: DeviceId, label: impl Into<String>) -> Self {
        Self {
            guardian_id,
            device_id,
            label: label.into(),
            trust_level: TrustLevel::High,
            cooldown_secs: 900,
        }
    }
}

/// Collection wrapper to make it harder to misuse raw vectors.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GuardianSet {
    guardians: Vec<GuardianProfile>,
}

impl GuardianSet {
    /// Create from guardian profiles.
    pub fn new(guardians: Vec<GuardianProfile>) -> Self {
        Self { guardians }
    }

    /// Number of guardians.
    pub fn len(&self) -> usize {
        self.guardians.len()
    }

    /// Whether the set is empty.
    pub fn is_empty(&self) -> bool {
        self.guardians.is_empty()
    }

    /// Iterate over guardians.
    pub fn iter(&self) -> impl Iterator<Item = &GuardianProfile> {
        self.guardians.iter()
    }

    /// Lookup guardian by device id.
    pub fn by_device(&self, device_id: &DeviceId) -> Option<&GuardianProfile> {
        self.guardians
            .iter()
            .find(|guardian| &guardian.device_id == device_id)
    }

    /// Lookup guardian by identifier.
    pub fn by_guardian_id(&self, guardian_id: &GuardianId) -> Option<&GuardianProfile> {
        self.guardians
            .iter()
            .find(|guardian| &guardian.guardian_id == guardian_id)
    }

    /// Convert into inner vector.
    pub fn into_vec(self) -> Vec<GuardianProfile> {
        self.guardians
    }
}

impl<'a> IntoIterator for &'a GuardianSet {
    type Item = &'a GuardianProfile;
    type IntoIter = std::slice::Iter<'a, GuardianProfile>;

    fn into_iter(self) -> Self::IntoIter {
        self.guardians.iter()
    }
}

/// Evidence recorded after a successful guardian recovery flow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryEvidence {
    /// Account being recovered.
    pub account_id: AccountId,
    /// Device that initiated recovery.
    pub recovering_device: DeviceId,
    /// Guardians that approved.
    pub guardians: Vec<GuardianId>,
    /// Epoch-second timestamp when ceremony completed.
    pub issued_at: u64,
    /// When the guardians exiting cooldown may approve again.
    pub cooldown_expires_at: u64,
    /// Timestamp when the dispute window closes.
    pub dispute_window_ends_at: u64,
    /// Guardian metadata for auditing.
    pub guardian_profiles: Vec<GuardianProfile>,
    /// Disputes filed during the dispute window.
    pub disputes: Vec<RecoveryDispute>,
    /// Optional aggregate signature for audits.
    pub threshold_signature: Option<ThresholdSignature>,
}

/// Record produced per guardian approval.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecoveryShare {
    /// Guardian metadata.
    pub guardian: GuardianProfile,
    /// Encrypted key share.
    pub share: Vec<u8>,
    /// Guardian's partial signature over the recovery grant.
    pub partial_signature: Vec<u8>,
    /// Timestamp when share was produced.
    pub issued_at: u64,
}

/// Dispute filed by a guardian during the dispute window.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecoveryDispute {
    /// Guardian raising the dispute.
    pub guardian_id: GuardianId,
    /// Human-readable reason.
    pub reason: String,
    /// Timestamp when the dispute was filed.
    pub filed_at: u64,
}

/// Generic request for guardian operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryRequest {
    /// Device making the request
    pub requesting_device: DeviceId,
    /// Account being operated on
    pub account_id: AccountId,
    /// Recovery context and justification
    pub context: aura_authenticate::guardian_auth::RecoveryContext,
    /// Required threshold of guardian approvals
    pub threshold: usize,
    /// Available guardians for the operation
    pub guardians: GuardianSet,
}

/// Generic response for guardian operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryResponse {
    /// Whether the operation succeeded
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
    /// Recovered key material (for key recovery operations)
    pub key_material: Option<Vec<u8>>,
    /// Guardian shares collected
    pub guardian_shares: Vec<RecoveryShare>,
    /// Evidence of the operation
    pub evidence: RecoveryEvidence,
    /// Threshold signature
    pub signature: ThresholdSignature,
}
