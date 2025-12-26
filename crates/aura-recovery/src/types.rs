//! Shared types for guardian recovery operations.
//!
//! All types use the authority model - guardians are identified by `AuthorityId`,
//! not by device. Device information is obtained via commitment tree queries.

use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::threshold::ThresholdSignature;
use aura_core::TrustLevel;
use serde::{Deserialize, Serialize};

/// Guardian profile in the authority model.
///
/// Guardians are identified by their authority, not their device.
/// Device information is derived from the commitment tree at runtime.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GuardianProfile {
    /// Guardian's authority identifier
    pub authority_id: AuthorityId,
    /// Human readable label for operator UX
    pub label: Option<String>,
    /// Trust level attached to this guardian edge
    pub trust_level: TrustLevel,
    /// Cooldown (seconds) enforced between approvals from this guardian
    pub cooldown_secs: u64,
}

impl GuardianProfile {
    /// Create a new guardian profile with default settings.
    pub fn new(authority_id: AuthorityId) -> Self {
        Self {
            authority_id,
            label: None,
            trust_level: TrustLevel::High,
            cooldown_secs: 900, // 15 minutes default
        }
    }

    /// Create a guardian profile with a label.
    pub fn with_label(authority_id: AuthorityId, label: impl Into<String>) -> Self {
        Self {
            authority_id,
            label: Some(label.into()),
            trust_level: TrustLevel::High,
            cooldown_secs: 900,
        }
    }
}

/// Collection of guardian profiles.
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

    /// Lookup guardian by authority.
    pub fn by_authority(&self, authority_id: &AuthorityId) -> Option<&GuardianProfile> {
        self.guardians
            .iter()
            .find(|g| &g.authority_id == authority_id)
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

/// Recovery share produced by a guardian.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecoveryShare {
    /// Guardian's authority
    pub guardian_id: AuthorityId,
    /// Guardian's label (for display)
    pub guardian_label: Option<String>,
    /// Encrypted key share
    pub share: Vec<u8>,
    /// Guardian's partial signature over the recovery grant
    pub partial_signature: Vec<u8>,
    /// Timestamp when share was produced (ms since epoch)
    pub issued_at_ms: u64,
}

/// Dispute filed by a guardian during the dispute window.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecoveryDispute {
    /// Guardian raising the dispute
    pub guardian_id: AuthorityId,
    /// Human-readable reason
    pub reason: String,
    /// Timestamp when the dispute was filed (ms since epoch)
    pub filed_at_ms: u64,
}

/// Evidence of a completed recovery operation.
///
/// This can be derived from facts in the journal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryEvidence {
    /// Context in which recovery occurred
    pub context_id: ContextId,
    /// Account authority being recovered
    pub account_id: AuthorityId,
    /// Guardians that approved
    pub approving_guardians: Vec<AuthorityId>,
    /// Timestamp when ceremony completed (ms since epoch)
    pub completed_at_ms: u64,
    /// When the dispute window closes (ms since epoch)
    pub dispute_window_ends_at_ms: u64,
    /// Disputes filed during the dispute window
    pub disputes: Vec<RecoveryDispute>,
    /// Optional aggregate signature
    pub threshold_signature: Option<ThresholdSignature>,
}

impl Default for RecoveryEvidence {
    fn default() -> Self {
        Self {
            context_id: ContextId::new_from_entropy([0u8; 32]),
            account_id: AuthorityId::new_from_entropy([0u8; 32]),
            approving_guardians: Vec::new(),
            completed_at_ms: 0,
            dispute_window_ends_at_ms: 0,
            disputes: Vec::new(),
            threshold_signature: None,
        }
    }
}

/// Request to initiate a recovery operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryRequest {
    /// Authority initiating the request
    pub initiator_id: AuthorityId,
    /// Account authority being operated on
    pub account_id: AuthorityId,
    /// Recovery context and justification
    pub context: aura_authentication::RecoveryContext,
    /// Required threshold of guardian approvals
    pub threshold: usize,
    /// Available guardians for the operation
    pub guardians: GuardianSet,
}

/// Response from a recovery operation.
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

impl RecoveryResponse {
    /// Create a success response.
    pub fn success(
        key_material: Option<Vec<u8>>,
        shares: Vec<RecoveryShare>,
        evidence: RecoveryEvidence,
        signature: ThresholdSignature,
    ) -> Self {
        Self {
            success: true,
            error: None,
            key_material,
            guardian_shares: shares,
            evidence,
            signature,
        }
    }

    /// Create an error response.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            success: false,
            error: Some(message.into()),
            key_material: None,
            guardian_shares: Vec::new(),
            evidence: RecoveryEvidence::default(),
            // Empty signature for error case
            signature: ThresholdSignature::new(vec![0u8; 64], 0, Vec::new(), Vec::new(), 0),
        }
    }
}
