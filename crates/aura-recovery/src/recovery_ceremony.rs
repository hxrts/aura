//! Consensus-Based Recovery Approval Ceremony
//!
//! This module provides a safe, consensus-backed recovery approval protocol
//! that ensures guardian quorum agreement before recovery operations execute.
//!
//! ## Problem Solved
//!
//! Without consensus, recovery can diverge:
//! - Guardian A approves recovery, commits share
//! - Network partition before other guardians respond
//! - Account thinks recovery is possible, but quorum isn't actually reached
//! - Recovery attempt fails with partial state changes
//!
//! ## Solution: Prestate-Bound Consensus
//!
//! 1. **Account** initiates recovery ceremony with request bound to prestate
//! 2. **Each guardian** validates request and provides approval bound to same prestate
//! 3. **Consensus** ensures k-of-n guardians agree BEFORE recovery executes
//! 4. Only after consensus does recovery operation commit
//!
//! ## Session Type Guarantee
//!
//! The choreography enforces linear protocol flow:
//! ```text
//! Account -> Coordinator: RecoveryRequest
//! Coordinator -> Guardian[*]: RequestApproval
//! Guardian[*] -> Coordinator: ApprovalResponse
//! [Consensus: k-of-n guardians approve]
//! choice {
//!     Coordinator -> Account: CommitRecovery
//! } or {
//!     Coordinator -> Account: AbortRecovery
//! }
//! ```
//!
//! ## Key Properties
//!
//! - **Atomicity**: Recovery commits only with quorum agreement
//! - **No Partial Recovery**: Insufficient approvals leave state unchanged
//! - **Deterministic ID**: `CeremonyId = H(prestate_hash || request_hash || nonce)`
//! - **Auditability**: All state changes recorded as journal facts

use aura_core::domain::FactValue;
use aura_core::effects::{JournalEffects, PhysicalTimeEffects, ThresholdSigningEffects};
use aura_core::identifiers::AuthorityId;
use aura_core::threshold::{policy_for, AgreementMode, CeremonyFlow, ThresholdSignature};
use aura_core::{AuraError, AuraResult, Hash32};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// =============================================================================
// CEREMONY TYPES
// =============================================================================

/// Unique identifier for a recovery ceremony instance.
///
/// Derived from `H(prestate_hash, request_hash, nonce)` to prevent
/// concurrent ceremonies for the same recovery request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RecoveryCeremonyId(pub Hash32);

impl RecoveryCeremonyId {
    /// Create a ceremony ID from constituent parts.
    pub fn new(prestate_hash: &Hash32, request_hash: &Hash32, nonce: u64) -> Self {
        let mut input = Vec::with_capacity(32 + 32 + 8);
        input.extend_from_slice(prestate_hash.as_bytes());
        input.extend_from_slice(request_hash.as_bytes());
        input.extend_from_slice(&nonce.to_le_bytes());
        Self(Hash32::from_bytes(&input))
    }

    /// Get the underlying hash.
    pub fn as_hash(&self) -> &Hash32 {
        &self.0
    }
}

/// Recovery operation types supported by the ceremony.
///
/// Note: This is distinct from `aura_authentication::RecoveryOperationType`
/// which is used for general recovery context management. This enum is
/// specifically for ceremony-based recovery operations with guardian approval.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CeremonyRecoveryOperation {
    /// Replace the entire commitment tree (device key recovery)
    ReplaceTree { new_tree_root: Hash32 },
    /// Add a new device to existing tree
    AddDevice { device_public_key: Vec<u8> },
    /// Remove a compromised device
    RemoveDevice { leaf_index: u32 },
    /// Update guardian set parameters
    UpdateGuardians { new_threshold: u16 },
    /// Emergency freeze account
    EmergencyFreeze,
    /// Unfreeze account
    Unfreeze,
}

/// A recovery request that initiates the ceremony.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CeremonyRecoveryRequest {
    /// Account authority being recovered
    pub account_authority: AuthorityId,
    /// The recovery operation requested
    pub operation: CeremonyRecoveryOperation,
    /// Justification for the recovery
    pub justification: String,
    /// Prestate hash at time of request
    pub prestate_hash: Hash32,
    /// Timestamp of request
    pub requested_at_ms: u64,
}

impl CeremonyRecoveryRequest {
    /// Compute hash of the request for ceremony ID derivation.
    #[allow(clippy::expect_used)] // serde_json serialization of simple structs is infallible
    pub fn compute_hash(&self) -> Hash32 {
        let bytes = serde_json::to_vec(self).expect("CeremonyRecoveryRequest should serialize");
        Hash32::from_bytes(&bytes)
    }
}

