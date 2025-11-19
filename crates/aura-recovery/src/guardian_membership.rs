//! Guardian Membership Change Choreography
//!
//! Adding and removing guardians from the guardian set.
//! This choreography handles proposals, voting, and implementation of membership changes.

use crate::{
    types::{GuardianProfile, GuardianSet, RecoveryRequest, RecoveryResponse, RecoveryShare},
    RecoveryResult,
};
use aura_authenticate::guardian_auth::RecoveryContext;
use aura_core::effects::TimeEffects;
use aura_core::frost::ThresholdSignature;
use aura_core::{identifiers::GuardianId, AccountId, AuraError, DeviceId};
use aura_macros::choreography;
use aura_protocol::effects::AuraEffects;
use aura_protocol::guards::BiscuitGuardEvaluator;
use aura_wot::{BiscuitTokenManager, ResourceScope};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Type of membership change
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MembershipChange {
    /// Add new guardian to the set
    AddGuardian {
        /// Profile of the guardian to add
        guardian: GuardianProfile,
    },
    /// Remove guardian from the set
    RemoveGuardian {
        /// Identifier of the guardian to remove
        guardian_id: GuardianId,
    },
    /// Update guardian information
    UpdateGuardian {
        /// Identifier of the guardian to update
        guardian_id: GuardianId,
        /// New profile information for the guardian
        new_profile: GuardianProfile,
    },
}

/// Guardian membership change proposal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MembershipProposal {
    /// Unique identifier for this membership change
    pub change_id: String,
    /// Account affected by the membership change
    pub account_id: AccountId,
    /// Device proposing the membership change
    pub proposing_device: DeviceId,
    /// The specific membership change being proposed
    pub change: MembershipChange,
    /// New threshold to set after the change (optional)
    pub new_threshold: Option<usize>,
    /// Recovery context and justification for the change
    pub context: RecoveryContext,
}

/// Guardian vote on membership change
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianVote {
    /// Unique identifier for the membership change being voted on
    pub change_id: String,
    /// Guardian identifier of the voting party
    pub guardian_id: GuardianId,
    /// Whether the guardian approves the change
    pub approved: bool,
    /// Cryptographic signature on the vote
    pub vote_signature: Vec<u8>,
    /// Human-readable rationale for the vote
    pub rationale: String,
    /// Timestamp when the vote was cast
    pub timestamp: u64,
}

/// Membership change completion notification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeCompletion {
    /// Unique identifier for the membership change
    pub change_id: String,
    /// Whether the membership change was successful
    pub success: bool,
    /// The guardian set after the change
    pub new_guardian_set: GuardianSet,
    /// New threshold after the change
    pub new_threshold: usize,
    /// Serialized evidence of the membership change
    pub change_evidence: Vec<u8>,
}

// Guardian Membership Change Choreography - 3 phase protocol
choreography! {
    #[namespace = "guardian_membership_change"]
    protocol GuardianMembershipChange {
        roles: ChangeInitiator, Guardian1, Guardian2, Guardian3;

        // Phase 1: Membership change proposal to all guardians
        ChangeInitiator[guard_capability = "initiate_membership_change",
                        flow_cost = 350,
                        journal_facts = "membership_change_proposed",
                        leakage_budget = [1, 0, 0]]
        -> Guardian1: ProposeChange(MembershipProposal);

        ChangeInitiator[guard_capability = "initiate_membership_change",
                        flow_cost = 350]
        -> Guardian2: ProposeChange(MembershipProposal);

        ChangeInitiator[guard_capability = "initiate_membership_change",
                        flow_cost = 350]
        -> Guardian3: ProposeChange(MembershipProposal);

        // Phase 2: Guardian votes back to change initiator
        Guardian1[guard_capability = "vote_membership_change,verify_membership_proposal",
                   flow_cost = 220,
                   journal_facts = "membership_vote_cast",
                   leakage_budget = [0, 1, 0]]
        -> ChangeInitiator: CastVote(GuardianVote);

        Guardian2[guard_capability = "vote_membership_change,verify_membership_proposal",
                   flow_cost = 220,
                   journal_facts = "membership_vote_cast"]
        -> ChangeInitiator: CastVote(GuardianVote);

        Guardian3[guard_capability = "vote_membership_change,verify_membership_proposal",
                   flow_cost = 220,
                   journal_facts = "membership_vote_cast"]
        -> ChangeInitiator: CastVote(GuardianVote);

        // Phase 3: Change completion broadcast to all guardians
        ChangeInitiator[guard_capability = "complete_membership_change",
                        flow_cost = 180,
                        journal_facts = "membership_change_completed",
                        journal_merge = true]
        -> Guardian1: CompleteChange(ChangeCompletion);

        ChangeInitiator[guard_capability = "complete_membership_change",
                        flow_cost = 180,
                        journal_merge = true]
        -> Guardian2: CompleteChange(ChangeCompletion);

        ChangeInitiator[guard_capability = "complete_membership_change",
                        flow_cost = 180,
                        journal_merge = true]
        -> Guardian3: CompleteChange(ChangeCompletion);
    }
}

