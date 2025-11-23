//! OTA orchestration stubs
//!
//! Placeholder for over-the-air orchestration capabilities
//! moved from aura-protocol to the runtime layer.

use aura_core::identifiers::AuthorityId;

/// OTA orchestration coordinator
#[derive(Debug)]
pub struct OtaOrchestrator {
    authority_id: AuthorityId,
}

impl OtaOrchestrator {
    pub fn new(authority_id: AuthorityId) -> Self {
        Self { authority_id }
    }

    pub fn authority_id(&self) -> AuthorityId {
        self.authority_id
    }
}