//! Guardian Membership Change Choreography
//!
//! Adding and removing guardians from the guardian set.
//! Uses the authority model - guardians are identified by AuthorityId.

use crate::{
    coordinator::{BaseCoordinator, BaseCoordinatorAccess, RecoveryCoordinator},
    effects::RecoveryEffects,
    facts::{MembershipChangeType, RecoveryFact},
    types::{GuardianProfile, GuardianSet, RecoveryRequest, RecoveryResponse, RecoveryShare},
    utils::{
        workflow::{
            context_id_from_operation_id, current_physical_time_or_zero, exact_physical_time,
            persist_recovery_fact, trace_id,
        },
        EvidenceBuilder,
    },
    RecoveryResult,
};
use async_trait::async_trait;
use aura_core::effects::PhysicalTimeEffects;
use aura_core::key_resolution::TrustedKeyResolver;
use aura_core::time::{PhysicalTime, TimeStamp};
use aura_core::types::identifiers::{AuthorityId, ContextId};
use aura_core::{hash, AuraError, Hash32};
use aura_macros::tell;
use aura_signature::{sign_ed25519_transcript, verify_ed25519_transcript, SecurityTranscript};
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
    /// Threshold required to approve the current membership change
    pub threshold: u16,
    /// New threshold to set after the change (optional)
    pub new_threshold: Option<u16>,
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

#[derive(Debug, Clone, Serialize)]
struct GuardianVoteTranscriptPayload {
    change_id: String,
    account_id: AuthorityId,
    proposer_id: AuthorityId,
    change: MembershipChange,
    threshold: u16,
    new_threshold: Option<u16>,
    proposal_timestamp: TimeStamp,
    guardian_id: AuthorityId,
    approved: bool,
    vote_timestamp: TimeStamp,
}

struct GuardianVoteTranscript<'a> {
    proposal: &'a MembershipProposal,
    guardian_id: AuthorityId,
    approved: bool,
    vote_timestamp: &'a TimeStamp,
}

impl SecurityTranscript for GuardianVoteTranscript<'_> {
    type Payload = GuardianVoteTranscriptPayload;

    const DOMAIN_SEPARATOR: &'static str = "aura.recovery.guardian-membership-vote";

    fn transcript_payload(&self) -> Self::Payload {
        GuardianVoteTranscriptPayload {
            change_id: self.proposal.change_id.clone(),
            account_id: self.proposal.account_id,
            proposer_id: self.proposal.proposer_id,
            change: self.proposal.change.clone(),
            threshold: self.proposal.threshold,
            new_threshold: self.proposal.new_threshold,
            proposal_timestamp: self.proposal.timestamp.clone(),
            guardian_id: self.guardian_id,
            approved: self.approved,
            vote_timestamp: self.vote_timestamp.clone(),
        }
    }
}

/// Encode the canonical guardian membership vote transcript.
pub fn guardian_vote_transcript_bytes(
    proposal: &MembershipProposal,
    guardian_id: AuthorityId,
    approved: bool,
    vote_timestamp: &TimeStamp,
) -> RecoveryResult<Vec<u8>> {
    GuardianVoteTranscript {
        proposal,
        guardian_id,
        approved,
        vote_timestamp,
    }
    .transcript_bytes()
    .map_err(|error| AuraError::crypto(format!("guardian vote transcript failed: {error}")))
}

/// Sign a guardian membership vote with the guardian's Ed25519 key.
pub async fn sign_guardian_vote<E>(
    effects: &E,
    proposal: &MembershipProposal,
    guardian_id: AuthorityId,
    approved: bool,
    vote_timestamp: &TimeStamp,
    private_key: &[u8],
) -> RecoveryResult<Vec<u8>>
where
    E: aura_core::effects::CryptoEffects + Send + Sync + ?Sized,
{
    let transcript = GuardianVoteTranscript {
        proposal,
        guardian_id,
        approved,
        vote_timestamp,
    };
    sign_ed25519_transcript(effects, &transcript, private_key)
        .await
        .map_err(|error| AuraError::crypto(format!("guardian vote signing failed: {error}")))
}

