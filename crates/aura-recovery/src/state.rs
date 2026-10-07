//! Recovery state derived from journal facts.
//!
//! This module provides fact-based state derivation for recovery operations,
//! eliminating mutable coordinator state in favor of fact reduction.
//!
//! # Architecture
//!
//! Instead of storing mutable state in coordinators, recovery state is derived
//! on-demand from the journal facts:
//!
//! 1. Facts are emitted during recovery operations (see `facts.rs`)
//! 2. State is reduced from facts when needed
//! 3. Coordinators become stateless, querying state as needed
//!
//! This approach:
//! - Ensures consistency across devices (facts replicate, state derives)
//! - Simplifies testing (no hidden mutable state)
//! - Enables time-travel debugging (replay facts to any point)
//!
//! # State transitions
//!
//! Guardian setup:
//! `AwaitingResponses -> ThresholdMet -> Completed`
//! `AwaitingResponses -> Failed(ThresholdUnsatisfied|Explicit)`
//! `ThresholdMet -> Completed`
//! `ThresholdMet -> Failed(Explicit)`
//!
//! Membership proposal:
//! `Pending -> Approved`
//! `Pending -> Rejected`
//!
//! Recovery:
//! `AwaitingShares -> Approved -> Completed`
//! `AwaitingShares -> Disputed`
//! `Approved -> Failed`
//! `Disputed -> Failed`
//!
//! Reduction is a pure function of the fact set. Early responses are held
//! until their setup/proposal/initiation fact is known, and terminal states
//! use explicit precedence (failure over completion, completion over
//! dispute) rather than arrival order; see [`RecoveryState::from_facts`].
//!
//! # Usage
//!
//! ```ignore
//! use aura_recovery::state::RecoveryState;
//!
//! // Derive state from facts
//! let state = RecoveryState::from_facts(&facts)?;
//!
//! // Query specific aspects
//! if let Some(setup) = state.active_setup() {
//!     println!("Setup in progress: {} guardians accepted", setup.accepted.len());
//! }
//! ```

use crate::facts::{MembershipChangeType, RecoveryFact};
use aura_core::types::identifiers::{AuthorityId, ContextId};
use aura_core::Hash32;
use aura_journal::DomainFact;
use std::collections::HashMap;

fn threshold_satisfied(count: usize, threshold: u16) -> bool {
    count >= threshold as usize
}

fn remaining_guardians(total: usize, declined: usize) -> usize {
    total.saturating_sub(declined)
}

/// Recovery state derived from journal facts.
///
/// This struct represents the current state of recovery operations,
/// computed by reducing all relevant facts.
#[derive(Debug, Clone, Default)]
pub struct RecoveryState {
    /// Active guardian setups by context
    setups: HashMap<ContextId, SetupState>,
    /// Active membership proposals by context
    proposals: HashMap<ContextId, MembershipProposalState>,
    /// Active recovery operations by context
    recoveries: HashMap<ContextId, RecoveryOperationState>,
}

impl RecoveryState {
    /// Create an empty recovery state.
    pub fn new() -> Self {
        Self::default()
    }
    /// Derive recovery state from a list of serialized facts.
    ///
    /// The result is a pure function of the fact *set*: arrival order and
    /// duplicates do not matter (see [`Self::from_facts`]).
    pub fn from_fact_bytes(facts: &[(String, Vec<u8>)]) -> Self {
        let decoded: Vec<RecoveryFact> = facts
            .iter()
            .filter(|(type_id, _)| type_id == crate::facts::RECOVERY_FACT_TYPE_ID)
            .filter_map(|(_, data)| RecoveryFact::from_bytes(data))
            .collect();
        Self::from_facts(&decoded)
    }

    /// Derive recovery state from a set of `RecoveryFact`s.
    ///
    /// Reduction is order-independent. Facts are first grouped per context;
    /// responses that arrive before their setup/proposal/initiation fact are
    /// held in the group, not dropped, and are applied once the whole set is
    /// known. Terminal states follow an explicit precedence policy instead of
    /// arrival order:
    ///
    /// - setup: explicit `Failed` > `Completed` > quorum derived from the
    ///   accepted/declined sets
    /// - proposal: `Rejected` > `Approved` > `Pending`
    /// - recovery: `Failed` > `Completed` > `Disputed` > `Approved` >
    ///   `AwaitingShares`
    ///
    /// Among competing facts of the same kind (two initiations, two failure
    /// reasons, two disputes) the winner is chosen by a total order (latest
    /// physical timestamp for initiations, then canonical bytes), so every
    /// replica derives the same state.
    pub fn from_facts(facts: &[RecoveryFact]) -> Self {
        let mut groups: HashMap<ContextId, ContextFacts<'_>> = HashMap::new();
        for fact in facts {
            groups.entry(fact.context_id()).or_default().push(fact);
        }

        let mut state = Self::new();
        for (context_id, group) in groups {
            if let Some(setup) = group.reduce_setup(context_id) {
                state.setups.insert(context_id, setup);
            }
            if let Some(proposal) = group.reduce_proposal(context_id) {
                state.proposals.insert(context_id, proposal);
            }
            if let Some(recovery) = group.reduce_recovery(context_id) {
                state.recoveries.insert(context_id, recovery);
            }
        }
        state
    }

