//! TODO fix - Simplified agent configuration using unified approach
//!
//! **CLEANUP**: Replaced complex configuration hierarchy with simple runtime setup.
//! Configuration is now handled through effect handlers and choreographic protocols,
//! eliminating 404 lines of over-engineered config structures.
//!
//! Essential configuration is moved to runtime initialization in main.rs.
//! Device settings are stored in the journal as CRDT facts.
//! Authentication settings are handled by choreographic protocols.

pub use aura_core::{AccountId, DeviceId};
use uuid;

/// Minimal essential configuration for agent startup
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentConfig {
    /// This device's unique identifier
    pub device_id: DeviceId,
    /// Account this device belongs to (if known)
    pub account_id: Option<AccountId>,
}

impl AgentConfig {
    /// Create minimal config for testing
    pub fn test(device_id: DeviceId) -> Self {
        Self {
            device_id,
            account_id: None,
        }
    }
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            device_id: DeviceId(uuid::Uuid::new_v4()),
            account_id: None,
        }
    }
}
