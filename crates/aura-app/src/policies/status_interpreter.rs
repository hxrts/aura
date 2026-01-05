//! Status interpretation for operation categories.
//!
//! This module provides functions to interpret consistency metadata
//! and construct category-specific status types.
//!
//! # Architecture
//!
//! ```text
//! Journal Layer              App Layer
//! ┌─────────────────┐        ┌─────────────────────────────┐
//! │ Fact            │        │ StatusInterpreter           │
//! │ + Consistency   │────────│   - get_optimistic_status() │
//! │                 │        │   - get_deferred_status()   │
//! └─────────────────┘        │   - get_ceremony_status()   │
//!                            └─────────────────────────────┘
//!                                        │
//!                                        ▼
//!                            ┌─────────────────────────────┐
//!                            │ OptimisticStatus            │
//!                            │ DeferredStatus              │
//!                            │ CeremonyStatus              │
//!                            └─────────────────────────────┘
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! use aura_app::policies::{StatusInterpreter, StatusContext};
//!
//! let interpreter = StatusInterpreter::new();
//!
//! // Get status for a Category A operation (e.g., message)
//! let status = interpreter.get_optimistic_status(&fact, &ctx)?;
//! if status.is_delivered(&expected_peers) {
//!     show_double_checkmark();
//! }
//!
//! // Get status for a Category B operation (e.g., permission change)
//! let status = interpreter.get_deferred_status(&proposal_id, &ctx)?;
//! if status.is_pending() {
//!     show_pending_approval_ui(&status.approvals);
//! }
//! ```

use aura_core::domain::status::{
    CeremonyResponse, CeremonyState, CeremonyStatus, DeferredStatus, OptimisticStatus,
    ParticipantResponse, ProposalState,
};
use aura_core::domain::Consistency;
use aura_core::identifiers::AuthorityId;
use aura_core::time::{OrderTime, PhysicalTime};
use aura_core::{CeremonyId, Hash32};
use aura_journal::Fact;

// =============================================================================
// Status Context Trait
// =============================================================================

/// Operation category discriminant for status interpretation.
///
/// This is a simplified version of `OperationCategory` without associated data,
/// used for dispatching to the correct status type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CategoryKind {
    /// Category A: Optimistic, immediate effect
    Optimistic,
    /// Category B: Deferred until approval
    Deferred,
    /// Category C: Blocked until ceremony completes
    Ceremony,
}

/// Context for status interpretation.
///
/// Provides access to journal and app-level state needed for status queries.
pub trait StatusContext: Send + Sync {
    /// Get the consistency metadata for a fact
    fn get_consistency(&self, fact_id: &OrderTime) -> Option<Consistency>;

    /// Get the category for a fact
    fn get_category(&self, fact: &Fact) -> CategoryKind;

    /// Get proposal details for a deferred operation
    fn get_proposal_details(&self, proposal_id: &str) -> Option<ProposalDetails>;

    /// Get ceremony details for a blocking operation
    fn get_ceremony_details(&self, ceremony_id: &CeremonyId) -> Option<CeremonyDetails>;
}

/// Details for a deferred (Category B) proposal.
#[derive(Debug, Clone)]
pub struct ProposalDetails {
    /// The proposal state
    pub state: ProposalState,
    /// Approvals received
    pub approvers: Vec<(AuthorityId, bool)>, // (approver, approved)
    /// When the proposal expires
    pub expires_at: PhysicalTime,
}

/// Details for a ceremony (Category C) operation.
#[derive(Debug, Clone)]
pub struct CeremonyDetails {
    /// The ceremony state
    pub state: CeremonyState,
    /// Participant responses
    pub responses: Vec<(AuthorityId, CeremonyResponse, PhysicalTime)>,
    /// Prestate hash
    pub prestate_hash: Hash32,
}

// =============================================================================
// Status Interpreter
// =============================================================================

/// Interprets consistency metadata into category-specific status types.
///
/// This provides a unified interface for the app layer to query
/// status information for any operation category.
#[derive(Debug, Clone, Default)]
pub struct StatusInterpreter;