/// A guardian's approval for a recovery request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryApproval {
    /// The ceremony being approved
    pub ceremony_id: RecoveryCeremonyId,
    /// Guardian providing approval
    pub guardian: AuthorityId,
    /// Whether the guardian approves
    pub approved: bool,
    /// Reason for rejection (if not approved)
    pub rejection_reason: Option<String>,
    /// Prestate hash at time of approval (must match ceremony)
    pub prestate_hash: Hash32,
    /// Guardian's signature over the approval
    pub signature: ThresholdSignature,
    /// Timestamp of approval
    pub approved_at_ms: u64,
}

/// Current status of a recovery ceremony.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecoveryCeremonyStatus {
    /// Ceremony initiated, collecting guardian approvals
    CollectingApprovals,
    /// Quorum reached, awaiting execution finalization
    AwaitingExecution,
    /// Consensus reached, recovery committed
    Committed,
    /// Ceremony aborted (insufficient approvals, rejection, or timeout)
    Aborted { reason: String },
}

/// Full state of a recovery ceremony.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryCeremonyState {
    /// Unique ceremony identifier
    pub ceremony_id: RecoveryCeremonyId,
    /// The recovery request
    pub request: CeremonyRecoveryRequest,
    /// Current status
    pub status: RecoveryCeremonyStatus,
    /// Guardian approvals by authority
    pub approvals: HashMap<AuthorityId, RecoveryApproval>,
    /// Guardian authorities participating
    pub guardians: Vec<AuthorityId>,
    /// Required threshold (k-of-n)
    pub threshold: u16,
    /// Timestamp when ceremony started
    pub started_at_ms: u64,
    /// Timeout for ceremony completion (ms)
    pub timeout_ms: u64,
    /// Agreement mode (A1/A2/A3)
    pub agreement_mode: AgreementMode,
}

impl RecoveryCeremonyState {
    /// Count approved guardians.
    pub fn approved_count(&self) -> usize {
        self.approvals.values().filter(|a| a.approved).count()
    }

    /// Count rejected guardians.
    pub fn rejected_count(&self) -> usize {
        self.approvals.values().filter(|a| !a.approved).count()
    }

    /// Check if quorum threshold is met.
    pub fn threshold_met(&self) -> bool {
        self.approved_count() >= self.threshold as usize
    }

    /// Check if any guardian has rejected.
    pub fn has_rejection(&self) -> bool {
        self.approvals.values().any(|a| !a.approved)
    }

    /// Get approved guardians.
    pub fn approved_guardians(&self) -> Vec<AuthorityId> {
        self.approvals
            .iter()
            .filter_map(|(id, a)| if a.approved { Some(*id) } else { None })
            .collect()
    }
}

// =============================================================================
// CEREMONY FACTS
// =============================================================================

/// Facts emitted during recovery ceremony lifecycle.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RecoveryCeremonyFact {
    /// Ceremony initiated
    CeremonyInitiated {
        ceremony_id: String,
        account_authority: String,
        operation_type: String,
        justification: String,
        guardians: Vec<String>,
        threshold: u16,
        timestamp_ms: u64,
    },
    /// Guardian approval received
    ApprovalReceived {
        ceremony_id: String,
        guardian: String,
        approved: bool,
        rejection_reason: Option<String>,
        timestamp_ms: u64,
    },
    /// Quorum threshold reached
    QuorumReached {
        ceremony_id: String,
        approved_count: u16,
        approved_guardians: Vec<String>,
        timestamp_ms: u64,
    },
    /// Ceremony committed (recovery executed)
    CeremonyCommitted {
        ceremony_id: String,
        account_authority: String,
        operation_type: String,
        approved_guardians: Vec<String>,
        evidence_hash: String,
        timestamp_ms: u64,
    },
    /// Ceremony aborted
    CeremonyAborted {
        ceremony_id: String,
        reason: String,
        timestamp_ms: u64,
    },
}

impl RecoveryCeremonyFact {
    /// Get the ceremony ID from any fact variant.
    pub fn ceremony_id(&self) -> &str {
        match self {
            RecoveryCeremonyFact::CeremonyInitiated { ceremony_id, .. } => ceremony_id,
            RecoveryCeremonyFact::ApprovalReceived { ceremony_id, .. } => ceremony_id,
            RecoveryCeremonyFact::QuorumReached { ceremony_id, .. } => ceremony_id,
            RecoveryCeremonyFact::CeremonyCommitted { ceremony_id, .. } => ceremony_id,
            RecoveryCeremonyFact::CeremonyAborted { ceremony_id, .. } => ceremony_id,
        }
    }

