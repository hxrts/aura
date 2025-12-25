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
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::Hash32;
use aura_journal::DomainFact;
use std::collections::HashMap;

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
    /// This is the primary entry point for state derivation. Facts should be
    /// provided in causal order (as they appear in the journal).
    pub fn from_fact_bytes(facts: &[(String, Vec<u8>)]) -> Self {
        let mut state = Self::new();

        for (type_id, data) in facts {
            if type_id == crate::facts::RECOVERY_FACT_TYPE_ID {
                if let Some(fact) = RecoveryFact::from_bytes(data) {
                    state.apply_fact(&fact);
                }
            }
        }

        state
    }

    /// Derive recovery state from a list of RecoveryFact instances.
    pub fn from_facts(facts: &[RecoveryFact]) -> Self {
        let mut state = Self::new();
        for fact in facts {
            state.apply_fact(fact);
        }
        state
    }

    /// Apply a single fact to update state.
    fn apply_fact(&mut self, fact: &RecoveryFact) {
        match fact {
            // Guardian Setup
            RecoveryFact::GuardianSetupInitiated {
                context_id,
                initiator_id,
                guardian_ids,
                threshold,
                initiated_at,
                ..
            } => {
                self.setups.insert(
                    *context_id,
                    SetupState {
                        context_id: *context_id,
                        initiator_id: *initiator_id,
                        initiated_at: initiated_at.ts_ms,
                        target_guardians: guardian_ids.clone(),
                        accepted: Vec::new(),
                        declined: Vec::new(),
                        threshold: *threshold,
                        status: SetupStatus::AwaitingResponses,
                    },
                );
            }

            RecoveryFact::GuardianAccepted {
                context_id,
                guardian_id,
                ..
            } => {
                if let Some(setup) = self.setups.get_mut(context_id) {
                    if !setup.accepted.contains(guardian_id) {
                        setup.accepted.push(*guardian_id);
                    }
                    // Check if threshold is met
                    if setup.accepted.len() >= setup.threshold as usize {
                        setup.status = SetupStatus::ThresholdMet;
                    }
                }
            }

            RecoveryFact::GuardianDeclined {
                context_id,
                guardian_id,
                ..
            } => {
                if let Some(setup) = self.setups.get_mut(context_id) {
                    if !setup.declined.contains(guardian_id) {
                        setup.declined.push(*guardian_id);
                    }
                    // Check if setup has failed (not enough guardians left)
                    let remaining = setup.target_guardians.len() - setup.declined.len();
                    if remaining < setup.threshold as usize {
                        setup.status = SetupStatus::Failed;
                    }
                }
            }

            RecoveryFact::GuardianSetupCompleted { context_id, .. } => {
                if let Some(setup) = self.setups.get_mut(context_id) {
                    setup.status = SetupStatus::Completed;
                }
            }

            RecoveryFact::GuardianSetupFailed { context_id, .. } => {
                if let Some(setup) = self.setups.get_mut(context_id) {
                    setup.status = SetupStatus::Failed;
                }
            }

            // Membership Changes
            RecoveryFact::MembershipChangeProposed {
                context_id,
                proposer_id,
                change_type,
                proposal_hash,
                proposed_at,
                ..
            } => {
                self.proposals.insert(
                    *context_id,
                    MembershipProposalState {
                        context_id: *context_id,
                        proposer_id: *proposer_id,
                        proposal_hash: *proposal_hash,
                        change_type: change_type.clone(),
                        proposed_at: proposed_at.ts_ms,
                        votes_for: Vec::new(),
                        votes_against: Vec::new(),
                        status: ProposalStatus::Pending,
                    },
                );
            }

            RecoveryFact::MembershipVoteCast {
                context_id,
                voter_id,
                approved,
                ..
            } => {
                if let Some(proposal) = self.proposals.get_mut(context_id) {
                    if *approved {
                        if !proposal.votes_for.contains(voter_id) {
                            proposal.votes_for.push(*voter_id);
                        }
                    } else if !proposal.votes_against.contains(voter_id) {
                        proposal.votes_against.push(*voter_id);
                    }
                }
            }

            RecoveryFact::MembershipChangeCompleted { context_id, .. } => {
                if let Some(proposal) = self.proposals.get_mut(context_id) {
                    proposal.status = ProposalStatus::Approved;
                }
            }

            RecoveryFact::MembershipChangeRejected { context_id, .. } => {
                if let Some(proposal) = self.proposals.get_mut(context_id) {
                    proposal.status = ProposalStatus::Rejected;
                }
            }

            // Key Recovery
            RecoveryFact::RecoveryInitiated {
                context_id,
                account_id,
                request_hash,
                initiated_at,
                ..
            } => {
                self.recoveries.insert(
                    *context_id,
                    RecoveryOperationState {
                        context_id: *context_id,
                        account_id: *account_id,
                        request_hash: *request_hash,
                        initiated_at: initiated_at.ts_ms,
                        shares_submitted: Vec::new(),
                        disputes: Vec::new(),
                        status: RecoveryStatus::AwaitingShares,
                    },
                );
            }

            RecoveryFact::RecoveryShareSubmitted {
                context_id,
                guardian_id,
                ..
            } => {
                if let Some(recovery) = self.recoveries.get_mut(context_id) {
                    if !recovery.shares_submitted.contains(guardian_id) {
                        recovery.shares_submitted.push(*guardian_id);
                    }
                }
            }

            RecoveryFact::RecoveryDisputeFiled {
                context_id,
                disputer_id,
                ..
            } => {
                if let Some(recovery) = self.recoveries.get_mut(context_id) {
                    if !recovery.disputes.contains(disputer_id) {
                        recovery.disputes.push(*disputer_id);
                    }
                    recovery.status = RecoveryStatus::Disputed;
                }
            }

            RecoveryFact::RecoveryCompleted { context_id, .. } => {
                if let Some(recovery) = self.recoveries.get_mut(context_id) {
                    recovery.status = RecoveryStatus::Completed;
                }
            }

            RecoveryFact::RecoveryFailed { context_id, .. } => {
                if let Some(recovery) = self.recoveries.get_mut(context_id) {
                    recovery.status = RecoveryStatus::Failed;
                }
            }

            // Events that don't affect state tracking
            RecoveryFact::GuardianInvitationSent { .. } => {}
        }
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
            .filter(|s| !matches!(s.status, SetupStatus::Completed | SetupStatus::Failed))
    }

    /// Get all active (non-completed/failed) proposals.
    pub fn active_proposals(&self) -> impl Iterator<Item = &MembershipProposalState> {
        self.proposals.values().filter(|p| {
            !matches!(
                p.status,
                ProposalStatus::Approved | ProposalStatus::Rejected
            )
        })
    }

    /// Get all active (non-completed/failed) recoveries.
    pub fn active_recoveries(&self) -> impl Iterator<Item = &RecoveryOperationState> {
        self.recoveries
            .values()
            .filter(|r| !matches!(r.status, RecoveryStatus::Completed | RecoveryStatus::Failed))
    }

    /// Check if there's any active operation for a context.
    pub fn has_active_operation(&self, context_id: &ContextId) -> bool {
        self.setup_for_context(context_id)
            .is_some_and(|s| !matches!(s.status, SetupStatus::Completed | SetupStatus::Failed))
            || self.proposal_for_context(context_id).is_some_and(|p| {
                !matches!(
                    p.status,
                    ProposalStatus::Approved | ProposalStatus::Rejected
                )
            })
            || self.recovery_for_context(context_id).is_some_and(|r| {
                !matches!(r.status, RecoveryStatus::Completed | RecoveryStatus::Failed)
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
    /// Check if setup can still succeed (enough guardians remaining).
    pub fn can_succeed(&self) -> bool {
        let remaining = self.target_guardians.len() - self.declined.len();
        remaining >= self.threshold as usize
    }

    /// Get guardians who haven't responded yet.
    pub fn pending_guardians(&self) -> Vec<&AuthorityId> {
        self.target_guardians
            .iter()
            .filter(|g| !self.accepted.contains(g) && !self.declined.contains(g))
            .collect()
    }
}

/// Status of a guardian setup operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupStatus {
    /// Waiting for guardian responses
    AwaitingResponses,
    /// Threshold number of guardians have accepted
    ThresholdMet,
    /// Setup completed successfully
    Completed,
    /// Setup failed (not enough guardians)
    Failed,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProposalStatus {
    /// Awaiting votes
    Pending,
    /// Proposal was approved
    Approved,
    /// Proposal was rejected
    Rejected,
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
    /// Check if threshold shares have been submitted.
    pub fn has_threshold_shares(&self, threshold: usize) -> bool {
        self.shares_submitted.len() >= threshold
    }
}

/// Status of a key recovery operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryStatus {
    /// Waiting for guardian shares
    AwaitingShares,
    /// A dispute has been filed
    Disputed,
    /// Recovery completed successfully
    Completed,
    /// Recovery failed
    Failed,
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
        assert_eq!(setup.status, SetupStatus::Failed);
    }

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
        assert_eq!(recovery.status, RecoveryStatus::Disputed);
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
}
