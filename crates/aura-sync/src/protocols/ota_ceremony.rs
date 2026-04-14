//! Consensus-Based OTA Hard Fork Activation Ceremony
//!
//! This module provides a safe, consensus-backed hard fork activation protocol
//! that ensures all devices coordinate upgrade activation at the same epoch.
//!
//! ## Problem Solved
//!
//! Without consensus, hard fork activation can diverge:
//! - Device A activates at epoch 100, rejects old protocol
//! - Device B hasn't received activation yet, still on old protocol
//! - Devices can no longer communicate until manual intervention
//!
//! ## Solution: Prestate-Bound Consensus
//!
//! 1. **Coordinator** proposes hard fork with activation epoch
//! 2. **Each device** validates readiness and declares commitment bound to prestate
//! 3. **Consensus** ensures M-of-N devices agree BEFORE any commits
//! 4. Only after consensus does any device commit the activation fact
//!
//! ## Session Type Guarantee
//!
//! The choreography enforces linear protocol flow:
//! ```text
//! Coordinator -> All: UpgradeProposal
//! Each Device -> Coordinator: ReadinessCommitment
//! [Consensus: M-of-N devices commit at same epoch]
//! choice {
//!     Coordinator -> All: CommitActivation
//! } or {
//!     Coordinator -> All: AbortActivation
//! }
//! ```
//!
//! ## Key Properties
//!
//! - **Atomicity**: All devices commit at same epoch or none do
//! - **No Orphaned Activations**: Partial activations are prevented
//! - **Deterministic ID**: `CeremonyId = H(prestate_hash || upgrade_hash || nonce)`
//! - **Epoch Fencing**: Hard forks enforce coordinated epoch boundaries

use aura_core::effects::JournalEffects;
use aura_core::threshold::{AgreementMode, ThresholdSignature};
use aura_core::types::Epoch;
use aura_core::{AuraError, AuraResult, AuthorityId, DeviceId, Hash32, SemanticVersion};
use aura_macros::tell;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use super::ota::{UpgradeKind, UpgradeProposal as OTAProposal};

mod facts;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod theorem_pack_tests;

pub use facts::{
    emit_ota_ceremony_aborted_fact, emit_ota_ceremony_committed_fact,
    emit_ota_ceremony_initiated_fact, emit_ota_commitment_received_fact,
    emit_ota_threshold_reached_fact,
};

// =============================================================================
// CEREMONY TYPES
// =============================================================================

/// Unique identifier for an OTA activation ceremony instance.
///
/// Derived from `H(prestate_hash, upgrade_hash, nonce)` to prevent
/// concurrent ceremonies for the same upgrade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OTACeremonyId(pub Hash32);

impl OTACeremonyId {
    /// Create a ceremony ID from constituent parts.
    ///
    /// The prestate hash ensures this ceremony can only proceed if the
    /// current system state matches expectations.
    pub fn new(prestate_hash: &Hash32, upgrade_hash: &Hash32, nonce: u64) -> Self {
        let mut input = Vec::with_capacity(32 + 32 + 8);
        input.extend_from_slice(prestate_hash.as_bytes());
        input.extend_from_slice(upgrade_hash.as_bytes());
        input.extend_from_slice(&nonce.to_le_bytes());
        Self(Hash32::from_bytes(&input))
    }

    /// Get the underlying hash.
    pub fn as_hash(&self) -> &Hash32 {
        &self.0
    }
}

/// An upgrade proposal for the ceremony.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpgradeProposal {
    /// Unique proposal ID
    pub proposal_id: Uuid,
    /// Package identifier
    pub package_id: Uuid,
    /// Target version
    pub version: SemanticVersion,
    /// Upgrade kind (must be HardFork for ceremony)
    pub kind: UpgradeKind,
    /// Package hash for verification
    pub package_hash: Hash32,
    /// Activation epoch (required for hard forks)
    pub activation_epoch: Epoch,
    /// Coordinator device
    pub coordinator: DeviceId,
}

impl UpgradeProposal {
    /// Create from an OTA protocol proposal.
    pub fn from_ota_proposal(
        ota: &OTAProposal,
        activation_epoch: Epoch,
        coordinator: DeviceId,
    ) -> AuraResult<Self> {
        // Parse version from string (format: "major.minor.patch")
        let version = Self::parse_version(&ota.version)?;

        Ok(Self {
            proposal_id: ota.proposal_id,
            package_id: ota.package_id,
            version,
            kind: ota.kind,
            package_hash: ota.package_hash,
            activation_epoch,
            coordinator,
        })
    }

