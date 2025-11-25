//! Agent runtime stubs
//!
//! Placeholder for agent runtime functionality that will integrate
//! with the new authority-centric architecture.

use aura_core::identifiers::AuthorityId;

/// Stub agent runtime
#[derive(Debug)]
pub struct AgentRuntime {
    #[allow(dead_code)] // Will be used in future agent runtime
    authority_id: AuthorityId,
}

impl AgentRuntime {
    #[allow(dead_code)]
    pub fn new(authority_id: AuthorityId) -> Self {
        Self { authority_id }
    }
}
