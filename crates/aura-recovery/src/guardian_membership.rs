//! Guardian Membership Change Choreography
//!
//! Adding and removing guardians from the guardian set.
//! Uses the authority model - guardians are identified by AuthorityId.

use crate::{
    coordinator::{BaseCoordinator, BaseCoordinatorAccess, RecoveryCoordinator},
    effects::RecoveryEffects,
    facts::{MembershipChangeType, RecoveryFact, RecoveryFactEmitter},
    types::{GuardianProfile, GuardianSet, RecoveryRequest, RecoveryResponse, RecoveryShare},
    utils::EvidenceBuilder,
    RecoveryResult,
};
use async_trait::async_trait;
use aura_core::effects::{JournalEffects, PhysicalTimeEffects};
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::time::{PhysicalTime, TimeStamp};
use aura_core::{hash, AuraError, Hash32};
use aura_journal::DomainFact;
use aura_macros::choreography;
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
        /// Authority of the guardian to remove
        guardian_id: AuthorityId,
    },
    /// Update guardian information
    UpdateGuardian {
        /// Authority of the guardian to update
        guardian_id: AuthorityId,
        /// New profile information for the guardian
        new_profile: GuardianProfile,
    },
}

/// Guardian membership change proposal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MembershipProposal {
    /// Unique identifier for this membership change
    pub change_id: String,
    /// Account authority affected by the membership change
    pub account_id: AuthorityId,
    /// Authority proposing the membership change
    pub proposer_id: AuthorityId,
    /// The specific membership change being proposed
    pub change: MembershipChange,
    /// New threshold to set after the change (optional)
    pub new_threshold: Option<usize>,
    /// Timestamp of proposal
    pub timestamp: TimeStamp,
}

