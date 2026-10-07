//! Order-independent invitation status (docs/105_journal.md,
//! "Order-independent reduction").
//!
//! An invitation's status is a pure function of its fact set:
//!
//! - `Sent` alone is `Pending`. `Sent` never reopens a resolved invitation.
//! - `Accepted`, `Declined` and `Cancelled` are terminal outcomes stamped with
//!   `CausalMetadata`. The first outcome wins: an outcome causally after
//!   another outcome of the same invitation was written against an already
//!   resolved invitation and has no effect.
//! - Concurrent first outcomes resolve by explicit precedence
//!   `Cancelled > Declined > Accepted` (a withdrawal of consent beats a
//!   concurrent consent), then by `causal_cmp` (smallest wins).
//!
//! Ceremony status is a monotone stage lattice and needs no causal stamp:
//! `Committed > Superseded > Aborted > AcceptanceReceived > Initiated`
//! (a consensus-finalized commit is final). Physical times are display only.

use crate::facts::{InvitationFact, INVITATION_FACT_TYPE_ID};
use crate::service::InvitationStatus;
use crate::view::CeremonyViewStatus;
use aura_core::effects::time::{LogicalClockEffects, TimeError};
use aura_core::time::{CausalClock, CausalMetadata, CausalTag, PhysicalTime};
use aura_core::types::identifiers::InvitationId;
use aura_journal::causal_reduction::{causal_cmp, merged_vector, CausalFact};
use aura_journal::DomainFact;
use std::cmp::Ordering;
use std::collections::BTreeMap;

/// Kind of a terminal invitation outcome, in ascending precedence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InvitationOutcomeKind {
    /// The invitee accepted.
    Accepted,
    /// The invitee declined.
    Declined,
    /// The sender cancelled.
    Cancelled,
}

impl InvitationOutcomeKind {
    /// Status this outcome resolves to.
    #[must_use]
    pub fn status(self) -> InvitationStatus {
        match self {
            Self::Accepted => InvitationStatus::Accepted,
            Self::Declined => InvitationStatus::Declined,
            Self::Cancelled => InvitationStatus::Cancelled,
        }
    }
}

/// One terminal outcome fact with its content-derived tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvitationOutcome {
    /// Invitation the outcome resolves.
    pub invitation_id: InvitationId,
    /// Outcome kind.
    pub kind: InvitationOutcomeKind,
    /// Writer's physical time, for display only.
    pub at: PhysicalTime,
    causal: CausalMetadata,
    tag: CausalTag,
}

impl InvitationOutcome {
    /// The outcome carried by `fact`, if it is `Accepted`, `Declined` or
    /// `Cancelled`. The tag hashes the canonical encoding; it is never read
    /// from the wire.
    #[must_use]
    pub fn from_fact(fact: &InvitationFact) -> Option<Self> {
        let (invitation_id, kind, at, causal) = match fact {
            InvitationFact::Accepted {
                invitation_id,
                accepted_at,
                causal,
                ..
            } => (
                invitation_id,
                InvitationOutcomeKind::Accepted,
                accepted_at,
                causal,
            ),
            InvitationFact::Declined {
                invitation_id,
                declined_at,
                causal,
                ..
            } => (
                invitation_id,
                InvitationOutcomeKind::Declined,
                declined_at,
                causal,
            ),
            InvitationFact::Cancelled {
                invitation_id,
                cancelled_at,
                causal,
                ..
            } => (
                invitation_id,
                InvitationOutcomeKind::Cancelled,
                cancelled_at,
                causal,
            ),
            _ => return None,
        };
        Some(Self {
            invitation_id: invitation_id.clone(),
            kind,
            at: at.clone(),
            causal: causal.clone(),
            tag: CausalTag::from_content(INVITATION_FACT_TYPE_ID, &fact.to_envelope().payload),
        })
    }
}

impl CausalFact for InvitationOutcome {
    fn causal_tag(&self) -> CausalTag {
        self.tag
    }

    fn causal_metadata(&self) -> &CausalMetadata {
        &self.causal
    }
}

/// The set of outcome facts observed for one invitation. Inserting is a set
/// union, so the resolved outcome is independent of arrival order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InvitationOutcomes {
    by_tag: BTreeMap<CausalTag, InvitationOutcome>,
}

impl InvitationOutcomes {
    /// Add one outcome (idempotent).
    pub fn insert(&mut self, outcome: InvitationOutcome) {
        self.by_tag.entry(outcome.tag).or_insert(outcome);
    }

