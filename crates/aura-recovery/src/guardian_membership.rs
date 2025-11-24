//! Guardian Membership Change Choreography
//!
//! Adding and removing guardians from the guardian set.
//! This choreography handles proposals, voting, and implementation of membership changes.

use crate::{
    coordinator::{BaseCoordinator, BaseCoordinatorAccess, RecoveryCoordinator},
    types::{GuardianProfile, GuardianSet, RecoveryRequest, RecoveryResponse, RecoveryShare},
    RecoveryResult,
};
use async_trait::async_trait;
use aura_authenticate::guardian_auth::RecoveryContext;
use aura_core::time::{PhysicalTime, TimeStamp};
use aura_core::{identifiers::GuardianId, AccountId, AuraError, DeviceId};
use aura_macros::choreography;
use aura_protocol::effects::AuraEffects;
use aura_protocol::guards::BiscuitGuardEvaluator;
use aura_wot::{BiscuitTokenManager, ContextOp};
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
    pub timestamp: TimeStamp,
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
    E: AuraEffects + ?Sized + 'static,
{
    base: BaseCoordinator<E>,
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

impl<E> BaseCoordinatorAccess<E> for GuardianMembershipCoordinator<E>
where
    E: AuraEffects + ?Sized + 'static,
{
    fn base(&self) -> &BaseCoordinator<E> {
        &self.base
    }
}

#[async_trait]
impl<E> RecoveryCoordinator<E> for GuardianMembershipCoordinator<E>
where
    E: AuraEffects + ?Sized + 'static,
{
    type Request = MembershipChangeRequest;
    type Response = RecoveryResponse;

    fn effect_system(&self) -> &Arc<E> {
        self.base_effect_system()
    }

    fn token_manager(&self) -> Option<&BiscuitTokenManager> {
        self.base_token_manager()
    }

    fn guard_evaluator(&self) -> Option<&BiscuitGuardEvaluator> {
        self.base_guard_evaluator()
    }

    fn operation_name(&self) -> &str {
        "guardian_membership"
    }

    async fn execute_recovery(&self, request: Self::Request) -> RecoveryResult<Self::Response> {
        self.execute_membership_change(request).await
    }
}

impl<E> GuardianMembershipCoordinator<E>
where
    E: AuraEffects + ?Sized + 'static,
{
    /// Create new coordinator
    pub fn new(effect_system: Arc<E>) -> Self {
        Self {
            base: BaseCoordinator::new(effect_system),
        }
    }

    /// Create new coordinator with Biscuit authorization
    pub fn new_with_biscuit(
        effect_system: Arc<E>,
        token_manager: BiscuitTokenManager,
        guard_evaluator: BiscuitGuardEvaluator,
    ) -> Self {
        Self {
            base: BaseCoordinator::new_with_biscuit(effect_system, token_manager, guard_evaluator),
        }
    }

    /// Execute membership change as change initiator
    pub async fn execute_membership_change(
        &self,
        request: MembershipChangeRequest,
    ) -> RecoveryResult<RecoveryResponse> {
        // Check authorization using the common helper
        if let Err(auth_error) = self
            .check_authorization(&request.base.account_id, ContextOp::UpdateGuardianSet)
            .await
        {
            return Ok(self.base.create_error_response(
                format!("Authorization failed: {}", auth_error),
                request.base.account_id,
                request.base.requesting_device,
            ));
        }

        let change_id =
            self.generate_operation_id(&request.base.account_id, &request.base.requesting_device);

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
                    return Ok(self.base.create_error_response(
                        format!(
                            "Insufficient guardian approvals for membership change: got {}, need {}",
                            approvals.len(),
                            request.base.threshold
                        ),
                        request.base.account_id,
                        request.base.requesting_device,
                    ));
                }

                // Apply the membership change
                let new_guardian_set =
                    self.apply_membership_change(&request.base.guardians, &request.change)?;
                let final_threshold = request.new_threshold.unwrap_or(request.base.threshold);

                // Validate the new configuration
                if new_guardian_set.len() < final_threshold {
                    return Ok(self.base.create_error_response(
                        format!(
                            "Invalid configuration: {} guardians cannot satisfy threshold of {}",
                            new_guardian_set.len(),
                            final_threshold
                        ),
                        request.base.account_id,
                        request.base.requesting_device,
                    ));
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
                            issued_at: vote.timestamp.to_index_ms() as u64,
                        }
                    })
                    .collect::<Vec<_>>();

                // Create evidence using the common utility
                let evidence = self.create_success_evidence(
                    request.base.account_id,
                    request.base.requesting_device,
                    &shares,
                );

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

                // Use the common response builder
                Ok(self.base.create_success_response(
                    None, // Membership changes don't produce key material
                    shares, evidence,
                ))
            }
            Err(e) => Ok(self.base.create_error_response(
                format!("Membership change choreography failed: {}", e),
                request.base.account_id,
                request.base.requesting_device,
            )),
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

        let ts = self
            .effect_system()
            .physical_time()
            .await
            .unwrap_or(PhysicalTime {
                ts_ms: 0,
                uncertainty: None,
            });

        Ok(GuardianVote {
            guardian_id: GuardianId::new(), // Would be actual guardian ID
            change_id: proposal.change_id,
            approved,
            vote_signature: vec![1; 64], // Placeholder signature
            rationale,
            timestamp: TimeStamp::PhysicalClock(ts),
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
        let physical_time = self
            .effect_system()
            .physical_time()
            .await
            .map_err(|e| aura_core::AuraError::internal(format!("Time error: {}", e)))?;
        let timestamp = TimeStamp::PhysicalClock(physical_time);
        let votes = vec![
            GuardianVote {
                guardian_id: GuardianId::new(),
                change_id: proposal.change_id.clone(),
                approved: true,
                vote_signature: vec![1; 64],
                rationale: "Approved - change looks valid".to_string(),
                timestamp: timestamp.clone(),
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
}
