//! TODO fix - Simplified agent configuration using unified approach
//!
//! **CLEANUP**: Replaced complex configuration hierarchy with simple runtime setup.
//! Configuration is now handled through effect handlers and choreographic protocols,
//! eliminating 404 lines of over-engineered config structures.
//!
//! Essential configuration is moved to runtime initialization in main.rs.
//! Device settings are stored in the journal as CRDT facts.
//! Authentication settings are handled by choreographic protocols.

pub use aura_core::{AccountId, AuthorityId};
use uuid;

/// Minimal essential configuration for agent startup
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentConfig {
    /// This authority's unique identifier
    pub authority_id: AuthorityId,
    /// Account this authority belongs to (if known)
    /// NOTE: Deprecated - authority_id replaces this in authority-centric model
    pub account_id: Option<AccountId>,
}

impl AgentConfig {
    /// Create minimal config for testing
    pub fn test(authority_id: AuthorityId) -> Self {
        Self {
            authority_id,
            account_id: None,
        }
    }
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            authority_id: AuthorityId(uuid::Uuid::from_bytes([0u8; 16])),
            account_id: None,
        }
    }
}
