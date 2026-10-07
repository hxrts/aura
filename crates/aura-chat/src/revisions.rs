//! Order-independent message edit and delete reduction.
//!
//! Each message has a multi-value register of revisions (docs/105_journal.md
//! §4.2.1). Rules, independent of arrival order:
//!
//! - An edit supersedes the edits its writer observed
//!   (`CausalMetadata::supersedes`) and every edit causally before it
//!   ([`register_survivors`]).
//! - Concurrent surviving edits resolve deterministically by [`causal_cmp`]:
//!   causal order, then the explicit policy "the edit that observed more
//!   history wins" (greater causal depth, then Lamport), then content tag.
//! - A delete is a tombstone: once any delete of a message is known, the
//!   message is deleted whatever edits are concurrent with or causally after
//!   it, and a send arriving after its delete stays hidden.
//!
//! Physical time (`edited_at`, `deleted_at`) is display only.

use crate::{ChatFact, CHAT_FACT_TYPE_ID};
use aura_core::hash::hash;
use aura_core::time::{CausalClock, CausalMetadata, CausalTag, LogicalTime, PhysicalTime};
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_journal::causal_reduction::{causal_cmp, register_survivors, CausalFact};
use aura_journal::DomainFact;
use std::collections::BTreeMap;

/// The message a revision applies to. The message id is content-addressed so
/// the key stays `Copy` for `CausalStampKey`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MessageRevisionKey {
    /// Relational context of the message.
    pub context_id: ContextId,
    /// Channel containing the message.
    pub channel_id: ChannelId,
    /// Hash of the message id.
    pub message: [u8; 32],
}

impl MessageRevisionKey {
    /// Key of `message_id` in `channel_id` of `context_id`.
    #[must_use]
    pub fn new(context_id: ContextId, channel_id: ChannelId, message_id: &str) -> Self {
        Self {
            context_id,
            channel_id,
            message: hash(message_id.as_bytes()),
        }
    }
}

/// What a revision does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageRevisionKind {
    /// Replace the content.
    Edit {
        /// New content (opaque bytes, typically UTF-8).
        new_payload: Vec<u8>,
    },
    /// Tombstone the message.
    Delete,
}

/// One edit or delete of a message, tagged by its canonical content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageRevision {
    key: MessageRevisionKey,
    tag: CausalTag,
    causal: CausalMetadata,
    /// Editor or deleter.
    pub actor_id: AuthorityId,
    /// Physical time of the revision (display only).
    pub at: PhysicalTime,
    /// Edit or delete.
    pub kind: MessageRevisionKind,
}

impl MessageRevision {
    /// The revision carried by an edit or delete fact.
    #[must_use]
    pub fn from_fact(fact: &ChatFact) -> Option<Self> {
        let (key, actor_id, at, causal, kind) = match fact {
            ChatFact::MessageEdited {
                context_id,
                channel_id,
                message_id,
                editor_id,
                new_payload,
                edited_at,
                causal,
            } => (
                MessageRevisionKey::new(*context_id, *channel_id, message_id),
                *editor_id,
                edited_at,
                causal,
                MessageRevisionKind::Edit {
                    new_payload: new_payload.clone(),
                },
            ),
            ChatFact::MessageDeleted {
                context_id,
                channel_id,
                message_id,
                deleter_id,
                deleted_at,
                causal,
            } => (
                MessageRevisionKey::new(*context_id, *channel_id, message_id),
                *deleter_id,
                deleted_at,
                causal,
                MessageRevisionKind::Delete,
            ),
            _ => return None,
        };
        Some(Self {
            key,
            tag: CausalTag::from_content(CHAT_FACT_TYPE_ID, &fact.to_envelope().payload),
            causal: causal.clone(),
            actor_id,
            at: at.clone(),
            kind,
        })
    }

    /// The revised message.
    #[must_use]
    pub fn key(&self) -> MessageRevisionKey {
        self.key
    }

