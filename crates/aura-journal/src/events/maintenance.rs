//! Maintenance event types
//!
//! Domain events representing maintenance operations like admin replacement.
//! These are facts that can be recorded in the journal.

use aura_core::identifiers::DeviceId;
use aura_core::AccountId;
use serde::{Deserialize, Serialize};

/// Admin replacement event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminReplaced {
    /// Account being modified
    pub account_id: AccountId,
    /// Device that performed the replacement
    pub replaced_by: DeviceId,
    /// New admin device
    pub new_admin: DeviceId,
    /// Epoch when the replacement becomes active
    pub activation_epoch: u64,
}

impl AdminReplaced {
    /// Create new admin replacement record
    pub fn new(
        account_id: AccountId,
        replaced_by: DeviceId,
        new_admin: DeviceId,
        activation_epoch: u64,
    ) -> Self {
        Self {
            account_id,
            replaced_by,
            new_admin,
            activation_epoch,
        }
    }
}

/// Maintenance events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MaintenanceEvent {
    /// Admin was replaced
    AdminReplaced(AdminReplaced),
    /// Extensible maintenance event
    Other {
        /// Type identifier for the event
        event_type: String,
        /// Serialized event data
        data: Vec<u8>,
    },
}