impl StatusInterpreter {
    /// Create a new status interpreter
    pub fn new() -> Self {
        Self
    }

    /// Get the optimistic status for a Category A operation.
    ///
    /// # Arguments
    ///
    /// * `fact` - The fact to get status for
    /// * `ctx` - The status context for lookups
    ///
    /// # Returns
    ///
    /// The optimistic status, or None if the fact doesn't exist or
    /// isn't a Category A operation.
    pub fn get_optimistic_status(
        &self,
        fact: &Fact,
        ctx: &dyn StatusContext,
    ) -> Option<OptimisticStatus> {
        // Verify this is a Category A operation
        let category = ctx.get_category(fact);
        if category != CategoryKind::Optimistic {
            return None;
        }

        // Get consistency metadata
        let consistency = ctx.get_consistency(fact.id())?;

        // Build the optimistic status
        Some(
            OptimisticStatus::new()
                .with_agreement(consistency.agreement.clone())
                .with_propagation(consistency.propagation),
        )
    }

    /// Get the optimistic status directly from consistency metadata.
    ///
    /// Use this when you already have the consistency data.
    pub fn optimistic_status_from_consistency(consistency: &Consistency) -> OptimisticStatus {
        let mut status = OptimisticStatus::new()
            .with_agreement(consistency.agreement.clone())
            .with_propagation(consistency.propagation.clone());

        if let Some(ref ack) = consistency.acknowledgment {
            status = status.with_acknowledgment(ack.clone());
        }

        status
    }

    /// Get the deferred status for a Category B operation.
    ///
    /// # Arguments
    ///
    /// * `proposal_id` - The proposal identifier
    /// * `ctx` - The status context for lookups
    ///
    /// # Returns
    ///
    /// The deferred status, or None if the proposal doesn't exist.
    pub fn get_deferred_status(
        &self,
        proposal_id: &str,
        ctx: &dyn StatusContext,
    ) -> Option<DeferredStatus> {
        let details = ctx.get_proposal_details(proposal_id)?;

        // Build the deferred status
        let mut status = DeferredStatus::new(
            proposal_id,
            aura_core::domain::status::ApprovalThreshold::Any, // Will be updated from details
            details.expires_at.clone(),
        );

        status.state = details.state;

        // Add approvals
        for (approver, approved) in &details.approvers {
            let decision = if *approved {
                aura_core::domain::status::ApprovalDecision::Approve
            } else {
                aura_core::domain::status::ApprovalDecision::Reject
            };
            status.approvals.record(
                *approver,
                decision,
                PhysicalTime {
                    ts_ms: 0,
                    uncertainty: None,
                }, // Timestamp not tracked in simple form
            );
        }

        Some(status)
    }

    /// Get the ceremony status for a Category C operation.
    ///
    /// # Arguments
    ///
    /// * `ceremony_id` - The ceremony identifier
    /// * `ctx` - The status context for lookups
    ///
    /// # Returns
    ///
    /// The ceremony status, or None if the ceremony doesn't exist.
    pub fn get_ceremony_status(
        &self,
        ceremony_id: &CeremonyId,
        ctx: &dyn StatusContext,
    ) -> Option<CeremonyStatus> {
        let details = ctx.get_ceremony_details(ceremony_id)?;

        // Build the ceremony status
        let mut status = CeremonyStatus::new(ceremony_id.clone(), details.prestate_hash);
        status.state = details.state;

        // Add responses
        for (participant, response, responded_at) in &details.responses {
            status.responses.push(ParticipantResponse {
                participant: *participant,
                response: response.clone(),
                responded_at: responded_at.clone(),
            });
        }

        Some(status)
    }