    /// Union with another set.
    pub fn merge(&mut self, other: Self) {
        for outcome in other.by_tag.into_values() {
            self.insert(outcome);
        }
    }

    /// The winning outcome: among outcomes no other outcome causally
    /// precedes, the highest precedence, then the smallest by `causal_cmp`.
    #[must_use]
    pub fn resolved(&self) -> Option<&InvitationOutcome> {
        let first = |candidate: &&InvitationOutcome| {
            !self
                .by_tag
                .values()
                .any(|other| other.causal.clock.happens_before(&candidate.causal.clock))
        };
        self.by_tag
            .values()
            .filter(first)
            .min_by(|a, b| b.kind.cmp(&a.kind).then_with(|| causal_cmp(*a, *b)))
    }

    /// Resolved status, if any outcome exists.
    #[must_use]
    pub fn status(&self) -> Option<InvitationStatus> {
        self.resolved().map(|outcome| outcome.kind.status())
    }

    /// Observed outcomes.
    pub fn iter(&self) -> impl Iterator<Item = &InvitationOutcome> {
        self.by_tag.values()
    }
}

/// Outcome sets of every invitation, for views that consume facts in batches.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InvitationLifecycleLog {
    outcomes: BTreeMap<InvitationId, InvitationOutcomes>,
}

impl InvitationLifecycleLog {
    /// Record an outcome fact. Returns the invitation it resolves, or `None`
    /// for facts that carry no outcome.
    pub fn insert(&mut self, fact: &InvitationFact) -> Option<InvitationId> {
        let outcome = InvitationOutcome::from_fact(fact)?;
        let id = outcome.invitation_id.clone();
        self.outcomes.entry(id.clone()).or_default().insert(outcome);
        Some(id)
    }

    /// Resolved status of `invitation_id`; `None` while no outcome exists.
    #[must_use]
    pub fn status(&self, invitation_id: &InvitationId) -> Option<InvitationStatus> {
        self.outcomes.get(invitation_id)?.status()
    }
}

/// Causal metadata for a new invitation outcome written at `clock`. Outcomes are first-wins, so nothing is
/// superseded; the clock alone orders it after what its writer saw.
#[must_use]
pub fn invitation_outcome_causal(clock: &aura_core::time::LogicalTime) -> CausalMetadata {
    CausalMetadata {
        revokes: Vec::new(),
        supersedes: Vec::new(),
        clock: CausalClock::from_logical(clock),
    }
}

/// Stamp a new outcome of `invitation_id`: advance the logical clock past
/// the observed outcomes of that invitation.
///
/// # Errors
/// Returns the logical clock's failure.
pub async fn stamp_invitation_outcome<L: LogicalClockEffects + ?Sized>(
    logical: &L,
    invitation_id: &InvitationId,
    observed: &[InvitationFact],
) -> Result<CausalMetadata, TimeError> {
    let outcomes: Vec<InvitationOutcome> = observed
        .iter()
        .filter_map(InvitationOutcome::from_fact)
        .filter(|outcome| &outcome.invitation_id == invitation_id)
        .collect();
    let vector = merged_vector(outcomes.iter().map(|outcome| &outcome.causal.clock));
    let clock = logical.logical_advance(Some(&vector)).await?;
    Ok(invitation_outcome_causal(&clock))
}

/// Precedence rank of a ceremony stage (higher wins).
#[must_use]
pub fn ceremony_status_rank(status: CeremonyViewStatus) -> u8 {
    match status {
        CeremonyViewStatus::Initiated => 0,
        CeremonyViewStatus::AcceptanceReceived => 1,
        CeremonyViewStatus::Aborted => 2,
        CeremonyViewStatus::Superseded => 3,
        CeremonyViewStatus::Committed => 4,
    }
}