    /// Get the timestamp from any fact variant.
    pub fn timestamp_ms(&self) -> u64 {
        match self {
            RecoveryCeremonyFact::CeremonyInitiated { timestamp_ms, .. } => *timestamp_ms,
            RecoveryCeremonyFact::ApprovalReceived { timestamp_ms, .. } => *timestamp_ms,
            RecoveryCeremonyFact::QuorumReached { timestamp_ms, .. } => *timestamp_ms,
            RecoveryCeremonyFact::CeremonyCommitted { timestamp_ms, .. } => *timestamp_ms,
            RecoveryCeremonyFact::CeremonyAborted { timestamp_ms, .. } => *timestamp_ms,
        }
    }

    /// Get operation type string for facts.
    fn operation_type_string(op: &CeremonyRecoveryOperation) -> String {
        match op {
            CeremonyRecoveryOperation::ReplaceTree { .. } => "replace_tree".to_string(),
            CeremonyRecoveryOperation::AddDevice { .. } => "add_device".to_string(),
            CeremonyRecoveryOperation::RemoveDevice { .. } => "remove_device".to_string(),
            CeremonyRecoveryOperation::UpdateGuardians { .. } => "update_guardians".to_string(),
            CeremonyRecoveryOperation::EmergencyFreeze => "emergency_freeze".to_string(),
            CeremonyRecoveryOperation::Unfreeze => "unfreeze".to_string(),
        }
    }
}

// =============================================================================
// CEREMONY CONFIGURATION
// =============================================================================

/// Configuration for recovery ceremonies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryCeremonyConfig {
    /// Default timeout for ceremony completion (ms)
    pub default_timeout_ms: u64,
    /// Whether to allow emergency bypass (single guardian for EmergencyFreeze)
    pub allow_emergency_bypass: bool,
}

impl Default for RecoveryCeremonyConfig {
    fn default() -> Self {
        Self {
            default_timeout_ms: 24 * 60 * 60 * 1000, // 24 hours
            allow_emergency_bypass: false,
        }
    }
}

// =============================================================================
// CEREMONY EXECUTOR
// =============================================================================

/// Executes recovery ceremonies with consensus guarantees.
///
/// The executor manages the lifecycle of recovery approval ceremonies,
/// ensuring atomicity through prestate binding and k-of-n quorum.
pub struct RecoveryCeremonyExecutor<E: RecoveryCeremonyEffects> {
    /// Effect system for all operations
    effects: E,
    /// Configuration
    config: RecoveryCeremonyConfig,
    /// Active ceremonies by ID
    ceremonies: HashMap<RecoveryCeremonyId, RecoveryCeremonyState>,
}

/// Combined effects required for recovery ceremonies.
pub trait RecoveryCeremonyEffects:
    JournalEffects + PhysicalTimeEffects + ThresholdSigningEffects + Send + Sync
{
}

// Blanket implementation
impl<T> RecoveryCeremonyEffects for T where
    T: JournalEffects + PhysicalTimeEffects + ThresholdSigningEffects + Send + Sync
{
}

impl<E: RecoveryCeremonyEffects> RecoveryCeremonyExecutor<E> {
    /// Create a new ceremony executor.
    pub fn new(effects: E, config: RecoveryCeremonyConfig) -> Self {
        Self {
            effects,
            config,
            ceremonies: HashMap::new(),
        }
    }

    /// Create with default configuration.
    pub fn with_defaults(effects: E) -> Self {
        Self::new(effects, RecoveryCeremonyConfig::default())
    }

    // =========================================================================
    // CEREMONY LIFECYCLE
    // =========================================================================

