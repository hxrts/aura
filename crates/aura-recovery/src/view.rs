//! Recovery View Delta and Reducer
//!
//! This module provides view-level reduction for recovery facts, transforming
//! journal facts into UI-level deltas for recovery status views.
//!
//! # Architecture
//!
//! View reduction is separate from journal-level reduction:
//! - **Journal reduction** (`RecoveryFactReducer`): Facts → `RelationalBinding` for storage
//! - **View reduction** (this module): Facts → `RecoveryDelta` for UI updates
//!
//! # Usage
//!
//! Register the reducer with the runtime's `ViewDeltaRegistry`:
//!
//! ```ignore
//! use aura_recovery::{RecoveryViewReducer, RECOVERY_FACT_TYPE_ID};
//! use aura_composition::ViewDeltaRegistry;
//!
//! let mut registry = ViewDeltaRegistry::new();
//! registry.register(RECOVERY_FACT_TYPE_ID, Box::new(RecoveryViewReducer));
//! ```

use aura_composition::{IntoViewDelta, ViewDelta, ViewDeltaReducer};
use aura_core::identifiers::AuthorityId;
use aura_journal::DomainFact;
use hex;

use crate::{RecoveryFact, RECOVERY_FACT_TYPE_ID};

/// Delta type for recovery view updates.
///
/// These deltas represent incremental changes to recovery UI state,
/// derived from journal facts during view reduction.
#[derive(Debug, Clone, PartialEq)]
pub enum RecoveryDelta {
    // ========================================================================
    // Guardian Setup Deltas
    // ========================================================================
    /// Guardian setup ceremony has started
    GuardianSetupStarted {
        /// Number of guardians being set up
        guardian_count: usize,
        /// Required threshold for recovery
        threshold: u16,
        /// Timestamp when setup started (ms since epoch)
        started_at: u64,
    },

    /// A guardian has responded to an invitation
    GuardianResponded {
        /// Authority ID of the guardian (for display lookup)
        guardian_id: String,
        /// Whether the guardian accepted or declined
        accepted: bool,
        /// Timestamp of the response
        responded_at: u64,
    },

    /// Progress update for guardian setup
    GuardianSetupProgress {
        /// Number of guardians who have accepted
        accepted_count: usize,
        /// Total number of guardians invited
        total_count: usize,
        /// Required threshold
        threshold: u16,
    },

    /// Guardian setup completed successfully
    GuardianSetupCompleted {
        /// List of guardian authority IDs
        guardian_ids: Vec<String>,
        /// Final threshold
        threshold: u16,
        /// Timestamp when completed
        completed_at: u64,
    },

    /// Guardian setup failed
    GuardianSetupFailed {
        /// Reason for failure
        reason: String,
        /// Timestamp when failed
        failed_at: u64,
    },

    // ========================================================================
    // Membership Change Deltas
    // ========================================================================
    /// A membership change proposal was created
    MembershipProposalCreated {
        /// Hash of the proposal (for tracking)
        proposal_hash: String,
        /// Description of the proposed change
        change_description: String,
        /// Timestamp when proposed
        proposed_at: u64,
    },

    /// A vote was received on a membership proposal
    MembershipVoteReceived {
        /// Hash of the proposal being voted on
        proposal_hash: String,
        /// Authority ID of the voter
        voter_id: String,
        /// Whether they voted in favor
        approved: bool,
        /// Current votes for
        votes_for: usize,
        /// Current votes against
        votes_against: usize,
    },

    /// Membership change was applied
    MembershipChangeApplied {
        /// Hash of the completed proposal
        proposal_hash: String,
        /// New number of guardians
        new_guardian_count: usize,
        /// New threshold
        new_threshold: u16,
        /// Timestamp when applied
        applied_at: u64,
    },

