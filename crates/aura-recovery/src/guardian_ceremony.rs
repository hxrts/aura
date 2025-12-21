//! Consensus-Based Guardian Ceremony
//!
//! This module implements a safe guardian key rotation ceremony using Aura Consensus
//! to ensure linearizable agreement on guardian set changes. The ceremony uses
//! session types to enforce protocol linearity and prestate binding to prevent forks.
//!
//! # Safety Properties
//!
//! 1. **Linear Protocol Flow**: Session types ensure exactly one outcome (commit/abort)
//! 2. **Prestate Binding**: Operations are bound to current guardian state, preventing concurrent ceremonies
//! 3. **Epoch Isolation**: Uncommitted key rotations don't affect signing capability
//! 4. **Consensus Agreement**: Guardians act as witnesses; threshold must agree for commit
//!
//! # Protocol Flow
//!
//! ```text
//! 1. Initiator proposes new guardian set with threshold k-of-n
//! 2. System computes prestate hash from current guardian configuration
//! 3. ConsensusId derived from (prestate_hash, operation_hash, nonce)
//! 4. FROST keys generated at new epoch (old epoch remains active)
//! 5. Guardians receive encrypted key shares and respond (accept/decline)
//! 6. If threshold accepts: CommitFact produced, new epoch activated
//! 7. If any declines or timeout: Ceremony fails, old epoch remains active
//! ```
//!
//! # Key Insight: Epoch Isolation
//!
//! The critical safety property is that key packages stored at uncommitted epochs
//! are inert. Only the committed epoch is used for signing. This eliminates the
//! need for explicit rollback - simply not committing is sufficient.

use crate::{effects::RecoveryEffects, RecoveryError, RecoveryResult};
use aura_core::{
    effects::{JournalEffects, PhysicalTimeEffects, RandomEffects},
    hash,
    identifiers::AuthorityId,
    time::PhysicalTime,
    Hash32,
};
use aura_macros::choreography;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

// ============================================================================
// Core Types
// ============================================================================

/// Unique identifier for a guardian ceremony instance
///
/// Derived from prestate hash, operation hash, and nonce to ensure
/// uniqueness and binding to current state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CeremonyId(pub Hash32);

impl CeremonyId {
    /// Create a new ceremony ID from components
    pub fn new(prestate_hash: Hash32, operation_hash: Hash32, nonce: u64) -> Self {
        let mut h = hash::hasher();
        h.update(b"GUARDIAN_CEREMONY_ID");
        h.update(&prestate_hash.0);
        h.update(&operation_hash.0);
        h.update(&nonce.to_le_bytes());
        CeremonyId(Hash32(h.finalize()))
    }
}

impl std::fmt::Display for CeremonyId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ceremony:{}", hex::encode(&self.0 .0[..8]))
    }
}

/// Operation to change guardian configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianRotationOp {
    /// New threshold required for operations (k)
    pub threshold_k: u16,
    /// Total number of guardians (n)
    pub total_n: u16,
    /// Authority IDs of the new guardian set
    pub guardian_ids: Vec<AuthorityId>,
    /// New epoch for the key rotation
    pub new_epoch: u64,
}

impl GuardianRotationOp {
    /// Compute the hash of this operation
    pub fn compute_hash(&self) -> Hash32 {
        let mut h = hash::hasher();
        h.update(b"GUARDIAN_ROTATION_OP");
        h.update(&self.threshold_k.to_le_bytes());
        h.update(&self.total_n.to_le_bytes());
        h.update(&(self.guardian_ids.len() as u32).to_le_bytes());
        for id in &self.guardian_ids {
            h.update(&id.to_bytes());
        }
        h.update(&self.new_epoch.to_le_bytes());
        Hash32(h.finalize())
    }
}

/// Current guardian state used for prestate computation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianState {
    /// Current epoch
    pub epoch: u64,
    /// Current threshold (k)
    pub threshold_k: u16,
    /// Current guardian authorities
    pub guardian_ids: Vec<AuthorityId>,
    /// Hash of the current public key package
    pub public_key_hash: Hash32,
}

