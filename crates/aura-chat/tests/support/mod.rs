//! Shared deterministic fixtures for chat integration tests.

use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};

/// Deterministic context id fixture.
pub fn test_context_id(seed: u8) -> ContextId {
    ContextId::new_from_entropy([seed; 32])
}

/// Deterministic channel id fixture.
pub fn test_channel_id(seed: u8) -> ChannelId {
    ChannelId::from_bytes([seed; 32])
}

/// Deterministic authority id fixture.
pub fn test_authority_id(seed: u8) -> AuthorityId {
    AuthorityId::new_from_entropy([seed; 32])
}
