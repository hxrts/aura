//! Maintenance event types
//!
//! Placeholder types for maintenance operations until the maintenance module is fully implemented.

use crate::AccountId;
use crate::identifiers::DeviceId;
use serde::{Deserialize, Serialize};
// uuid::Uuid removed - not used in this module

/// Admin replacement event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminReplaced {
    pub account_id: AccountId,
    pub replaced_by: DeviceId,
    pub new_admin: DeviceId,
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
    /// Placeholder for other maintenance events
    Other { event_type: String, data: Vec<u8> },
}
