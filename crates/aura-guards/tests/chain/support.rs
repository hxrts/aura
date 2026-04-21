#![allow(missing_docs)]

use aura_core::types::identifiers::{AuthorityId, ContextId};

pub fn test_authority(seed: u8) -> AuthorityId {
    AuthorityId::new_from_entropy([seed; 32])
}

pub fn test_context(seed: u8) -> ContextId {
    ContextId::new_from_entropy([seed; 32])
}
