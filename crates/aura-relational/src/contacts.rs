//! Order-independent contact reduction (docs/105_journal.md §4.2.1).
//!
//! Contact existence per `(owner, contact)` pair is a tagged observed-remove
//! set: `ContactFact::Added` is an add, `ContactFact::Removed` revokes exactly
//! the add tags its writer observed, so a concurrent re-add survives a remove.
//! The user nickname (`Renamed`) and the read receipt policy
//! (`ReadReceiptPolicyUpdated`) are multi-value registers. A `Removed` is also
//! a clearing write in both registers, superseding the writes its writer
//! observed. Concurrent register survivors resolve deterministically: the
//! nickname takes the latest surviving rename in causal order (a value wins
//! over a concurrent clear); the policy is `Enabled` only when every surviving
//! write enables it (privacy-first, most restrictive).
//!
//! Physical timestamps remain for display (`last_interaction_ms`) only.

use crate::facts::{ContactFact, ReadReceiptPolicy, CONTACT_FACT_TYPE_ID};
use aura_core::time::{CausalClock, CausalMetadata, CausalTag, LogicalTime, VectorClock};
use aura_core::types::identifiers::AuthorityId;
use aura_journal::causal_reduction::{
    causal_cmp, merged_vector, observed_remove_live, register_survivors, CausalFact,
};
use aura_journal::DomainFact;
use std::collections::{BTreeMap, BTreeSet};

/// `(owner, contact)`: the key of one contact relationship.
pub type ContactPair = (AuthorityId, AuthorityId);

/// A contact fact with its content-derived tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaggedContactFact {
    tag: CausalTag,
    fact: ContactFact,
}

impl TaggedContactFact {
    /// Wrap a decoded fact. The tag is the hash of the type id and canonical
    /// encoding, which names the author and the writer's clock; it is never
    /// read from the wire.
    #[must_use]
    pub fn new(fact: ContactFact) -> Self {
        let envelope = fact.to_envelope();
        Self {
            tag: CausalTag::from_content(CONTACT_FACT_TYPE_ID, &envelope.payload),
            fact,
        }
    }

    /// The fact's tag.
    #[must_use]
    pub fn tag(&self) -> CausalTag {
        self.tag
    }

    /// The decoded fact.
    #[must_use]
    pub fn fact(&self) -> &ContactFact {
        &self.fact
    }

    /// The relationship the fact is about.
    #[must_use]
    pub fn pair(&self) -> ContactPair {
        (self.fact.owner_id(), self.fact.contact_id())
    }
}

impl CausalFact for TaggedContactFact {
    fn causal_tag(&self) -> CausalTag {
        self.tag
    }

    fn causal_metadata(&self) -> &CausalMetadata {
        self.fact.causal()
    }
}

/// What a new contact fact is about; determines what it revokes or supersedes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContactCausalKey {
    /// `ContactFact::Added`.
    Add {
        /// Contact list owner.
        owner: AuthorityId,
        /// Contact.
        contact: AuthorityId,
    },
    /// `ContactFact::Removed`.
    Remove {
        /// Contact list owner.
        owner: AuthorityId,
        /// Contact.
        contact: AuthorityId,
    },
    /// `ContactFact::Renamed`.
    Rename {
        /// Contact list owner.
        owner: AuthorityId,
        /// Contact.
        contact: AuthorityId,
    },
    /// `ContactFact::ReadReceiptPolicyUpdated`.
    ReadReceiptPolicy {
        /// Contact list owner.
        owner: AuthorityId,
        /// Contact.
        contact: AuthorityId,
    },
}

impl ContactCausalKey {
    /// The relationship the new fact is about.
    #[must_use]
    pub fn pair(&self) -> ContactPair {
        match *self {
            Self::Add { owner, contact }
            | Self::Remove { owner, contact }
            | Self::Rename { owner, contact }
            | Self::ReadReceiptPolicy { owner, contact } => (owner, contact),
        }
    }
}

fn is_nickname_write(fact: &ContactFact) -> bool {
    matches!(
        fact,
        ContactFact::Renamed { .. } | ContactFact::Removed { .. }
    )
}