    /// Membership change was rejected
    MembershipChangeRejected {
        /// Hash of the rejected proposal
        proposal_hash: String,
        /// Reason for rejection
        reason: String,
        /// Timestamp when rejected
        rejected_at: u64,
    },

    // ========================================================================
    // Key Recovery Deltas
    // ========================================================================
    /// Key recovery has been initiated
    RecoveryStarted {
        /// Account authority ID being recovered
        account_id: String,
        /// Number of shares needed for recovery
        shares_needed: u16,
        /// Timestamp when started
        started_at: u64,
    },

    /// A recovery share was received from a guardian
    RecoveryShareReceived {
        /// Guardian authority ID who submitted the share
        guardian_id: String,
        /// Current number of shares received
        shares_received: usize,
        /// Total shares needed
        shares_needed: u16,
    },

    /// Recovery is in the dispute window
    RecoveryDisputeWindow {
        /// Timestamp when dispute window ends (ms since epoch)
        dispute_end_ms: u64,
        /// Number of disputes filed so far
        disputes_filed: usize,
    },

    /// Recovery completed successfully
    RecoverySucceeded {
        /// Account authority ID that was recovered
        account_id: String,
        /// Timestamp when recovery completed
        completed_at: u64,
    },

    /// Recovery failed
    RecoveryFailed {
        /// Account authority ID that attempted recovery
        account_id: String,
        /// Reason for failure
        reason: String,
        /// Timestamp when failed
        failed_at: u64,
    },
}

/// Helper to format an AuthorityId for display
fn format_authority_id(id: &AuthorityId) -> String {
    // Use a shortened hex representation for display
    // AuthorityId is 16 bytes (UUID), so use indices 0,1 and 14,15
    let bytes = id.to_bytes();
    format!(
        "{:02x}{:02x}..{:02x}{:02x}",
        bytes[0], bytes[1], bytes[14], bytes[15]
    )
}

/// View reducer for recovery facts.
///
/// Transforms `RecoveryFact` instances into `RecoveryDelta` view updates.
pub struct RecoveryViewReducer;