    /// Determine the appropriate status type for a fact based on its category.
    ///
    /// This is a convenience method that dispatches to the appropriate
    /// status getter based on the operation category.
    pub fn get_status_for_fact(
        &self,
        fact: &Fact,
        ctx: &dyn StatusContext,
    ) -> StatusResult {
        let category = ctx.get_category(fact);

        match category {
            CategoryKind::Optimistic => {
                if let Some(status) = self.get_optimistic_status(fact, ctx) {
                    StatusResult::Optimistic(status)
                } else {
                    StatusResult::Unknown
                }
            }
            CategoryKind::Deferred => {
                // For deferred ops, we'd need to extract the proposal ID from the fact
                // This is fact-type specific, so we return Unknown here
                StatusResult::Unknown
            }
            CategoryKind::Ceremony => {
                // For ceremony ops, we'd need to extract the ceremony ID from the fact
                // This is fact-type specific, so we return Unknown here
                StatusResult::Unknown
            }
        }
    }
}

// =============================================================================
// Status Result Enum
// =============================================================================

/// Result of status interpretation, discriminated by category.
#[derive(Debug, Clone)]
pub enum StatusResult {
    /// Category A: Optimistic operation status
    Optimistic(OptimisticStatus),
    /// Category B: Deferred operation status
    Deferred(DeferredStatus),
    /// Category C: Ceremony/blocking operation status
    Ceremony(CeremonyStatus),
    /// Unknown or not found
    Unknown,
}

impl StatusResult {
    /// Check if this is a finalized status (regardless of category)
    pub fn is_finalized(&self) -> bool {
        match self {
            Self::Optimistic(s) => s.is_finalized(),
            Self::Deferred(s) => s.is_finalized(),
            Self::Ceremony(s) => s.is_committed(),
            Self::Unknown => false,
        }
    }

    /// Check if this is in a terminal state
    pub fn is_terminal(&self) -> bool {
        match self {
            Self::Optimistic(s) => s.is_finalized(),
            Self::Deferred(s) => s.state.is_terminal(),
            Self::Ceremony(s) => s.is_terminal(),
            Self::Unknown => true,
        }
    }
}

// =============================================================================
// No-Op Context for Testing
// =============================================================================

/// A no-op status context for testing
#[derive(Debug, Clone, Default)]
pub struct NoOpStatusContext;

impl StatusContext for NoOpStatusContext {
    fn get_consistency(&self, _fact_id: &OrderTime) -> Option<Consistency> {
        None
    }

    fn get_category(&self, _fact: &Fact) -> CategoryKind {
        CategoryKind::Optimistic
    }

    fn get_proposal_details(&self, _proposal_id: &str) -> Option<ProposalDetails> {
        None
    }

    fn get_ceremony_details(&self, _ceremony_id: &CeremonyId) -> Option<CeremonyDetails> {
        None
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::domain::{Agreement, OperationCategory, Propagation};

    #[test]
    fn test_optimistic_status_from_consistency() {
        let consistency = Consistency {
            category: OperationCategory::Optimistic,
            agreement: Agreement::Finalized {
                consensus_id: aura_core::query::ConsensusId([1u8; 32]),
            },
            propagation: Propagation::Complete,
            acknowledgment: None,
        };

        let status = StatusInterpreter::optimistic_status_from_consistency(&consistency);

        assert!(status.is_finalized());
        assert!(status.is_propagated());
    }

    #[test]
    fn test_status_result_is_finalized() {
        let status = OptimisticStatus::new().with_agreement(Agreement::Finalized {
            consensus_id: aura_core::query::ConsensusId([1u8; 32]),
        });

        let result = StatusResult::Optimistic(status);
        assert!(result.is_finalized());
        assert!(result.is_terminal());

        let result = StatusResult::Unknown;
        assert!(!result.is_finalized());
        assert!(result.is_terminal());
    }

    #[test]
    fn test_category_kind() {
        assert_eq!(CategoryKind::Optimistic, CategoryKind::Optimistic);
        assert_ne!(CategoryKind::Optimistic, CategoryKind::Deferred);
        assert_ne!(CategoryKind::Optimistic, CategoryKind::Ceremony);
    }
}