    // =========================================================================
    // Query Methods
    // =========================================================================

    /// Get the active setup for a context, if any.
    pub fn setup_for_context(&self, context_id: &ContextId) -> Option<&SetupState> {
        self.setups.get(context_id)
    }

    /// Get the active membership proposal for a context, if any.
    pub fn proposal_for_context(&self, context_id: &ContextId) -> Option<&MembershipProposalState> {
        self.proposals.get(context_id)
    }

    /// Get the active recovery operation for a context, if any.
    pub fn recovery_for_context(&self, context_id: &ContextId) -> Option<&RecoveryOperationState> {
        self.recoveries.get(context_id)
    }

    /// Get all active (non-completed/failed) setups.
    pub fn active_setups(&self) -> impl Iterator<Item = &SetupState> {
        self.setups
            .values()
            .filter(|s| !matches!(s.status, SetupStatus::Completed | SetupStatus::Failed(_)))
    }

    /// Get all active (non-completed/failed) proposals.
    pub fn active_proposals(&self) -> impl Iterator<Item = &MembershipProposalState> {
        self.proposals.values().filter(|p| {
            !matches!(
                p.status,
                ProposalStatus::Approved | ProposalStatus::Rejected(_)
            )
        })
    }

    /// Get all active (non-completed/failed) recoveries.
    pub fn active_recoveries(&self) -> impl Iterator<Item = &RecoveryOperationState> {
        self.recoveries.values().filter(|r| {
            !matches!(
                r.status,
                RecoveryStatus::Completed | RecoveryStatus::Failed(_)
            )
        })
    }

    /// Check if there's any active operation for a context.
    pub fn has_active_operation(&self, context_id: &ContextId) -> bool {
        self.setup_for_context(context_id)
            .is_some_and(|s| !matches!(s.status, SetupStatus::Completed | SetupStatus::Failed(_)))
            || self.proposal_for_context(context_id).is_some_and(|p| {
                !matches!(
                    p.status,
                    ProposalStatus::Approved | ProposalStatus::Rejected(_)
                )
            })
            || self.recovery_for_context(context_id).is_some_and(|r| {
                !matches!(
                    r.status,
                    RecoveryStatus::Completed | RecoveryStatus::Failed(_)
                )
            })
    }
}

/// All recovery facts for one context, collected before any are applied so
/// that early responses are held rather than dropped.
#[derive(Default)]
struct ContextFacts<'a> {
    setup_initiations: Vec<&'a RecoveryFact>,
    accepted: Vec<AuthorityId>,
    declined: Vec<AuthorityId>,
    setup_completed: bool,
    setup_failures: Vec<&'a str>,
    proposals: Vec<&'a RecoveryFact>,
    votes_for: Vec<AuthorityId>,
    votes_against: Vec<AuthorityId>,
    proposal_approved: bool,
    proposal_rejections: Vec<&'a str>,
    recovery_initiations: Vec<&'a RecoveryFact>,
    shares: Vec<AuthorityId>,
    recovery_approved: bool,
    disputes: Vec<(AuthorityId, &'a str)>,
    recovery_completed: bool,
    recovery_failures: Vec<&'a str>,
}

/// Deterministic winner among competing facts of one kind: latest physical
/// timestamp, ties broken by canonical encoding.
fn latest_fact<'a>(facts: &[&'a RecoveryFact]) -> Option<&'a RecoveryFact> {
    facts
        .iter()
        .copied()
        .max_by_key(|fact| (fact.timestamp_ms(), fact.to_bytes()))
}

/// Canonical (sorted, deduplicated) authority set.
fn canonical_set(mut ids: Vec<AuthorityId>) -> Vec<AuthorityId> {
    ids.sort();
    ids.dedup();
    ids
}

/// Deterministic choice among competing reason strings.
fn canonical_reason(reasons: &[&str]) -> Option<String> {
    reasons.iter().min().map(|reason| (*reason).to_string())
}

impl<'a> ContextFacts<'a> {
    fn push(&mut self, fact: &'a RecoveryFact) {
        match fact {
            RecoveryFact::GuardianSetupInitiated { .. } => self.setup_initiations.push(fact),
            RecoveryFact::GuardianInvitationSent { .. } => {}
            RecoveryFact::GuardianAccepted { guardian_id, .. } => self.accepted.push(*guardian_id),
            RecoveryFact::GuardianDeclined { guardian_id, .. } => self.declined.push(*guardian_id),
            RecoveryFact::GuardianSetupCompleted { .. } => self.setup_completed = true,
            RecoveryFact::GuardianSetupFailed { reason, .. } => self.setup_failures.push(reason),
            RecoveryFact::MembershipChangeProposed { .. } => self.proposals.push(fact),
            RecoveryFact::MembershipVoteCast {
                voter_id, approved, ..
            } => {
                if *approved {
                    self.votes_for.push(*voter_id);
                } else {
                    self.votes_against.push(*voter_id);
                }
            }
            RecoveryFact::MembershipChangeCompleted { .. } => self.proposal_approved = true,
            RecoveryFact::MembershipChangeRejected { reason, .. } => {
                self.proposal_rejections.push(reason);
            }
            RecoveryFact::RecoveryInitiated { .. } => self.recovery_initiations.push(fact),
            RecoveryFact::RecoveryShareSubmitted { guardian_id, .. } => {
                self.shares.push(*guardian_id);
            }
            RecoveryFact::RecoveryApproved { .. } => self.recovery_approved = true,
            RecoveryFact::RecoveryDisputeFiled {
                disputer_id,
                reason,
                ..
            } => self.disputes.push((*disputer_id, reason)),
            RecoveryFact::RecoveryCompleted { .. } => self.recovery_completed = true,
            RecoveryFact::RecoveryFailed { reason, .. } => self.recovery_failures.push(reason),
        }
    }

