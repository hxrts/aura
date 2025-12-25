//! Recovery Protocol using Relational Contexts
//!
//! This module implements the recovery protocol using RelationalContexts,
//! replacing the device-centric recovery model with authority-based recovery.

use crate::facts::{RecoveryFact, RecoveryFactEmitter};
use aura_core::effects::{JournalEffects, NetworkEffects, PhysicalTimeEffects};
use aura_core::epochs::Epoch;
use aura_core::frost::{PublicKeyPackage, Share};
use aura_core::hash;
use aura_core::identifiers::ContextId;
use aura_core::relational::{ConsensusProof, RecoveryGrant, RecoveryOp};
use aura_core::time::{PhysicalTime, TimeStamp};
use aura_core::Prestate;
use aura_core::{AuraError, AuthorityId, Hash32, Result};
use aura_journal::DomainFact;
use aura_macros::choreography;
use aura_relational::RelationalContext;
use futures::lock::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Recovery protocol coordinator
#[derive(Debug, Clone)]
pub struct RecoveryProtocol {
    /// Recovery context for coordination
    pub recovery_context: Arc<RelationalContext>,
    /// Account authority being recovered
    pub account_authority: AuthorityId,
    /// Guardian authorities participating
    pub guardian_authorities: Vec<AuthorityId>,
    /// Recovery threshold
    pub threshold: usize,
    /// Collected guardian approvals
    approvals: Vec<GuardianApproval>,
}

/// Recovery request data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryRequest {
    /// Unique recovery ceremony ID
    pub recovery_id: String,
    /// Account authority requesting recovery
    pub account_authority: AuthorityId,
    /// New tree commitment after recovery
    pub new_tree_commitment: Hash32,
    /// Recovery operation type
    pub operation: RecoveryOperation,
    /// Justification for recovery
    pub justification: String,
}

/// Recovery operation types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecoveryOperation {
    /// Replace the entire tree (device key recovery)
    ReplaceTree {
        /// New public key for the recovered tree
        new_public_key: Vec<u8>,
    },
    /// Add a new device to existing tree
    AddDevice {
        /// Public key of the new device
        device_public_key: Vec<u8>,
    },
    /// Remove a compromised device
    RemoveDevice {
        /// Leaf index of device to remove
        leaf_index: u32,
    },
    /// Update guardian set
    UpdateGuardians {
        /// New guardian authorities
        new_guardians: Vec<AuthorityId>,
        /// New threshold
        new_threshold: usize,
    },
}

/// Guardian approval for recovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianApproval {
    /// Guardian authority ID
    pub guardian_id: AuthorityId,
    /// Recovery request being approved
    pub recovery_id: String,
    /// Guardian's signature over the recovery grant
    pub signature: Vec<u8>,
    /// Timestamp
    pub timestamp: TimeStamp,
}

/// Recovery outcome from a ceremony
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryOutcome {
    /// Whether recovery succeeded
    pub success: bool,
    /// Recovery grant if successful
    pub recovery_grant: Option<RecoveryGrant>,
    /// Error message if failed
    pub error: Option<String>,
    /// Guardian approvals received
    pub approvals: Vec<GuardianApproval>,
}

impl RecoveryProtocol {
    /// Create a new recovery protocol
    pub fn new(
        recovery_context: Arc<RelationalContext>,
        account_authority: AuthorityId,
        guardian_authorities: Vec<AuthorityId>,
        threshold: usize,
    ) -> Self {
        Self {
            recovery_context,
            account_authority,
            guardian_authorities,
            threshold,
            approvals: Vec::new(),
        }
    }

    /// Get current tree commitment
    fn current_commitment(&self) -> Result<Hash32> {
        self.recovery_context.journal_commitment()
    }

    /// Get guardian commitment
    fn guardian_commitment(&self) -> Hash32 {
        let mut bytes = Vec::new();
        for guardian in &self.guardian_authorities {
            bytes.extend_from_slice(&guardian.to_bytes());
        }
        Hash32::from_bytes(&hash::hash(&bytes))
    }