    /// Parse a version string in "major.minor.patch" format.
    fn parse_version(version_str: &str) -> AuraResult<SemanticVersion> {
        let parts: Vec<&str> = version_str.split('.').collect();
        if parts.len() != 3 {
            return Err(AuraError::invalid(format!(
                "Invalid version format: {version_str}. Expected major.minor.patch"
            )));
        }

        let major = parts[0]
            .parse::<u16>()
            .map_err(|_| AuraError::invalid(format!("Invalid major version: {}", parts[0])))?;
        let minor = parts[1]
            .parse::<u16>()
            .map_err(|_| AuraError::invalid(format!("Invalid minor version: {}", parts[1])))?;
        let patch = parts[2]
            .parse::<u16>()
            .map_err(|_| AuraError::invalid(format!("Invalid patch version: {}", parts[2])))?;

        Ok(SemanticVersion::new(major, minor, patch))
    }

    /// Compute hash of the upgrade proposal.
    #[allow(clippy::expect_used)] // serde_json serialization of simple structs is infallible
    pub fn compute_hash(&self) -> Hash32 {
        let bytes = serde_json::to_vec(self).expect("UpgradeProposal should serialize");
        Hash32::from_bytes(&bytes)
    }
}

/// A device's commitment to an upgrade activation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadinessCommitment {
    /// The ceremony being committed to
    pub ceremony_id: OTACeremonyId,
    /// Device making the commitment
    pub device: DeviceId,
    /// Authority whose keys signed this commitment
    ///
    /// This is the device's authority - used for threshold signing.
    /// A device with 1-of-1 authority uses single-signer Ed25519.
    /// A device with m-of-n authority uses FROST threshold signing.
    pub authority: AuthorityId,
    /// Prestate hash at time of commitment
    pub prestate_hash: Hash32,
    /// Whether device is ready
    pub ready: bool,
    /// Reason if not ready
    pub reason: Option<String>,
    /// Threshold signature over the commitment
    ///
    /// Created via `ThresholdSigningEffects::sign()` with `SigningContext::ota_activation()`.
    /// This provides cryptographic proof that the device's authority approved the activation.
    pub signature: ThresholdSignature,
    /// Timestamp of commitment
    pub committed_at_ms: u64,
}

/// Current status of an OTA ceremony.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OTACeremonyStatus {
    /// Ceremony initiated, collecting readiness commitments
    CollectingCommitments,
    /// Threshold reached, awaiting final consensus
    AwaitingConsensus,
    /// Consensus reached, activation committed
    Committed,
    /// Ceremony aborted (insufficient readiness, timeout, or explicit abort)
    Aborted { reason: OTACeremonyAbortReason },
}

/// Typed terminal failure for OTA activation ceremonies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OTACeremonyAbortReason {
    /// Ceremony exceeded its configured deadline.
    TimedOut,
    /// Ceremony was manually cancelled.
    Manual { reason: String },
}

impl std::fmt::Display for OTACeremonyAbortReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OTACeremonyAbortReason::TimedOut => write!(f, "Ceremony timed out"),
            OTACeremonyAbortReason::Manual { reason } => write!(f, "{reason}"),
        }
    }
}

/// Typed commit payload for the OTA activation choreography.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OTACeremonyCommit {
    /// Ceremony being finalized.
    pub ceremony_id: OTACeremonyId,
    /// Activation epoch authorized by the ceremony.
    pub activation_epoch: Epoch,
    /// Ready devices that contributed to the threshold.
    pub ready_devices: Vec<DeviceId>,
}

/// Typed abort payload for the OTA activation choreography.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OTACeremonyAbort {
    /// Ceremony being terminated.
    pub ceremony_id: OTACeremonyId,
    /// Protocol-visible abort reason.
    pub reason: OTACeremonyAbortReason,
}

/// Evidence-bearing readiness witness produced when OTA threshold is reached.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OTAReadinessWitness {
    /// Ceremony whose readiness threshold was satisfied.
    pub ceremony_id: OTACeremonyId,
    /// Ready devices contributing to the threshold.
    pub ready_devices: Vec<DeviceId>,
    /// Number of ready devices observed when the witness was issued.
    pub ready_count: u32,
    /// Threshold required by the ceremony.
    pub threshold: u32,
}

/// Public readiness result for OTA commitment processing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum OTAReadinessOutcome {
    /// The ceremony is still collecting readiness commitments.
    Collecting,
    /// The readiness threshold has been met and witnessed.
    ThresholdReached(OTAReadinessWitness),
}

impl OTAReadinessOutcome {
    /// Whether the readiness threshold has been met.
    pub fn threshold_reached(&self) -> bool {
        matches!(self, Self::ThresholdReached(_))
    }
}