    fn reduce_setup(&self, context_id: ContextId) -> Option<SetupState> {
        let RecoveryFact::GuardianSetupInitiated {
            initiator_id,
            guardian_ids,
            threshold,
            initiated_at,
            ..
        } = latest_fact(&self.setup_initiations)?
        else {
            return None;
        };
        let mut setup = SetupState {
            context_id,
            initiator_id: *initiator_id,
            initiated_at: initiated_at.ts_ms,
            target_guardians: guardian_ids.clone(),
            accepted: canonical_set(self.accepted.clone()),
            declined: canonical_set(self.declined.clone()),
            threshold: *threshold,
            status: SetupStatus::AwaitingResponses,
        };
        setup.status = if let Some(reason) = canonical_reason(&self.setup_failures) {
            SetupStatus::Failed(SetupFailure::Explicit { reason })
        } else if self.setup_completed {
            SetupStatus::Completed
        } else {
            match setup.quorum_progress() {
                SetupQuorumProgress::ThresholdMet { .. } => SetupStatus::ThresholdMet,
                SetupQuorumProgress::ThresholdImpossible { .. } => {
                    SetupStatus::Failed(SetupFailure::ThresholdUnsatisfied)
                }
                SetupQuorumProgress::AwaitingResponses { .. } => SetupStatus::AwaitingResponses,
            }
        };
        Some(setup)
    }

    fn reduce_proposal(&self, context_id: ContextId) -> Option<MembershipProposalState> {
        let RecoveryFact::MembershipChangeProposed {
            proposer_id,
            change_type,
            proposal_hash,
            proposed_at,
            ..
        } = latest_fact(&self.proposals)?
        else {
            return None;
        };
        let status = if let Some(reason) = canonical_reason(&self.proposal_rejections) {
            ProposalStatus::Rejected(ProposalRejection { reason })
        } else if self.proposal_approved {
            ProposalStatus::Approved
        } else {
            ProposalStatus::Pending
        };
        Some(MembershipProposalState {
            context_id,
            proposer_id: *proposer_id,
            proposal_hash: *proposal_hash,
            change_type: change_type.clone(),
            proposed_at: proposed_at.ts_ms,
            votes_for: canonical_set(self.votes_for.clone()),
            votes_against: canonical_set(self.votes_against.clone()),
            status,
        })
    }

    fn reduce_recovery(&self, context_id: ContextId) -> Option<RecoveryOperationState> {
        let RecoveryFact::RecoveryInitiated {
            account_id,
            request_hash,
            initiated_at,
            ..
        } = latest_fact(&self.recovery_initiations)?
        else {
            return None;
        };
        let dispute = self
            .disputes
            .iter()
            .min()
            .map(|(disputer_id, reason)| RecoveryDispute {
                disputer_id: *disputer_id,
                reason: (*reason).to_string(),
            });
        let status = if let Some(reason) = canonical_reason(&self.recovery_failures) {
            RecoveryStatus::Failed(RecoveryFailure { reason })
        } else if self.recovery_completed {
            RecoveryStatus::Completed
        } else if let Some(dispute) = dispute {
            RecoveryStatus::Disputed(dispute)
        } else if self.recovery_approved {
            RecoveryStatus::Approved
        } else {
            RecoveryStatus::AwaitingShares
        };
        Some(RecoveryOperationState {
            context_id,
            account_id: *account_id,
            request_hash: *request_hash,
            initiated_at: initiated_at.ts_ms,
            shares_submitted: canonical_set(self.shares.clone()),
            disputes: canonical_set(self.disputes.iter().map(|(id, _)| *id).collect()),
            status,
        })
    }
}

/// State of a guardian setup operation.
#[derive(Debug, Clone)]
pub struct SetupState {
    /// Context ID for this setup
    pub context_id: ContextId,
    /// Authority who initiated the setup
    pub initiator_id: AuthorityId,
    /// Timestamp when setup was initiated (ms since epoch)
    pub initiated_at: u64,
    /// Guardians being invited
    pub target_guardians: Vec<AuthorityId>,
    /// Guardians who have accepted
    pub accepted: Vec<AuthorityId>,
    /// Guardians who have declined
    pub declined: Vec<AuthorityId>,
    /// Required threshold for recovery
    pub threshold: u16,
    /// Current status
    pub status: SetupStatus,
}