fn is_policy_write(fact: &ContactFact) -> bool {
    matches!(
        fact,
        ContactFact::ReadReceiptPolicyUpdated { .. } | ContactFact::Removed { .. }
    )
}

/// Merged vector clock of the observed contact facts: the `observed`
/// argument for the writer's `LogicalClockEffects::logical_advance`.
#[must_use]
pub fn observed_contact_vector(observed: &[TaggedContactFact]) -> VectorClock {
    merged_vector(observed.iter().map(|fact| &fact.fact.causal().clock))
}

/// Causal metadata for a new contact fact about `key`, written by a writer
/// holding `observed` at its freshly advanced logical `clock`. A removal
/// revokes every observed add of the pair and supersedes every observed
/// nickname and policy write; a rename or policy write supersedes the
/// observed writes of its register.
#[must_use]
pub fn contact_causal(
    key: ContactCausalKey,
    observed: &[TaggedContactFact],
    clock: &LogicalTime,
) -> CausalMetadata {
    let pair = key.pair();
    let tags_where = |matches: fn(&ContactFact) -> bool| -> Vec<CausalTag> {
        observed
            .iter()
            .filter(|fact| fact.pair() == pair && matches(&fact.fact))
            .map(|fact| fact.tag)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    };
    let (revokes, supersedes) = match key {
        ContactCausalKey::Add { .. } => (Vec::new(), Vec::new()),
        ContactCausalKey::Remove { .. } => (
            tags_where(|fact| matches!(fact, ContactFact::Added { .. })),
            tags_where(|fact| is_nickname_write(fact) || is_policy_write(fact)),
        ),
        ContactCausalKey::Rename { .. } => (Vec::new(), tags_where(is_nickname_write)),
        ContactCausalKey::ReadReceiptPolicy { .. } => (Vec::new(), tags_where(is_policy_write)),
    };
    CausalMetadata {
        revokes,
        supersedes,
        clock: CausalClock::from_logical(clock),
    }
}

/// Reduced state of one live contact relationship.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactRecord {
    /// Contact list owner.
    pub owner_id: AuthorityId,
    /// Contact.
    pub contact_id: AuthorityId,
    /// The causally latest live `ContactFact::Added`: canonical creation evidence.
    pub latest_add: ContactFact,
    /// Latest human-readable nickname carried by a live add.
    pub nickname_suggestion: Option<String>,
    /// Resolved user nickname (`None` when cleared or never set).
    pub nickname: Option<String>,
    /// Latest invitation code carried by a live add.
    pub invitation_code: Option<String>,
    /// Resolved read receipt policy.
    pub read_receipt_policy: ReadReceiptPolicy,
    /// Latest physical timestamp among live adds and surviving renames (display only).
    pub last_interaction_ms: u64,
}

fn human_nickname(fact: &ContactFact) -> Option<String> {
    match fact {
        ContactFact::Added {
            contact_id,
            nickname,
            ..
        } if !nickname.trim().is_empty() && *nickname != contact_id.to_string() => {
            Some(nickname.clone())
        }
        _ => None,
    }
}

fn surviving<'a>(
    facts: &[&'a TaggedContactFact],
    is_write: fn(&ContactFact) -> bool,
) -> Vec<&'a TaggedContactFact> {
    let writes: Vec<&TaggedContactFact> = facts
        .iter()
        .copied()
        .filter(|fact| is_write(&fact.fact))
        .collect();
    let survivors = register_survivors(&writes);
    let mut kept: Vec<_> = writes
        .into_iter()
        .filter(|write| survivors.contains(&write.tag))
        .collect();
    kept.sort_by(|a, b| causal_cmp(*a, *b));
    kept
}

