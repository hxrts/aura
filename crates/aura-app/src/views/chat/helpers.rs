use aura_core::hash::hash;
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};

/// Canonical default channel name for the account-authority self channel.
pub const NOTE_TO_SELF_CHANNEL_NAME: &str = "Note to Self";
/// Canonical topic for the account-authority self channel.
pub const NOTE_TO_SELF_CHANNEL_TOPIC: &str = "Private notes for your account authority";

/// Derive the deterministic relational context for the account-authority self channel.
#[must_use]
pub fn note_to_self_context_id(authority_id: AuthorityId) -> ContextId {
    ContextId::new_from_entropy(hash(&authority_id.to_bytes()))
}

/// Derive the deterministic channel identifier for the account-authority self channel.
#[must_use]
pub fn note_to_self_channel_id(authority_id: AuthorityId) -> ChannelId {
    let mut seed = Vec::with_capacity("note-to-self:".len() + authority_id.to_bytes().len());
    seed.extend_from_slice(b"note-to-self:");
    seed.extend_from_slice(&authority_id.to_bytes());
    ChannelId::from_bytes(hash(&seed))
}

/// Returns true when a channel name refers to the canonical self channel.
#[must_use]
pub fn is_note_to_self_channel_name(name: &str) -> bool {
    name.eq_ignore_ascii_case(NOTE_TO_SELF_CHANNEL_NAME)
}
