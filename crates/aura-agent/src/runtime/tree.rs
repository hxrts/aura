//! Tree operation stubs
//!
//! Placeholder for tree-based operations and commitment tree
//! integration in the authority-centric architecture.

use aura_core::identifiers::AuthorityId;

/// Stub tree operations
#[derive(Debug)]
pub struct TreeOperations {
    authority_id: AuthorityId,
}

impl TreeOperations {
    pub fn new(authority_id: AuthorityId) -> Self {
        Self { authority_id }
    }

    pub fn authority_id(&self) -> AuthorityId {
        self.authority_id
    }
}