fn reduce_pair(pair: ContactPair, facts: &[&TaggedContactFact]) -> Option<ContactRecord> {
    let adds: Vec<_> = facts
        .iter()
        .filter(|fact| matches!(fact.fact, ContactFact::Added { .. }))
        .map(|fact| (pair, fact.tag))
        .collect();
    let reversals: Vec<_> = facts
        .iter()
        .filter(|fact| matches!(fact.fact, ContactFact::Removed { .. }))
        .map(|fact| (pair, fact.fact.causal().revokes.as_slice()))
        .collect();
    let live = observed_remove_live(&adds, &reversals);
    let mut live_adds: Vec<&TaggedContactFact> = facts
        .iter()
        .copied()
        .filter(|fact| live.contains(&fact.tag))
        .collect();
    live_adds.sort_by(|a, b| causal_cmp(*a, *b));
    let latest_add = live_adds.last()?;

    let renames = surviving(facts, is_nickname_write);
    let nickname = renames.iter().rev().find_map(|write| match &write.fact {
        ContactFact::Renamed { new_nickname, .. } => Some(new_nickname.clone()),
        _ => None,
    });
    let policies = surviving(facts, is_policy_write);
    let all_enabled = policies.iter().all(|write| {
        matches!(
            write.fact,
            ContactFact::ReadReceiptPolicyUpdated {
                policy: ReadReceiptPolicy::Enabled,
                ..
            }
        )
    });
    let read_receipt_policy = if !policies.is_empty() && all_enabled {
        ReadReceiptPolicy::Enabled
    } else {
        ReadReceiptPolicy::Disabled
    };
    let last_interaction_ms = live_adds
        .iter()
        .chain(renames.iter())
        .filter_map(|fact| match &fact.fact {
            ContactFact::Added { added_at, .. } => Some(added_at.ts_ms),
            ContactFact::Renamed { renamed_at, .. } => Some(renamed_at.ts_ms),
            _ => None,
        })
        .max()
        .unwrap_or(0);

    Some(ContactRecord {
        owner_id: pair.0,
        contact_id: pair.1,
        latest_add: latest_add.fact.clone(),
        nickname_suggestion: live_adds
            .iter()
            .rev()
            .find_map(|add| human_nickname(&add.fact)),
        nickname: nickname.filter(|name| !name.is_empty()),
        invitation_code: live_adds.iter().rev().find_map(|add| match &add.fact {
            ContactFact::Added {
                invitation_code, ..
            } => invitation_code.clone(),
            _ => None,
        }),
        read_receipt_policy,
        last_interaction_ms,
    })
}

/// Live contact relationships of a contact fact set. A deterministic function
/// of the set: independent of arrival order and duplicates.
#[must_use]
pub fn reduce_contacts<'a>(
    facts: impl IntoIterator<Item = &'a TaggedContactFact>,
) -> BTreeMap<ContactPair, ContactRecord> {
    let mut by_pair: BTreeMap<ContactPair, BTreeMap<CausalTag, &TaggedContactFact>> =
        BTreeMap::new();
    for fact in facts {
        by_pair
            .entry(fact.pair())
            .or_default()
            .insert(fact.tag, fact);
    }
    by_pair
        .into_iter()
        .filter_map(|(pair, facts)| {
            let facts: Vec<_> = facts.into_values().collect();
            reduce_pair(pair, &facts).map(|record| (pair, record))
        })
        .collect()
}

/// Contact fact set with its live relationships, for O(log n) existence
/// checks after a replay.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContactExistenceIndex {
    facts: BTreeMap<ContactPair, BTreeMap<CausalTag, TaggedContactFact>>,
    live: BTreeSet<ContactPair>,
}

impl ContactExistenceIndex {
    /// Create an empty index.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a fact to the set and re-reduce its relationship.
    pub fn apply_fact(&mut self, fact: &ContactFact) {
        let tagged = TaggedContactFact::new(fact.clone());
        let pair = tagged.pair();
        let facts = self.facts.entry(pair).or_default();
        facts.insert(tagged.tag, tagged);
        if reduce_contacts(facts.values()).contains_key(&pair) {
            self.live.insert(pair);
        } else {
            self.live.remove(&pair);
        }
    }

    /// Whether `owner_id` currently has `contact_id` as a live contact.
    #[must_use]
    pub fn contains(&self, owner_id: AuthorityId, contact_id: AuthorityId) -> bool {
        self.live.contains(&(owner_id, contact_id))
    }
}

/// Deterministic stamping for tests and mocks.
pub mod test_support {
    use super::*;
    use aura_core::types::identifiers::DeviceId;