/// Compare two ceremony stages by precedence.
#[must_use]
pub fn ceremony_status_cmp(a: CeremonyViewStatus, b: CeremonyViewStatus) -> Ordering {
    ceremony_status_rank(a).cmp(&ceremony_status_rank(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::time::{LogicalTime, VectorClock};
    use aura_core::types::identifiers::AuthorityId;
    use aura_core::DeviceId;
    use aura_journal::causal_reduction::assert_permutation_invariant;

    fn device(byte: u8) -> DeviceId {
        DeviceId::new_from_entropy([byte; 32])
    }

    fn causal(entries: &[(u8, u64)]) -> CausalMetadata {
        let mut vector = VectorClock::new();
        for (byte, counter) in entries {
            vector.insert(device(*byte), *counter);
        }
        let lamport = entries.iter().map(|(_, c)| *c).max().unwrap_or(0);
        invitation_outcome_causal(&LogicalTime { vector, lamport })
    }

    fn author(byte: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([byte; 32])
    }

    fn id() -> InvitationId {
        InvitationId::new("inv-permutation")
    }

    fn accepted(clock: &[(u8, u64)], at: u64) -> InvitationFact {
        InvitationFact::accepted_ms(id(), author(2), at, causal(clock))
    }

    fn declined(clock: &[(u8, u64)], at: u64) -> InvitationFact {
        InvitationFact::declined_ms(id(), author(2), at, causal(clock))
    }

    fn cancelled(clock: &[(u8, u64)], at: u64) -> InvitationFact {
        InvitationFact::cancelled_ms(id(), author(1), at, causal(clock))
    }

    fn sent() -> InvitationFact {
        InvitationFact::sent_ms(
            aura_core::types::identifiers::ContextId::new_from_entropy([3; 32]),
            id(),
            author(1),
            author(2),
            crate::InvitationType::Contact { nickname: None },
            1,
            None,
            None,
        )
    }

    fn reduce(facts: &[InvitationFact]) -> Option<InvitationStatus> {
        let mut log = InvitationLifecycleLog::default();
        for fact in facts {
            log.insert(fact);
        }
        log.status(&id())
    }

    #[test]
    fn pending_until_an_outcome_exists() {
        assert_eq!(assert_permutation_invariant(&[sent()], reduce), None);
    }

    #[test]
    fn first_outcome_wins_over_causally_later_outcomes() {
        // Accepted, then (having observed it) a cancel with a larger physical
        // time: the later cancel cannot reopen or replace the acceptance.
        let facts = [
            sent(),
            accepted(&[(2, 1)], 900),
            cancelled(&[(1, 1), (2, 1)], 100),
        ];
        assert_eq!(
            assert_permutation_invariant(&facts, reduce),
            Some(InvitationStatus::Accepted)
        );
    }

    #[test]
    fn concurrent_outcomes_resolve_by_precedence_not_physical_time() {
        let facts = [
            sent(),
            accepted(&[(2, 1)], 1),
            declined(&[(3, 1)], 2),
            cancelled(&[(1, 1)], 3),
        ];
        assert_eq!(
            assert_permutation_invariant(&facts, reduce),
            Some(InvitationStatus::Cancelled)
        );
        let facts = [accepted(&[(2, 1)], 9), declined(&[(3, 1)], 1)];
        assert_eq!(
            assert_permutation_invariant(&facts, reduce),
            Some(InvitationStatus::Declined)
        );
    }

    #[test]
    fn concurrent_with_first_but_after_another_still_resolves_deterministically() {
        // a -> c causally, b concurrent with both: first outcomes are {a, b}.
        let facts = [
            accepted(&[(2, 1)], 1),
            declined(&[(3, 1)], 1),
            cancelled(&[(1, 1), (2, 1)], 1),
        ];
        assert_eq!(
            assert_permutation_invariant(&facts, reduce),
            Some(InvitationStatus::Declined)
        );
    }

    #[test]
    fn same_kind_concurrent_outcomes_and_duplicates_are_order_independent() {
        let facts = [
            accepted(&[(2, 1)], 5),
            accepted(&[(3, 1)], 4),
            accepted(&[(2, 1)], 5),
        ];
        let winner = assert_permutation_invariant(&facts, |order| {
            let mut outcomes = InvitationOutcomes::default();
            for fact in order {
                outcomes.insert(InvitationOutcome::from_fact(fact).unwrap());
            }
            (outcomes.iter().count(), outcomes.resolved().cloned())
        });
        assert_eq!(winner.0, 2);
    }

    #[test]
    fn ceremony_precedence_is_a_total_stage_order() {
        use CeremonyViewStatus::*;
        let order = [
            Initiated,
            AcceptanceReceived,
            Aborted,
            Superseded,
            Committed,
        ];
        for pair in order.windows(2) {
            assert_eq!(ceremony_status_cmp(pair[0], pair[1]), Ordering::Less);
        }
    }
}