/// Verify a guardian membership vote against a trusted guardian public key.
pub async fn verify_guardian_vote_signature<E>(
    effects: &E,
    proposal: &MembershipProposal,
    vote: &GuardianVote,
    key_resolver: &impl TrustedKeyResolver,
) -> RecoveryResult<bool>
where
    E: aura_core::effects::CryptoEffects + Send + Sync + ?Sized,
{
    if vote.change_id != proposal.change_id {
        return Ok(false);
    }
    let trusted_key = key_resolver
        .resolve_guardian_key(vote.guardian_id)
        .map_err(|error| {
            AuraError::crypto(format!(
                "trusted guardian vote key resolution failed for {}: {error}",
                vote.guardian_id
            ))
        })?;
    let transcript = GuardianVoteTranscript {
        proposal,
        guardian_id: vote.guardian_id,
        approved: vote.approved,
        vote_timestamp: &vote.timestamp,
    };
    verify_ed25519_transcript(
        effects,
        &transcript,
        &vote.vote_signature,
        trusted_key.bytes(),
    )
    .await
    .map_err(|error| AuraError::crypto(format!("guardian vote verification failed: {error}")))
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
    pub new_threshold: u16,
    /// Serialized evidence of the membership change
    pub change_evidence: Vec<u8>,
}