/// Guardian membership coordinator
pub struct GuardianMembershipCoordinator<E>
where
    E: AuraEffects + ?Sized,
{
    _effect_system: Arc<E>,
    /// Optional token manager for Biscuit authorization
    token_manager: Option<BiscuitTokenManager>,
    /// Optional guard evaluator for Biscuit authorization
    guard_evaluator: Option<BiscuitGuardEvaluator>,
}

/// Extended request for membership changes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MembershipChangeRequest {
    /// Base request information
    pub base: RecoveryRequest,
    /// The change to make
    pub change: MembershipChange,
    /// New threshold after the change (optional)
    pub new_threshold: Option<usize>,
}

impl<E> GuardianMembershipCoordinator<E>
where
    E: AuraEffects + ?Sized,
{
    /// Create new coordinator
    pub fn new(effect_system: Arc<E>) -> Self {
        Self {
            _effect_system: effect_system,
            token_manager: None,
            guard_evaluator: None,
        }
    }

    /// Create new coordinator with Biscuit authorization
    pub fn new_with_biscuit(
        effect_system: Arc<E>,
        token_manager: BiscuitTokenManager,
        guard_evaluator: BiscuitGuardEvaluator,
    ) -> Self {
        Self {
            _effect_system: effect_system,
            token_manager: Some(token_manager),
            guard_evaluator: Some(guard_evaluator),
        }
    }

    /// Execute membership change as change initiator
    pub async fn execute_membership_change(
        &self,
        request: MembershipChangeRequest,
    ) -> RecoveryResult<RecoveryResponse> {
        // Check authorization using Biscuit tokens
        if let Err(auth_error) = self.check_membership_authorization(&request).await {
            return Ok(RecoveryResponse {
                success: false,
                error: Some(format!("Authorization failed: {}", auth_error)),
                key_material: None,
                guardian_shares: Vec::new(),
                evidence: self.create_failed_evidence(&request),
                signature: self.create_empty_signature(),
            });
        }

        let change_id = self.generate_change_id(&request);

        // Convert generic request to choreography-specific proposal
        let proposal = MembershipProposal {
            change_id: change_id.clone(),
            account_id: request.base.account_id,
            proposing_device: request.base.requesting_device,
            change: request.change.clone(),
            new_threshold: request.new_threshold,
            context: request.base.context.clone(),
        };

        // Execute the choreographic protocol
        let result = self.execute_choreographic_membership_change(proposal).await;

        match result {
            Ok(votes) => {
                // Count approval votes
                let approvals: Vec<_> = votes.into_iter().filter(|v| v.approved).collect();

                // Check if we have enough approvals
                if approvals.len() < request.base.threshold {
                    return Ok(RecoveryResponse {
                        success: false,
                        error: Some(format!(
                            "Insufficient guardian approvals for membership change: got {}, need {}",
                            approvals.len(),
                            request.base.threshold
                        )),
                        key_material: None,
                        guardian_shares: Vec::new(),
                        evidence: self.create_failed_evidence(&request),
                        signature: self.create_empty_signature(),
                    });
                }

                // Apply the membership change
                let new_guardian_set =
                    self.apply_membership_change(&request.base.guardians, &request.change)?;
                let final_threshold = request.new_threshold.unwrap_or(request.base.threshold);

                // Validate the new configuration
                if new_guardian_set.len() < final_threshold {
                    return Ok(RecoveryResponse {
                        success: false,
                        error: Some(format!(
                            "Invalid configuration: {} guardians cannot satisfy threshold of {}",
                            new_guardian_set.len(),
                            final_threshold
                        )),
                        key_material: None,
                        guardian_shares: Vec::new(),
                        evidence: self.create_failed_evidence(&request),
                        signature: self.create_empty_signature(),
                    });
                }

                // Convert votes to shares for compatibility
                let shares = approvals
                    .into_iter()
                    .map(|vote| {
                        RecoveryShare {
                            guardian: GuardianProfile {
                                guardian_id: vote.guardian_id,
                                device_id: DeviceId::new(), // Placeholder
                                label: "Guardian".to_string(),
                                trust_level: aura_core::TrustLevel::High,
                                cooldown_secs: 900,
                            },
                            share: vote.rationale.into_bytes(),
                            partial_signature: vote.vote_signature,
                            issued_at: vote.timestamp,
                        }
                    })
                    .collect::<Vec<_>>();

                // Create evidence and signature
                let evidence = self.create_evidence(&request, &shares);
                let signature = self.aggregate_signature(&shares);

                // Create completion message for final phase
                let completion = ChangeCompletion {
                    change_id: change_id.clone(),
                    success: true,
                    new_guardian_set: new_guardian_set.clone(),
                    new_threshold: final_threshold,
                    change_evidence: serde_json::to_vec(&evidence).unwrap_or_default(),
                };

                // Phase 3 would broadcast completion through choreography
                self.broadcast_change_completion(completion).await?;

                Ok(RecoveryResponse {
                    success: true,
                    error: None,
                    key_material: None, // Membership changes don't produce key material
                    guardian_shares: shares,
                    evidence,
                    signature,
                })
            }
            Err(e) => Ok(RecoveryResponse {
                success: false,
                error: Some(format!("Membership change choreography failed: {}", e)),
                key_material: None,
                guardian_shares: Vec::new(),
                evidence: self.create_failed_evidence(&request),
                signature: self.create_empty_signature(),
            }),
        }
    }

    /// Execute as guardian (vote on membership change)
    pub async fn vote_as_guardian(
        &self,
        proposal: MembershipProposal,
        approved: bool,
    ) -> RecoveryResult<GuardianVote> {
        // For now, simulate guardian voting
        // In real implementation, this would run the guardian side of the choreography
        let rationale = if approved {
            "Change approved after review".to_string()
        } else {
            "Change denied due to security concerns".to_string()
        };

        Ok(GuardianVote {
            guardian_id: GuardianId::new(), // Would be actual guardian ID
            change_id: proposal.change_id,
            approved,
            vote_signature: vec![1; 64], // Placeholder signature
            rationale,
            timestamp: 0, // Placeholder timestamp
        })
    }

    /// Execute choreographic membership change protocol (Phase 1-2)
    async fn execute_choreographic_membership_change(
        &self,
        proposal: MembershipProposal,
    ) -> RecoveryResult<Vec<GuardianVote>> {
        // Phase 1: Send proposals to all guardians (choreographic send operations)
        // This would be handled by the generated choreography runtime

        // Phase 2: Collect guardian votes (choreographic receive operations)
        // For now, simulate the expected responses that would come through choreography
        let timestamp = self.current_timestamp().await;
        let votes = vec![
            GuardianVote {
                guardian_id: GuardianId::new(),
                change_id: proposal.change_id.clone(),
                approved: true,
                vote_signature: vec![1; 64],
                rationale: "Approved - change looks valid".to_string(),
                timestamp,
            },
            GuardianVote {
                guardian_id: GuardianId::new(),
                change_id: proposal.change_id.clone(),
                approved: true,
                vote_signature: vec![2; 64],
                rationale: "Approved - meets security requirements".to_string(),
                timestamp,
            },
        ];

        Ok(votes)
    }

    /// Broadcast change completion (Phase 3)
    async fn broadcast_change_completion(
        &self,
        _completion: ChangeCompletion,
    ) -> RecoveryResult<()> {
        // This would be handled by the choreographic broadcast in the generated code
        // The choreography runtime would send completion messages to all guardians
        Ok(())
    }

    /// Get current timestamp
    async fn current_timestamp(&self) -> u64 {
        TimeEffects::current_timestamp(self._effect_system.as_ref()).await
    }

    /// Generate unique change ID
    fn generate_change_id(&self, request: &MembershipChangeRequest) -> String {
        format!(
            "membership_{}_{}",
            request.base.account_id, request.base.requesting_device
        )
    }

    fn aggregate_signature(&self, shares: &[RecoveryShare]) -> ThresholdSignature {
        let mut combined_signature = Vec::new();
        for share in shares {
            combined_signature.extend_from_slice(&share.partial_signature);
        }

        let signature_bytes = if combined_signature.len() >= 64 {
            combined_signature[..64].to_vec()
        } else {
            let mut padded = combined_signature;
            padded.resize(64, 0);
            padded
        };

        let signers: Vec<u16> = shares
            .iter()
            .enumerate()
            .map(|(idx, _)| idx as u16)
            .collect();

        ThresholdSignature::new(signature_bytes, signers)
    }

    fn apply_membership_change(
        &self,
        current_set: &GuardianSet,
        change: &MembershipChange,
    ) -> RecoveryResult<GuardianSet> {
        let mut guardians = current_set.clone().into_vec();

        match change {
            MembershipChange::AddGuardian { guardian } => {
                // Check if guardian already exists
                if guardians
                    .iter()
                    .any(|g| g.guardian_id == guardian.guardian_id)
                {
                    return Err(AuraError::invalid("Guardian already exists in set"));
                }
                guardians.push(guardian.clone());
            }
            MembershipChange::RemoveGuardian { guardian_id } => {
                guardians.retain(|g| g.guardian_id != *guardian_id);
                if guardians.is_empty() {
                    return Err(AuraError::invalid("Cannot remove last guardian"));
                }
            }
            MembershipChange::UpdateGuardian {
                guardian_id,
                new_profile,
            } => {
                if let Some(guardian) = guardians.iter_mut().find(|g| g.guardian_id == *guardian_id)
                {
                    *guardian = new_profile.clone();
                } else {
                    return Err(AuraError::invalid("Guardian not found in set"));
                }
            }
        }

        Ok(GuardianSet::new(guardians))
    }

    fn create_evidence(
        &self,
        _request: &MembershipChangeRequest,
        _shares: &[RecoveryShare],
    ) -> crate::types::RecoveryEvidence {
        // Placeholder implementation
        crate::types::RecoveryEvidence {
            account_id: AccountId::new(),
            recovering_device: DeviceId::new(),
            guardians: Vec::new(),
            issued_at: 0,
            cooldown_expires_at: 0,
            dispute_window_ends_at: 0,
            guardian_profiles: Vec::new(),
            disputes: Vec::new(),
            threshold_signature: None,
        }
    }

    fn create_failed_evidence(
        &self,
        _request: &MembershipChangeRequest,
    ) -> crate::types::RecoveryEvidence {
        // Placeholder implementation
        crate::types::RecoveryEvidence {
            account_id: AccountId::new(),
            recovering_device: DeviceId::new(),
            guardians: Vec::new(),
            issued_at: 0,
            cooldown_expires_at: 0,
            dispute_window_ends_at: 0,
            guardian_profiles: Vec::new(),
            disputes: Vec::new(),
            threshold_signature: None,
        }
    }

    fn create_empty_signature(&self) -> ThresholdSignature {
        ThresholdSignature::new(vec![0; 64], vec![])
    }

    /// Check if the membership change request is authorized using Biscuit tokens
    async fn check_membership_authorization(
        &self,
        request: &MembershipChangeRequest,
    ) -> Result<(), String> {
        let (token_manager, guard_evaluator) = match (&self.token_manager, &self.guard_evaluator) {
            (Some(tm), Some(ge)) => (tm, ge),
            _ => return Err("Biscuit authorization components not available".to_string()),
        };

        let token = token_manager.current_token();

        let resource_scope = ResourceScope::Recovery {
            recovery_type: "GuardianSet".to_string(),
        };

        // Check authorization for membership change initiation
        let authorized = guard_evaluator
            .check_guard(token, "initiate_membership_change", &resource_scope)
            .map_err(|e| format!("Biscuit authorization error: {}", e))?;

        if !authorized {
            return Err(
                "Biscuit token does not grant permission to initiate membership change".to_string(),
            );
        }

        Ok(())
    }
}