/// Full state of an OTA activation ceremony.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OTACeremonyState {
    /// Unique ceremony identifier
    pub ceremony_id: OTACeremonyId,
    /// The upgrade proposal
    pub proposal: UpgradeProposal,
    /// Current status
    pub status: OTACeremonyStatus,
    /// Agreement mode (A1/A2/A3)
    pub agreement_mode: AgreementMode,
    /// Readiness commitments by device
    pub commitments: HashMap<DeviceId, ReadinessCommitment>,
    /// Required threshold (M-of-N)
    pub threshold: u32,
    /// Total quorum size
    pub quorum_size: u32,
    /// Timestamp when ceremony started
    pub started_at_ms: u64,
    /// Timeout for ceremony completion (ms)
    pub timeout_ms: u64,
}

impl OTACeremonyState {
    /// Count ready devices.
    pub fn ready_count(&self) -> u32 {
        let ready_count = self.commitments.values().filter(|c| c.ready).count();
        ready_count as u32
    }

    /// Check if threshold is met.
    pub fn threshold_met(&self) -> bool {
        self.ready_count() >= self.threshold
    }

    /// Get ready devices.
    pub fn ready_devices(&self) -> Vec<DeviceId> {
        self.commitments
            .iter()
            .filter_map(|(id, c)| if c.ready { Some(*id) } else { None })
            .collect()
    }
}

// =============================================================================
// CEREMONY FACTS
// =============================================================================

/// Facts emitted during OTA ceremony lifecycle.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum OTACeremonyFact {
    /// Ceremony initiated by coordinator
    CeremonyInitiated {
        ceremony_id: String,
        /// Optional trace identifier for ceremony correlation
        #[serde(default, skip_serializing_if = "Option::is_none")]
        trace_id: Option<String>,
        proposal_id: String,
        package_id: String,
        version: String,
        activation_epoch: Epoch,
        coordinator: String,
        threshold: u32,
        quorum_size: u32,
        timestamp_ms: u64,
    },
    /// Device commitment received
    CommitmentReceived {
        ceremony_id: String,
        /// Optional trace identifier for ceremony correlation
        #[serde(default, skip_serializing_if = "Option::is_none")]
        trace_id: Option<String>,
        device: String,
        ready: bool,
        reason: Option<String>,
        timestamp_ms: u64,
    },
    /// Threshold reached
    ThresholdReached {
        ceremony_id: String,
        /// Optional trace identifier for ceremony correlation
        #[serde(default, skip_serializing_if = "Option::is_none")]
        trace_id: Option<String>,
        ready_count: u32,
        ready_devices: Vec<String>,
        timestamp_ms: u64,
    },
    /// Ceremony committed (activation will occur at epoch)
    CeremonyCommitted {
        ceremony_id: String,
        /// Optional trace identifier for ceremony correlation
        #[serde(default, skip_serializing_if = "Option::is_none")]
        trace_id: Option<String>,
        activation_epoch: Epoch,
        ready_devices: Vec<String>,
        threshold_signature: Vec<u8>,
        timestamp_ms: u64,
    },
    /// Ceremony aborted
    CeremonyAborted {
        ceremony_id: String,
        /// Optional trace identifier for ceremony correlation
        #[serde(default, skip_serializing_if = "Option::is_none")]
        trace_id: Option<String>,
        reason: String,
        timestamp_ms: u64,
    },
    /// Ceremony superseded by a newer ceremony
    ///
    /// Emitted when a new ceremony replaces an existing one. The old ceremony
    /// should stop processing immediately. Supersession propagates via anti-entropy.
    CeremonySuperseded {
        /// The ceremony being superseded (old ceremony)
        superseded_ceremony_id: String,
        /// The ceremony that supersedes it (new ceremony)
        superseding_ceremony_id: String,
        /// Reason for supersession (e.g., "prestate_stale", "newer_request", "timeout")
        reason: String,
        /// Optional trace identifier for ceremony correlation
        #[serde(default, skip_serializing_if = "Option::is_none")]
        trace_id: Option<String>,
        /// Timestamp in milliseconds
        timestamp_ms: u64,
    },
}

impl OTACeremonyFact {
    /// Get the ceremony ID from any fact variant (returns superseded ID for supersession facts).
    pub fn ceremony_id(&self) -> &str {
        match self {
            OTACeremonyFact::CeremonyInitiated { ceremony_id, .. } => ceremony_id,
            OTACeremonyFact::CommitmentReceived { ceremony_id, .. } => ceremony_id,
            OTACeremonyFact::ThresholdReached { ceremony_id, .. } => ceremony_id,
            OTACeremonyFact::CeremonyCommitted { ceremony_id, .. } => ceremony_id,
            OTACeremonyFact::CeremonyAborted { ceremony_id, .. } => ceremony_id,
            OTACeremonyFact::CeremonySuperseded {
                superseded_ceremony_id,
                ..
            } => superseded_ceremony_id,
        }
    }