// Runtime reconfiguration still consumes the guardian_handoff bundle contract
// exposed by this choreography surface.
tell!(include_str!("src/guardian_membership.tell"));

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
    pub new_threshold: Option<u16>,
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
        persist_recovery_fact(self.effect_system().as_ref(), &fact).await
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

    fn change_id(account_id: &AuthorityId, now_ms: u64) -> String {
        format!("membership_{account_id}_{now_ms}")
    }

    fn proposal_hash(change_id: &str) -> Hash32 {
        Hash32(hash::hash(change_id.as_bytes()))
    }

    async fn emit_change_rejected(
        &self,
        context_id: ContextId,
        proposal_hash: Hash32,
        change_id: &str,
        reason: impl Into<String>,
    ) -> RecoveryResult<()> {
        let rejected_fact = RecoveryFact::MembershipChangeRejected {
            context_id,
            proposal_hash,
            trace_id: trace_id(change_id),
            reason: reason.into(),
            rejected_at: current_physical_time_or_zero(self.effect_system().as_ref()).await,
        };
        self.emit_fact(rejected_fact).await
    }

    fn ensure_guardian_absent(
        guardians: &[GuardianProfile],
        guardian_id: AuthorityId,
    ) -> RecoveryResult<()> {
        if guardians.iter().any(|g| g.authority_id == guardian_id) {
            Err(AuraError::invalid("Guardian already exists in set"))
        } else {
            Ok(())
        }
    }

    fn ensure_guardians_remaining(guardians: &[GuardianProfile]) -> RecoveryResult<()> {
        if guardians.is_empty() {
            Err(AuraError::invalid("Cannot remove last guardian"))
        } else {
            Ok(())
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
        let change_id = Self::change_id(&request.base.account_id, now_ms);
        let context_id = context_id_from_operation_id(&change_id);

        // Emit MembershipChangeProposed fact
        let proposal_hash = Self::proposal_hash(&change_id);
        let proposed_fact = RecoveryFact::MembershipChangeProposed {
            context_id,
            proposer_id: request.base.initiator_id,
            trace_id: trace_id(&change_id),
            change_type: Self::to_fact_change_type(&request.change),
            proposal_hash,
            proposed_at: exact_physical_time(now_ms),
        };
        self.emit_fact(proposed_fact).await?;

        // Create proposal for choreographic protocol
        let proposal = MembershipProposal {
            change_id: change_id.clone(),
            account_id: request.base.account_id,
            proposer_id: request.base.initiator_id,
            change: request.change.clone(),
            threshold: request.base.threshold,
            new_threshold: request.new_threshold,
            timestamp: TimeStamp::PhysicalClock(exact_physical_time(now_ms)),
        };

        // Execute choreographic protocol (Phase 1-2)
        let votes = self
            .execute_choreographic_membership_change(proposal)
            .await?;

        // Count approval votes
        let approvals: Vec<_> = votes.into_iter().filter(|v| v.approved).collect();

        // Check if we have enough approvals
        if approvals.len() < request.base.threshold as usize {
            let reason = format!(
                "Insufficient guardian approvals: got {}, need {}",
                approvals.len(),
                request.base.threshold
            );
            let _ = self
                .emit_change_rejected(context_id, proposal_hash, &change_id, reason.clone())
                .await;

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
        if new_guardian_set.len() < final_threshold as usize {
            let reason = format!(
                "Invalid configuration: {} guardians cannot satisfy threshold of {}",
                new_guardian_set.len(),
                final_threshold
            );
            let _ = self
                .emit_change_rejected(context_id, proposal_hash, &change_id, reason.clone())
                .await;

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
            trace_id: trace_id(&change_id),
            new_guardian_ids: new_guardian_set.iter().map(|g| g.authority_id).collect(),
            new_threshold: final_threshold,
            completed_at: current_physical_time_or_zero(self.effect_system().as_ref()).await,
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
        guardian_private_key: &[u8],
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

        let vote_timestamp = TimeStamp::PhysicalClock(physical_time.clone());
        let vote_signature = sign_guardian_vote(
            self.effect_system().as_ref(),
            &proposal,
            guardian_id,
            approved,
            &vote_timestamp,
            guardian_private_key,
        )
        .await?;

        // Emit MembershipVoteCast fact
        let context_id = context_id_from_operation_id(&proposal.change_id);
        let proposal_hash = Self::proposal_hash(&proposal.change_id);
        let vote_fact = RecoveryFact::MembershipVoteCast {
            context_id,
            voter_id: guardian_id,
            trace_id: trace_id(&proposal.change_id),
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
            timestamp: vote_timestamp,
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
            .map_err(|e| AuraError::internal(format!("Time error: {e}")))?;

        #[cfg(not(test))]
        {
            let _ = (proposal, physical_time);
            Err(AuraError::internal(
                "guardian membership votes must be supplied by signed guardian runtimes",
            ))
        }

        #[cfg(test)]
        {
            // Simulate guardian votes
            let mut votes = Vec::new();
            for guardian in proposal.account_id.to_bytes().iter().take(2) {
                let guardian_id = AuthorityId::new_from_entropy(hash::hash(&[*guardian; 32]));
                let (private_key, _) = self
                    .effect_system()
                    .ed25519_generate_keypair()
                    .await
                    .map_err(|error| {
                        AuraError::crypto(format!("test vote keygen failed: {error}"))
                    })?;
                let vote_timestamp = TimeStamp::PhysicalClock(physical_time.clone());
                let vote_signature = sign_guardian_vote(
                    self.effect_system().as_ref(),
                    &proposal,
                    guardian_id,
                    true,
                    &vote_timestamp,
                    &private_key,
                )
                .await?;

                votes.push(GuardianVote {
                    change_id: proposal.change_id.clone(),
                    guardian_id,
                    approved: true,
                    vote_signature,
                    rationale: "Approved - change validated".to_string(),
                    timestamp: vote_timestamp,
                });
            }

            Ok(votes)
        }
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
                Self::ensure_guardian_absent(&guardians, guardian.authority_id)?;
                guardians.push(guardian.clone());
            }
            MembershipChange::RemoveGuardian { guardian_id } => {
                guardians.retain(|g| g.authority_id != *guardian_id);
                Self::ensure_guardians_remaining(&guardians)?;
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
    use aura_core::effects::CryptoCoreEffects;
    use aura_core::key_resolution::{
        KeyResolutionError, TrustedKeyDomain, TrustedKeyResolver, TrustedPublicKey,
    };
    use aura_testkit::MockEffects;
    use std::collections::BTreeMap;
    use std::sync::Arc;

    #[derive(Default)]
    struct TestGuardianKeyResolver {
        keys: BTreeMap<AuthorityId, Vec<u8>>,
    }

    impl TestGuardianKeyResolver {
        fn with_guardian_key(mut self, guardian: AuthorityId, key: Vec<u8>) -> Self {
            self.keys.insert(guardian, key);
            self
        }
    }

    impl TrustedKeyResolver for TestGuardianKeyResolver {
        fn resolve_authority_threshold_key(
            &self,
            _authority: AuthorityId,
            _epoch: u64,
        ) -> Result<TrustedPublicKey, KeyResolutionError> {
            Err(KeyResolutionError::Unknown {
                domain: TrustedKeyDomain::AuthorityThreshold,
            })
        }

        fn resolve_device_key(
            &self,
            _device: aura_core::DeviceId,
        ) -> Result<TrustedPublicKey, KeyResolutionError> {
            Err(KeyResolutionError::Unknown {
                domain: TrustedKeyDomain::Device,
            })
        }

        fn resolve_guardian_key(
            &self,
            guardian: AuthorityId,
        ) -> Result<TrustedPublicKey, KeyResolutionError> {
            let key = self
                .keys
                .get(&guardian)
                .ok_or(KeyResolutionError::Unknown {
                    domain: TrustedKeyDomain::Guardian,
                })?;
            Ok(TrustedPublicKey::active(
                TrustedKeyDomain::Guardian,
                None,
                key.clone(),
                Hash32(hash::hash(key)),
            ))
        }

        fn resolve_release_key(
            &self,
            _authority: AuthorityId,
        ) -> Result<TrustedPublicKey, KeyResolutionError> {
            Err(KeyResolutionError::Unknown {
                domain: TrustedKeyDomain::Release,
            })
        }
    }

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
                context: aura_authentication::RecoveryContext {
                    operation_type:
                        aura_authentication::RecoveryOperationType::GuardianSetModification,
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
        let (private_key, public_key) = effects.ed25519_generate_keypair().await.unwrap();
        let coordinator = GuardianMembershipCoordinator::new(effects.clone());

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
            threshold: 2,
            new_threshold: None,
            timestamp: TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: 1000,
                uncertainty: None,
            }),
        };

        let guardian_id = test_authority_id(1);
        let vote = coordinator
            .vote_as_guardian(proposal.clone(), guardian_id, true, &private_key)
            .await;

        assert!(vote.is_ok());
        let v = vote.unwrap();
        assert!(v.approved);
        assert_eq!(v.guardian_id, guardian_id);
        let key_resolver =
            TestGuardianKeyResolver::default().with_guardian_key(guardian_id, public_key);
        assert!(
            verify_guardian_vote_signature(effects.as_ref(), &proposal, &v, &key_resolver)
                .await
                .unwrap()
        );
    }

    #[test]
    fn guardian_vote_transcript_binds_proposal_context() {
        let base = MembershipProposal {
            change_id: "test-change-123".to_string(),
            account_id: test_authority_id(10),
            proposer_id: test_authority_id(0),
            change: MembershipChange::AddGuardian {
                guardian: GuardianProfile::with_label(
                    test_authority_id(4),
                    "Guardian 4".to_string(),
                ),
            },
            threshold: 2,
            new_threshold: None,
            timestamp: TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: 1000,
                uncertainty: None,
            }),
        };
        let mut changed_threshold = base.clone();
        changed_threshold.new_threshold = Some(3);
        let guardian_id = test_authority_id(1);
        let vote_timestamp = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 1001,
            uncertainty: None,
        });

        let base_transcript =
            guardian_vote_transcript_bytes(&base, guardian_id, true, &vote_timestamp).unwrap();
        let threshold_transcript =
            guardian_vote_transcript_bytes(&changed_threshold, guardian_id, true, &vote_timestamp)
                .unwrap();
        let denied_transcript =
            guardian_vote_transcript_bytes(&base, guardian_id, false, &vote_timestamp).unwrap();

        assert_ne!(base_transcript, threshold_transcript);
        assert_ne!(base_transcript, denied_transcript);
    }

    #[tokio::test]
    async fn guardian_vote_verification_rejects_forged_or_replayed_votes() {
        let crypto = aura_effects::RealCryptoHandler::for_simulation_seed([0xA7; 32]);
        let (private_key, public_key) = crypto.ed25519_generate_keypair().await.unwrap();
        let proposal = MembershipProposal {
            change_id: "test-change-verify".to_string(),
            account_id: test_authority_id(10),
            proposer_id: test_authority_id(0),
            change: MembershipChange::AddGuardian {
                guardian: GuardianProfile::with_label(
                    test_authority_id(4),
                    "Guardian 4".to_string(),
                ),
            },
            threshold: 2,
            new_threshold: None,
            timestamp: TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: 1000,
                uncertainty: None,
            }),
        };
        let vote_timestamp = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 1001,
            uncertainty: None,
        });
        let guardian_id = test_authority_id(1);
        let signature = sign_guardian_vote(
            &crypto,
            &proposal,
            guardian_id,
            true,
            &vote_timestamp,
            &private_key,
        )
        .await
        .unwrap();
        let vote = GuardianVote {
            change_id: proposal.change_id.clone(),
            guardian_id,
            approved: true,
            vote_signature: signature,
            rationale: "approved".to_string(),
            timestamp: vote_timestamp,
        };
        let key_resolver =
            TestGuardianKeyResolver::default().with_guardian_key(guardian_id, public_key);

        assert!(
            verify_guardian_vote_signature(&crypto, &proposal, &vote, &key_resolver)
                .await
                .unwrap()
        );

        let mut wrong_proposal = proposal.clone();
        wrong_proposal.change_id = "different-change".to_string();
        assert!(
            !verify_guardian_vote_signature(&crypto, &wrong_proposal, &vote, &key_resolver)
                .await
                .unwrap()
        );

        let mut tampered_approval = vote.clone();
        tampered_approval.approved = false;
        assert!(!verify_guardian_vote_signature(
            &crypto,
            &proposal,
            &tampered_approval,
            &key_resolver,
        )
        .await
        .unwrap());

        let mut wrong_guardian = vote.clone();
        wrong_guardian.guardian_id = test_authority_id(2);
        assert!(
            verify_guardian_vote_signature(&crypto, &proposal, &wrong_guardian, &key_resolver)
                .await
                .is_err()
        );

        let mut forged = vote;
        forged.vote_signature = vec![0; 64];
        assert!(
            !verify_guardian_vote_signature(&crypto, &proposal, &forged, &key_resolver)
                .await
                .unwrap()
        );
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

    /// Cannot remove the last guardian — would leave the account unrecoverable.
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

    /// Cannot add a duplicate guardian — would inflate the quorum.
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