    /// Initiate a new recovery ceremony.
    pub async fn initiate_ceremony(
        &mut self,
        request: CeremonyRecoveryRequest,
        guardians: Vec<AuthorityId>,
        threshold: u16,
    ) -> AuraResult<RecoveryCeremonyId> {
        // Validate inputs
        if threshold as usize > guardians.len() {
            return Err(AuraError::invalid(format!(
                "Threshold {} cannot exceed guardian count {}",
                threshold,
                guardians.len()
            )));
        }

        if guardians.is_empty() {
            return Err(AuraError::invalid("Must have at least one guardian"));
        }

        // Verify prestate matches current state
        let current_prestate = self.compute_prestate_hash().await?;
        if request.prestate_hash != current_prestate {
            return Err(AuraError::invalid(
                "Request prestate doesn't match current state",
            ));
        }

        // Compute request hash
        let request_hash = request.compute_hash();

        // Generate nonce from current time
        let nonce = self
            .effects
            .physical_time()
            .await
            .map_err(|e| AuraError::internal(format!("Time error: {e}")))?
            .ts_ms;

        // Create ceremony ID
        let ceremony_id = RecoveryCeremonyId::new(&current_prestate, &request_hash, nonce);

        // Create ceremony state
        let state = RecoveryCeremonyState {
            ceremony_id,
            request: request.clone(),
            status: RecoveryCeremonyStatus::CollectingApprovals,
            approvals: HashMap::new(),
            guardians: guardians.clone(),
            threshold,
            started_at_ms: nonce,
            timeout_ms: self.config.default_timeout_ms,
            agreement_mode: policy_for(CeremonyFlow::RecoveryApproval).initial_mode(),
        };

        // Store ceremony
        self.ceremonies.insert(ceremony_id, state);

        // Emit ceremony initiated fact
        self.emit_ceremony_initiated_fact(ceremony_id, &request, &guardians, threshold)
            .await?;

        Ok(ceremony_id)
    }

    /// Process a guardian approval.
    pub async fn process_approval(
        &mut self,
        ceremony_id: RecoveryCeremonyId,
        approval: RecoveryApproval,
    ) -> AuraResult<bool> {
        // Get current prestate and time
        let current_prestate = self.compute_prestate_hash().await?;
        let now = self
            .effects
            .physical_time()
            .await
            .map_err(|e| AuraError::internal(format!("Time error: {e}")))?
            .ts_ms;

        // Track if quorum was reached
        let quorum_reached;

        // Process in a block to limit borrow scope
        {
            let ceremony = self
                .ceremonies
                .get_mut(&ceremony_id)
                .ok_or_else(|| AuraError::not_found("Ceremony not found"))?;

            // Verify ceremony is collecting approvals
            if ceremony.status != RecoveryCeremonyStatus::CollectingApprovals {
                return Err(AuraError::invalid(format!(
                    "Ceremony not collecting approvals: {:?}",
                    ceremony.status
                )));
            }

            // Verify prestate matches
            if approval.prestate_hash != current_prestate {
                return Err(AuraError::invalid(
                    "Prestate hash mismatch - state has changed since approval was created",
                ));
            }

            // Verify ceremony ID matches
            if approval.ceremony_id != ceremony_id {
                return Err(AuraError::invalid("Approval is for different ceremony"));
            }

            // Verify guardian is part of ceremony
            if !ceremony.guardians.contains(&approval.guardian) {
                return Err(AuraError::permission_denied(
                    "Guardian not part of this ceremony",
                ));
            }

            // Check timeout
            if now > ceremony.started_at_ms + ceremony.timeout_ms {
                ceremony.status = RecoveryCeremonyStatus::Aborted {
                    reason: "Ceremony timed out".to_string(),
                };
                return Ok(false);
            }

            // Check for duplicate approval
            if ceremony.approvals.contains_key(&approval.guardian) {
                return Err(AuraError::invalid(
                    "Guardian has already submitted approval",
                ));
            }

            // Store approval
            ceremony
                .approvals
                .insert(approval.guardian, approval.clone());

            // Check if quorum is now met
            quorum_reached = ceremony.threshold_met()
                && ceremony.status == RecoveryCeremonyStatus::CollectingApprovals;

            if quorum_reached {
                ceremony.status = RecoveryCeremonyStatus::AwaitingExecution;
                ceremony.agreement_mode = AgreementMode::CoordinatorSoftSafe;
            }
        }

        // Emit approval received fact
        self.emit_approval_received_fact(ceremony_id, &approval)
            .await?;

        // Emit quorum reached fact if applicable
        if quorum_reached {
            self.emit_quorum_reached_fact(ceremony_id).await?;
        }

        Ok(quorum_reached)
    }