    /// Run consensus protocol
    async fn run_consensus(&self, operation: &RecoveryOperation) -> Result<ConsensusProof> {
        // Create prestate
        let prestate = Prestate {
            authority_commitments: vec![(self.account_authority, self.current_commitment()?)],
            context_commitment: self.recovery_context.journal_commitment()?,
        };

        // Run consensus using consensus adapter
        // For recovery, we use empty key packages since this is coordination, not FROST signing
        let key_packages: HashMap<AuthorityId, Share> = HashMap::new();
        let derived_group_key = hash::hash(self.account_authority.0.as_bytes()).to_vec();
        let group_public_key = PublicKeyPackage::new(
            derived_group_key,
            std::collections::BTreeMap::new(), // empty signer keys for recovery coordination
            1,
            1,
        );
        let epoch = Epoch::from(1); // Recovery uses a default epoch

        aura_relational::run_consensus(&prestate, operation, key_packages, group_public_key, epoch)
            .await
    }

    /// Initiate recovery ceremony
    pub async fn initiate_recovery(&mut self, request: RecoveryRequest) -> Result<RecoveryOutcome> {
        // Validate request
        if request.account_authority != self.account_authority {
            return Err(AuraError::invalid("Account authority mismatch"));
        }
        // Reset approvals for a new ceremony
        self.approvals.clear();

        // Create recovery operation
        let recovery_op = match &request.operation {
            RecoveryOperation::ReplaceTree { .. } => RecoveryOp::ReplaceTree {
                new_tree_root: request.new_tree_commitment,
            },
            RecoveryOperation::AddDevice { device_public_key } => RecoveryOp::AddDevice {
                device_public_key: device_public_key.clone(),
            },
            RecoveryOperation::RemoveDevice { leaf_index } => RecoveryOp::RemoveDevice {
                leaf_index: *leaf_index,
            },
            RecoveryOperation::UpdateGuardians { new_threshold, .. } => RecoveryOp::UpdatePolicy {
                new_threshold: *new_threshold as u16,
            },
        };

        // Run consensus to get proof
        let consensus_proof = self.run_consensus(&request.operation).await?;

        // Create recovery grant
        let grant = RecoveryGrant {
            account_old: self.current_commitment()?,
            account_new: request.new_tree_commitment,
            guardian: self.guardian_commitment(),
            operation: recovery_op,
            consensus_proof,
        };

        // Record as a context-scoped detail fact.
        let _ = self
            .recovery_context
            .add_recovery_grant(self.account_authority, grant.clone())?;

        let result = RecoveryOutcome {
            success: true,
            recovery_grant: Some(grant),
            error: None,
            approvals: self.approvals.clone(),
        };

        Ok(result)
    }

    /// Process guardian approval
    pub async fn process_guardian_approval(&mut self, approval: GuardianApproval) -> Result<()> {
        // Verify guardian is in the set
        if !self.guardian_authorities.contains(&approval.guardian_id) {
            return Err(AuraError::permission_denied("Guardian not in recovery set"));
        }

        if approval.signature.is_empty() {
            return Err(AuraError::invalid("Missing guardian signature"));
        }

        // Record approval if unique
        if !self
            .approvals
            .iter()
            .any(|existing| existing.guardian_id == approval.guardian_id)
        {
            self.approvals.push(approval);
        }

        if !self.is_threshold_met(&self.approvals) {
            return Err(AuraError::permission_denied(
                "Recovery threshold not yet satisfied",
            ));
        }

        Ok(())
    }

    /// Check if recovery threshold is met
    pub fn is_threshold_met(&self, approvals: &[GuardianApproval]) -> bool {
        // Count unique guardian approvals
        let unique_guardians: std::collections::HashSet<_> =
            approvals.iter().map(|a| a.guardian_id).collect();

        unique_guardians.len() >= self.threshold
    }
}

// Recovery Protocol Choreography
choreography! {
    #[namespace = "recovery_protocol"]
    protocol RecoveryProtocol {
        roles: Account, Guardian, Coordinator;

        // Step 1: Account initiates recovery
        Account[guard_capability = "initiate_recovery", flow_cost = 100]
        -> Coordinator: InitiateRecovery(RecoveryRequest);

        // Step 2: Coordinator distributes to guardians
        Coordinator[guard_capability = "coordinate_recovery", flow_cost = 50]
        -> Guardian: DistributeRecoveryRequest(RecoveryRequest);

        // Step 3: Guardians submit approvals
        Guardian[guard_capability = "approve_recovery", flow_cost = 50]
        -> Coordinator: SubmitApproval(GuardianApproval);

        // Step 4: Coordinator aggregates and responds
        Coordinator[guard_capability = "finalize_recovery", flow_cost = 100]
        -> Account: RecoveryComplete(RecoveryOutcome);
    }
}

