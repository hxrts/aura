//! Choreography adapter implementation
//!
//! Adapter for integrating with the choreographic programming
//! system from aura-protocol in the authority-centric runtime.

use aura_core::identifiers::AuthorityId;

/// Adapter for choreography integration
#[derive(Debug)]
pub struct AuraHandlerAdapter {
    authority_id: AuthorityId,
}

impl AuraHandlerAdapter {
    pub fn new(authority_id: AuthorityId) -> Self {
        Self { authority_id }
    }

    pub fn authority_id(&self) -> AuthorityId {
        self.authority_id
    }
}

/// Choreography adapter alias for backwards compatibility
pub type ChoreographyAdapter = AuraHandlerAdapter;