    /// Commit the ceremony after consensus.
    pub async fn commit_ceremony(&mut self, ceremony_id: RecoveryCeremonyId) -> AuraResult<()> {
        let policy = policy_for(CeremonyFlow::RecoveryExecution);
        if !policy.allows_mode(AgreementMode::ConsensusFinalized) {
            return Err(AuraError::invalid(
                "Recovery execution does not permit consensus finalization",
            ));
        }
        // Get ceremony info before mutable borrow
        let (account_authority, operation_type, approved_guardians) = {
            let ceremony = self
                .ceremonies
                .get(&ceremony_id)
                .ok_or_else(|| AuraError::not_found("Ceremony not found"))?;

            // Verify ceremony is awaiting execution
            if ceremony.status != RecoveryCeremonyStatus::AwaitingExecution {
                return Err(AuraError::invalid(format!(
                    "Ceremony not awaiting execution: {:?}",
                    ceremony.status
                )));
            }

            // Verify quorum is still met
            if !ceremony.threshold_met() {
                return Err(AuraError::invalid("Quorum threshold no longer met"));
            }

            (
                ceremony.request.account_authority,
                ceremony.request.operation.clone(),
                ceremony.approved_guardians(),
            )
        };

        // Update status
        if let Some(ceremony) = self.ceremonies.get_mut(&ceremony_id) {
            ceremony.status = RecoveryCeremonyStatus::Committed;
            ceremony.agreement_mode = AgreementMode::ConsensusFinalized;
        }

        // Emit committed fact
        self.emit_ceremony_committed_fact(
            ceremony_id,
            account_authority,
            &operation_type,
            &approved_guardians,
        )
        .await?;

        Ok(())
    }

    /// Abort the ceremony.
    pub async fn abort_ceremony(
        &mut self,
        ceremony_id: RecoveryCeremonyId,
        reason: &str,
    ) -> AuraResult<()> {
        let ceremony = self
            .ceremonies
            .get_mut(&ceremony_id)
            .ok_or_else(|| AuraError::not_found("Ceremony not found"))?;

        // Can abort from any non-terminal state
        match &ceremony.status {
            RecoveryCeremonyStatus::Committed => {
                return Err(AuraError::invalid("Cannot abort committed ceremony"));
            }
            RecoveryCeremonyStatus::Aborted { .. } => {
                return Ok(()); // Already aborted, idempotent
            }
            _ => {}
        }

        ceremony.status = RecoveryCeremonyStatus::Aborted {
            reason: reason.to_string(),
        };

        // Emit aborted fact
        self.emit_ceremony_aborted_fact(ceremony_id, reason).await?;

        Ok(())
    }

    // =========================================================================
    // GUARDIAN SIDE
    // =========================================================================