/// Recovery protocol handler
pub struct RecoveryProtocolHandler {
    protocol: Arc<RecoveryProtocol>,
    approvals: Arc<Mutex<HashMap<String, Vec<GuardianApproval>>>>,
}

impl RecoveryProtocolHandler {
    /// Create a new recovery handler
    pub fn new(protocol: Arc<RecoveryProtocol>) -> Self {
        Self {
            protocol,
            approvals: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Emit a recovery fact to the journal
    async fn emit_fact(
        &self,
        fact: RecoveryFact,
        time_effects: &dyn PhysicalTimeEffects,
        journal_effects: &dyn JournalEffects,
    ) -> Result<()> {
        let timestamp = time_effects.physical_time().await?.ts_ms;

        let mut journal = journal_effects.get_journal().await?;
        journal.facts.insert_with_context(
            RecoveryFactEmitter::fact_key(&fact),
            aura_core::FactValue::Bytes(DomainFact::to_bytes(&fact)),
            fact.context_id().to_string(),
            timestamp,
            None,
        );
        journal_effects.persist_journal(&journal).await?;
        Ok(())
    }

    /// Handle recovery initiation
    pub async fn handle_recovery_initiation(
        &self,
        request: RecoveryRequest,
        time_effects: &dyn PhysicalTimeEffects,
        network: &dyn NetworkEffects,
        journal: &dyn JournalEffects,
    ) -> Result<()> {
        // Initialize approval tracking
        let mut approvals = self.approvals.lock().await;
        approvals.insert(request.recovery_id.clone(), Vec::new());

        // Create context ID for this recovery ceremony
        let context_id = ContextId::new_from_entropy(hash::hash(request.recovery_id.as_bytes()));

        // Emit RecoveryInitiated fact
        let timestamp = time_effects.physical_time().await?.ts_ms;
        let request_hash = Hash32(hash::hash(request.recovery_id.as_bytes()));
        let initiated_fact = RecoveryFact::RecoveryInitiated {
            context_id,
            account_id: request.account_authority,
            trace_id: Some(request.recovery_id.clone()),
            request_hash,
            initiated_at: PhysicalTime {
                ts_ms: timestamp,
                uncertainty: None,
            },
        };
        self.emit_fact(initiated_fact, time_effects, journal)
            .await?;

        // Notify guardians via effects
        self.notify_guardians_via_effects(&request, time_effects, network, journal)
            .await?;

        Ok(())
    }

    /// Handle guardian approval
    pub async fn handle_guardian_approval(
        &self,
        approval: GuardianApproval,
        time_effects: &dyn PhysicalTimeEffects,
        network: &dyn NetworkEffects,
        journal: &dyn JournalEffects,
    ) -> Result<bool> {
        // Create context ID for this recovery ceremony
        let context_id = ContextId::new_from_entropy(hash::hash(approval.recovery_id.as_bytes()));

        // Emit RecoveryShareSubmitted fact
        let timestamp = time_effects.physical_time().await?.ts_ms;
        let share_hash = Hash32(hash::hash(&approval.signature));
        let share_fact = RecoveryFact::RecoveryShareSubmitted {
            context_id,
            guardian_id: approval.guardian_id,
            trace_id: Some(approval.recovery_id.clone()),
            share_hash,
            submitted_at: PhysicalTime {
                ts_ms: timestamp,
                uncertainty: None,
            },
        };
        self.emit_fact(share_fact, time_effects, journal).await?;

        // Add approval
        let mut approvals = self.approvals.lock().await;
        let ceremony_approvals = approvals
            .entry(approval.recovery_id.clone())
            .or_insert_with(Vec::new);

        ceremony_approvals.push(approval.clone());

        // Check if threshold met
        let threshold_met = self.protocol.is_threshold_met(ceremony_approvals);

        if threshold_met {
            // Finalize recovery via effects
            self.finalize_recovery_via_effects(
                &approval.recovery_id,
                ceremony_approvals,
                time_effects,
                network,
                journal,
            )
            .await?;
        }

        Ok(threshold_met)
    }

    /// Notify guardians about recovery request via NetworkEffects
    async fn notify_guardians_via_effects(
        &self,
        request: &RecoveryRequest,
        time_effects: &dyn PhysicalTimeEffects,
        network: &dyn NetworkEffects,
        journal: &dyn JournalEffects,
    ) -> Result<()> {
        // Serialize the recovery request
        let message_data =
            serde_json::to_vec(request).map_err(|e| AuraError::serialization(e.to_string()))?;

        // Send recovery request to each guardian via network effects
        for guardian_id in &self.protocol.guardian_authorities {
            network
                .send_to_peer(guardian_id.0, message_data.clone())
                .await
                .map_err(|e| AuraError::network(format!("Failed to notify guardian: {}", e)))?;
        }

        // Update journal state with recovery initiation
        self.update_journal_recovery_state_via_effects(
            &request.recovery_id,
            "initiated",
            &[],
            time_effects,
            journal,
        )
        .await?;

        Ok(())
    }

    /// Finalize recovery via effects
    async fn finalize_recovery_via_effects(
        &self,
        recovery_id: &str,
        approvals: &[GuardianApproval],
        time_effects: &dyn PhysicalTimeEffects,
        network: &dyn NetworkEffects,
        journal: &dyn JournalEffects,
    ) -> Result<()> {
        // Create context ID for this recovery ceremony
        let context_id = ContextId::new_from_entropy(hash::hash(recovery_id.as_bytes()));

        // Emit RecoveryCompleted fact
        let timestamp = time_effects.physical_time().await?.ts_ms;
        let evidence_hash = Hash32(hash::hash(recovery_id.as_bytes()));
        let completed_fact = RecoveryFact::RecoveryCompleted {
            context_id,
            account_id: self.protocol.account_authority,
            trace_id: Some(recovery_id.to_string()),
            evidence_hash,
            completed_at: PhysicalTime {
                ts_ms: timestamp,
                uncertainty: None,
            },
        };
        self.emit_fact(completed_fact, time_effects, journal)
            .await?;

        // Create recovery outcome
        let result = RecoveryOutcome {
            success: true,
            recovery_grant: None, // Would be populated from actual consensus
            error: None,
            approvals: approvals.to_vec(),
        };

        // Serialize the recovery result
        let result_data =
            serde_json::to_vec(&result).map_err(|e| AuraError::serialization(e.to_string()))?;

        // Notify account of recovery completion via network effects
        network
            .send_to_peer(self.protocol.account_authority.0, result_data)
            .await
            .map_err(|e| AuraError::network(format!("Failed to notify account: {}", e)))?;

        // Update journal state with recovery completion
        self.update_journal_recovery_state_via_effects(
            recovery_id,
            "completed",
            approvals,
            time_effects,
            journal,
        )
        .await?;

        Ok(())
    }

    /// Update recovery state in journal via JournalEffects
    async fn update_journal_recovery_state_via_effects(
        &self,
        recovery_id: &str,
        state: &str,
        approvals: &[GuardianApproval],
        time_effects: &dyn PhysicalTimeEffects,
        journal_effects: &dyn JournalEffects,
    ) -> Result<()> {
        // Create a fact representing the recovery state change
        let timestamp = time_effects.physical_time().await?.ts_ms / 1000; // Convert milliseconds to seconds

        let state_data = serde_json::json!({
            "recovery_id": recovery_id,
            "state": state,
            "approvals_count": approvals.len(),
            "timestamp": timestamp,
        });

        let mut journal = journal_effects.get_journal().await?;
        journal.facts.insert_with_context(
            format!("recovery_state:{}", recovery_id),
            aura_core::journal::FactValue::String(state_data.to_string()),
            self.protocol.account_authority.to_string(),
            timestamp,
            None,
        );
        journal_effects.persist_journal(&journal).await?;

        Ok(())
    }
}