    fn is_edit(&self) -> bool {
        matches!(self.kind, MessageRevisionKind::Edit { .. })
    }
}

impl CausalFact for MessageRevision {
    fn causal_tag(&self) -> CausalTag {
        self.tag
    }
    fn causal_metadata(&self) -> &CausalMetadata {
        &self.causal
    }
}

/// Causal metadata for a new edit or delete of `key`, written by a writer
/// holding `observed` at its freshly advanced logical `clock`: it supersedes
/// every observed edit of the message.
#[must_use]
pub fn message_revision_causal(
    key: MessageRevisionKey,
    observed: &[MessageRevision],
    clock: &LogicalTime,
) -> CausalMetadata {
    let mut supersedes: Vec<_> = observed
        .iter()
        .filter(|revision| revision.key == key && revision.is_edit())
        .map(|revision| revision.tag)
        .collect();
    supersedes.sort();
    supersedes.dedup();
    CausalMetadata {
        revokes: Vec::new(),
        supersedes,
        clock: CausalClock::from_logical(clock),
    }
}

/// Resolved revision state of one message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRevisionOutcome<'a> {
    /// No edit or delete known.
    Unrevised,
    /// The winning edit.
    Edited(&'a MessageRevision),
    /// Tombstoned.
    Deleted,
}

/// Every known revision of one message.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MessageRevisionRegister {
    revisions: BTreeMap<CausalTag, MessageRevision>,
}

impl MessageRevisionRegister {
    /// Record `revision`; returns whether it was new.
    pub fn insert(&mut self, revision: MessageRevision) -> bool {
        self.revisions.insert(revision.tag, revision).is_none()
    }

    /// The resolved state: delete wins, else the winning surviving edit.
    #[must_use]
    pub fn resolve(&self) -> MessageRevisionOutcome<'_> {
        if self.revisions.values().any(|revision| !revision.is_edit()) {
            return MessageRevisionOutcome::Deleted;
        }
        let edits: Vec<_> = self.revisions.values().collect();
        let survivors = register_survivors(&edits);
        edits
            .into_iter()
            .filter(|edit| survivors.contains(&edit.tag))
            .max_by(|a, b| causal_cmp(*a, *b))
            .map_or(
                MessageRevisionOutcome::Unrevised,
                MessageRevisionOutcome::Edited,
            )
    }
}

/// Revision registers of many messages, keyed by message.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MessageRevisions {
    registers: BTreeMap<MessageRevisionKey, MessageRevisionRegister>,
}

impl MessageRevisions {
    /// Record `revision`; returns whether it was new.
    pub fn insert(&mut self, revision: MessageRevision) -> bool {
        self.registers
            .entry(revision.key)
            .or_default()
            .insert(revision)
    }

    /// Resolved state of `key`.
    #[must_use]
    pub fn resolve(&self, key: &MessageRevisionKey) -> MessageRevisionOutcome<'_> {
        self.registers.get(key).map_or(
            MessageRevisionOutcome::Unrevised,
            MessageRevisionRegister::resolve,
        )
    }

    /// Forget every revision of messages in `channel_id`.
    pub fn remove_channel(&mut self, channel_id: &ChannelId) {
        self.registers
            .retain(|key, _| key.channel_id != *channel_id);
    }

    /// Forget every revision.
    pub fn clear(&mut self) {
        self.registers.clear();
    }
}

/// Test helpers: deterministic writer clocks.
pub mod test_support {
    use super::*;
    use aura_core::types::identifiers::DeviceId;
    use aura_journal::causal_reduction::merged_vector;

