use aura_core::{AuthorityId, ContextId, Hash32};
use aura_guards::{DecodedIngress, IngressSource, VerifiedIngressMetadata};
use aura_journal::{FactJournal, JournalNamespace};
use aura_sync::protocols::journal_apply::{JournalApplyService, RemoteJournalDelta};

fn main() {
    let authority = AuthorityId::new_from_entropy([1; 32]);
    let journal = FactJournal::new(JournalNamespace::Authority(authority));
    let metadata = VerifiedIngressMetadata::new(
        IngressSource::Authority(authority),
        ContextId::new_from_entropy([2; 32]),
        None,
        Hash32::zero(),
        1,
    );
    let decoded = DecodedIngress::new(RemoteJournalDelta::from_facts(Vec::new()), metadata);

    let _ = JournalApplyService::new().apply_verified_delta(journal, decoded);
}
