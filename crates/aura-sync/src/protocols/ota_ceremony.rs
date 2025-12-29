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

use aura_core::domain::FactValue;
use aura_core::effects::{JournalEffects, PhysicalTimeEffects, ThresholdSigningEffects};
use aura_core::threshold::{
    policy_for, AgreementMode, CeremonyFlow, SigningContext, ThresholdSignature,
};
use aura_core::{AuraError, AuraResult, AuthorityId, DeviceId, Hash32, SemanticVersion};
use aura_core::types::Epoch;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use super::ota::{UpgradeKind, UpgradeProposal as OTAProposal};

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
                "Invalid version format: {}. Expected major.minor.patch",
                version_str
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
    Aborted { reason: String },
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
    pub threshold: usize,
    /// Total quorum size
    pub quorum_size: usize,
    /// Timestamp when ceremony started
    pub started_at_ms: u64,
    /// Timeout for ceremony completion (ms)
    pub timeout_ms: u64,
}

impl OTACeremonyState {
    /// Count ready devices.
    pub fn ready_count(&self) -> usize {
        self.commitments.values().filter(|c| c.ready).count()
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
        threshold: usize,
        quorum_size: usize,
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
        ready_count: usize,
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
}

impl OTACeremonyFact {
    /// Get the ceremony ID from any fact variant.
    pub fn ceremony_id(&self) -> &str {
        match self {
            OTACeremonyFact::CeremonyInitiated { ceremony_id, .. } => ceremony_id,
            OTACeremonyFact::CommitmentReceived { ceremony_id, .. } => ceremony_id,
            OTACeremonyFact::ThresholdReached { ceremony_id, .. } => ceremony_id,
            OTACeremonyFact::CeremonyCommitted { ceremony_id, .. } => ceremony_id,
            OTACeremonyFact::CeremonyAborted { ceremony_id, .. } => ceremony_id,
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
    pub threshold: usize,
    /// Total quorum size (N in M-of-N)
    pub quorum_size: usize,
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
//    }

// =============================================================================
// CEREMONY EXECUTOR
// =============================================================================

/// Executes OTA activation ceremonies with consensus guarantees.
///
/// The executor manages the lifecycle of hard fork activation ceremonies,
/// ensuring atomicity through prestate binding and M-of-N consensus.
pub struct OTACeremonyExecutor<E: OTACeremonyEffects> {
    /// Effect system for all operations
    effects: E,
    /// Configuration
    config: OTACeremonyConfig,
    /// Active ceremonies by ID
    ceremonies: HashMap<OTACeremonyId, OTACeremonyState>,
}

/// Combined effects required for OTA ceremonies.
pub trait OTACeremonyEffects:
    JournalEffects + PhysicalTimeEffects + ThresholdSigningEffects + Send + Sync
{
}

// Blanket implementation
impl<T> OTACeremonyEffects for T where
    T: JournalEffects + PhysicalTimeEffects + ThresholdSigningEffects + Send + Sync
{
}

impl<E: OTACeremonyEffects> OTACeremonyExecutor<E> {
    /// Create a new ceremony executor.
    pub fn new(effects: E, config: OTACeremonyConfig) -> Self {
        Self {
            effects,
            config,
            ceremonies: HashMap::new(),
        }
    }

    /// Create with default configuration.
    pub fn with_defaults(effects: E) -> Self {
        Self::new(effects, OTACeremonyConfig::default())
    }

    // =========================================================================
    // CEREMONY LIFECYCLE - COORDINATOR SIDE
    // =========================================================================

    /// Initiate a new OTA activation ceremony.
    ///
    /// Only for hard forks - soft forks don't need ceremony coordination.
    pub async fn initiate_ceremony(
        &mut self,
        proposal: UpgradeProposal,
        current_epoch: Epoch,
    ) -> AuraResult<OTACeremonyId> {
        // Verify this is a hard fork
        if proposal.kind != UpgradeKind::HardFork {
            return Err(AuraError::invalid(
                "OTA ceremony only required for hard forks",
            ));
        }

        // Verify activation epoch has sufficient notice
        if proposal.activation_epoch.value()
            < current_epoch.value() + self.config.min_activation_notice_epochs
        {
            return Err(AuraError::invalid(format!(
                "Activation epoch {} too soon. Current: {}, minimum notice: {} epochs",
                proposal.activation_epoch, current_epoch, self.config.min_activation_notice_epochs
            )));
        }

        // Get current prestate
        let prestate_hash = self.compute_prestate_hash().await?;

        // Compute upgrade hash
        let upgrade_hash = proposal.compute_hash();

        // Generate nonce from current time
        let nonce = self
            .effects
            .physical_time()
            .await
            .map_err(|e| AuraError::internal(format!("Time error: {}", e)))?
            .ts_ms;

        // Create ceremony ID
        let ceremony_id = OTACeremonyId::new(&prestate_hash, &upgrade_hash, nonce);

        // Create ceremony state
        let state = OTACeremonyState {
            ceremony_id,
            proposal: proposal.clone(),
            status: OTACeremonyStatus::CollectingCommitments,
            agreement_mode: policy_for(CeremonyFlow::OtaActivation).initial_mode(),
            commitments: HashMap::new(),
            threshold: self.config.threshold,
            quorum_size: self.config.quorum_size,
            started_at_ms: nonce,
            timeout_ms: self.config.timeout_ms,
        };

        // Store ceremony
        self.ceremonies.insert(ceremony_id, state);

        // Emit ceremony initiated fact
        self.emit_ceremony_initiated_fact(ceremony_id, &proposal)
            .await?;

        Ok(ceremony_id)
    }

    /// Process a readiness commitment from a device.
    pub async fn process_commitment(
        &mut self,
        ceremony_id: OTACeremonyId,
        commitment: ReadinessCommitment,
    ) -> AuraResult<bool> {
        // Get current prestate and time before borrowing
        let current_prestate = self.compute_prestate_hash().await?;
        let now = self
            .effects
            .physical_time()
            .await
            .map_err(|e| AuraError::internal(format!("Time error: {}", e)))?
            .ts_ms;

        // Track if we need to emit threshold reached
        let threshold_reached;

        // Process commitment in a block to limit borrow scope
        {
            let ceremony = self
                .ceremonies
                .get_mut(&ceremony_id)
                .ok_or_else(|| AuraError::not_found("Ceremony not found"))?;

            // Verify ceremony is collecting commitments
            if ceremony.status != OTACeremonyStatus::CollectingCommitments {
                return Err(AuraError::invalid(format!(
                    "Ceremony not collecting commitments: {:?}",
                    ceremony.status
                )));
            }

            // Verify prestate matches
            if commitment.prestate_hash != current_prestate {
                return Err(AuraError::invalid(
                    "Prestate hash mismatch - state has changed since commitment was created",
                ));
            }

            // Verify ceremony ID matches
            if commitment.ceremony_id != ceremony_id {
                return Err(AuraError::invalid("Commitment is for different ceremony"));
            }

            // Check timeout
            if now > ceremony.started_at_ms + ceremony.timeout_ms {
                ceremony.status = OTACeremonyStatus::Aborted {
                    reason: "Ceremony timed out".to_string(),
                };
                return Ok(false);
            }

            // Check for duplicate commitment
            if ceremony.commitments.contains_key(&commitment.device) {
                return Err(AuraError::invalid("Device has already committed"));
            }

            // Store commitment
            ceremony
                .commitments
                .insert(commitment.device, commitment.clone());

            // Check if threshold is now met
            threshold_reached = ceremony.threshold_met()
                && ceremony.status == OTACeremonyStatus::CollectingCommitments;

            if threshold_reached {
                ceremony.status = OTACeremonyStatus::AwaitingConsensus;
                ceremony.agreement_mode = AgreementMode::CoordinatorSoftSafe;
            }
        }

        // Emit commitment received fact
        self.emit_commitment_received_fact(ceremony_id, &commitment)
            .await?;

        // Emit threshold reached fact if applicable
        if threshold_reached {
            self.emit_threshold_reached_fact(ceremony_id).await?;
        }

        Ok(threshold_reached)
    }

    /// Commit the ceremony after consensus.
    ///
    /// This is the final step - all devices will activate at the specified epoch.
    pub async fn commit_ceremony(&mut self, ceremony_id: OTACeremonyId) -> AuraResult<Epoch> {
        // Get ceremony info before mutable borrow
        let (activation_epoch, ready_devices) = {
            let ceremony = self
                .ceremonies
                .get(&ceremony_id)
                .ok_or_else(|| AuraError::not_found("Ceremony not found"))?;

            // Verify ceremony is awaiting consensus
            if ceremony.status != OTACeremonyStatus::AwaitingConsensus {
                return Err(AuraError::invalid(format!(
                    "Ceremony not awaiting consensus: {:?}",
                    ceremony.status
                )));
            }

            // Verify threshold is still met
            if !ceremony.threshold_met() {
                return Err(AuraError::invalid("Threshold no longer met"));
            }

            (ceremony.proposal.activation_epoch, ceremony.ready_devices())
        };

        // Update status
        if let Some(ceremony) = self.ceremonies.get_mut(&ceremony_id) {
            ceremony.status = OTACeremonyStatus::Committed;
            ceremony.agreement_mode = AgreementMode::ConsensusFinalized;
        }

        // NOTE: True FROST threshold signing requires architectural changes:
        // - OTA ceremony currently tracks DeviceId, but signing requires AuthorityId
        // - Each device would need to create a FROST signature share, not just agree
        // - A coordinator would aggregate shares into a single FROST signature
        // For now, we bundle individual device signatures as proof of M-of-N agreement.
        // This provides cryptographic proof that each device approved, just not aggregated.
        let threshold_signature = self.create_activation_signature(ceremony_id).await?;

        // Emit committed fact
        self.emit_ceremony_committed_fact(
            ceremony_id,
            activation_epoch,
            &ready_devices,
            &threshold_signature,
        )
        .await?;

        // Remove terminal ceremony state to prevent unbounded growth.
        self.ceremonies.remove(&ceremony_id);

        Ok(activation_epoch)
    }

    /// Abort the ceremony.
    pub async fn abort_ceremony(
        &mut self,
        ceremony_id: OTACeremonyId,
        reason: &str,
    ) -> AuraResult<()> {
        let ceremony = self
            .ceremonies
            .get_mut(&ceremony_id)
            .ok_or_else(|| AuraError::not_found("Ceremony not found"))?;

        // Can abort from any non-terminal state
        match &ceremony.status {
            OTACeremonyStatus::Committed => {
                return Err(AuraError::invalid("Cannot abort committed ceremony"));
            }
            OTACeremonyStatus::Aborted { .. } => {
                return Ok(()); // Already aborted, idempotent
            }
            _ => {}
        }

        ceremony.status = OTACeremonyStatus::Aborted {
            reason: reason.to_string(),
        };

        // Emit aborted fact
        self.emit_ceremony_aborted_fact(ceremony_id, reason).await?;

        // Remove terminal ceremony state to prevent unbounded growth.
        self.ceremonies.remove(&ceremony_id);

        Ok(())
    }

    // =========================================================================
    // CEREMONY LIFECYCLE - DEVICE SIDE
    // =========================================================================

    /// Create a readiness commitment for a ceremony.
    ///
    /// This creates a cryptographically signed commitment using the device's authority keys.
    /// For 1-of-1 authorities, this uses single-signer Ed25519.
    /// For m-of-n authorities, this uses FROST threshold signing.
    pub async fn create_readiness_commitment(
        &self,
        ceremony_id: OTACeremonyId,
        device: DeviceId,
        authority: AuthorityId,
        ready: bool,
        reason: Option<String>,
    ) -> AuraResult<ReadinessCommitment> {
        // Get current prestate
        let prestate_hash = self.compute_prestate_hash().await?;
        let committed_at_ms = self
            .effects
            .physical_time()
            .await
            .map_err(|e| AuraError::internal(format!("Time error: {}", e)))?
            .ts_ms;

        // Get the upgrade hash from the ceremony
        let ceremony = self
            .ceremonies
            .get(&ceremony_id)
            .ok_or_else(|| AuraError::not_found("Ceremony not found"))?;

        let upgrade_hash = ceremony.proposal.compute_hash();

        // Create signing context for OTA activation
        let signing_context = SigningContext::ota_activation(
            authority,
            ceremony_id.0 .0, // [u8; 32] from OTACeremonyId
            upgrade_hash.0,   // [u8; 32] from Hash32
            prestate_hash.0,  // [u8; 32] from Hash32
            ceremony.proposal.activation_epoch,
            ready,
        );

        // Sign using threshold signing effects
        // This automatically uses FROST for m-of-n or Ed25519 for 1-of-1
        let signature = self.effects.sign(signing_context).await?;

        Ok(ReadinessCommitment {
            ceremony_id,
            device,
            authority,
            prestate_hash,
            ready,
            reason,
            signature,
            committed_at_ms,
        })
    }

    // =========================================================================
    // INTERNAL HELPERS
    // =========================================================================

    /// Compute current prestate hash from journal state.
    async fn compute_prestate_hash(&self) -> AuraResult<Hash32> {
        let journal = self.effects.get_journal().await?;
        let journal_bytes = serde_json::to_vec(&journal.facts)
            .map_err(|e| AuraError::serialization(e.to_string()))?;
        Ok(Hash32::from_bytes(&journal_bytes))
    }

    /// Create aggregated signature bundle for activation.
    ///
    /// This bundles the threshold signatures from all ready devices, providing
    /// cryptographic proof of M-of-N device consensus for the hard fork activation.
    ///
    /// ## Signature Structure
    ///
    /// Each device's `ReadinessCommitment` contains a `ThresholdSignature` created via
    /// `ThresholdSigningEffects::sign()` with `SigningContext::ota_activation()`.
    ///
    /// - For 1-of-1 authorities: Single Ed25519 signature over the commitment
    /// - For m-of-n authorities: FROST aggregated signature from device's threshold setup
    ///
    /// The activation bundle aggregates these individual authority signatures into a
    /// single verifiable proof that M-of-N devices approved the hard fork.
    ///
    /// ## Bundle Format
    ///
    /// ```text
    /// ceremony_id (32 bytes) || device_count (2 bytes) || [
    ///     authority_id (32 bytes) || signature_len (2 bytes) || signature_bytes...
    /// ] for each ready device
    /// ```
    async fn create_activation_signature(&self, ceremony_id: OTACeremonyId) -> AuraResult<Vec<u8>> {
        let ceremony = self
            .ceremonies
            .get(&ceremony_id)
            .ok_or_else(|| AuraError::not_found("Ceremony not found"))?;

        // Collect ready commitments with their signatures
        let ready_commitments: Vec<&ReadinessCommitment> =
            ceremony.commitments.values().filter(|c| c.ready).collect();

        if ready_commitments.is_empty() {
            return Err(AuraError::invalid(
                "No ready device signatures to aggregate",
            ));
        }

        // Serialize the aggregated signature bundle with authority info for verification:
        // - ceremony_id (32 bytes)
        // - device_count (2 bytes)
        // - For each device:
        //   - authority_id (32 bytes)
        //   - signature_len (2 bytes)
        //   - signature_bytes
        let mut aggregated = Vec::new();
        aggregated.extend_from_slice(ceremony_id.0.as_bytes());
        aggregated.extend_from_slice(&(ready_commitments.len() as u16).to_le_bytes());

        for commitment in ready_commitments {
            // Include authority ID so verifiers know which public key to use
            aggregated.extend_from_slice(&commitment.authority.to_bytes());
            // Include signature with length prefix
            let sig_len = commitment.signature.signature.len() as u16;
            aggregated.extend_from_slice(&sig_len.to_le_bytes());
            aggregated.extend_from_slice(&commitment.signature.signature);
        }

        Ok(aggregated)
    }

    /// Emit ceremony initiated fact.
    async fn emit_ceremony_initiated_fact(
        &self,
        ceremony_id: OTACeremonyId,
        proposal: &UpgradeProposal,
    ) -> AuraResult<()> {
        let timestamp_ms = self
            .effects
            .physical_time()
            .await
            .map_err(|e| AuraError::internal(format!("Time error: {}", e)))?
            .ts_ms;
        let ceremony_id_hex = hex::encode(ceremony_id.0.as_bytes());
        let fact = OTACeremonyFact::CeremonyInitiated {
            ceremony_id: ceremony_id_hex.clone(),
            trace_id: Some(ceremony_id_hex.clone()),
            proposal_id: proposal.proposal_id.to_string(),
            package_id: proposal.package_id.to_string(),
            version: proposal.version.to_string(),
            activation_epoch: proposal.activation_epoch,
            coordinator: hex::encode(proposal.coordinator.0.as_bytes()),
            threshold: self.config.threshold,
            quorum_size: self.config.quorum_size,
            timestamp_ms,
        };

        let mut journal = self.effects.get_journal().await?;
        let key = format!("ota:initiated:{}", ceremony_id_hex);
        let fact_bytes =
            serde_json::to_vec(&fact).map_err(|e| AuraError::serialization(e.to_string()))?;
        journal.facts.insert(key, FactValue::Bytes(fact_bytes));
        self.effects.persist_journal(&journal).await?;

        Ok(())
    }

    /// Emit commitment received fact.
    async fn emit_commitment_received_fact(
        &self,
        ceremony_id: OTACeremonyId,
        commitment: &ReadinessCommitment,
    ) -> AuraResult<()> {
        let timestamp_ms = self
            .effects
            .physical_time()
            .await
            .map_err(|e| AuraError::internal(format!("Time error: {}", e)))?
            .ts_ms;
        let ceremony_id_hex = hex::encode(ceremony_id.0.as_bytes());
        let fact = OTACeremonyFact::CommitmentReceived {
            ceremony_id: ceremony_id_hex.clone(),
            trace_id: Some(ceremony_id_hex.clone()),
            device: hex::encode(commitment.device.0.as_bytes()),
            ready: commitment.ready,
            reason: commitment.reason.clone(),
            timestamp_ms,
        };

        let mut journal = self.effects.get_journal().await?;
        let key = format!(
            "ota:commitment:{}:{}",
            ceremony_id_hex,
            hex::encode(commitment.device.0.as_bytes())
        );
        let fact_bytes =
            serde_json::to_vec(&fact).map_err(|e| AuraError::serialization(e.to_string()))?;
        journal.facts.insert(key, FactValue::Bytes(fact_bytes));
        self.effects.persist_journal(&journal).await?;

        Ok(())
    }

    /// Emit threshold reached fact.
    async fn emit_threshold_reached_fact(&self, ceremony_id: OTACeremonyId) -> AuraResult<()> {
        let timestamp_ms = self
            .effects
            .physical_time()
            .await
            .map_err(|e| AuraError::internal(format!("Time error: {}", e)))?
            .ts_ms;
        let ceremony_id_hex = hex::encode(ceremony_id.0.as_bytes());

        let (ready_count, ready_devices) = {
            let ceremony = self
                .ceremonies
                .get(&ceremony_id)
                .ok_or_else(|| AuraError::not_found("Ceremony not found"))?;
            (
                ceremony.ready_count(),
                ceremony
                    .ready_devices()
                    .into_iter()
                    .map(|d| hex::encode(d.0.as_bytes()))
                    .collect(),
            )
        };

        let fact = OTACeremonyFact::ThresholdReached {
            ceremony_id: ceremony_id_hex.clone(),
            trace_id: Some(ceremony_id_hex.clone()),
            ready_count,
            ready_devices,
            timestamp_ms,
        };

        let mut journal = self.effects.get_journal().await?;
        let key = format!("ota:threshold:{}", ceremony_id_hex);
        let fact_bytes =
            serde_json::to_vec(&fact).map_err(|e| AuraError::serialization(e.to_string()))?;
        journal.facts.insert(key, FactValue::Bytes(fact_bytes));
        self.effects.persist_journal(&journal).await?;

        Ok(())
    }

    /// Emit ceremony committed fact.
    async fn emit_ceremony_committed_fact(
        &self,
        ceremony_id: OTACeremonyId,
        activation_epoch: Epoch,
        ready_devices: &[DeviceId],
        threshold_signature: &[u8],
    ) -> AuraResult<()> {
        let timestamp_ms = self
            .effects
            .physical_time()
            .await
            .map_err(|e| AuraError::internal(format!("Time error: {}", e)))?
            .ts_ms;
        let ceremony_id_hex = hex::encode(ceremony_id.0.as_bytes());
        let fact = OTACeremonyFact::CeremonyCommitted {
            ceremony_id: ceremony_id_hex.clone(),
            trace_id: Some(ceremony_id_hex.clone()),
            activation_epoch,
            ready_devices: ready_devices
                .iter()
                .map(|d| hex::encode(d.0.as_bytes()))
                .collect(),
            threshold_signature: threshold_signature.to_vec(),
            timestamp_ms,
        };

        let mut journal = self.effects.get_journal().await?;
        let key = format!("ota:committed:{}", ceremony_id_hex);
        let fact_bytes =
            serde_json::to_vec(&fact).map_err(|e| AuraError::serialization(e.to_string()))?;
        journal.facts.insert(key, FactValue::Bytes(fact_bytes));
        self.effects.persist_journal(&journal).await?;

        Ok(())
    }

    /// Emit ceremony aborted fact.
    async fn emit_ceremony_aborted_fact(
        &self,
        ceremony_id: OTACeremonyId,
        reason: &str,
    ) -> AuraResult<()> {
        let timestamp_ms = self
            .effects
            .physical_time()
            .await
            .map_err(|e| AuraError::internal(format!("Time error: {}", e)))?
            .ts_ms;
        let ceremony_id_hex = hex::encode(ceremony_id.0.as_bytes());
        let fact = OTACeremonyFact::CeremonyAborted {
            ceremony_id: ceremony_id_hex.clone(),
            trace_id: Some(ceremony_id_hex.clone()),
            reason: reason.to_string(),
            timestamp_ms,
        };

        let mut journal = self.effects.get_journal().await?;
        let key = format!("ota:aborted:{}", ceremony_id_hex);
        let fact_bytes =
            serde_json::to_vec(&fact).map_err(|e| AuraError::serialization(e.to_string()))?;
        journal.facts.insert(key, FactValue::Bytes(fact_bytes));
        self.effects.persist_journal(&journal).await?;

        Ok(())
    }

    /// Get ceremony state.
    pub fn get_ceremony(&self, ceremony_id: &OTACeremonyId) -> Option<&OTACeremonyState> {
        self.ceremonies.get(ceremony_id)
    }

    /// Check if a ceremony exists.
    pub fn has_ceremony(&self, ceremony_id: &OTACeremonyId) -> bool {
        self.ceremonies.contains_key(ceremony_id)
    }

    /// Get all active ceremonies.
    pub fn active_ceremonies(&self) -> Vec<&OTACeremonyState> {
        self.ceremonies
            .values()
            .filter(|c| {
                !matches!(
                    c.status,
                    OTACeremonyStatus::Committed | OTACeremonyStatus::Aborted { .. }
                )
            })
            .collect()
    }

    /// Cleanup ceremonies that have completed or timed out.
    pub fn cleanup_stale_ceremonies(&mut self, now_ms: u64) -> usize {
        let before = self.ceremonies.len();
        self.ceremonies.retain(|_, ceremony| {
            let timed_out = now_ms > ceremony.started_at_ms.saturating_add(ceremony.timeout_ms);
            let terminal = matches!(
                ceremony.status,
                OTACeremonyStatus::Committed | OTACeremonyStatus::Aborted { .. }
            );
            !(timed_out || terminal)
        });
        before.saturating_sub(self.ceremonies.len())
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use aura_core::effects::{JournalEffects, PhysicalTimeEffects, ThresholdSigningEffects};
    use aura_core::time::PhysicalTime;
    use aura_core::types::epochs::Epoch;
    use aura_core::{AuraError, ContextId, FlowBudget, Journal};
    use aura_core::threshold::{ParticipantIdentity, ThresholdConfig, ThresholdState};
    use std::sync::{Arc, Mutex};

    fn test_prestate() -> Hash32 {
        Hash32([1u8; 32])
    }

    fn test_upgrade_hash() -> Hash32 {
        Hash32([2u8; 32])
    }

    fn test_authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    #[derive(Clone)]
    struct TestEffects {
        journal: Arc<Mutex<Journal>>,
        time_ms: Arc<Mutex<u64>>,
    }

    impl TestEffects {
        fn new() -> Self {
            Self {
                journal: Arc::new(Mutex::new(Journal::new())),
                time_ms: Arc::new(Mutex::new(1_700_000_000_000)),
            }
        }

        fn snapshot_journal(&self) -> Journal {
            let journal = self.journal.lock().unwrap();
            let mut copy = Journal::new();
            copy.facts = journal.facts.clone();
            copy.caps = journal.caps.clone();
            copy
        }
    }

    #[async_trait]
    impl JournalEffects for TestEffects {
        async fn merge_facts(&self, target: &Journal, delta: &Journal) -> Result<Journal, AuraError> {
            let mut merged = Journal::new();
            merged.facts = target.facts.clone();
            merged.caps = target.caps.clone();
            merged.merge_facts(delta.read_facts().clone());
            Ok(merged)
        }

        async fn refine_caps(&self, target: &Journal, refinement: &Journal) -> Result<Journal, AuraError> {
            let mut refined = Journal::new();
            refined.facts = target.facts.clone();
            refined.caps = target.caps.clone();
            refined.refine_caps(refinement.read_caps().clone());
            Ok(refined)
        }

        async fn get_journal(&self) -> Result<Journal, AuraError> {
            Ok(self.snapshot_journal())
        }

        async fn persist_journal(&self, journal: &Journal) -> Result<(), AuraError> {
            let mut stored = self.journal.lock().unwrap();
            stored.facts = journal.facts.clone();
            stored.caps = journal.caps.clone();
            Ok(())
        }

        async fn get_flow_budget(
            &self,
            _context: &ContextId,
            _peer: &AuthorityId,
        ) -> Result<FlowBudget, AuraError> {
            Ok(FlowBudget::new(1_000, Epoch::new(0)))
        }

        async fn update_flow_budget(
            &self,
            _context: &ContextId,
            _peer: &AuthorityId,
            budget: &FlowBudget,
        ) -> Result<FlowBudget, AuraError> {
            Ok(*budget)
        }

        async fn charge_flow_budget(
            &self,
            _context: &ContextId,
            _peer: &AuthorityId,
            cost: u32,
        ) -> Result<FlowBudget, AuraError> {
            let mut budget = FlowBudget::new(1_000, Epoch::new(0));
            budget.spent = cost as u64;
            Ok(budget)
        }
    }

    #[async_trait]
    impl PhysicalTimeEffects for TestEffects {
        async fn physical_time(&self) -> Result<PhysicalTime, aura_core::effects::time::TimeError> {
            let mut time = self.time_ms.lock().unwrap();
            *time += 1;
            Ok(PhysicalTime {
                ts_ms: *time,
                uncertainty: None,
            })
        }

        async fn sleep_ms(
            &self,
            _ms: u64,
        ) -> Result<(), aura_core::effects::time::TimeError> {
            Ok(())
        }
    }

    #[async_trait]
    impl ThresholdSigningEffects for TestEffects {
        async fn bootstrap_authority(
            &self,
            _authority: &AuthorityId,
        ) -> Result<Vec<u8>, AuraError> {
            Ok(vec![0u8; 32])
        }

        async fn sign(
            &self,
            _context: SigningContext,
        ) -> Result<ThresholdSignature, AuraError> {
            Ok(ThresholdSignature::single_signer(
                vec![0u8; 64],
                vec![0u8; 32],
                0,
            ))
        }

        async fn threshold_config(&self, _authority: &AuthorityId) -> Option<ThresholdConfig> {
            Some(ThresholdConfig {
                threshold: 1,
                total_participants: 1,
            })
        }

        async fn threshold_state(&self, authority: &AuthorityId) -> Option<ThresholdState> {
            Some(ThresholdState {
                epoch: 0,
                threshold: 1,
                total_participants: 1,
                participants: vec![ParticipantIdentity::guardian(*authority)],
                agreement_mode: AgreementMode::Provisional,
            })
        }

        async fn has_signing_capability(&self, _authority: &AuthorityId) -> bool {
            true
        }

        async fn public_key_package(&self, _authority: &AuthorityId) -> Option<Vec<u8>> {
            Some(vec![0u8; 32])
        }

        async fn rotate_keys(
            &self,
            _authority: &AuthorityId,
            _new_threshold: u16,
            _new_total_participants: u16,
            _participants: &[ParticipantIdentity],
        ) -> Result<(u64, Vec<Vec<u8>>, Vec<u8>), AuraError> {
            Ok((1, vec![vec![0u8; 32]], vec![0u8; 32]))
        }

        async fn commit_key_rotation(
            &self,
            _authority: &AuthorityId,
            _new_epoch: u64,
        ) -> Result<(), AuraError> {
            Ok(())
        }

        async fn rollback_key_rotation(
            &self,
            _authority: &AuthorityId,
            _failed_epoch: u64,
        ) -> Result<(), AuraError> {
            Ok(())
        }
    }

    #[test]
    fn test_ceremony_id_determinism() {
        let id1 = OTACeremonyId::new(&test_prestate(), &test_upgrade_hash(), 12345);
        let id2 = OTACeremonyId::new(&test_prestate(), &test_upgrade_hash(), 12345);
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_ceremony_id_uniqueness_with_nonce() {
        let id1 = OTACeremonyId::new(&test_prestate(), &test_upgrade_hash(), 12345);
        let id2 = OTACeremonyId::new(&test_prestate(), &test_upgrade_hash(), 12346);
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_ceremony_id_uniqueness_with_prestate() {
        let prestate1 = Hash32([1u8; 32]);
        let prestate2 = Hash32([3u8; 32]);
        let id1 = OTACeremonyId::new(&prestate1, &test_upgrade_hash(), 12345);
        let id2 = OTACeremonyId::new(&prestate2, &test_upgrade_hash(), 12345);
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_ceremony_status_transitions() {
        let status = OTACeremonyStatus::CollectingCommitments;
        assert!(matches!(status, OTACeremonyStatus::CollectingCommitments));

        let status = OTACeremonyStatus::AwaitingConsensus;
        assert!(matches!(status, OTACeremonyStatus::AwaitingConsensus));

        let status = OTACeremonyStatus::Committed;
        assert!(matches!(status, OTACeremonyStatus::Committed));

        let status = OTACeremonyStatus::Aborted {
            reason: "test".to_string(),
        };
        assert!(matches!(status, OTACeremonyStatus::Aborted { .. }));
    }

    #[test]
    fn test_ceremony_state_threshold_check() {
        let mut state = OTACeremonyState {
            ceremony_id: OTACeremonyId::new(&test_prestate(), &test_upgrade_hash(), 1),
            proposal: UpgradeProposal {
                proposal_id: Uuid::from_bytes(1u128.to_be_bytes()),
                package_id: Uuid::from_bytes(2u128.to_be_bytes()),
                version: SemanticVersion::new(2, 0, 0),
                kind: UpgradeKind::HardFork,
                package_hash: Hash32([0u8; 32]),
                activation_epoch: Epoch::new(200),
                coordinator: DeviceId::from_bytes([1; 32]),
            },
            status: OTACeremonyStatus::CollectingCommitments,
            agreement_mode: policy_for(CeremonyFlow::OtaActivation).initial_mode(),
            commitments: HashMap::new(),
            threshold: 2,
            quorum_size: 3,
            started_at_ms: 0,
            timeout_ms: 1000,
        };

        // No commitments - threshold not met
        assert!(!state.threshold_met());
        assert_eq!(state.ready_count(), 0);

        // One ready commitment - still not met
        state.commitments.insert(
            DeviceId::from_bytes([2; 32]),
            ReadinessCommitment {
                ceremony_id: state.ceremony_id,
                device: DeviceId::from_bytes([2; 32]),
                authority: test_authority(2),
                prestate_hash: test_prestate(),
                ready: true,
                reason: None,
                signature: ThresholdSignature::single_signer(vec![], vec![], 0),
                committed_at_ms: 0,
            },
        );
        assert!(!state.threshold_met());
        assert_eq!(state.ready_count(), 1);

        // Two ready commitments - threshold met
        state.commitments.insert(
            DeviceId::from_bytes([3; 32]),
            ReadinessCommitment {
                ceremony_id: state.ceremony_id,
                device: DeviceId::from_bytes([3; 32]),
                authority: test_authority(3),
                prestate_hash: test_prestate(),
                ready: true,
                reason: None,
                signature: ThresholdSignature::single_signer(vec![], vec![], 0),
                committed_at_ms: 0,
            },
        );
        assert!(state.threshold_met());
        assert_eq!(state.ready_count(), 2);

        // Verify ready devices list
        let ready = state.ready_devices();
        assert_eq!(ready.len(), 2);
    }

    #[test]
    fn test_readiness_commitment_serialization() {
        let commitment = ReadinessCommitment {
            ceremony_id: OTACeremonyId::new(&test_prestate(), &test_upgrade_hash(), 1),
            device: DeviceId::from_bytes([42u8; 32]),
            authority: test_authority(42),
            prestate_hash: Hash32([0u8; 32]),
            ready: true,
            reason: None,
            signature: ThresholdSignature::single_signer(vec![1, 2, 3], vec![4, 5, 6], 0),
            committed_at_ms: 12345,
        };

        let bytes = serde_json::to_vec(&commitment).unwrap();
        let restored: ReadinessCommitment = serde_json::from_slice(&bytes).unwrap();

        assert!(restored.ready);
        assert_eq!(restored.committed_at_ms, 12345);
        assert_eq!(restored.authority, test_authority(42));
    }

    #[test]
    fn test_ota_ceremony_fact_serialization() {
        let fact = OTACeremonyFact::CeremonyInitiated {
            ceremony_id: "abc123".to_string(),
            trace_id: None,
            proposal_id: "prop-1".to_string(),
            package_id: "pkg-1".to_string(),
            version: "2.0.0".to_string(),
            activation_epoch: Epoch::new(200),
            coordinator: "coord-1".to_string(),
            threshold: 2,
            quorum_size: 3,
            timestamp_ms: 12345,
        };

        let bytes = serde_json::to_vec(&fact).unwrap();
        let restored: OTACeremonyFact = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(restored.ceremony_id(), "abc123");
        assert_eq!(restored.timestamp_ms(), 12345);
    }

    #[test]
    fn test_upgrade_proposal_hash() {
        let proposal1 = UpgradeProposal {
            proposal_id: Uuid::from_bytes(1u128.to_be_bytes()),
            package_id: Uuid::from_bytes(2u128.to_be_bytes()),
            version: SemanticVersion::new(2, 0, 0),
            kind: UpgradeKind::HardFork,
            package_hash: Hash32([0u8; 32]),
            activation_epoch: Epoch::new(200),
            coordinator: DeviceId::from_bytes([1; 32]),
        };

        let proposal2 = UpgradeProposal {
            proposal_id: Uuid::from_bytes(1u128.to_be_bytes()),
            package_id: Uuid::from_bytes(2u128.to_be_bytes()),
            version: SemanticVersion::new(2, 0, 0),
            kind: UpgradeKind::HardFork,
            package_hash: Hash32([0u8; 32]),
            activation_epoch: Epoch::new(200),
            coordinator: DeviceId::from_bytes([1; 32]),
        };

        // Same proposals should have same hash
        assert_eq!(proposal1.compute_hash(), proposal2.compute_hash());

        // Different proposal should have different hash
        let proposal3 = UpgradeProposal {
            activation_epoch: Epoch::new(300), // Different epoch
            ..proposal1.clone()
        };
        assert_ne!(proposal1.compute_hash(), proposal3.compute_hash());
    }

    #[tokio::test]
    async fn test_ota_ceremony_commit_emits_fact() {
        let effects = TestEffects::new();
        let config = OTACeremonyConfig {
            threshold: 1,
            quorum_size: 1,
            timeout_ms: 1_000,
            min_activation_notice_epochs: 0,
        };
        let mut executor = OTACeremonyExecutor::new(effects.clone(), config);

        let proposal = UpgradeProposal {
            proposal_id: Uuid::from_bytes(1u128.to_be_bytes()),
            package_id: Uuid::from_bytes(2u128.to_be_bytes()),
            version: SemanticVersion::new(2, 0, 0),
            kind: UpgradeKind::HardFork,
            package_hash: Hash32([9u8; 32]),
            activation_epoch: Epoch::new(10),
            coordinator: DeviceId::from_bytes([8u8; 32]),
        };

        let ceremony_id = executor
            .initiate_ceremony(proposal, Epoch::new(0))
            .await
            .unwrap();

        let commitment = executor
            .create_readiness_commitment(
                ceremony_id,
                DeviceId::from_bytes([1u8; 32]),
                test_authority(9),
                true,
                None,
            )
            .await
            .unwrap();

        let threshold_reached = executor
            .process_commitment(ceremony_id, commitment)
            .await
            .unwrap();
        assert!(threshold_reached);

        executor.commit_ceremony(ceremony_id).await.unwrap();

        let journal = effects.snapshot_journal();
        let key = format!("ota:committed:{}", hex::encode(ceremony_id.0.as_bytes()));
        assert!(
            journal.facts.contains_key(&key),
            "Expected committed fact in journal"
        );
    }
}