impl GuardianState {
    /// Compute prestate hash for this guardian configuration
    pub fn compute_prestate_hash(&self, authority_id: &AuthorityId) -> Hash32 {
        let mut h = hash::hasher();
        h.update(b"GUARDIAN_PRESTATE");
        h.update(&authority_id.to_bytes());
        h.update(&self.epoch.to_le_bytes());
        h.update(&self.threshold_k.to_le_bytes());
        h.update(&(self.guardian_ids.len() as u32).to_le_bytes());

        // Sort guardian IDs for determinism
        let mut sorted_ids = self.guardian_ids.clone();
        sorted_ids.sort();
        for id in sorted_ids {
            h.update(&id.to_bytes());
        }

        h.update(&self.public_key_hash.0);
        Hash32(h.finalize())
    }

    /// Create an empty/initial guardian state
    pub fn empty() -> Self {
        Self {
            epoch: 0,
            threshold_k: 0,
            guardian_ids: Vec::new(),
            public_key_hash: Hash32::default(),
        }
    }
}

/// Guardian's response to a ceremony invitation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CeremonyResponse {
    /// Guardian accepts the new configuration
    Accept,
    /// Guardian declines participation
    Decline,
    /// Guardian hasn't responded yet
    Pending,
}

/// Status of a ceremony
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CeremonyStatus {
    /// Ceremony is waiting for guardian responses
    AwaitingResponses {
        accepted: usize,
        declined: usize,
        pending: usize,
    },
    /// Ceremony completed successfully
    Committed { new_epoch: u64 },
    /// Ceremony was aborted
    Aborted { reason: String },
}

/// Complete state of an ongoing ceremony
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CeremonyState {
    /// Unique ceremony identifier
    pub ceremony_id: CeremonyId,
    /// Authority initiating the ceremony
    pub initiator_id: AuthorityId,
    /// Prestate hash this ceremony is bound to
    pub prestate_hash: Hash32,
    /// The proposed rotation operation
    pub operation: GuardianRotationOp,
    /// Responses from each guardian
    pub responses: HashMap<AuthorityId, CeremonyResponse>,
    /// Encrypted key packages for each guardian (after key generation)
    pub key_packages: Vec<Vec<u8>>,
    /// Public key package for the new configuration
    pub public_key_package: Vec<u8>,
    /// Current status
    pub status: CeremonyStatus,
    /// When the ceremony was initiated
    pub initiated_at: PhysicalTime,
    /// When the ceremony was completed (if completed)
    pub completed_at: Option<PhysicalTime>,
}

impl CeremonyState {
    /// Check if enough guardians have accepted
    pub fn has_threshold(&self) -> bool {
        let accepted = self
            .responses
            .values()
            .filter(|r| **r == CeremonyResponse::Accept)
            .count();
        accepted >= self.operation.threshold_k as usize
    }

    /// Check if any guardian has declined
    pub fn has_decline(&self) -> bool {
        self.responses
            .values()
            .any(|r| *r == CeremonyResponse::Decline)
    }

    /// Check if all guardians have responded
    pub fn all_responded(&self) -> bool {
        !self
            .responses
            .values()
            .any(|r| *r == CeremonyResponse::Pending)
    }

    /// Get count of responses by type
    pub fn response_counts(&self) -> (usize, usize, usize) {
        let accepted = self
            .responses
            .values()
            .filter(|r| **r == CeremonyResponse::Accept)
            .count();
        let declined = self
            .responses
            .values()
            .filter(|r| **r == CeremonyResponse::Decline)
            .count();
        let pending = self
            .responses
            .values()
            .filter(|r| **r == CeremonyResponse::Pending)
            .count();
        (accepted, declined, pending)
    }
}

// ============================================================================
// Ceremony Facts
// ============================================================================

/// Facts emitted during guardian ceremonies for journal persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CeremonyFact {
    /// Ceremony was initiated
    Initiated {
        ceremony_id: Hash32,
        initiator_id: AuthorityId,
        prestate_hash: Hash32,
        operation_hash: Hash32,
        threshold_k: u16,
        total_n: u16,
        guardian_ids: Vec<AuthorityId>,
        initiated_at: PhysicalTime,
    },
    /// Guardian responded to ceremony
    GuardianResponded {
        ceremony_id: Hash32,
        guardian_id: AuthorityId,
        response: CeremonyResponse,
        responded_at: PhysicalTime,
    },
    /// Ceremony was committed (new epoch activated)
    Committed {
        ceremony_id: Hash32,
        new_epoch: u64,
        threshold_k: u16,
        guardian_ids: Vec<AuthorityId>,
        committed_at: PhysicalTime,
    },
    /// Ceremony was aborted
    Aborted {
        ceremony_id: Hash32,
        reason: String,
        aborted_at: PhysicalTime,
    },
}