    /// Causal metadata for a revision of `key` written by `device` after
    /// observing `observed`.
    #[must_use]
    pub fn causal_after(
        device: u8,
        key: MessageRevisionKey,
        observed: &[MessageRevision],
    ) -> CausalMetadata {
        let mut vector = merged_vector(observed.iter().map(|r| &r.causal_metadata().clock));
        let id = DeviceId(uuid::Uuid::from_bytes([device; 16]));
        let next = vector.get(&id).copied().unwrap_or(0) + 1;
        vector.insert(id, next);
        message_revision_causal(
            key,
            observed,
            &LogicalTime {
                vector,
                lamport: next,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::causal_after;
    use super::*;
    use crate::test_support::{test_authority_id, test_channel_id, test_context_id};
    use aura_journal::causal_reduction::assert_permutation_invariant;

    const MSG: &str = "m1";

    fn key() -> MessageRevisionKey {
        MessageRevisionKey::new(test_context_id(1), test_channel_id(1), MSG)
    }

    fn edit(device: u8, body: &str, observed: &[&ChatFact]) -> ChatFact {
        let observed: Vec<_> = observed
            .iter()
            .filter_map(|f| MessageRevision::from_fact(f))
            .collect();
        ChatFact::message_edited_ms(
            test_context_id(1),
            test_channel_id(1),
            MSG.to_string(),
            test_authority_id(device),
            body.as_bytes().to_vec(),
            0,
            causal_after(device, key(), &observed),
        )
    }

    fn delete(device: u8, observed: &[&ChatFact]) -> ChatFact {
        let observed: Vec<_> = observed
            .iter()
            .filter_map(|f| MessageRevision::from_fact(f))
            .collect();
        ChatFact::message_deleted_ms(
            test_context_id(1),
            test_channel_id(1),
            MSG.to_string(),
            test_authority_id(device),
            0,
            causal_after(device, key(), &observed),
        )
    }

    /// Resolved content, `None` when deleted.
    fn reduce(facts: &[ChatFact]) -> Option<Option<Vec<u8>>> {
        let mut revisions = MessageRevisions::default();
        for fact in facts {
            if let Some(revision) = MessageRevision::from_fact(fact) {
                revisions.insert(revision);
            }
        }
        match revisions.resolve(&key()) {
            MessageRevisionOutcome::Deleted => None,
            MessageRevisionOutcome::Unrevised => Some(None),
            MessageRevisionOutcome::Edited(revision) => match &revision.kind {
                MessageRevisionKind::Edit { new_payload } => Some(Some(new_payload.clone())),
                MessageRevisionKind::Delete => unreachable!("edits only"),
            },
        }
    }

    #[test]
    fn later_edit_supersedes_observed_edit() {
        let first = edit(1, "a", &[]);
        let second = edit(2, "b", &[&first]);
        let state = assert_permutation_invariant(&[first, second], reduce);
        assert_eq!(state, Some(Some(b"b".to_vec())));
    }

    #[test]
    fn concurrent_edits_resolve_deterministically() {
        let base = edit(1, "base", &[]);
        let left = edit(2, "left", &[&base]);
        let right = edit(3, "right", &[&base]);
        let state = assert_permutation_invariant(&[base, left.clone(), right.clone()], reduce);
        let Some(Some(winner)) = state else {
            panic!("concurrent edits leave an edit")
        };
        assert!(winner == b"left" || winner == b"right");
        // An edit observing both survivors resolves the conflict.
        let merge = edit(1, "merged", &[&left, &right]);
        let state = assert_permutation_invariant(&[left, right, merge], reduce);
        assert_eq!(state, Some(Some(b"merged".to_vec())));
    }

    #[test]
    fn delete_wins_over_concurrent_and_later_edits() {
        let first = edit(1, "a", &[]);
        let removed = delete(2, &[&first]);
        let concurrent = edit(3, "c", &[&first]);
        let after = edit(1, "after", &[&first, &removed]);
        let state = assert_permutation_invariant(&[first, removed, concurrent, after], reduce);
        assert_eq!(state, None);
    }

    #[test]
    fn delete_before_send_arrival_is_a_tombstone() {
        // Only the delete is known: the message resolves deleted, so a later
        // send arrival stays hidden.
        let state = assert_permutation_invariant(&[delete(2, &[])], reduce);
        assert_eq!(state, None);
    }
}
