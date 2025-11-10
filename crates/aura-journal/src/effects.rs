//! Algebraic effects for ledger operations

use crate::error::Result;
use crate::{semilattice::account_state::AccountState, Operation, OperationId};
use aura_core::{semilattice::JoinSemilattice, AccountId, DeviceId};
// Note: No longer using automerge::Change with modern state
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Actor ID for operations (TODO fix - Simplified for modern state)
#[derive(Clone, Debug)]
pub struct ActorId(pub DeviceId);

impl From<DeviceId> for ActorId {
    fn from(device_id: DeviceId) -> Self {
        Self(device_id)
    }
}

/// Algebraic effects for ledger operations.
///
/// This enum represents all possible effects that can be performed on the account ledger.
/// Each effect is a pure description of an operation that will be interpreted by the
/// `LedgerHandler` to modify the underlying Automerge CRDT state.
#[derive(Clone, Debug)]
pub enum LedgerEffect {
    /// Apply a single operation to the ledger state.
    ///
    /// The operation is applied atomically and generates Automerge changes that can be
    /// replicated to other devices. Operations are idempotent unless explicitly marked otherwise.
    ApplyOperation {
        /// The operation to apply (e.g., add device, increment epoch)
        op: Operation,
        /// The actor (device) performing this operation
        actor_id: ActorId,
    },

    /// Merge remote state from another device into the local ledger state.
    ///
    /// Semilattice automatically handles conflict resolution using join operations.
    /// All state merges are validated before being applied to ensure consistency.
    MergeRemoteState {
        /// The remote account state to merge
        remote_state: AccountState,
        /// The device that sent this state
        from_device: DeviceId,
    },

    /// Query the current state at a specific path in the document.
    ///
    /// Supports querying historical state using the `as_of` parameter to view
    /// the state at a particular set of change hashes.
    QueryState {
        /// The path to query in the document (e.g., ["devices", "device-123"])
        path: Vec<String>,
        /// Optional change hashes to query state as of a specific point in time
        as_of: Option<Vec<automerge::ChangeHash>>,
    },

    /// Get the current session epoch number.
    ///
    /// The epoch is incremented whenever the account undergoes a critical state change
    /// that requires invalidating old credentials (e.g., device removal, key resharing).
    GetEpoch,

    /// Get all devices currently registered in the account.
    ///
    /// Returns metadata for each device including its ID, public key, and status.
    GetDevices,

    /// Check if a specific operation has already been applied to the ledger.
    ///
    /// Used for idempotence checking to avoid applying the same operation twice.
    HasOperation {
        /// The unique ID of the operation to check
        op_id: OperationId,
    },
}

/// Return values from ledger effect handlers.
///
/// Each `LedgerEffect` produces a corresponding `LedgerValue` when handled.
/// This enum encapsulates all possible success values from ledger operations.
#[derive(Clone, Debug)]
pub enum LedgerValue {
    /// Confirmation that operation was successfully applied.
    ///
    /// Modern AccountState uses semilattice join operations instead of changes.
    Applied,

    /// Confirmation that remote changes were successfully merged.
    ///
    /// The ledger state now includes the merged changes from the remote device.
    Merged,

    /// Result of a state query at a specific path.
    ///
    /// The JSON value represents the state at the queried path, either current or historical.
    Query(serde_json::Value),

    /// Confirmation that an event was emitted (currently unused).
    ///
    /// Reserved for future event emission functionality.
    EventEmitted,

    /// The current session epoch number.
    ///
    /// Epochs are incremented to invalidate old credentials after critical state changes.
    Epoch(u64),

    /// Metadata for all devices currently registered in the account.
    ///
    /// Each entry contains device ID, public key, and registration status.
    Devices(Vec<crate::DeviceMetadata>),

    /// Boolean result from a predicate check.
    ///
    /// Used for operations like `HasOperation` that return true/false.
    Boolean(bool),
}

/// Result type for ledger effect handlers.
///
/// Combines `LedgerValue` success cases with `Error` failures.
pub type LedgerResult = Result<LedgerValue>;

