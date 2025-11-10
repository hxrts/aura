//! Journal operation handlers

use super::{JournalContext, JournalHandler};
use crate::error::{AuraError, Result};
use crate::operations::JournalOperation;
use crate::semilattice::account_state::AccountState;
use std::sync::Arc;
use std::sync::RwLock;

/// Main journal handler that processes operations against the account state
pub struct StateHandler {
    /// Account state (protected by RwLock for concurrent access)
    state: Arc<RwLock<AccountState>>,
}

impl StateHandler {
    /// Create a new state handler
    pub fn new(state: Arc<RwLock<AccountState>>) -> Self {
        Self { state }
    }
}

impl JournalHandler for StateHandler {
    fn handle(
        &self,
        operation: JournalOperation,
        _context: &JournalContext,
    ) -> Result<serde_json::Value> {
        match operation {
            JournalOperation::AddDevice { device } => {
                let mut state = self.state.write().map_err(|_| {
                    AuraError::internal("Failed to acquire write lock on account state")
                })?;

                state.add_device(device);

                Ok(serde_json::json!({
                    "operation": "add_device",
                    "success": true
                }))
            }

            JournalOperation::RemoveDevice { device_id } => {
                let mut state = self.state.write().map_err(|_| {
                    AuraError::internal("Failed to acquire write lock on account state")
                })?;

                state.remove_device(device_id);

                Ok(serde_json::json!({
                    "operation": "remove_device",
                    "device_id": device_id.to_string(),
                    "success": true
                }))
            }

            JournalOperation::AddGuardian { guardian } => {
                let mut state = self.state.write().map_err(|_| {
                    AuraError::internal("Failed to acquire write lock on account state")
                })?;

                state.add_guardian(guardian);

                Ok(serde_json::json!({
                    "operation": "add_guardian",
                    "success": true
                }))
            }

            JournalOperation::IncrementEpoch => {
                let mut state = self.state.write().map_err(|_| {
                    AuraError::internal("Failed to acquire write lock on account state")
                })?;

                state.increment_epoch();
                let new_epoch = state.get_epoch();

                Ok(serde_json::json!({
                    "operation": "increment_epoch",
                    "new_epoch": new_epoch,
                    "success": true
                }))
            }

            JournalOperation::GetDevices => {
                let state = self.state.read().map_err(|_| {
                    AuraError::internal("Failed to acquire read lock on account state")
                })?;

                let devices = state.get_devices();

                Ok(serde_json::json!({
                    "operation": "get_devices",
                    "devices": devices.len(),
                    "success": true
                }))
            }

            JournalOperation::GetEpoch => {
                let state = self.state.read().map_err(|_| {
                    AuraError::internal("Failed to acquire read lock on account state")
                })?;

                let epoch = state.get_epoch();

                Ok(serde_json::json!({
                    "operation": "get_epoch",
                    "epoch": epoch,
                    "success": true
                }))
            }
        }
    }
}

/// No-op handler for testing
pub struct NoOpHandler;

impl JournalHandler for NoOpHandler {
    fn handle(
        &self,
        operation: JournalOperation,
        _context: &JournalContext,
    ) -> Result<serde_json::Value> {
        Ok(serde_json::json!({
            "operation": format!("{:?}", operation),
            "handler": "no_op",
            "success": true
        }))
    }
}