/// Guardian vote on membership change
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianVote {
    /// Unique identifier for the membership change being voted on
    pub change_id: String,
    /// Guardian authority of the voting party
    pub guardian_id: AuthorityId,
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

/// Guardian membership coordinator.
///
/// Stateless coordinator that derives state from facts.
pub struct GuardianMembershipCoordinator<E: RecoveryEffects> {
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

impl<E: RecoveryEffects> BaseCoordinatorAccess<E> for GuardianMembershipCoordinator<E> {
    fn base(&self) -> &BaseCoordinator<E> {
        &self.base
    }
}

#[async_trait]
impl<E: RecoveryEffects + 'static> RecoveryCoordinator<E> for GuardianMembershipCoordinator<E> {
    type Request = MembershipChangeRequest;
    type Response = RecoveryResponse;

    fn effect_system(&self) -> &Arc<E> {
        self.base_effect_system()
    }

    fn operation_name(&self) -> &str {
        "guardian_membership"
    }

    async fn execute_recovery(&self, request: Self::Request) -> RecoveryResult<Self::Response> {
        self.execute_membership_change(request).await
    }
}

impl<E: RecoveryEffects + 'static> GuardianMembershipCoordinator<E> {
    /// Create a new coordinator.
    pub fn new(effect_system: Arc<E>) -> Self {
        Self {
            base: BaseCoordinator::new(effect_system),
        }
    }

    /// Emit a recovery fact to the journal.
    async fn emit_fact(&self, fact: RecoveryFact) -> RecoveryResult<()> {
        let timestamp = self
            .effect_system()
            .physical_time()
            .await
            .map(|t| t.ts_ms)
            .unwrap_or(0);

        let mut journal = self.effect_system().get_journal().await?;
        journal.facts.insert_with_context(
            RecoveryFactEmitter::fact_key(&fact),
            aura_core::FactValue::Bytes(DomainFact::to_bytes(&fact)),
            fact.context_id().to_string(),
            timestamp,
            None,
        );
        self.effect_system().persist_journal(&journal).await?;
        Ok(())
    }

    /// Convert local MembershipChange to facts MembershipChangeType
    fn to_fact_change_type(change: &MembershipChange) -> MembershipChangeType {
        match change {
            MembershipChange::AddGuardian { guardian } => MembershipChangeType::AddGuardian {
                guardian_id: guardian.authority_id,
            },
            MembershipChange::RemoveGuardian { guardian_id } => {
                MembershipChangeType::RemoveGuardian {
                    guardian_id: *guardian_id,
                }
            }
            MembershipChange::UpdateGuardian { .. } => {
                // Update is modeled as a threshold update in the fact system
                MembershipChangeType::UpdateThreshold { new_threshold: 0 }
            }
        }
    }

    /// Execute membership change as change initiator.
    pub async fn execute_membership_change(
        &self,
        request: MembershipChangeRequest,
    ) -> RecoveryResult<RecoveryResponse> {
        // Get current timestamp for unique ID generation
        let now_ms = self
            .effect_system()
            .physical_time()
            .await
            .map(|t| t.ts_ms)
            .unwrap_or(0);

        // Create change ID and context ID using hash of account + timestamp
        let change_id = format!("membership_{}_{}", request.base.account_id, now_ms);
        let context_id = ContextId::new_from_entropy(hash::hash(change_id.as_bytes()));

        // Emit MembershipChangeProposed fact
        let proposal_hash = Hash32(hash::hash(change_id.as_bytes()));
        let proposed_fact = RecoveryFact::MembershipChangeProposed {
            context_id,
            proposer_id: request.base.initiator_id,
            change_type: Self::to_fact_change_type(&request.change),
            proposal_hash,
            proposed_at: PhysicalTime {
                ts_ms: now_ms,
                uncertainty: None,
            },
        };
        self.emit_fact(proposed_fact).await?;

        // Create proposal for choreographic protocol
        let proposal = MembershipProposal {
            change_id: change_id.clone(),
            account_id: request.base.account_id,
            proposer_id: request.base.initiator_id,
            change: request.change.clone(),
            new_threshold: request.new_threshold,
            timestamp: TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: now_ms,
                uncertainty: None,
            }),
        };

        // Execute choreographic protocol (Phase 1-2)
        let votes = self
            .execute_choreographic_membership_change(proposal)
            .await?;

        // Count approval votes
        let approvals: Vec<_> = votes.into_iter().filter(|v| v.approved).collect();

        // Check if we have enough approvals
        if approvals.len() < request.base.threshold {
            let rejected_fact =
                RecoveryFact::MembershipChangeRejected {
                    context_id,
                    proposal_hash,
                    reason: format!(
                        "Insufficient guardian approvals: got {}, need {}",
                        approvals.len(),
                        request.base.threshold
                    ),
                    rejected_at: self.effect_system().physical_time().await.unwrap_or(
                        PhysicalTime {
                            ts_ms: 0,
                            uncertainty: None,
                        },
                    ),
                };
            let _ = self.emit_fact(rejected_fact).await;

            return Ok(RecoveryResponse::error(format!(
                "Insufficient guardian approvals: got {}, need {}",
                approvals.len(),
                request.base.threshold
            )));
        }

        // Apply the membership change
        let new_guardian_set =
            self.apply_membership_change(&request.base.guardians, &request.change)?;
        let final_threshold = request.new_threshold.unwrap_or(request.base.threshold);

        // Validate the new configuration
        if new_guardian_set.len() < final_threshold {
            let rejected_fact =
                RecoveryFact::MembershipChangeRejected {
                    context_id,
                    proposal_hash,
                    reason: format!(
                        "Invalid configuration: {} guardians cannot satisfy threshold of {}",
                        new_guardian_set.len(),
                        final_threshold
                    ),
                    rejected_at: self.effect_system().physical_time().await.unwrap_or(
                        PhysicalTime {
                            ts_ms: 0,
                            uncertainty: None,
                        },
                    ),
                };
            let _ = self.emit_fact(rejected_fact).await;

            return Ok(RecoveryResponse::error(format!(
                "Invalid configuration: {} guardians cannot satisfy threshold of {}",
                new_guardian_set.len(),
                final_threshold
            )));
        }

        // Convert votes to shares
        let shares: Vec<RecoveryShare> = approvals
            .iter()
            .map(|vote| RecoveryShare {
                guardian_id: vote.guardian_id,
                guardian_label: Some(vote.rationale.clone()),
                share: change_id.as_bytes().to_vec(),
                partial_signature: vote.vote_signature.clone(),
                issued_at_ms: now_ms,
            })
            .collect();

        // Emit completion fact
        let completed_fact = RecoveryFact::MembershipChangeCompleted {
            context_id,
            proposal_hash,
            new_guardian_ids: new_guardian_set.iter().map(|g| g.authority_id).collect(),
            new_threshold: final_threshold as u16,
            completed_at: self
                .effect_system()
                .physical_time()
                .await
                .unwrap_or(PhysicalTime {
                    ts_ms: 0,
                    uncertainty: None,
                }),
        };
        self.emit_fact(completed_fact).await?;

        // Create evidence
        let evidence =
            EvidenceBuilder::success(context_id, request.base.account_id, &shares, now_ms);

        // Create completion for Phase 3
        let completion = ChangeCompletion {
            change_id,
            success: true,
            new_guardian_set,
            new_threshold: final_threshold,
            change_evidence: serde_json::to_vec(&evidence).unwrap_or_default(),
        };

        // Broadcast completion (Phase 3)
        self.broadcast_change_completion(completion).await?;

        Ok(BaseCoordinator::<E>::success_response(
            None, shares, evidence,
        ))
    }

    /// Execute as guardian (vote on membership change).
    pub async fn vote_as_guardian(
        &self,
        proposal: MembershipProposal,
        guardian_id: AuthorityId,
        approved: bool,
    ) -> RecoveryResult<GuardianVote> {
        let rationale = if approved {
            "Change approved after review".to_string()
        } else {
            "Change denied due to security concerns".to_string()
        };

        let physical_time = self
            .effect_system()
            .physical_time()
            .await
            .unwrap_or(PhysicalTime {
                ts_ms: 0,
                uncertainty: None,
            });

        // Create vote signature
        let mut sig_input = Vec::new();
        sig_input.extend_from_slice(&guardian_id.to_bytes());
        sig_input.extend_from_slice(proposal.change_id.as_bytes());
        sig_input.push(approved as u8);
        let vote_signature = hash::hash(&sig_input).to_vec();

        // Emit MembershipVoteCast fact
        let context_id = ContextId::new_from_entropy(hash::hash(proposal.change_id.as_bytes()));
        let proposal_hash = Hash32(hash::hash(proposal.change_id.as_bytes()));
        let vote_fact = RecoveryFact::MembershipVoteCast {
            context_id,
            voter_id: guardian_id,
            proposal_hash,
            approved,
            voted_at: physical_time.clone(),
        };
        self.emit_fact(vote_fact).await?;

        Ok(GuardianVote {
            change_id: proposal.change_id,
            guardian_id,
            approved,
            vote_signature,
            rationale,
            timestamp: TimeStamp::PhysicalClock(physical_time),
        })
    }

    /// Execute choreographic membership change protocol (Phase 1-2).
    async fn execute_choreographic_membership_change(
        &self,
        proposal: MembershipProposal,
    ) -> RecoveryResult<Vec<GuardianVote>> {
        let physical_time = self
            .effect_system()
            .physical_time()
            .await
            .map_err(|e| AuraError::internal(format!("Time error: {}", e)))?;

        // Simulate guardian votes
        let mut votes = Vec::new();
        for guardian in proposal.account_id.to_bytes().iter().take(2) {
            let guardian_id = AuthorityId::new_from_entropy(hash::hash(&[*guardian; 32]));
            let mut sig_input = Vec::new();
            sig_input.extend_from_slice(&guardian_id.to_bytes());
            sig_input.extend_from_slice(proposal.change_id.as_bytes());
            sig_input.push(1u8);

            votes.push(GuardianVote {
                change_id: proposal.change_id.clone(),
                guardian_id,
                approved: true,
                vote_signature: hash::hash(&sig_input).to_vec(),
                rationale: "Approved - change validated".to_string(),
                timestamp: TimeStamp::PhysicalClock(physical_time.clone()),
            });
        }

        Ok(votes)
    }

    /// Broadcast change completion (Phase 3).
    async fn broadcast_change_completion(
        &self,
        _completion: ChangeCompletion,
    ) -> RecoveryResult<()> {
        // Handled by choreographic broadcast in generated code
        Ok(())
    }

    /// Apply membership change to guardian set.
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
                    .any(|g| g.authority_id == guardian.authority_id)
                {
                    return Err(AuraError::invalid("Guardian already exists in set"));
                }
                guardians.push(guardian.clone());
            }
            MembershipChange::RemoveGuardian { guardian_id } => {
                guardians.retain(|g| g.authority_id != *guardian_id);
                if guardians.is_empty() {
                    return Err(AuraError::invalid("Cannot remove last guardian"));
                }
            }
            MembershipChange::UpdateGuardian {
                guardian_id,
                new_profile,
            } => {
                if let Some(guardian) = guardians
                    .iter_mut()
                    .find(|g| g.authority_id == *guardian_id)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::GuardianProfile;
    use aura_testkit::MockEffects;
    use std::sync::Arc;

    fn test_authority_id(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn create_test_request() -> MembershipChangeRequest {
        let guardians = vec![
            GuardianProfile::with_label(test_authority_id(1), "Guardian 1".to_string()),
            GuardianProfile::with_label(test_authority_id(2), "Guardian 2".to_string()),
            GuardianProfile::with_label(test_authority_id(3), "Guardian 3".to_string()),
        ];

        MembershipChangeRequest {
            base: crate::types::RecoveryRequest {
                initiator_id: test_authority_id(0),
                account_id: test_authority_id(10),
                context: aura_authenticate::RecoveryContext {
                    operation_type:
                        aura_authenticate::RecoveryOperationType::GuardianSetModification,
                    justification: "Test membership change".to_string(),
                    is_emergency: false,
                    timestamp: 0,
                },
                threshold: 2,
                guardians: GuardianSet::new(guardians),
            },
            change: MembershipChange::AddGuardian {
                guardian: GuardianProfile::with_label(
                    test_authority_id(4),
                    "Guardian 4".to_string(),
                ),
            },
            new_threshold: None,
        }
    }

    #[tokio::test]
    async fn test_membership_coordinator_creation() {
        let effects = Arc::new(MockEffects::deterministic());
        let coordinator = GuardianMembershipCoordinator::new(effects);

        assert_eq!(coordinator.operation_name(), "guardian_membership");
    }

    #[tokio::test]
    async fn test_membership_change_add_guardian() {
        let effects = Arc::new(MockEffects::deterministic());
        let coordinator = GuardianMembershipCoordinator::new(effects);

        let request = create_test_request();
        let response = coordinator.execute_membership_change(request).await;

        assert!(response.is_ok());
        let resp = response.unwrap();
        assert!(resp.success);
    }

    #[tokio::test]
    async fn test_membership_change_remove_guardian() {
        let effects = Arc::new(MockEffects::deterministic());
        let coordinator = GuardianMembershipCoordinator::new(effects);

        let mut request = create_test_request();
        request.change = MembershipChange::RemoveGuardian {
            guardian_id: test_authority_id(3),
        };

        let response = coordinator.execute_membership_change(request).await;

        assert!(response.is_ok());
    }

    #[tokio::test]
    async fn test_vote_as_guardian() {
        let effects = Arc::new(MockEffects::deterministic());
        let coordinator = GuardianMembershipCoordinator::new(effects);

        let proposal = MembershipProposal {
            change_id: "test-change-123".to_string(),
            account_id: test_authority_id(10),
            proposer_id: test_authority_id(0),
            change: MembershipChange::AddGuardian {
                guardian: GuardianProfile::with_label(
                    test_authority_id(4),
                    "Guardian 4".to_string(),
                ),
            },
            new_threshold: None,
            timestamp: TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: 1000,
                uncertainty: None,
            }),
        };

        let guardian_id = test_authority_id(1);
        let vote = coordinator
            .vote_as_guardian(proposal, guardian_id, true)
            .await;

        assert!(vote.is_ok());
        let v = vote.unwrap();
        assert!(v.approved);
        assert_eq!(v.guardian_id, guardian_id);
    }

    #[test]
    fn test_apply_add_guardian() {
        let effects = Arc::new(MockEffects::deterministic());
        let coordinator = GuardianMembershipCoordinator::new(effects);

        let guardians = vec![
            GuardianProfile::with_label(test_authority_id(1), "Guardian 1".to_string()),
            GuardianProfile::with_label(test_authority_id(2), "Guardian 2".to_string()),
        ];
        let current_set = GuardianSet::new(guardians);

        let new_guardian =
            GuardianProfile::with_label(test_authority_id(3), "Guardian 3".to_string());
        let change = MembershipChange::AddGuardian {
            guardian: new_guardian,
        };

        let result = coordinator.apply_membership_change(&current_set, &change);

        assert!(result.is_ok());
        let new_set = result.unwrap();
        assert_eq!(new_set.len(), 3);
    }

    #[test]
    fn test_apply_remove_guardian() {
        let effects = Arc::new(MockEffects::deterministic());
        let coordinator = GuardianMembershipCoordinator::new(effects);

        let guardians = vec![
            GuardianProfile::with_label(test_authority_id(1), "Guardian 1".to_string()),
            GuardianProfile::with_label(test_authority_id(2), "Guardian 2".to_string()),
        ];
        let current_set = GuardianSet::new(guardians);

        let change = MembershipChange::RemoveGuardian {
            guardian_id: test_authority_id(1),
        };

        let result = coordinator.apply_membership_change(&current_set, &change);

        assert!(result.is_ok());
        let new_set = result.unwrap();
        assert_eq!(new_set.len(), 1);
    }

    #[test]
    fn test_apply_remove_last_guardian_fails() {
        let effects = Arc::new(MockEffects::deterministic());
        let coordinator = GuardianMembershipCoordinator::new(effects);

        let guardians = vec![GuardianProfile::with_label(
            test_authority_id(1),
            "Guardian 1".to_string(),
        )];
        let current_set = GuardianSet::new(guardians);

        let change = MembershipChange::RemoveGuardian {
            guardian_id: test_authority_id(1),
        };

        let result = coordinator.apply_membership_change(&current_set, &change);

        assert!(result.is_err());
    }

    #[test]
    fn test_apply_add_duplicate_guardian_fails() {
        let effects = Arc::new(MockEffects::deterministic());
        let coordinator = GuardianMembershipCoordinator::new(effects);

        let guardians = vec![
            GuardianProfile::with_label(test_authority_id(1), "Guardian 1".to_string()),
            GuardianProfile::with_label(test_authority_id(2), "Guardian 2".to_string()),
        ];
        let current_set = GuardianSet::new(guardians);

        let change = MembershipChange::AddGuardian {
            guardian: GuardianProfile::with_label(test_authority_id(1), "Guardian 1".to_string()),
        };

        let result = coordinator.apply_membership_change(&current_set, &change);

        assert!(result.is_err());
    }
}