    /// The clock `device` reaches after observing `observed`.
    #[must_use]
    pub fn advance<F: CausalFact>(device: u8, observed: &[F]) -> LogicalTime {
        let mut vector = merged_vector(observed.iter().map(|fact| &fact.causal_metadata().clock));
        let id = DeviceId(uuid::Uuid::from_bytes([device; 16]));
        let next = vector.get(&id).copied().unwrap_or(0) + 1;
        vector.insert(id, next);
        LogicalTime {
            vector,
            lamport: next,
        }
    }

    /// Causal metadata for `key` written by `device` after observing the
    /// untagged `observed` facts.
    #[must_use]
    pub fn causal_after(
        device: u8,
        key: ContactCausalKey,
        observed: &[&ContactFact],
    ) -> CausalMetadata {
        let observed: Vec<_> = observed
            .iter()
            .map(|fact| TaggedContactFact::new((*fact).clone()))
            .collect();
        causal(device, key, &observed)
    }

    /// Metadata of a fact written by `device` having observed no contact
    /// fact: a first add.
    #[must_use]
    pub fn fresh(device: u8) -> CausalMetadata {
        CausalMetadata {
            revokes: Vec::new(),
            supersedes: Vec::new(),
            clock: CausalClock::from_logical(&advance::<TaggedContactFact>(device, &[])),
        }
    }