impl CeremonyFact {
    /// Get a unique key for this fact
    pub fn fact_key(&self) -> String {
        match self {
            CeremonyFact::Initiated { ceremony_id, .. } => {
                format!("ceremony:{}:initiated", hex::encode(&ceremony_id.0[..8]))
            }
            CeremonyFact::GuardianResponded {
                ceremony_id,
                guardian_id,
                ..
            } => {
                format!(
                    "ceremony:{}:response:{}",
                    hex::encode(&ceremony_id.0[..8]),
                    guardian_id
                )
            }
            CeremonyFact::Committed { ceremony_id, .. } => {
                format!("ceremony:{}:committed", hex::encode(&ceremony_id.0[..8]))
            }
            CeremonyFact::Aborted { ceremony_id, .. } => {
                format!("ceremony:{}:aborted", hex::encode(&ceremony_id.0[..8]))
            }
        }
    }

    /// Get the ceremony ID
    pub fn ceremony_id(&self) -> Hash32 {
        match self {
            CeremonyFact::Initiated { ceremony_id, .. } => *ceremony_id,
            CeremonyFact::GuardianResponded { ceremony_id, .. } => *ceremony_id,
            CeremonyFact::Committed { ceremony_id, .. } => *ceremony_id,
            CeremonyFact::Aborted { ceremony_id, .. } => *ceremony_id,
        }
    }
}

// ============================================================================
// Choreography Definition
// ============================================================================

// Guardian Ceremony Choreography - uses session types for linear protocol flow
choreography! {
    #[namespace = "guardian_ceremony"]
    protocol GuardianCeremony {
        roles: Initiator, Guardian[n];

        // Phase 1: Propose rotation (bound to prestate)
        Initiator[guard_capability = "initiate_guardian_ceremony",
                  flow_cost = 500,
                  journal_facts = "ceremony_initiated",
                  leakage_budget = [1, 0, 0]]
        -> Guardian[*]: ProposeRotation(CeremonyProposal);

        // Phase 2: Guardians respond (exactly once per guardian - linear!)
        Guardian[*][guard_capability = "respond_guardian_ceremony",
                    flow_cost = 200,
                    journal_facts = "guardian_responded",
                    leakage_budget = [0, 1, 0]]
        -> Initiator: RespondCeremony(CeremonyResponseMsg);

        // Phase 3: Commit or Abort (exclusive choice - exactly one outcome)
        choice {
            // All required guardians accepted - commit
            Initiator[guard_capability = "commit_guardian_ceremony",
                      flow_cost = 300,
                      journal_facts = "ceremony_committed",
                      journal_merge = true]
            -> Guardian[*]: CommitCeremony(CeremonyCommit);
        } or {
            // Some guardian declined or timeout - abort
            Initiator[guard_capability = "abort_guardian_ceremony",
                      flow_cost = 100,
                      journal_facts = "ceremony_aborted"]
            -> Guardian[*]: AbortCeremony(CeremonyAbort);
        }
    }
}

/// Ceremony proposal sent to guardians
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CeremonyProposal {
    /// Unique ceremony identifier
    pub ceremony_id: CeremonyId,
    /// Authority initiating the ceremony
    pub initiator_id: AuthorityId,
    /// Prestate hash this ceremony is bound to
    pub prestate_hash: Hash32,
    /// The proposed operation
    pub operation: GuardianRotationOp,
    /// Encrypted key package for this specific guardian
    pub encrypted_key_package: Vec<u8>,
    /// Nonce for decryption
    pub encryption_nonce: [u8; 12],
    /// Ephemeral public key for key agreement
    pub ephemeral_public_key: Vec<u8>,
}

/// Response from a guardian
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CeremonyResponseMsg {
    /// Ceremony being responded to
    pub ceremony_id: CeremonyId,
    /// Guardian sending the response
    pub guardian_id: AuthorityId,
    /// The response
    pub response: CeremonyResponse,
    /// Guardian's signature over the ceremony (for commit proof)
    pub signature: Vec<u8>,
}

