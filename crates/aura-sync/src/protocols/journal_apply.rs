//! Canonical verified remote journal application boundary.

use crate::core::SyncResult;
use aura_core::{Fact as CoreFact, Journal as CoreJournal};
use aura_guards::VerifiedIngress;
use aura_journal::{Fact, FactJournal as Journal, JournalNamespace};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

#[cfg(test)]
static APPLY_PATH_HITS: AtomicUsize = AtomicUsize::new(0);

#[cfg(test)]
pub(crate) fn apply_path_hits_for_tests() -> usize {
    APPLY_PATH_HITS.load(Ordering::SeqCst)
}

#[cfg(test)]
fn record_apply_path_hit_for_tests() {
    APPLY_PATH_HITS.fetch_add(1, Ordering::SeqCst);
}

#[cfg(not(test))]
fn record_apply_path_hit_for_tests() {}

/// Verified remote facts ready for canonical journal application.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteJournalDelta {
    facts: Vec<Fact>,
}

impl RemoteJournalDelta {
    #[must_use]
    pub fn from_facts(facts: Vec<Fact>) -> Self {
        Self { facts }
    }

    #[must_use]
    pub fn facts(&self) -> &[Fact] {
        &self.facts
    }
}

/// Result of applying one verified remote journal delta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalApplyOutcome {
    pub namespace: JournalNamespace,
    pub facts_applied: usize,
}

/// Single auditable boundary for remote journal/fact application.
#[derive(Debug, Clone, Default)]
pub struct JournalApplyService;

impl JournalApplyService {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Apply verified remote facts to the supplied journal.
    pub fn apply_verified_delta(
        &self,
        mut target: Journal,
        delta: VerifiedIngress<RemoteJournalDelta>,
    ) -> SyncResult<(Journal, JournalApplyOutcome)> {
        record_apply_path_hit_for_tests();
        let (delta, _) = delta.into_parts();
        let facts_applied = delta.facts.len();
        for fact in delta.facts {
            target.add_fact(fact)?;
        }
        let outcome = JournalApplyOutcome {
            namespace: target.namespace.clone(),
            facts_applied,
        };
        Ok((target, outcome))
    }

    /// Apply a verified core journal delta to a core journal.
    pub fn apply_verified_core_delta(
        &self,
        mut target: CoreJournal,
        delta: VerifiedIngress<RemoteCoreJournalDelta>,
    ) -> SyncResult<(CoreJournal, CoreJournalApplyOutcome)> {
        record_apply_path_hit_for_tests();
        let (delta, _) = delta.into_parts();
        let facts_applied = delta.fact_count();
        target.merge_facts(delta.facts);
        Ok((target, CoreJournalApplyOutcome { facts_applied }))
    }

    /// Apply verified relational/domain facts through the same boundary by
    /// returning the verified payload to the runtime committer.
    pub fn accept_verified_relational_facts<T>(
        &self,
        delta: VerifiedIngress<T>,
    ) -> SyncResult<VerifiedIngress<T>> {
        record_apply_path_hit_for_tests();
        Ok(delta)
    }
}

/// Verified remote core journal delta ready for canonical application.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteCoreJournalDelta {
    facts: CoreFact,
}

impl RemoteCoreJournalDelta {
    #[must_use]
    pub fn from_facts(facts: CoreFact) -> Self {
        Self { facts }
    }

    #[must_use]
    pub fn facts(&self) -> &CoreFact {
        &self.facts
    }

    #[must_use]
    pub fn fact_count(&self) -> usize {
        self.facts.iter().count()
    }
}

/// Result of applying one verified remote core journal delta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreJournalApplyOutcome {
    pub facts_applied: usize,
}

#[cfg(test)]
mod tests {
    use super::{
        apply_path_hits_for_tests, JournalApplyService, RemoteCoreJournalDelta, RemoteJournalDelta,
    };
    use aura_core::time::{OrderTime, TimeStamp};
    use aura_core::{hash, AuthorityId, ContextId, FactValue, Hash32, Journal as CoreJournal};
    use aura_guards::{
        DecodedIngress, IngressSource, IngressVerificationEvidence, VerifiedIngress,
        VerifiedIngressMetadata, REQUIRED_INGRESS_VERIFICATION_CHECKS,
    };
    use aura_journal::{Fact, FactContent, FactJournal as Journal, JournalNamespace, SnapshotFact};

    fn verified_delta(facts: Vec<Fact>) -> VerifiedIngress<RemoteJournalDelta> {
        let authority = AuthorityId::new_from_entropy([1; 32]);
        let context = ContextId::new_from_entropy([2; 32]);
        let metadata = VerifiedIngressMetadata::new(
            IngressSource::Authority(authority),
            context,
            None,
            Hash32(hash::hash(b"remote-journal-delta")),
            1,
        );
        let evidence = IngressVerificationEvidence::new(
            metadata.clone(),
            REQUIRED_INGRESS_VERIFICATION_CHECKS,
        )
        .expect("test evidence is complete");
        DecodedIngress::new(RemoteJournalDelta::from_facts(facts), metadata)
            .verify(evidence)
            .expect("test evidence is complete")
    }

    #[test]
    fn apply_boundary_accepts_only_verified_delta_type() {
        let authority = AuthorityId::new_from_entropy([3; 32]);
        let before = apply_path_hits_for_tests();
        let journal = Journal::new(JournalNamespace::Authority(authority));
        let fact = Fact::new(
            OrderTime([4; 32]),
            TimeStamp::OrderClock(OrderTime([4; 32])),
            FactContent::Snapshot(SnapshotFact {
                state_hash: Hash32::default(),
                superseded_facts: vec![],
                sequence: 1,
            }),
        );
        let service = JournalApplyService::new();

        let (journal, outcome) = service
            .apply_verified_delta(journal, verified_delta(vec![fact]))
            .expect("verified delta should apply");

        assert_eq!(outcome.facts_applied, 1);
        assert_eq!(journal.facts.len(), 1);
        assert!(apply_path_hits_for_tests() > before);
    }

    #[test]
    fn apply_boundary_merges_verified_core_delta() {
        let service = JournalApplyService::new();
        let before = apply_path_hits_for_tests();
        let mut facts = aura_core::Fact::new();
        facts
            .insert_with_context(
                "attested_op:test",
                FactValue::Bytes(vec![1, 2, 3]),
                aura_core::ActorId::synthetic("test"),
                aura_core::FactTimestamp::new(1),
                None,
            )
            .expect("fact inserts");

        let authority = AuthorityId::new_from_entropy([9; 32]);
        let context = ContextId::new_from_entropy([8; 32]);
        let metadata = VerifiedIngressMetadata::new(
            IngressSource::Authority(authority),
            context,
            None,
            Hash32(hash::hash(b"remote-core-journal-delta")),
            1,
        );
        let evidence = IngressVerificationEvidence::new(
            metadata.clone(),
            REQUIRED_INGRESS_VERIFICATION_CHECKS,
        )
        .expect("test evidence is complete");
        let verified = DecodedIngress::new(RemoteCoreJournalDelta::from_facts(facts), metadata)
            .verify(evidence)
            .expect("verified core delta");

        let (journal, outcome) = service
            .apply_verified_core_delta(CoreJournal::new(), verified)
            .expect("verified core delta should apply");

        assert_eq!(outcome.facts_applied, 1);
        assert_eq!(journal.read_facts().iter().count(), 1);
        assert!(apply_path_hits_for_tests() > before);
    }
}