    /// Causal metadata for `key` written by `device` after observing `observed`.
    #[must_use]
    pub fn causal(
        device: u8,
        key: ContactCausalKey,
        observed: &[TaggedContactFact],
    ) -> CausalMetadata {
        contact_causal(key, observed, &advance(device, observed))
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::causal;
    use super::*;
    use aura_core::types::identifiers::ContextId;
    use aura_journal::causal_reduction::assert_permutation_invariant;

    fn ctx() -> ContextId {
        ContextId::new_from_entropy([7; 32])
    }

    fn who(byte: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([byte; 32])
    }

    fn owner() -> AuthorityId {
        who(1)
    }

    fn peer() -> AuthorityId {
        who(2)
    }

    fn add(device: u8, name: &str, at: u64, observed: &[TaggedContactFact]) -> TaggedContactFact {
        let key = ContactCausalKey::Add {
            owner: owner(),
            contact: peer(),
        };
        TaggedContactFact::new(ContactFact::added_ms(
            ctx(),
            owner(),
            peer(),
            name.to_string(),
            at,
            causal(device, key, observed),
        ))
    }

    fn remove(device: u8, at: u64, observed: &[TaggedContactFact]) -> TaggedContactFact {
        let key = ContactCausalKey::Remove {
            owner: owner(),
            contact: peer(),
        };
        TaggedContactFact::new(ContactFact::removed_ms(
            ctx(),
            owner(),
            peer(),
            at,
            causal(device, key, observed),
        ))
    }

    fn rename(
        device: u8,
        name: &str,
        at: u64,
        observed: &[TaggedContactFact],
    ) -> TaggedContactFact {
        let key = ContactCausalKey::Rename {
            owner: owner(),
            contact: peer(),
        };
        TaggedContactFact::new(ContactFact::renamed_ms(
            ctx(),
            owner(),
            peer(),
            name.to_string(),
            at,
            causal(device, key, observed),
        ))
    }

    fn policy(
        device: u8,
        value: ReadReceiptPolicy,
        observed: &[TaggedContactFact],
    ) -> TaggedContactFact {
        let key = ContactCausalKey::ReadReceiptPolicy {
            owner: owner(),
            contact: peer(),
        };
        TaggedContactFact::new(ContactFact::read_receipt_policy_updated_ms(
            ctx(),
            owner(),
            peer(),
            value,
            1,
            causal(device, key, observed),
        ))
    }

    fn reduce(facts: &[TaggedContactFact]) -> BTreeMap<ContactPair, ContactRecord> {
        reduce_contacts(facts)
    }

    fn record(facts: &[TaggedContactFact]) -> Option<ContactRecord> {
        assert_permutation_invariant(facts, reduce).remove(&(owner(), peer()))
    }

    #[test]
    fn remove_before_add_arrival_still_removes() {
        let a1 = add(1, "Alice", 1, &[]);
        let r = remove(1, 2, std::slice::from_ref(&a1));
        assert!(record(&[r.clone(), a1.clone()]).is_none());
        let mut index = ContactExistenceIndex::new();
        index.apply_fact(r.fact());
        assert!(!index.contains(owner(), peer()));
        index.apply_fact(a1.fact());
        assert!(!index.contains(owner(), peer()));
    }

    #[test]
    fn add_remove_re_add_and_unobserved_concurrent_add_survive() {
        let a1 = add(1, "Alice", 1, &[]);
        let r = remove(1, 2, std::slice::from_ref(&a1));
        let a2 = add(1, "Alicia", 3, &[a1.clone(), r.clone()]);
        let live = record(&[a1.clone(), r.clone(), a2.clone()]).expect("re-add is live");
        assert_eq!(live.latest_add, a2.fact().clone());
        assert_eq!(live.nickname_suggestion.as_deref(), Some("Alicia"));

        // A second device re-adds concurrently with the remove: the remove
        // did not observe it, so it survives.
        let concurrent = add(2, "Al", 2, &[]);
        let live = record(&[a1, r, concurrent.clone()]).expect("concurrent add survives");
        assert_eq!(live.latest_add, concurrent.fact().clone());
    }

    #[test]
    fn rename_races_resolve_deterministically() {
        let a = add(1, "Alice", 1, &[]);
        let left = rename(1, "Left", 2, std::slice::from_ref(&a));
        let right = rename(2, "Right", 2, std::slice::from_ref(&a));
        let raced = record(&[a.clone(), left.clone(), right.clone()]).expect("live");
        let mut both = [left.clone(), right.clone()];
        both.sort_by(causal_cmp);
        let expected = match both[1].fact() {
            ContactFact::Renamed { new_nickname, .. } => new_nickname.clone(),
            _ => unreachable!(),
        };
        assert_eq!(raced.nickname, Some(expected));

        let merged = rename(1, "Merged", 3, &[a.clone(), left.clone(), right.clone()]);
        let resolved = record(&[a, left, right, merged]).expect("live");
        assert_eq!(resolved.nickname.as_deref(), Some("Merged"));
    }

    #[test]
    fn remove_clears_registers_but_concurrent_rename_survives() {
        let a = add(1, "Alice", 1, &[]);
        let named = rename(1, "Ally", 2, std::slice::from_ref(&a));
        let enabled = policy(1, ReadReceiptPolicy::Enabled, &[a.clone(), named.clone()]);
        let r = remove(1, 4, &[a.clone(), named.clone(), enabled.clone()]);
        let a2 = add(
            1,
            "Alice",
            5,
            &[a.clone(), named.clone(), enabled.clone(), r.clone()],
        );
        let fresh = record(&[
            a.clone(),
            named.clone(),
            enabled.clone(),
            r.clone(),
            a2.clone(),
        ])
        .expect("re-added");
        assert_eq!(fresh.nickname, None);
        assert_eq!(fresh.read_receipt_policy, ReadReceiptPolicy::Disabled);

        let concurrent = rename(2, "Kept", 3, std::slice::from_ref(&a));
        let kept = record(&[a, r, a2, concurrent]).expect("re-added");
        assert_eq!(kept.nickname.as_deref(), Some("Kept"));
    }

    #[test]
    fn concurrent_policy_writes_resolve_most_restrictive() {
        let a = add(1, "Alice", 1, &[]);
        let on = policy(1, ReadReceiptPolicy::Enabled, std::slice::from_ref(&a));
        let off = policy(2, ReadReceiptPolicy::Disabled, std::slice::from_ref(&a));
        let raced = record(&[a.clone(), on.clone(), off.clone()]).expect("live");
        assert_eq!(raced.read_receipt_policy, ReadReceiptPolicy::Disabled);

        let later = policy(
            2,
            ReadReceiptPolicy::Enabled,
            &[a.clone(), on.clone(), off.clone()],
        );
        let resolved = record(&[a, on, off, later]).expect("live");
        assert_eq!(resolved.read_receipt_policy, ReadReceiptPolicy::Enabled);
    }

    #[test]
    fn duplicate_delivery_does_not_change_state() {
        let a = add(1, "Alice", 1, &[]);
        let r = remove(1, 2, std::slice::from_ref(&a));
        assert_eq!(
            reduce(&[a.clone(), r.clone(), r.clone(), a.clone()]),
            reduce(&[a, r])
        );
    }
}