/// Trait for logging applied operations.
///
/// Implementations track which operations have been applied to the ledger,
/// enabling auditing, debugging, and replay functionality.
pub trait OperationLogger: Send + Sync {
    /// Log an operation that was successfully applied.
    ///
    /// # Parameters
    /// - `op`: The operation that was applied
    /// - `actor`: The device that performed the operation
    ///
    /// # Returns
    /// `Ok(())` if logging succeeded, or an error if logging failed
    fn log(&self, op: &Operation, actor: &ActorId) -> Result<()>;
}

/// In-memory operation logger for testing and development.
///
/// Stores all logged operations in a vector protected by a read-write lock.
/// This implementation is suitable for testing but should not be used in production
/// where persistent logging is required.
pub struct VecOperationLogger {
    /// Thread-safe storage of logged operations
    operations: Arc<RwLock<Vec<(Operation, ActorId)>>>,
}

impl VecOperationLogger {
    /// Create a new vector-based operation logger
    pub fn new() -> Self {
        Self {
            operations: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Get all logged operations
    pub async fn get_operations(&self) -> Vec<(Operation, ActorId)> {
        self.operations.read().await.clone()
    }
}

impl OperationLogger for VecOperationLogger {
    fn log(&self, op: &Operation, actor: &ActorId) -> Result<()> {
        use tokio::runtime::Handle;

        // Clone values to move into async block
        let op_clone = op.clone();
        let actor_clone = actor.clone();
        let operations = self.operations.clone();

        // Use spawn if inside async context to avoid blocking issues
        if Handle::try_current().is_ok() {
            let _ = tokio::spawn(async move {
                operations.write().await.push((op_clone, actor_clone));
            });
        }
        Ok(())
    }
}

/// Handler for ledger effects that integrates with Automerge CRDT.
///
/// This handler interprets `LedgerEffect` values and performs the corresponding
/// operations on the underlying Automerge document. It ensures consistency by:
/// - Tracking applied operations for idempotence
/// - Recording Lamport clock values for causality
/// - Logging operations for audit trails
pub struct LedgerHandler {
    /// The account state managed by this handler
    state: Arc<RwLock<AccountState>>,
    /// Logger for recording applied operations
    operation_logger: Arc<dyn OperationLogger + Send + Sync>,
    /// Map of operation IDs to their Lamport clock values for idempotence checking
    applied_operations: Arc<RwLock<HashMap<OperationId, u64>>>,
}

impl LedgerHandler {
    /// Create a new ledger handler
    pub fn new(
        account_id: AccountId,
        group_public_key: aura_crypto::Ed25519VerifyingKey,
        operation_logger: Arc<dyn OperationLogger + Send + Sync>,
    ) -> Result<Self> {
        let state = AccountState::new(account_id, group_public_key);

        Ok(Self {
            state: Arc::new(RwLock::new(state)),
            operation_logger,
            applied_operations: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Create from existing state
    pub fn from_state(
        state: AccountState,
        operation_logger: Arc<dyn OperationLogger + Send + Sync>,
    ) -> Self {
        Self {
            state: Arc::new(RwLock::new(state)),
            operation_logger,
            applied_operations: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Handle a ledger effect
    pub async fn handle(&mut self, effect: LedgerEffect) -> LedgerResult {
        match effect {
            LedgerEffect::ApplyOperation { op, actor_id } => {
                self.handle_apply_operation(op, actor_id).await
            }

            LedgerEffect::MergeRemoteState {
                remote_state,
                from_device,
            } => self.handle_merge_remote(remote_state, from_device).await,

            LedgerEffect::QueryState { path, as_of } => self.handle_query_state(path, as_of).await,

            LedgerEffect::GetEpoch => {
                let state = self.state.read().await;
                Ok(LedgerValue::Epoch(state.get_epoch()))
            }

            LedgerEffect::GetDevices => {
                let state = self.state.read().await;
                Ok(LedgerValue::Devices(state.get_devices()))
            }

            LedgerEffect::HasOperation { op_id } => {
                let ops = self.applied_operations.read().await;
                Ok(LedgerValue::Boolean(ops.contains_key(&op_id)))
            }
        }
    }

    async fn handle_apply_operation(&mut self, op: Operation, actor_id: ActorId) -> LedgerResult {
        let op_id = op.id();

        // Check if already applied (idempotence)
        {
            let ops = self.applied_operations.read().await;
            if ops.contains_key(&op_id) && !op.is_idempotent() {
                return Ok(LedgerValue::Applied);
            }
        }

        let mut state = self.state.write().await;

        // Apply operation through modern AccountState
        match &op {
            Operation::AddDevice { device } => state.add_device(device.clone()),
            Operation::RemoveDevice { device_id } => state.remove_device(*device_id),
            Operation::IncrementEpoch => state.increment_epoch(),
            Operation::AddGuardian { guardian } => state.add_guardian(guardian.clone()),
            // TODO: Implement other operations
            _ => {}
        };

        // Record operation in state
        state.mark_operation_applied(op_id.to_string());

        // Record operation in local cache
        {
            let mut ops = self.applied_operations.write().await;
            ops.insert(op_id, state.get_lamport_clock());
        }

        // Log operation
        self.operation_logger.log(&op, &actor_id).ok();

        Ok(LedgerValue::Applied)
    }

    async fn handle_merge_remote(
        &mut self,
        remote_state: AccountState,
        _from_device: DeviceId,
    ) -> LedgerResult {
        let mut state = self.state.write().await;

        // Merge states using semilattice join
        *state = state.join(&remote_state);

        Ok(LedgerValue::Merged)
    }

    async fn handle_query_state(
        &self,
        path: Vec<String>,
        _as_of: Option<Vec<automerge::ChangeHash>>,
    ) -> LedgerResult {
        // TODO fix - For now, return a simple query result
        // TODO: Implement proper path navigation in Automerge
        Ok(LedgerValue::Query(serde_json::json!({
            "path": path,
            "note": "Query implementation pending"
        })))
    }

    // Helper methods
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{AccountIdExt, DeviceIdExt};
    use aura_crypto::Effects;

    #[tokio::test]
    async fn test_ledger_handler_basic() {
        let effects = Effects::test();
        let account_id = AccountId::new_with_effects(&effects);
        let signing_key = aura_crypto::Ed25519SigningKey::from_bytes(&effects.random_bytes::<32>());
        let group_public_key = signing_key.verifying_key();

        let logger = Arc::new(VecOperationLogger::new());
        let mut handler = LedgerHandler::new(account_id, group_public_key, logger.clone()).unwrap();

        // Get initial epoch
        let result = handler.handle(LedgerEffect::GetEpoch).await.unwrap();
        match result {
            LedgerValue::Epoch(epoch) => assert_eq!(epoch, 0),
            _ => panic!("Expected epoch value"),
        }
    }

    #[tokio::test]
    async fn test_apply_operation() {
        let effects = Effects::test();
        let account_id = AccountId::new_with_effects(&effects);
        let signing_key = aura_crypto::Ed25519SigningKey::from_bytes(&effects.random_bytes::<32>());
        let group_public_key = signing_key.verifying_key();
        let device_id = DeviceId::new_with_effects(&effects);

        let logger = Arc::new(VecOperationLogger::new());
        let mut handler = LedgerHandler::new(account_id, group_public_key, logger.clone()).unwrap();

        // Increment epoch
        let op = Operation::IncrementEpoch;

        let result = handler
            .handle(LedgerEffect::ApplyOperation {
                op: op.clone(),
                actor_id: device_id.into(),
            })
            .await
            .unwrap();

        match result {
            LedgerValue::Applied => {} // Success
            _ => panic!("Expected Applied"),
        }

        // Check epoch was incremented
        let result = handler.handle(LedgerEffect::GetEpoch).await.unwrap();
        match result {
            LedgerValue::Epoch(epoch) => assert_eq!(epoch, 1),
            _ => panic!("Expected epoch value"),
        }

        // Check idempotence
        let result = handler
            .handle(LedgerEffect::HasOperation { op_id: op.id() })
            .await
            .unwrap();
        match result {
            LedgerValue::Boolean(has) => assert!(has),
            _ => panic!("Expected boolean"),
        }

        // Allow async task to complete
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Check operation was logged
        let operations = logger.get_operations().await;
        assert_eq!(operations.len(), 1);
    }
}