    /// Create an approval for a ceremony (guardian side).
    pub async fn create_approval(
        &self,
        ceremony_id: RecoveryCeremonyId,
        guardian: AuthorityId,
        approved: bool,
        rejection_reason: Option<String>,
    ) -> AuraResult<RecoveryApproval> {
        let prestate_hash = self.compute_prestate_hash().await?;
        let approved_at_ms = self
            .effects
            .physical_time()
            .await
            .map_err(|e| AuraError::internal(format!("Time error: {e}")))?
            .ts_ms;

        // Create signature (placeholder)
        let signature = ThresholdSignature::single_signer(
            vec![0u8; 64], // Placeholder
            vec![0u8; 32], // Placeholder
            0,
        );

        Ok(RecoveryApproval {
            ceremony_id,
            guardian,
            approved,
            rejection_reason,
            prestate_hash,
            signature,
            approved_at_ms,
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

    /// Emit ceremony initiated fact.
    async fn emit_ceremony_initiated_fact(
        &self,
        ceremony_id: RecoveryCeremonyId,
        request: &CeremonyRecoveryRequest,
        guardians: &[AuthorityId],
        threshold: u16,
    ) -> AuraResult<()> {
        let timestamp_ms = self
            .effects
            .physical_time()
            .await
            .map_err(|e| AuraError::internal(format!("Time error: {e}")))?
            .ts_ms;
        let fact = RecoveryCeremonyFact::CeremonyInitiated {
            ceremony_id: hex::encode(ceremony_id.0.as_bytes()),
            account_authority: request.account_authority.to_string(),
            operation_type: RecoveryCeremonyFact::operation_type_string(&request.operation),
            justification: request.justification.clone(),
            guardians: guardians.iter().map(|g| g.to_string()).collect(),
            threshold,
            timestamp_ms,
        };

        let mut journal = self.effects.get_journal().await?;
        let key = format!(
            "recovery:initiated:{}",
            hex::encode(ceremony_id.0.as_bytes())
        );
        let fact_bytes =
            serde_json::to_vec(&fact).map_err(|e| AuraError::serialization(e.to_string()))?;
        journal.facts.insert(key, FactValue::Bytes(fact_bytes));
        self.effects.persist_journal(&journal).await?;

        Ok(())
    }

    /// Emit approval received fact.
    async fn emit_approval_received_fact(
        &self,
        ceremony_id: RecoveryCeremonyId,
        approval: &RecoveryApproval,
    ) -> AuraResult<()> {
        let timestamp_ms = self
            .effects
            .physical_time()
            .await
            .map_err(|e| AuraError::internal(format!("Time error: {e}")))?
            .ts_ms;
        let fact = RecoveryCeremonyFact::ApprovalReceived {
            ceremony_id: hex::encode(ceremony_id.0.as_bytes()),
            guardian: approval.guardian.to_string(),
            approved: approval.approved,
            rejection_reason: approval.rejection_reason.clone(),
            timestamp_ms,
        };

        let mut journal = self.effects.get_journal().await?;
        let key = format!(
            "recovery:approval:{}:{}",
            hex::encode(ceremony_id.0.as_bytes()),
            approval.guardian
        );
        let fact_bytes =
            serde_json::to_vec(&fact).map_err(|e| AuraError::serialization(e.to_string()))?;
        journal.facts.insert(key, FactValue::Bytes(fact_bytes));
        self.effects.persist_journal(&journal).await?;

        Ok(())
    }

    /// Emit quorum reached fact.
    async fn emit_quorum_reached_fact(&self, ceremony_id: RecoveryCeremonyId) -> AuraResult<()> {
        let timestamp_ms = self
            .effects
            .physical_time()
            .await
            .map_err(|e| AuraError::internal(format!("Time error: {e}")))?
            .ts_ms;

        let (approved_count, approved_guardians) = {
            let ceremony = self
                .ceremonies
                .get(&ceremony_id)
                .ok_or_else(|| AuraError::not_found("Ceremony not found"))?;
            (
                ceremony.approved_count() as u16,
                ceremony
                    .approved_guardians()
                    .into_iter()
                    .map(|g| g.to_string())
                    .collect(),
            )
        };

        let fact = RecoveryCeremonyFact::QuorumReached {
            ceremony_id: hex::encode(ceremony_id.0.as_bytes()),
            approved_count,
            approved_guardians,
            timestamp_ms,
        };

        let mut journal = self.effects.get_journal().await?;
        let key = format!("recovery:quorum:{}", hex::encode(ceremony_id.0.as_bytes()));
        let fact_bytes =
            serde_json::to_vec(&fact).map_err(|e| AuraError::serialization(e.to_string()))?;
        journal.facts.insert(key, FactValue::Bytes(fact_bytes));
        self.effects.persist_journal(&journal).await?;

        Ok(())
    }

    /// Emit ceremony committed fact.
    async fn emit_ceremony_committed_fact(
        &self,
        ceremony_id: RecoveryCeremonyId,
        account_authority: AuthorityId,
        operation_type: &CeremonyRecoveryOperation,
        approved_guardians: &[AuthorityId],
    ) -> AuraResult<()> {
        let timestamp_ms = self
            .effects
            .physical_time()
            .await
            .map_err(|e| AuraError::internal(format!("Time error: {e}")))?
            .ts_ms;

        // Create evidence hash from ceremony + guardians
        let mut evidence_input = ceremony_id.0.as_bytes().to_vec();
        for g in approved_guardians {
            evidence_input.extend_from_slice(g.uuid().as_bytes());
        }
        let evidence_hash = Hash32::from_bytes(&evidence_input);

        let fact = RecoveryCeremonyFact::CeremonyCommitted {
            ceremony_id: hex::encode(ceremony_id.0.as_bytes()),
            account_authority: account_authority.to_string(),
            operation_type: RecoveryCeremonyFact::operation_type_string(operation_type),
            approved_guardians: approved_guardians.iter().map(|g| g.to_string()).collect(),
            evidence_hash: hex::encode(evidence_hash.as_bytes()),
            timestamp_ms,
        };

        let mut journal = self.effects.get_journal().await?;
        let key = format!(
            "recovery:committed:{}",
            hex::encode(ceremony_id.0.as_bytes())
        );
        let fact_bytes =
            serde_json::to_vec(&fact).map_err(|e| AuraError::serialization(e.to_string()))?;
        journal.facts.insert(key, FactValue::Bytes(fact_bytes));
        self.effects.persist_journal(&journal).await?;

        Ok(())
    }

    /// Emit ceremony aborted fact.
    async fn emit_ceremony_aborted_fact(
        &self,
        ceremony_id: RecoveryCeremonyId,
        reason: &str,
    ) -> AuraResult<()> {
        let timestamp_ms = self
            .effects
            .physical_time()
            .await
            .map_err(|e| AuraError::internal(format!("Time error: {e}")))?
            .ts_ms;
        let fact = RecoveryCeremonyFact::CeremonyAborted {
            ceremony_id: hex::encode(ceremony_id.0.as_bytes()),
            reason: reason.to_string(),
            timestamp_ms,
        };

        let mut journal = self.effects.get_journal().await?;
        let key = format!("recovery:aborted:{}", hex::encode(ceremony_id.0.as_bytes()));
        let fact_bytes =
            serde_json::to_vec(&fact).map_err(|e| AuraError::serialization(e.to_string()))?;
        journal.facts.insert(key, FactValue::Bytes(fact_bytes));
        self.effects.persist_journal(&journal).await?;

        Ok(())
    }

    /// Get ceremony state.
    pub fn get_ceremony(&self, ceremony_id: &RecoveryCeremonyId) -> Option<&RecoveryCeremonyState> {
        self.ceremonies.get(ceremony_id)
    }

    /// Check if a ceremony exists.
    pub fn has_ceremony(&self, ceremony_id: &RecoveryCeremonyId) -> bool {
        self.ceremonies.contains_key(ceremony_id)
    }

    /// Get all active ceremonies.
    pub fn active_ceremonies(&self) -> Vec<&RecoveryCeremonyState> {
        self.ceremonies
            .values()
            .filter(|c| {
                !matches!(
                    c.status,
                    RecoveryCeremonyStatus::Committed | RecoveryCeremonyStatus::Aborted { .. }
                )
            })
            .collect()
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn test_prestate() -> Hash32 {
        Hash32([1u8; 32])
    }

    fn test_request_hash() -> Hash32 {
        Hash32([2u8; 32])
    }

    fn test_authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    #[test]
    fn test_ceremony_id_determinism() {
        let id1 = RecoveryCeremonyId::new(&test_prestate(), &test_request_hash(), 12345);
        let id2 = RecoveryCeremonyId::new(&test_prestate(), &test_request_hash(), 12345);
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_ceremony_id_uniqueness_with_nonce() {
        let id1 = RecoveryCeremonyId::new(&test_prestate(), &test_request_hash(), 12345);
        let id2 = RecoveryCeremonyId::new(&test_prestate(), &test_request_hash(), 12346);
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_ceremony_id_uniqueness_with_prestate() {
        let prestate1 = Hash32([1u8; 32]);
        let prestate2 = Hash32([3u8; 32]);
        let id1 = RecoveryCeremonyId::new(&prestate1, &test_request_hash(), 12345);
        let id2 = RecoveryCeremonyId::new(&prestate2, &test_request_hash(), 12345);
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_ceremony_status_transitions() {
        let status = RecoveryCeremonyStatus::CollectingApprovals;
        assert!(matches!(
            status,
            RecoveryCeremonyStatus::CollectingApprovals
        ));

        let status = RecoveryCeremonyStatus::AwaitingExecution;
        assert!(matches!(status, RecoveryCeremonyStatus::AwaitingExecution));

        let status = RecoveryCeremonyStatus::Committed;
        assert!(matches!(status, RecoveryCeremonyStatus::Committed));

        let status = RecoveryCeremonyStatus::Aborted {
            reason: "test".to_string(),
        };
        assert!(matches!(status, RecoveryCeremonyStatus::Aborted { .. }));
    }

    #[test]
    fn test_ceremony_state_threshold_check() {
        let mut state = RecoveryCeremonyState {
            ceremony_id: RecoveryCeremonyId::new(&test_prestate(), &test_request_hash(), 1),
            request: CeremonyRecoveryRequest {
                account_authority: test_authority(0),
                operation: CeremonyRecoveryOperation::EmergencyFreeze,
                justification: "Test".to_string(),
                prestate_hash: test_prestate(),
                requested_at_ms: 0,
            },
            status: RecoveryCeremonyStatus::CollectingApprovals,
            approvals: HashMap::new(),
            guardians: vec![test_authority(1), test_authority(2), test_authority(3)],
            threshold: 2,
            started_at_ms: 0,
            timeout_ms: 1000,
            agreement_mode: AgreementMode::CoordinatorSoftSafe,
        };

        // No approvals - threshold not met
        assert!(!state.threshold_met());
        assert_eq!(state.approved_count(), 0);

        // One approval - still not met
        state.approvals.insert(
            test_authority(1),
            RecoveryApproval {
                ceremony_id: state.ceremony_id,
                guardian: test_authority(1),
                approved: true,
                rejection_reason: None,
                prestate_hash: test_prestate(),
                signature: ThresholdSignature::single_signer(vec![], vec![], 0),
                approved_at_ms: 0,
            },
        );
        assert!(!state.threshold_met());
        assert_eq!(state.approved_count(), 1);

        // Two approvals - threshold met
        state.approvals.insert(
            test_authority(2),
            RecoveryApproval {
                ceremony_id: state.ceremony_id,
                guardian: test_authority(2),
                approved: true,
                rejection_reason: None,
                prestate_hash: test_prestate(),
                signature: ThresholdSignature::single_signer(vec![], vec![], 0),
                approved_at_ms: 0,
            },
        );
        assert!(state.threshold_met());
        assert_eq!(state.approved_count(), 2);

        // Verify approved guardians list
        let approved = state.approved_guardians();
        assert_eq!(approved.len(), 2);
    }

    #[test]
    fn test_recovery_approval_serialization() {
        let approval = RecoveryApproval {
            ceremony_id: RecoveryCeremonyId::new(&test_prestate(), &test_request_hash(), 1),
            guardian: test_authority(42),
            approved: true,
            rejection_reason: None,
            prestate_hash: Hash32([0u8; 32]),
            signature: ThresholdSignature::single_signer(vec![1, 2, 3], vec![4, 5, 6], 0),
            approved_at_ms: 12345,
        };

        let bytes = serde_json::to_vec(&approval).unwrap();
        let restored: RecoveryApproval = serde_json::from_slice(&bytes).unwrap();

        assert!(restored.approved);
        assert_eq!(restored.approved_at_ms, 12345);
    }

    #[test]
    fn test_recovery_ceremony_fact_serialization() {
        let fact = RecoveryCeremonyFact::CeremonyInitiated {
            ceremony_id: "abc123".to_string(),
            account_authority: "auth-1".to_string(),
            operation_type: "emergency_freeze".to_string(),
            justification: "Test".to_string(),
            guardians: vec!["g1".to_string(), "g2".to_string()],
            threshold: 2,
            timestamp_ms: 12345,
        };

        let bytes = serde_json::to_vec(&fact).unwrap();
        let restored: RecoveryCeremonyFact = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(restored.ceremony_id(), "abc123");
        assert_eq!(restored.timestamp_ms(), 12345);
    }

    #[test]
    fn test_recovery_request_hash() {
        let request1 = CeremonyRecoveryRequest {
            account_authority: test_authority(1),
            operation: CeremonyRecoveryOperation::EmergencyFreeze,
            justification: "Test".to_string(),
            prestate_hash: Hash32([0u8; 32]),
            requested_at_ms: 12345,
        };

        let request2 = CeremonyRecoveryRequest {
            account_authority: test_authority(1),
            operation: CeremonyRecoveryOperation::EmergencyFreeze,
            justification: "Test".to_string(),
            prestate_hash: Hash32([0u8; 32]),
            requested_at_ms: 12345,
        };

        // Same requests should have same hash
        assert_eq!(request1.compute_hash(), request2.compute_hash());

        // Different request should have different hash
        let request3 = CeremonyRecoveryRequest {
            justification: "Different".to_string(),
            ..request1.clone()
        };
        assert_ne!(request1.compute_hash(), request3.compute_hash());
    }

    #[test]
    fn test_has_rejection_detection() {
        let mut state = RecoveryCeremonyState {
            ceremony_id: RecoveryCeremonyId::new(&test_prestate(), &test_request_hash(), 1),
            request: CeremonyRecoveryRequest {
                account_authority: test_authority(0),
                operation: CeremonyRecoveryOperation::EmergencyFreeze,
                justification: "Test".to_string(),
                prestate_hash: test_prestate(),
                requested_at_ms: 0,
            },
            status: RecoveryCeremonyStatus::CollectingApprovals,
            approvals: HashMap::new(),
            guardians: vec![test_authority(1), test_authority(2)],
            threshold: 1,
            started_at_ms: 0,
            timeout_ms: 1000,
            agreement_mode: AgreementMode::CoordinatorSoftSafe,
        };

        // No rejections initially
        assert!(!state.has_rejection());

        // Add approval
        state.approvals.insert(
            test_authority(1),
            RecoveryApproval {
                ceremony_id: state.ceremony_id,
                guardian: test_authority(1),
                approved: true,
                rejection_reason: None,
                prestate_hash: test_prestate(),
                signature: ThresholdSignature::single_signer(vec![], vec![], 0),
                approved_at_ms: 0,
            },
        );
        assert!(!state.has_rejection());

        // Add rejection
        state.approvals.insert(
            test_authority(2),
            RecoveryApproval {
                ceremony_id: state.ceremony_id,
                guardian: test_authority(2),
                approved: false,
                rejection_reason: Some("Suspicious request".to_string()),
                prestate_hash: test_prestate(),
                signature: ThresholdSignature::single_signer(vec![], vec![], 0),
                approved_at_ms: 0,
            },
        );
        assert!(state.has_rejection());
        assert_eq!(state.rejected_count(), 1);
    }
}
