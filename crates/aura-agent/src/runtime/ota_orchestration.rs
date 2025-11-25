//! OTA orchestration stubs
//!
//! Placeholder for over-the-air orchestration capabilities
//! moved from aura-protocol to the runtime layer.

use aura_core::identifiers::AuthorityId;

/// OTA orchestration coordinator
#[derive(Debug)]
#[allow(dead_code)] // Part of future OTA orchestration API
pub struct OtaOrchestrator {
    authority_id: AuthorityId,
}

impl OtaOrchestrator {
    #[allow(dead_code)] // Part of future OTA orchestration API
    pub fn new(authority_id: AuthorityId) -> Self {
        Self { authority_id }
    }

    #[allow(dead_code)] // Part of future OTA orchestration API
    pub fn authority_id(&self) -> AuthorityId {
        self.authority_id
    }
}