impl SetupState {
    /// Classify whether the guardian setup can still reach quorum.
    pub fn quorum_progress(&self) -> SetupQuorumProgress {
        let accepted = self.accepted.len();
        if threshold_satisfied(accepted, self.threshold) {
            return SetupQuorumProgress::ThresholdMet {
                accepted,
                threshold: self.threshold,
            };
        }

        let remaining = remaining_guardians(self.target_guardians.len(), self.declined.len());
        if !threshold_satisfied(remaining, self.threshold) {
            return SetupQuorumProgress::ThresholdImpossible {
                remaining,
                threshold: self.threshold,
            };
        }

        SetupQuorumProgress::AwaitingResponses {
            accepted,
            declined: self.declined.len(),
            remaining,
            threshold: self.threshold,
        }
    }

    /// Check if setup can still succeed (enough guardians remaining).
    pub fn can_succeed(&self) -> bool {
        !matches!(
            self.quorum_progress(),
            SetupQuorumProgress::ThresholdImpossible { .. }
        )
    }

    /// Get guardians who haven't responded yet.
    pub fn pending_guardians(&self) -> Vec<&AuthorityId> {
        self.target_guardians
            .iter()
            .filter(|g| !self.accepted.contains(g) && !self.declined.contains(g))
            .collect()
    }
}

/// Explicit quorum progress for guardian setup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupQuorumProgress {
    AwaitingResponses {
        accepted: usize,
        declined: usize,
        remaining: usize,
        threshold: u16,
    },
    ThresholdMet {
        accepted: usize,
        threshold: u16,
    },
    ThresholdImpossible {
        remaining: usize,
        threshold: u16,
    },
}

/// Status of a guardian setup operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupStatus {
    /// Waiting for guardian responses
    AwaitingResponses,
    /// Threshold number of guardians have accepted
    ThresholdMet,
    /// Setup completed successfully
    Completed,
    /// Setup failed (not enough guardians)
    Failed(SetupFailure),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupFailure {
    ThresholdUnsatisfied,
    Explicit { reason: String },
}

/// State of a membership change proposal.
#[derive(Debug, Clone)]
pub struct MembershipProposalState {
    /// Context ID for this proposal
    pub context_id: ContextId,
    /// Authority who proposed the change
    pub proposer_id: AuthorityId,
    /// Hash of the proposal
    pub proposal_hash: Hash32,
    /// Type of membership change
    pub change_type: MembershipChangeType,
    /// Timestamp when proposed (ms since epoch)
    pub proposed_at: u64,
    /// Authorities who voted for
    pub votes_for: Vec<AuthorityId>,
    /// Authorities who voted against
    pub votes_against: Vec<AuthorityId>,
    /// Current status
    pub status: ProposalStatus,
}

impl MembershipProposalState {
    /// Get total votes cast.
    pub fn total_votes(&self) -> usize {
        self.votes_for.len() + self.votes_against.len()
    }
}

/// Status of a membership change proposal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProposalStatus {
    /// Awaiting votes
    Pending,
    /// Proposal was approved
    Approved,
    /// Proposal was rejected
    Rejected(ProposalRejection),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposalRejection {
    pub reason: String,
}

/// State of a key recovery operation.
#[derive(Debug, Clone)]
pub struct RecoveryOperationState {
    /// Context ID for this recovery
    pub context_id: ContextId,
    /// Account being recovered
    pub account_id: AuthorityId,
    /// Hash of the recovery request
    pub request_hash: Hash32,
    /// Timestamp when initiated (ms since epoch)
    pub initiated_at: u64,
    /// Guardians who have submitted shares
    pub shares_submitted: Vec<AuthorityId>,
    /// Guardians who have filed disputes
    pub disputes: Vec<AuthorityId>,
    /// Current status
    pub status: RecoveryStatus,
}

impl RecoveryOperationState {
    /// Classify whether enough guardian shares have been submitted.
    pub fn share_progress(&self, threshold: usize) -> RecoveryShareProgress {
        let submitted = self.shares_submitted.len();
        if submitted >= threshold {
            RecoveryShareProgress::ThresholdMet {
                submitted,
                threshold,
            }
        } else {
            RecoveryShareProgress::AwaitingThreshold {
                submitted,
                threshold,
            }
        }
    }

    /// Check if threshold shares have been submitted.
    pub fn has_threshold_shares(&self, threshold: usize) -> bool {
        matches!(
            self.share_progress(threshold),
            RecoveryShareProgress::ThresholdMet { .. }
        )
    }
}

/// Explicit share-progress classification for recovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryShareProgress {
    AwaitingThreshold { submitted: usize, threshold: usize },
    ThresholdMet { submitted: usize, threshold: usize },
}