impl ViewDeltaReducer for RecoveryViewReducer {
    fn handles_type(&self) -> &'static str {
        RECOVERY_FACT_TYPE_ID
    }

    fn reduce_fact(
        &self,
        binding_type: &str,
        binding_data: &[u8],
        _own_authority: Option<AuthorityId>,
    ) -> Vec<ViewDelta> {
        if binding_type != RECOVERY_FACT_TYPE_ID {
            return vec![];
        }

        let Some(fact) = RecoveryFact::from_bytes(binding_data) else {
            return vec![];
        };

        let delta = match fact {
            // Guardian Setup
            RecoveryFact::GuardianSetupInitiated {
                guardian_ids,
                threshold,
                initiated_at,
                ..
            } => Some(RecoveryDelta::GuardianSetupStarted {
                guardian_count: guardian_ids.len(),
                threshold,
                started_at: initiated_at.ts_ms,
            }),

            RecoveryFact::GuardianInvitationSent { .. } => {
                // Invitation sent doesn't need a separate delta - setup progress covers it
                None
            }

            RecoveryFact::GuardianAccepted {
                guardian_id,
                accepted_at,
                ..
            } => Some(RecoveryDelta::GuardianResponded {
                guardian_id: format_authority_id(&guardian_id),
                accepted: true,
                responded_at: accepted_at.ts_ms,
            }),

            RecoveryFact::GuardianDeclined {
                guardian_id,
                declined_at,
                ..
            } => Some(RecoveryDelta::GuardianResponded {
                guardian_id: format_authority_id(&guardian_id),
                accepted: false,
                responded_at: declined_at.ts_ms,
            }),

            RecoveryFact::GuardianSetupCompleted {
                guardian_ids,
                threshold,
                completed_at,
                ..
            } => Some(RecoveryDelta::GuardianSetupCompleted {
                guardian_ids: guardian_ids.iter().map(format_authority_id).collect(),
                threshold,
                completed_at: completed_at.ts_ms,
            }),

            RecoveryFact::GuardianSetupFailed {
                reason, failed_at, ..
            } => Some(RecoveryDelta::GuardianSetupFailed {
                reason,
                failed_at: failed_at.ts_ms,
            }),

            // Membership Changes
            RecoveryFact::MembershipChangeProposed {
                change_type,
                proposal_hash,
                proposed_at,
                ..
            } => {
                let change_description = match change_type {
                    crate::facts::MembershipChangeType::AddGuardian { guardian_id } => {
                        format!("Add guardian {}", format_authority_id(&guardian_id))
                    }
                    crate::facts::MembershipChangeType::RemoveGuardian { guardian_id } => {
                        format!("Remove guardian {}", format_authority_id(&guardian_id))
                    }
                    crate::facts::MembershipChangeType::UpdateThreshold { new_threshold } => {
                        format!("Update threshold to {}", new_threshold)
                    }
                };
                Some(RecoveryDelta::MembershipProposalCreated {
                    proposal_hash: hex::encode(proposal_hash.0),
                    change_description,
                    proposed_at: proposed_at.ts_ms,
                })
            }

            RecoveryFact::MembershipVoteCast {
                voter_id,
                proposal_hash,
                approved,
                ..
            } => Some(RecoveryDelta::MembershipVoteReceived {
                proposal_hash: hex::encode(proposal_hash.0),
                voter_id: format_authority_id(&voter_id),
                approved,
                // Note: vote counts would need to be derived from accumulated facts
                votes_for: if approved { 1 } else { 0 },
                votes_against: if approved { 0 } else { 1 },
            }),

            RecoveryFact::MembershipChangeCompleted {
                proposal_hash,
                new_guardian_ids,
                new_threshold,
                completed_at,
                ..
            } => Some(RecoveryDelta::MembershipChangeApplied {
                proposal_hash: hex::encode(proposal_hash.0),
                new_guardian_count: new_guardian_ids.len(),
                new_threshold,
                applied_at: completed_at.ts_ms,
            }),

            RecoveryFact::MembershipChangeRejected {
                proposal_hash,
                reason,
                rejected_at,
                ..
            } => Some(RecoveryDelta::MembershipChangeRejected {
                proposal_hash: hex::encode(proposal_hash.0),
                reason,
                rejected_at: rejected_at.ts_ms,
            }),

            // Key Recovery
            RecoveryFact::RecoveryInitiated {
                account_id,
                initiated_at,
                ..
            } => Some(RecoveryDelta::RecoveryStarted {
                account_id: format_authority_id(&account_id),
                shares_needed: 0, // Would need context to know threshold
                started_at: initiated_at.ts_ms,
            }),

            RecoveryFact::RecoveryShareSubmitted { guardian_id, .. } => {
                Some(RecoveryDelta::RecoveryShareReceived {
                    guardian_id: format_authority_id(&guardian_id),
                    shares_received: 1, // Would need accumulated state
                    shares_needed: 0,   // Would need context
                })
            }

            RecoveryFact::RecoveryDisputeFiled { filed_at, .. } => {
                // Assume 1 hour dispute window
                Some(RecoveryDelta::RecoveryDisputeWindow {
                    dispute_end_ms: filed_at.ts_ms + 3_600_000,
                    disputes_filed: 1, // Would need accumulated state
                })
            }

            RecoveryFact::RecoveryCompleted {
                account_id,
                completed_at,
                ..
            } => Some(RecoveryDelta::RecoverySucceeded {
                account_id: format_authority_id(&account_id),
                completed_at: completed_at.ts_ms,
            }),

            RecoveryFact::RecoveryFailed {
                account_id,
                reason,
                failed_at,
                ..
            } => Some(RecoveryDelta::RecoveryFailed {
                account_id: format_authority_id(&account_id),
                reason,
                failed_at: failed_at.ts_ms,
            }),
        };

        delta.map(|d| vec![d.into_view_delta()]).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_composition::downcast_delta;
    use aura_core::{identifiers::ContextId, time::PhysicalTime, Hash32};

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
    fn test_guardian_setup_initiated_reduction() {
        let reducer = RecoveryViewReducer;

        let fact = RecoveryFact::GuardianSetupInitiated {
            context_id: test_context_id(),
            initiator_id: test_authority_id(1),
            trace_id: None,
            guardian_ids: vec![
                test_authority_id(2),
                test_authority_id(3),
                test_authority_id(4),
            ],
            threshold: 2,
            initiated_at: pt(1234567890),
        };

        let bytes = fact.to_bytes();
        let deltas = reducer.reduce_fact(RECOVERY_FACT_TYPE_ID, &bytes, None);

        assert_eq!(deltas.len(), 1);
        let delta = downcast_delta::<RecoveryDelta>(&deltas[0]).unwrap();
        match delta {
            RecoveryDelta::GuardianSetupStarted {
                guardian_count,
                threshold,
                started_at,
            } => {
                assert_eq!(*guardian_count, 3);
                assert_eq!(*threshold, 2);
                assert_eq!(*started_at, 1234567890);
            }
            _ => panic!("Expected GuardianSetupStarted delta"),
        }
    }

    #[test]
    fn test_guardian_accepted_reduction() {
        let reducer = RecoveryViewReducer;

        let fact = RecoveryFact::GuardianAccepted {
            context_id: test_context_id(),
            guardian_id: test_authority_id(5),
            trace_id: None,
            accepted_at: pt(1234567899),
        };

        let bytes = fact.to_bytes();
        let deltas = reducer.reduce_fact(RECOVERY_FACT_TYPE_ID, &bytes, None);

        assert_eq!(deltas.len(), 1);
        let delta = downcast_delta::<RecoveryDelta>(&deltas[0]).unwrap();
        match delta {
            RecoveryDelta::GuardianResponded { accepted, .. } => {
                assert!(*accepted);
            }
            _ => panic!("Expected GuardianResponded delta"),
        }
    }

    #[test]
    fn test_guardian_declined_reduction() {
        let reducer = RecoveryViewReducer;

        let fact = RecoveryFact::GuardianDeclined {
            context_id: test_context_id(),
            guardian_id: test_authority_id(6),
            trace_id: None,
            declined_at: pt(1234567900),
        };

        let bytes = fact.to_bytes();
        let deltas = reducer.reduce_fact(RECOVERY_FACT_TYPE_ID, &bytes, None);

        assert_eq!(deltas.len(), 1);
        let delta = downcast_delta::<RecoveryDelta>(&deltas[0]).unwrap();
        match delta {
            RecoveryDelta::GuardianResponded { accepted, .. } => {
                assert!(!*accepted);
            }
            _ => panic!("Expected GuardianResponded delta"),
        }
    }

    #[test]
    fn test_recovery_completed_reduction() {
        let reducer = RecoveryViewReducer;

        let fact = RecoveryFact::RecoveryCompleted {
            context_id: test_context_id(),
            account_id: test_authority_id(1),
            trace_id: None,
            evidence_hash: test_hash(99),
            completed_at: pt(1234567999),
        };

        let bytes = fact.to_bytes();
        let deltas = reducer.reduce_fact(RECOVERY_FACT_TYPE_ID, &bytes, None);

        assert_eq!(deltas.len(), 1);
        let delta = downcast_delta::<RecoveryDelta>(&deltas[0]).unwrap();
        match delta {
            RecoveryDelta::RecoverySucceeded { completed_at, .. } => {
                assert_eq!(*completed_at, 1234567999);
            }
            _ => panic!("Expected RecoverySucceeded delta"),
        }
    }

    #[test]
    fn test_membership_proposal_reduction() {
        let reducer = RecoveryViewReducer;

        let fact = RecoveryFact::MembershipChangeProposed {
            context_id: test_context_id(),
            proposer_id: test_authority_id(1),
            trace_id: None,
            change_type: crate::facts::MembershipChangeType::UpdateThreshold { new_threshold: 3 },
            proposal_hash: test_hash(42),
            proposed_at: pt(1234567890),
        };

        let bytes = fact.to_bytes();
        let deltas = reducer.reduce_fact(RECOVERY_FACT_TYPE_ID, &bytes, None);

        assert_eq!(deltas.len(), 1);
        let delta = downcast_delta::<RecoveryDelta>(&deltas[0]).unwrap();
        match delta {
            RecoveryDelta::MembershipProposalCreated {
                change_description, ..
            } => {
                assert!(change_description.contains("threshold"));
                assert!(change_description.contains("3"));
            }
            _ => panic!("Expected MembershipProposalCreated delta"),
        }
    }

    #[test]
    fn test_wrong_type_returns_empty() {
        let reducer = RecoveryViewReducer;
        let deltas = reducer.reduce_fact("wrong_type", b"some data", None);
        assert!(deltas.is_empty());
    }

    #[test]
    fn test_invalid_data_returns_empty() {
        let reducer = RecoveryViewReducer;
        let deltas = reducer.reduce_fact(RECOVERY_FACT_TYPE_ID, b"invalid json data", None);
        assert!(deltas.is_empty());
    }

    #[test]
    fn test_invitation_sent_returns_none() {
        let reducer = RecoveryViewReducer;

        let fact = RecoveryFact::GuardianInvitationSent {
            context_id: test_context_id(),
            guardian_id: test_authority_id(2),
            trace_id: None,
            invitation_hash: test_hash(10),
            sent_at: pt(1234567890),
        };

        let bytes = fact.to_bytes();
        let deltas = reducer.reduce_fact(RECOVERY_FACT_TYPE_ID, &bytes, None);

        // Invitation sent doesn't produce a separate delta
        assert!(deltas.is_empty());
    }

    #[test]
    fn test_reduction_commutes_for_independent_facts() {
        let reducer = RecoveryViewReducer;

        let fact_a = RecoveryFact::GuardianAccepted {
            context_id: test_context_id(),
            guardian_id: test_authority_id(7),
            trace_id: None,
            accepted_at: pt(1111),
        };

        let fact_b = RecoveryFact::RecoveryCompleted {
            context_id: test_context_id(),
            account_id: test_authority_id(1),
            trace_id: None,
            evidence_hash: test_hash(55),
            completed_at: pt(2222),
        };

        let mut deltas_ab = Vec::new();
        deltas_ab.extend(reducer.reduce_fact(RECOVERY_FACT_TYPE_ID, &fact_a.to_bytes(), None));
        deltas_ab.extend(reducer.reduce_fact(RECOVERY_FACT_TYPE_ID, &fact_b.to_bytes(), None));

        let mut deltas_ba = Vec::new();
        deltas_ba.extend(reducer.reduce_fact(RECOVERY_FACT_TYPE_ID, &fact_b.to_bytes(), None));
        deltas_ba.extend(reducer.reduce_fact(RECOVERY_FACT_TYPE_ID, &fact_a.to_bytes(), None));

        let mut keys_ab: Vec<String> = deltas_ab
            .iter()
            .map(|delta| {
                let delta = downcast_delta::<RecoveryDelta>(delta).unwrap();
                format!("{:?}", delta)
            })
            .collect();
        let mut keys_ba: Vec<String> = deltas_ba
            .iter()
            .map(|delta| {
                let delta = downcast_delta::<RecoveryDelta>(delta).unwrap();
                format!("{:?}", delta)
            })
            .collect();

        keys_ab.sort();
        keys_ba.sort();
        assert_eq!(keys_ab, keys_ba);
    }
}