/// Commit message finalizing the ceremony
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CeremonyCommit {
    /// Ceremony being committed
    pub ceremony_id: CeremonyId,
    /// New epoch that is now active
    pub new_epoch: u64,
    /// Aggregated signatures from accepting guardians
    pub threshold_signature: Vec<u8>,
    /// List of guardians who accepted
    pub participants: Vec<AuthorityId>,
}

/// Abort message canceling the ceremony
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CeremonyAbort {
    /// Ceremony being aborted
    pub ceremony_id: CeremonyId,
    /// Reason for abort
    pub reason: String,
}

// ============================================================================
// Ceremony Executor
// ============================================================================

/// Executes guardian ceremonies with consensus guarantees
pub struct GuardianCeremonyExecutor<E: RecoveryEffects> {
    effects: Arc<E>,
}

impl<E: RecoveryEffects + 'static> GuardianCeremonyExecutor<E> {
    /// Create a new ceremony executor
    pub fn new(effects: Arc<E>) -> Self {
        Self { effects }
    }

    /// Emit a ceremony fact to the journal
    async fn emit_fact(&self, fact: CeremonyFact) -> RecoveryResult<()> {
        let timestamp = self
            .effects
            .physical_time()
            .await
            .map(|t| t.ts_ms)
            .unwrap_or(0);

        let mut journal = self.effects.get_journal().await?;
        journal.facts.insert_with_context(
            fact.fact_key(),
            aura_core::FactValue::Bytes(serde_json::to_vec(&fact).unwrap_or_default()),
            format!("ceremony:{}", hex::encode(&fact.ceremony_id().0[..8])),
            timestamp,
            None,
        );
        self.effects.persist_journal(&journal).await?;
        Ok(())
    }

    /// Get current guardian state for prestate computation
    pub async fn get_current_guardian_state(
        &self,
        authority_id: &AuthorityId,
    ) -> RecoveryResult<GuardianState> {
        // Get full threshold state from the signing service
        if let Some(state) = self.effects.threshold_state(authority_id).await {
            let public_key = self
                .effects
                .public_key_package(authority_id)
                .await
                .unwrap_or_default();
            let public_key_hash = Hash32(hash::hash(&public_key));

            // Parse guardian ID strings back to AuthorityIds
            let guardian_ids: Vec<AuthorityId> = state
                .guardian_ids
                .iter()
                .filter_map(|s| s.parse::<AuthorityId>().ok())
                .collect();

            Ok(GuardianState {
                epoch: state.epoch,
                threshold_k: state.threshold,
                guardian_ids,
                public_key_hash,
            })
        } else {
            // No existing guardian configuration
            Ok(GuardianState::empty())
        }
    }

    /// Check if there's a pending ceremony for this authority
    pub async fn has_pending_ceremony(&self, _authority_id: &AuthorityId) -> RecoveryResult<bool> {
        // Check journal for initiated but not committed/aborted ceremonies
        let journal = self.effects.get_journal().await?;

        // Look for ceremony facts
        for (key, _) in journal.facts.iter() {
            if key.starts_with("ceremony:") && key.ends_with(":initiated") {
                // Check if this ceremony has a corresponding commit or abort
                let ceremony_prefix = key.trim_end_matches(":initiated");
                let has_commit = journal
                    .facts
                    .contains_key(&format!("{}:committed", ceremony_prefix));
                let has_abort = journal
                    .facts
                    .contains_key(&format!("{}:aborted", ceremony_prefix));

                if !has_commit && !has_abort {
                    return Ok(true); // Found a pending ceremony
                }
            }
        }

        Ok(false)
    }

    /// Initiate a new guardian ceremony
    ///
    /// This is the main entry point for starting a guardian rotation.
    /// Returns the ceremony state which can be used to track progress.
    pub async fn initiate_ceremony(
        &self,
        authority_id: AuthorityId,
        new_threshold_k: u16,
        new_guardian_ids: Vec<AuthorityId>,
    ) -> RecoveryResult<CeremonyState> {
        let total_n = new_guardian_ids.len() as u16;

        // Validate inputs
        if new_threshold_k > total_n {
            return Err(RecoveryError::invalid(format!(
                "Threshold {} cannot exceed total guardians {}",
                new_threshold_k, total_n
            )));
        }

        if new_guardian_ids.is_empty() {
            return Err(RecoveryError::invalid("Must have at least one guardian"));
        }

        // Check for pending ceremony
        if self.has_pending_ceremony(&authority_id).await? {
            return Err(RecoveryError::invalid(
                "Cannot start ceremony: another ceremony is already pending",
            ));
        }

        // Get current state and compute prestate hash
        let current_state = self.get_current_guardian_state(&authority_id).await?;
        let prestate_hash = current_state.compute_prestate_hash(&authority_id);

        // Generate nonce for ceremony ID
        let nonce_bytes = self.effects.random_bytes(8).await;
        let nonce = u64::from_le_bytes(nonce_bytes.try_into().unwrap_or([0u8; 8]));

        // Create the operation
        let new_epoch = current_state.epoch + 1;
        let operation = GuardianRotationOp {
            threshold_k: new_threshold_k,
            total_n,
            guardian_ids: new_guardian_ids.clone(),
            new_epoch,
        };
        let operation_hash = operation.compute_hash();

        // Derive ceremony ID
        let ceremony_id = CeremonyId::new(prestate_hash, operation_hash, nonce);

        tracing::info!(
            %ceremony_id,
            %authority_id,
            threshold = new_threshold_k,
            guardians = total_n,
            new_epoch,
            "Initiating guardian ceremony"
        );

        // Generate new FROST keys at the new epoch
        // IMPORTANT: These keys are stored but NOT activated until commit
        let guardian_id_strings: Vec<String> =
            new_guardian_ids.iter().map(|id| id.to_string()).collect();

        let (_epoch, key_packages, public_key_package) = self
            .effects
            .rotate_keys(
                &authority_id,
                new_threshold_k,
                total_n,
                &guardian_id_strings,
            )
            .await
            .map_err(|e| RecoveryError::internal(format!("Key rotation failed: {}", e)))?;

        // Initialize response tracking
        let mut responses = HashMap::new();
        for guardian_id in &new_guardian_ids {
            responses.insert(*guardian_id, CeremonyResponse::Pending);
        }

        let now = self.effects.physical_time().await.unwrap_or(PhysicalTime {
            ts_ms: 0,
            uncertainty: None,
        });

        // Emit initiated fact
        self.emit_fact(CeremonyFact::Initiated {
            ceremony_id: ceremony_id.0,
            initiator_id: authority_id,
            prestate_hash,
            operation_hash,
            threshold_k: new_threshold_k,
            total_n,
            guardian_ids: new_guardian_ids.clone(),
            initiated_at: now.clone(),
        })
        .await?;

        let state = CeremonyState {
            ceremony_id,
            initiator_id: authority_id,
            prestate_hash,
            operation,
            responses,
            key_packages,
            public_key_package,
            status: CeremonyStatus::AwaitingResponses {
                accepted: 0,
                declined: 0,
                pending: new_guardian_ids.len(),
            },
            initiated_at: now,
            completed_at: None,
        };

        Ok(state)
    }

    /// Record a guardian's response to the ceremony
    pub async fn record_response(
        &self,
        state: &mut CeremonyState,
        guardian_id: AuthorityId,
        response: CeremonyResponse,
    ) -> RecoveryResult<()> {
        // Verify guardian is part of this ceremony
        if !state.responses.contains_key(&guardian_id) {
            return Err(RecoveryError::invalid(format!(
                "Guardian {} is not part of this ceremony",
                guardian_id
            )));
        }

        // Record the response
        state.responses.insert(guardian_id, response);

        let now = self.effects.physical_time().await.unwrap_or(PhysicalTime {
            ts_ms: 0,
            uncertainty: None,
        });

        // Emit response fact
        self.emit_fact(CeremonyFact::GuardianResponded {
            ceremony_id: state.ceremony_id.0,
            guardian_id,
            response,
            responded_at: now,
        })
        .await?;

        // Update status
        let (accepted, declined, pending) = state.response_counts();
        state.status = CeremonyStatus::AwaitingResponses {
            accepted,
            declined,
            pending,
        };

        tracing::info!(
            ceremony_id = %state.ceremony_id,
            %guardian_id,
            ?response,
            accepted,
            declined,
            pending,
            "Guardian responded to ceremony"
        );

        Ok(())
    }

    /// Attempt to commit the ceremony
    ///
    /// This should be called when all guardians have responded (or timeout).
    /// Returns true if the ceremony was committed, false if it was aborted.
    pub async fn try_commit(
        &self,
        state: &mut CeremonyState,
        authority_id: &AuthorityId,
    ) -> RecoveryResult<bool> {
        let now = self.effects.physical_time().await.unwrap_or(PhysicalTime {
            ts_ms: 0,
            uncertainty: None,
        });

        // Check if any guardian declined
        if state.has_decline() {
            let reason = "One or more guardians declined".to_string();
            state.status = CeremonyStatus::Aborted {
                reason: reason.clone(),
            };
            state.completed_at = Some(now.clone());

            // Emit abort fact
            self.emit_fact(CeremonyFact::Aborted {
                ceremony_id: state.ceremony_id.0,
                reason,
                aborted_at: now,
            })
            .await?;

            // Note: No explicit rollback needed - the new epoch keys are simply never activated
            tracing::info!(
                ceremony_id = %state.ceremony_id,
                "Guardian ceremony aborted - guardian declined"
            );

            return Ok(false);
        }

        // Check if we have threshold
        if !state.has_threshold() {
            if state.all_responded() {
                // All responded but didn't reach threshold
                let reason = format!(
                    "Insufficient acceptances: got {}, need {}",
                    state
                        .responses
                        .values()
                        .filter(|r| **r == CeremonyResponse::Accept)
                        .count(),
                    state.operation.threshold_k
                );
                state.status = CeremonyStatus::Aborted {
                    reason: reason.clone(),
                };
                state.completed_at = Some(now.clone());

                self.emit_fact(CeremonyFact::Aborted {
                    ceremony_id: state.ceremony_id.0,
                    reason,
                    aborted_at: now,
                })
                .await?;

                tracing::info!(
                    ceremony_id = %state.ceremony_id,
                    "Guardian ceremony aborted - insufficient acceptances"
                );

                return Ok(false);
            }

            // Still waiting for more responses
            return Ok(false);
        }

        // Threshold reached! Commit the key rotation
        let new_epoch = state.operation.new_epoch;

        self.effects
            .commit_key_rotation(authority_id, new_epoch)
            .await
            .map_err(|e| {
                RecoveryError::internal(format!("Failed to commit key rotation: {}", e))
            })?;

        // Get participants who accepted
        let participants: Vec<AuthorityId> = state
            .responses
            .iter()
            .filter(|(_, r)| **r == CeremonyResponse::Accept)
            .map(|(id, _)| *id)
            .collect();

        state.status = CeremonyStatus::Committed { new_epoch };
        state.completed_at = Some(now.clone());

        // Emit commit fact
        self.emit_fact(CeremonyFact::Committed {
            ceremony_id: state.ceremony_id.0,
            new_epoch,
            threshold_k: state.operation.threshold_k,
            guardian_ids: participants.clone(),
            committed_at: now,
        })
        .await?;

        tracing::info!(
            ceremony_id = %state.ceremony_id,
            new_epoch,
            threshold = state.operation.threshold_k,
            participants = participants.len(),
            "Guardian ceremony committed successfully"
        );

        Ok(true)
    }

    /// Abort a ceremony manually
    pub async fn abort_ceremony(
        &self,
        state: &mut CeremonyState,
        reason: String,
    ) -> RecoveryResult<()> {
        let now = self.effects.physical_time().await.unwrap_or(PhysicalTime {
            ts_ms: 0,
            uncertainty: None,
        });

        state.status = CeremonyStatus::Aborted {
            reason: reason.clone(),
        };
        state.completed_at = Some(now.clone());

        // Emit abort fact
        self.emit_fact(CeremonyFact::Aborted {
            ceremony_id: state.ceremony_id.0,
            reason: reason.clone(),
            aborted_at: now,
        })
        .await?;

        // Note: The new epoch keys are simply orphaned, not explicitly deleted
        // This is the "epoch isolation" property - uncommitted epochs are inert

        tracing::info!(
            ceremony_id = %state.ceremony_id,
            %reason,
            "Guardian ceremony aborted"
        );

        Ok(())
    }
}