    /// Get the timestamp from any fact variant.
    pub fn timestamp_ms(&self) -> u64 {
        match self {
            OTACeremonyFact::CeremonyInitiated { timestamp_ms, .. } => *timestamp_ms,
            OTACeremonyFact::CommitmentReceived { timestamp_ms, .. } => *timestamp_ms,
            OTACeremonyFact::ThresholdReached { timestamp_ms, .. } => *timestamp_ms,
            OTACeremonyFact::CeremonyCommitted { timestamp_ms, .. } => *timestamp_ms,
            OTACeremonyFact::CeremonyAborted { timestamp_ms, .. } => *timestamp_ms,
            OTACeremonyFact::CeremonySuperseded { timestamp_ms, .. } => *timestamp_ms,
        }
    }
}

// =============================================================================
// CEREMONY CONFIGURATION
// =============================================================================

/// Configuration for OTA ceremonies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OTACeremonyConfig {
    /// Required threshold (M in M-of-N)
    pub threshold: u32,
    /// Total quorum size (N in M-of-N)
    pub quorum_size: u32,
    /// Timeout for ceremony completion (ms)
    pub timeout_ms: u64,
    /// Minimum advance notice for activation epoch (epochs)
    pub min_activation_notice_epochs: u64,
}

impl Default for OTACeremonyConfig {
    fn default() -> Self {
        Self {
            threshold: 2,
            quorum_size: 3,
            timeout_ms: 24 * 60 * 60 * 1000, // 24 hours for hard fork coordination
            min_activation_notice_epochs: 100, // At least 100 epochs notice
        }
    }
}

// =============================================================================
// CHOREOGRAPHY DOCUMENTATION
// =============================================================================
//
// Protocol: OTAActivationCeremony
// Roles: Coordinator, Device[n]
//
// Flow:
// 1. Coordinator -> Device[*]: ProposeUpgrade(UpgradeProposal)
//    - guard_capability: "ota:propose"
//    - flow_cost: 1
//    - journal_facts: OTACeremonyFact::CeremonyInitiated
//
// 2. Device[*] -> Coordinator: ReadinessCommitment
//    - guard_capability: "ota:commit_readiness"
//    - flow_cost: 1
//    - journal_facts: OTACeremonyFact::CommitmentReceived
//
// 3. When threshold reached:
//    - journal_facts: OTACeremonyFact::ThresholdReached
//
// 4. Exclusive choice after consensus attempt:
//    choice {
//        Coordinator -> Device[*]: CommitActivation
//        - guard_capability: "ota:activate"
//        - flow_cost: 1
//        - journal_facts: OTACeremonyFact::CeremonyCommitted
//    } or {
//        Coordinator -> Device[*]: AbortActivation
//        - guard_capability: "ota:abort"
//        - flow_cost: 1
//        - journal_facts: OTACeremonyFact::CeremonyAborted

// OTA activation choreography protocol surface.
//
// This is the protocol-critical public contract for OTA readiness, commit, and
// abort sequencing. The executor migration in later phases should converge on
// this generated surface instead of preserving the handwritten state machine as
// a permanent parallel path.
mod ota_activation_protocol_surface {
    #![allow(unreachable_code)]

    use super::*;

    tell!(include_str!("src/protocols/ota_activation.tell"));
}

pub use ota_activation_protocol_surface::telltale_session_types_ota_activation;
//    }

// =============================================================================
// CEREMONY EXECUTOR
// =============================================================================

/// Compute the current OTA ceremony prestate hash from journal state.
pub async fn compute_ota_ceremony_prestate_hash<E>(effects: &E) -> AuraResult<Hash32>
where
    E: JournalEffects + ?Sized,
{
    let journal = effects.get_journal().await?;
    let journal_bytes =
        serde_json::to_vec(&journal.facts).map_err(|e| AuraError::serialization(e.to_string()))?;
    Ok(Hash32::from_bytes(&journal_bytes))
}

/// Create the aggregated readiness-signature bundle for OTA activation.
pub fn create_ota_activation_signature(
    ceremony_id: OTACeremonyId,
    commitments: &[ReadinessCommitment],
) -> AuraResult<Vec<u8>> {
    let ready_commitments: Vec<&ReadinessCommitment> = commitments
        .iter()
        .filter(|commitment| commitment.ready)
        .collect();

    if ready_commitments.is_empty() {
        return Err(AuraError::invalid(
            "No ready device signatures to aggregate",
        ));
    }

    let mut aggregated = Vec::new();
    aggregated.extend_from_slice(ceremony_id.0.as_bytes());
    aggregated.extend_from_slice(&(ready_commitments.len() as u16).to_le_bytes());

    for commitment in ready_commitments {
        aggregated.extend_from_slice(&commitment.authority.to_bytes());
        let sig_len = commitment.signature.signature.len() as u16;
        aggregated.extend_from_slice(&sig_len.to_le_bytes());
        aggregated.extend_from_slice(&commitment.signature.signature);
    }

    Ok(aggregated)
}