/// Status of a key recovery operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryStatus {
    /// Waiting for guardian shares
    AwaitingShares,
    /// Guardian approvals reached quorum
    Approved,
    /// A dispute has been filed
    Disputed(RecoveryDispute),
    /// Recovery completed successfully
    Completed,
    /// Recovery failed
    Failed(RecoveryFailure),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryDispute {
    pub disputer_id: AuthorityId,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryFailure {
    pub reason: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::time::PhysicalTime;

    fn test_context_id() -> ContextId {
        ContextId::new_from_entropy([42u8; 32])
    }

    fn test_authority_id(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn test_hash(seed: u8) -> Hash32 {
        Hash32([seed; 32])
    }

    fn pt(ts_ms: u64) -> PhysicalTime {
        PhysicalTime {
            ts_ms,
            uncertainty: None,
        }
    }

    /// Setup state derives correctly from initiation + acceptance facts.
    #[test]
    fn test_setup_state_derivation() {
        let ctx = test_context_id();
        let initiator = test_authority_id(1);
        let guardian1 = test_authority_id(2);
        let guardian2 = test_authority_id(3);
        let guardian3 = test_authority_id(4);

        let facts = vec![
            RecoveryFact::GuardianSetupInitiated {
                context_id: ctx,
                initiator_id: initiator,
                trace_id: None,
                guardian_ids: vec![guardian1, guardian2, guardian3],
                threshold: 2,
                initiated_at: pt(1000),
            },
            RecoveryFact::GuardianAccepted {
                context_id: ctx,
                guardian_id: guardian1,
                trace_id: None,
                accepted_at: pt(2000),
            },
        ];

        let state = RecoveryState::from_facts(&facts);
        let setup = state.setup_for_context(&ctx).unwrap();

        assert_eq!(setup.accepted.len(), 1);
        assert_eq!(setup.status, SetupStatus::AwaitingResponses);
        assert!(setup.accepted.contains(&guardian1));
    }

    /// Setup reaches ThresholdMet when exactly threshold guardians accept.
    #[test]
    fn test_setup_threshold_met() {
        let ctx = test_context_id();
        let initiator = test_authority_id(1);
        let guardian1 = test_authority_id(2);
        let guardian2 = test_authority_id(3);

        let facts = vec![
            RecoveryFact::GuardianSetupInitiated {
                context_id: ctx,
                initiator_id: initiator,
                trace_id: None,
                guardian_ids: vec![guardian1, guardian2],
                threshold: 2,
                initiated_at: pt(1000),
            },
            RecoveryFact::GuardianAccepted {
                context_id: ctx,
                guardian_id: guardian1,
                trace_id: None,
                accepted_at: pt(2000),
            },
            RecoveryFact::GuardianAccepted {
                context_id: ctx,
                guardian_id: guardian2,
                trace_id: None,
                accepted_at: pt(3000),
            },
        ];

        let state = RecoveryState::from_facts(&facts);
        let setup = state.setup_for_context(&ctx).unwrap();

        assert_eq!(setup.accepted.len(), 2);
        assert_eq!(setup.status, SetupStatus::ThresholdMet);
    }

    /// Setup fails when a guardian declines, dropping below threshold.
    #[test]
    fn test_setup_failed() {
        let ctx = test_context_id();
        let initiator = test_authority_id(1);
        let guardian1 = test_authority_id(2);
        let guardian2 = test_authority_id(3);

        let facts = vec![
            RecoveryFact::GuardianSetupInitiated {
                context_id: ctx,
                initiator_id: initiator,
                trace_id: None,
                guardian_ids: vec![guardian1, guardian2],
                threshold: 2,
                initiated_at: pt(1000),
            },
            RecoveryFact::GuardianDeclined {
                context_id: ctx,
                guardian_id: guardian1,
                trace_id: None,
                declined_at: pt(2000),
            },
        ];

        let state = RecoveryState::from_facts(&facts);
        let setup = state.setup_for_context(&ctx).unwrap();

        assert_eq!(setup.declined.len(), 1);
        assert_eq!(
            setup.status,
            SetupStatus::Failed(SetupFailure::ThresholdUnsatisfied)
        );
    }

    /// Membership proposal state tracks votes for and against.
    #[test]
    fn test_membership_proposal() {
        let ctx = test_context_id();
        let proposer = test_authority_id(1);
        let voter1 = test_authority_id(2);
        let voter2 = test_authority_id(3);

        let facts = vec![
            RecoveryFact::MembershipChangeProposed {
                context_id: ctx,
                proposer_id: proposer,
                trace_id: None,
                change_type: MembershipChangeType::UpdateThreshold { new_threshold: 3 },
                proposal_hash: test_hash(1),
                proposed_at: pt(1000),
            },
            RecoveryFact::MembershipVoteCast {
                context_id: ctx,
                voter_id: voter1,
                trace_id: None,
                proposal_hash: test_hash(1),
                approved: true,
                voted_at: pt(2000),
            },
            RecoveryFact::MembershipVoteCast {
                context_id: ctx,
                voter_id: voter2,
                trace_id: None,
                proposal_hash: test_hash(1),
                approved: false,
                voted_at: pt(3000),
            },
        ];

        let state = RecoveryState::from_facts(&facts);
        let proposal = state.proposal_for_context(&ctx).unwrap();

        assert_eq!(proposal.votes_for.len(), 1);
        assert_eq!(proposal.votes_against.len(), 1);
        assert_eq!(proposal.status, ProposalStatus::Pending);
    }

    /// Rejection reason survives fact reduction — needed for UX display.
    #[test]
    fn test_membership_rejection_preserves_reason() {
        let ctx = test_context_id();
        let proposer = test_authority_id(1);

        let facts = vec![
            RecoveryFact::MembershipChangeProposed {
                context_id: ctx,
                proposer_id: proposer,
                trace_id: None,
                change_type: MembershipChangeType::UpdateThreshold { new_threshold: 3 },
                proposal_hash: test_hash(1),
                proposed_at: pt(1000),
            },
            RecoveryFact::MembershipChangeRejected {
                context_id: ctx,
                proposal_hash: test_hash(1),
                trace_id: None,
                reason: "guardian quorum denied".to_string(),
                rejected_at: pt(1500),
            },
        ];

        let state = RecoveryState::from_facts(&facts);
        let proposal = state.proposal_for_context(&ctx).unwrap();

        assert_eq!(
            proposal.status,
            ProposalStatus::Rejected(ProposalRejection {
                reason: "guardian quorum denied".to_string(),
            })
        );
    }

    /// Recovery operation tracks submitted shares and stays in AwaitingShares.
    #[test]
    fn test_recovery_operation() {
        let ctx = test_context_id();
        let account = test_authority_id(1);
        let guardian1 = test_authority_id(2);

        let facts = vec![
            RecoveryFact::RecoveryInitiated {
                context_id: ctx,
                account_id: account,
                trace_id: None,
                request_hash: test_hash(1),
                initiated_at: pt(1000),
            },
            RecoveryFact::RecoveryShareSubmitted {
                context_id: ctx,
                guardian_id: guardian1,
                trace_id: None,
                share_hash: test_hash(2),
                submitted_at: pt(2000),
            },
        ];

        let state = RecoveryState::from_facts(&facts);
        let recovery = state.recovery_for_context(&ctx).unwrap();

        assert_eq!(recovery.shares_submitted.len(), 1);
        assert_eq!(recovery.status, RecoveryStatus::AwaitingShares);
        assert!(recovery.shares_submitted.contains(&guardian1));
    }

    /// Recovery transitions to Approved after an approval fact.
    #[test]
    fn test_recovery_approved() {
        let ctx = test_context_id();
        let account = test_authority_id(1);

        let facts = vec![
            RecoveryFact::RecoveryInitiated {
                context_id: ctx,
                account_id: account,
                trace_id: None,
                request_hash: test_hash(1),
                initiated_at: pt(1000),
            },
            RecoveryFact::RecoveryApproved {
                context_id: ctx,
                account_id: account,
                trace_id: None,
                approvals_hash: test_hash(2),
                approved_at: pt(1500),
            },
        ];

        let state = RecoveryState::from_facts(&facts);
        let recovery = state.recovery_for_context(&ctx).unwrap();

        assert_eq!(recovery.status, RecoveryStatus::Approved);
    }

    #[test]
    fn test_recovery_disputed() {
        let ctx = test_context_id();
        let account = test_authority_id(1);
        let disputer = test_authority_id(2);

        let facts = vec![
            RecoveryFact::RecoveryInitiated {
                context_id: ctx,
                account_id: account,
                trace_id: None,
                request_hash: test_hash(1),
                initiated_at: pt(1000),
            },
            RecoveryFact::RecoveryDisputeFiled {
                context_id: ctx,
                disputer_id: disputer,
                trace_id: None,
                reason: "Unauthorized recovery attempt".to_string(),
                filed_at: pt(2000),
            },
        ];

        let state = RecoveryState::from_facts(&facts);
        let recovery = state.recovery_for_context(&ctx).unwrap();

        assert_eq!(recovery.disputes.len(), 1);
        assert_eq!(
            recovery.status,
            RecoveryStatus::Disputed(RecoveryDispute {
                disputer_id: disputer,
                reason: "Unauthorized recovery attempt".to_string(),
            })
        );
    }

    #[test]
    fn test_recovery_failure_preserves_reason() {
        let ctx = test_context_id();
        let account = test_authority_id(1);

        let facts = vec![
            RecoveryFact::RecoveryInitiated {
                context_id: ctx,
                account_id: account,
                trace_id: None,
                request_hash: test_hash(1),
                initiated_at: pt(1000),
            },
            RecoveryFact::RecoveryFailed {
                context_id: ctx,
                account_id: account,
                trace_id: None,
                reason: "guardian share verification failed".to_string(),
                failed_at: pt(1500),
            },
        ];

        let state = RecoveryState::from_facts(&facts);
        let recovery = state.recovery_for_context(&ctx).unwrap();

        assert_eq!(
            recovery.status,
            RecoveryStatus::Failed(RecoveryFailure {
                reason: "guardian share verification failed".to_string(),
            })
        );
    }

    /// Duplicate share submission from the same guardian doesn't inflate
    /// the share count. If deduplication fails, a single guardian could
    /// satisfy the threshold alone by submitting multiple times.
    #[test]
    fn test_duplicate_share_submission_deduplicated() {
        let ctx = test_context_id();
        let account = test_authority_id(1);
        let guardian1 = test_authority_id(2);

        let facts = vec![
            RecoveryFact::RecoveryInitiated {
                context_id: ctx,
                account_id: account,
                trace_id: None,
                request_hash: test_hash(1),
                initiated_at: pt(1000),
            },
            RecoveryFact::RecoveryShareSubmitted {
                context_id: ctx,
                guardian_id: guardian1,
                trace_id: None,
                share_hash: test_hash(2),
                submitted_at: pt(2000),
            },
            // Same guardian submits again
            RecoveryFact::RecoveryShareSubmitted {
                context_id: ctx,
                guardian_id: guardian1,
                trace_id: None,
                share_hash: test_hash(3),
                submitted_at: pt(3000),
            },
        ];

        let state = RecoveryState::from_facts(&facts);
        let recovery = state.recovery_for_context(&ctx).unwrap();

        assert_eq!(
            recovery.shares_submitted.len(),
            1,
            "Duplicate share from same guardian must be deduplicated"
        );
    }

    #[test]
    fn test_setup_quorum_progress_classifies_threshold_impossible() {
        let setup = SetupState {
            context_id: test_context_id(),
            initiator_id: test_authority_id(1),
            initiated_at: 1000,
            target_guardians: vec![
                test_authority_id(2),
                test_authority_id(3),
                test_authority_id(4),
            ],
            accepted: vec![test_authority_id(2)],
            declined: vec![test_authority_id(3), test_authority_id(4)],
            threshold: 2,
            status: SetupStatus::AwaitingResponses,
        };

        assert_eq!(
            setup.quorum_progress(),
            SetupQuorumProgress::ThresholdImpossible {
                remaining: 1,
                threshold: 2,
            }
        );
        assert!(!setup.can_succeed());
    }

    #[test]
    fn test_recovery_share_progress_classifies_threshold_met() {
        let recovery = RecoveryOperationState {
            context_id: test_context_id(),
            account_id: test_authority_id(1),
            request_hash: test_hash(9),
            initiated_at: 1000,
            shares_submitted: vec![test_authority_id(2), test_authority_id(3)],
            disputes: Vec::new(),
            status: RecoveryStatus::AwaitingShares,
        };

        assert_eq!(
            recovery.share_progress(2),
            RecoveryShareProgress::ThresholdMet {
                submitted: 2,
                threshold: 2,
            }
        );
        assert!(recovery.has_threshold_shares(2));
    }

    #[test]
    fn test_active_operations_query() {
        let ctx1 = test_context_id();
        let ctx2 = ContextId::new_from_entropy([43u8; 32]);
        let initiator = test_authority_id(1);
        let guardian = test_authority_id(2);

        let facts = vec![
            // Active setup
            RecoveryFact::GuardianSetupInitiated {
                context_id: ctx1,
                initiator_id: initiator,
                trace_id: None,
                guardian_ids: vec![guardian],
                threshold: 1,
                initiated_at: pt(1000),
            },
            // Completed setup
            RecoveryFact::GuardianSetupInitiated {
                context_id: ctx2,
                initiator_id: initiator,
                trace_id: None,
                guardian_ids: vec![guardian],
                threshold: 1,
                initiated_at: pt(2000),
            },
            RecoveryFact::GuardianAccepted {
                context_id: ctx2,
                guardian_id: guardian,
                trace_id: None,
                accepted_at: pt(3000),
            },
            RecoveryFact::GuardianSetupCompleted {
                context_id: ctx2,
                guardian_ids: vec![],
                trace_id: None,
                threshold: 1,
                completed_at: pt(4000),
            },
        ];

        let state = RecoveryState::from_facts(&facts);

        let active: Vec<_> = state.active_setups().collect();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].context_id, ctx1);

        assert!(state.has_active_operation(&ctx1));
        assert!(!state.has_active_operation(&ctx2));
    }

    fn permutations(facts: &[RecoveryFact]) -> Vec<Vec<RecoveryFact>> {
        if facts.len() <= 1 {
            return vec![facts.to_vec()];
        }
        let mut out = Vec::new();
        for i in 0..facts.len() {
            let mut rest = facts.to_vec();
            let head = rest.remove(i);
            for mut tail in permutations(&rest) {
                tail.insert(0, head.clone());
                out.push(tail);
            }
        }
        out
    }

    fn snapshot(state: &RecoveryState, ctx: &ContextId) -> String {
        format!(
            "{:?}|{:?}|{:?}",
            state.setup_for_context(ctx),
            state.proposal_for_context(ctx),
            state.recovery_for_context(ctx)
        )
    }

    /// Every permutation (plus a duplicated copy) reduces to one state.
    fn assert_order_independent(facts: &[RecoveryFact]) -> RecoveryState {
        let ctx = test_context_id();
        let expected = RecoveryState::from_facts(facts);
        let expected_snapshot = snapshot(&expected, &ctx);
        for order in permutations(facts) {
            assert_eq!(
                snapshot(&RecoveryState::from_facts(&order), &ctx),
                expected_snapshot
            );
            let mut doubled = order.clone();
            doubled.extend(order);
            assert_eq!(
                snapshot(&RecoveryState::from_facts(&doubled), &ctx),
                expected_snapshot
            );
        }
        expected
    }

    #[test]
    fn setup_after_approvals_holds_early_responses() {
        let ctx = test_context_id();
        let (g1, g2, g3) = (
            test_authority_id(2),
            test_authority_id(3),
            test_authority_id(4),
        );
        let facts = vec![
            RecoveryFact::GuardianAccepted {
                context_id: ctx,
                guardian_id: g1,
                trace_id: None,
                accepted_at: pt(2000),
            },
            RecoveryFact::GuardianAccepted {
                context_id: ctx,
                guardian_id: g2,
                trace_id: None,
                accepted_at: pt(2100),
            },
            RecoveryFact::GuardianDeclined {
                context_id: ctx,
                guardian_id: g3,
                trace_id: None,
                declined_at: pt(2200),
            },
            RecoveryFact::GuardianSetupInitiated {
                context_id: ctx,
                initiator_id: test_authority_id(1),
                trace_id: None,
                guardian_ids: vec![g1, g2, g3],
                threshold: 2,
                initiated_at: pt(1000),
            },
        ];
        let state = assert_order_independent(&facts);
        let setup = state.setup_for_context(&ctx).unwrap();
        assert_eq!(setup.accepted.len(), 2);
        assert_eq!(setup.declined, vec![g3]);
        assert_eq!(setup.status, SetupStatus::ThresholdMet);
    }

    #[test]
    fn post_terminal_setup_facts_do_not_change_terminal_state() {
        let ctx = test_context_id();
        let (g1, g2) = (test_authority_id(2), test_authority_id(3));
        let facts = vec![
            RecoveryFact::GuardianSetupInitiated {
                context_id: ctx,
                initiator_id: test_authority_id(1),
                trace_id: None,
                guardian_ids: vec![g1, g2],
                threshold: 2,
                initiated_at: pt(1000),
            },
            RecoveryFact::GuardianAccepted {
                context_id: ctx,
                guardian_id: g1,
                trace_id: None,
                accepted_at: pt(2000),
            },
            RecoveryFact::GuardianSetupCompleted {
                context_id: ctx,
                guardian_ids: vec![g1],
                trace_id: None,
                threshold: 2,
                completed_at: pt(3000),
            },
            RecoveryFact::GuardianDeclined {
                context_id: ctx,
                guardian_id: g2,
                trace_id: None,
                declined_at: pt(4000),
            },
        ];
        let state = assert_order_independent(&facts);
        assert_eq!(
            state.setup_for_context(&ctx).unwrap().status,
            SetupStatus::Completed
        );
    }

    #[test]
    fn concurrent_recovery_completion_and_failure_resolve_to_failure() {
        let ctx = test_context_id();
        let account = test_authority_id(1);
        let facts = vec![
            RecoveryFact::RecoveryShareSubmitted {
                context_id: ctx,
                guardian_id: test_authority_id(2),
                trace_id: None,
                share_hash: test_hash(3),
                submitted_at: pt(1500),
            },
            RecoveryFact::RecoveryInitiated {
                context_id: ctx,
                account_id: account,
                trace_id: None,
                request_hash: test_hash(1),
                initiated_at: pt(1000),
            },
            RecoveryFact::RecoveryCompleted {
                context_id: ctx,
                account_id: account,
                trace_id: None,
                evidence_hash: test_hash(2),
                completed_at: pt(3000),
            },
            RecoveryFact::RecoveryFailed {
                context_id: ctx,
                account_id: account,
                trace_id: None,
                reason: "b".to_string(),
                failed_at: pt(3000),
            },
            RecoveryFact::RecoveryFailed {
                context_id: ctx,
                account_id: account,
                trace_id: None,
                reason: "a".to_string(),
                failed_at: pt(2900),
            },
            RecoveryFact::RecoveryDisputeFiled {
                context_id: ctx,
                disputer_id: test_authority_id(4),
                trace_id: None,
                reason: "late".to_string(),
                filed_at: pt(4000),
            },
        ];
        let state = assert_order_independent(&facts);
        let recovery = state.recovery_for_context(&ctx).unwrap();
        assert_eq!(
            recovery.status,
            RecoveryStatus::Failed(RecoveryFailure {
                reason: "a".to_string()
            })
        );
        assert_eq!(recovery.shares_submitted.len(), 1);
        assert_eq!(recovery.disputes.len(), 1);
    }

    #[test]
    fn concurrent_setup_completion_and_failure_and_proposal_outcomes() {
        let ctx = test_context_id();
        let g1 = test_authority_id(2);
        let facts = vec![
            RecoveryFact::GuardianSetupInitiated {
                context_id: ctx,
                initiator_id: test_authority_id(1),
                trace_id: None,
                guardian_ids: vec![g1],
                threshold: 1,
                initiated_at: pt(1000),
            },
            RecoveryFact::GuardianSetupCompleted {
                context_id: ctx,
                guardian_ids: vec![g1],
                trace_id: None,
                threshold: 1,
                completed_at: pt(2000),
            },
            RecoveryFact::GuardianSetupFailed {
                context_id: ctx,
                reason: "timeout".to_string(),
                trace_id: None,
                failed_at: pt(2000),
            },
            RecoveryFact::MembershipChangeCompleted {
                context_id: ctx,
                proposal_hash: test_hash(5),
                trace_id: None,
                new_guardian_ids: vec![g1],
                new_threshold: 1,
                completed_at: pt(2500),
            },
            RecoveryFact::MembershipChangeProposed {
                context_id: ctx,
                proposer_id: g1,
                trace_id: None,
                change_type: MembershipChangeType::UpdateThreshold { new_threshold: 1 },
                proposal_hash: test_hash(5),
                proposed_at: pt(2100),
            },
        ];
        let state = assert_order_independent(&facts);
        assert_eq!(
            state.setup_for_context(&ctx).unwrap().status,
            SetupStatus::Failed(SetupFailure::Explicit {
                reason: "timeout".to_string()
            })
        );
        assert_eq!(
            state.proposal_for_context(&ctx).unwrap().status,
            ProposalStatus::Approved
        );
    }
}