// ============================================================================
// Simplified API for TUI Integration
// ============================================================================

/// High-level ceremony API for TUI integration
///
/// Provides a simplified interface that handles the ceremony lifecycle
/// with demo-mode auto-acceptance for testing.
pub struct GuardianCeremonyManager<E: RecoveryEffects> {
    executor: GuardianCeremonyExecutor<E>,
    /// If true, guardians auto-accept (for demo mode)
    pub demo_mode: bool,
}

impl<E: RecoveryEffects + 'static> GuardianCeremonyManager<E> {
    /// Create a new ceremony manager
    pub fn new(effects: Arc<E>, demo_mode: bool) -> Self {
        Self {
            executor: GuardianCeremonyExecutor::new(effects),
            demo_mode,
        }
    }

    /// Execute a complete guardian ceremony
    ///
    /// Demo mode: Automatically accepts for all guardians.
    /// Production mode: Waits for real guardian responses via protocol.
    pub async fn execute_ceremony(
        &self,
        authority_id: AuthorityId,
        threshold_k: u16,
        guardian_ids: Vec<AuthorityId>,
    ) -> RecoveryResult<CeremonyState> {
        // Initiate the ceremony
        let mut state = self
            .executor
            .initiate_ceremony(authority_id, threshold_k, guardian_ids.clone())
            .await?;

        if self.demo_mode {
            // Auto-accept for all guardians in demo mode
            for guardian_id in &guardian_ids {
                self.executor
                    .record_response(&mut state, *guardian_id, CeremonyResponse::Accept)
                    .await?;
            }

            // Commit the ceremony
            self.executor.try_commit(&mut state, &authority_id).await?;
        }

        Ok(state)
    }

    /// Check if there's a pending ceremony
    pub async fn has_pending(&self, authority_id: &AuthorityId) -> RecoveryResult<bool> {
        self.executor.has_pending_ceremony(authority_id).await
    }

    /// Get current guardian state
    pub async fn get_state(&self, authority_id: &AuthorityId) -> RecoveryResult<GuardianState> {
        self.executor.get_current_guardian_state(authority_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_testkit::MockEffects;

    fn test_authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    #[test]
    fn test_ceremony_id_deterministic() {
        let prestate = Hash32([1u8; 32]);
        let operation = Hash32([2u8; 32]);

        let id1 = CeremonyId::new(prestate, operation, 42);
        let id2 = CeremonyId::new(prestate, operation, 42);
        let id3 = CeremonyId::new(prestate, operation, 43);

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_guardian_state_prestate_hash_deterministic() {
        let authority = test_authority(1);

        let state1 = GuardianState {
            epoch: 1,
            threshold_k: 2,
            guardian_ids: vec![test_authority(2), test_authority(3)],
            public_key_hash: Hash32([0xAB; 32]),
        };

        let state2 = GuardianState {
            epoch: 1,
            threshold_k: 2,
            guardian_ids: vec![test_authority(3), test_authority(2)], // Different order
            public_key_hash: Hash32([0xAB; 32]),
        };

        // Should be equal due to sorting
        assert_eq!(
            state1.compute_prestate_hash(&authority),
            state2.compute_prestate_hash(&authority)
        );
    }

    #[test]
    fn test_ceremony_state_threshold_check() {
        let mut responses = HashMap::new();
        responses.insert(test_authority(1), CeremonyResponse::Accept);
        responses.insert(test_authority(2), CeremonyResponse::Accept);
        responses.insert(test_authority(3), CeremonyResponse::Pending);

        let state = CeremonyState {
            ceremony_id: CeremonyId(Hash32([0; 32])),
            initiator_id: test_authority(0),
            prestate_hash: Hash32([0; 32]),
            operation: GuardianRotationOp {
                threshold_k: 2,
                total_n: 3,
                guardian_ids: vec![test_authority(1), test_authority(2), test_authority(3)],
                new_epoch: 1,
            },
            responses,
            key_packages: vec![],
            public_key_package: vec![],
            status: CeremonyStatus::AwaitingResponses {
                accepted: 2,
                declined: 0,
                pending: 1,
            },
            initiated_at: PhysicalTime {
                ts_ms: 0,
                uncertainty: None,
            },
            completed_at: None,
        };

        assert!(state.has_threshold()); // 2-of-3 met
        assert!(!state.has_decline());
        assert!(!state.all_responded()); // One still pending
    }

    #[tokio::test]
    async fn test_ceremony_executor_creation() {
        let effects = Arc::new(MockEffects::deterministic());
        let executor = GuardianCeremonyExecutor::new(effects);

        let authority = test_authority(0);
        let state = executor.get_current_guardian_state(&authority).await;
        assert!(state.is_ok());
    }
}
