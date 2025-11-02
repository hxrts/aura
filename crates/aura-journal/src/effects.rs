//! Algebraic effects for ledger operations

use crate::{AccountState, Operation, OperationId};
use crate::error::Result;
use aura_types::{AccountId, DeviceId};
use automerge::Change;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Actor ID for operations
#[derive(Clone, Debug)]
pub struct ActorId(pub automerge::ActorId);

impl From<DeviceId> for ActorId {
    fn from(device_id: DeviceId) -> Self {
        // Convert DeviceId to bytes using its string representation
        let device_str = device_id.to_string();
        let device_bytes = device_str.as_bytes();
        let mut actor_bytes = [0u8; 16];
        let len = std::cmp::min(device_bytes.len(), 16);
        actor_bytes[..len].copy_from_slice(&device_bytes[..len]);
        Self(automerge::ActorId::from(actor_bytes))
    }
}

/// Ledger effects as an algebraic effect system
#[derive(Clone, Debug)]
pub enum LedgerEffect {
    /// Apply an operation to the state
    ApplyOperation {
        op: Operation,
        actor_id: ActorId,
    },
    
    /// Merge remote changes into local state
    MergeRemoteChanges {
        changes: Vec<Change>,
        from_device: DeviceId,
    },
    
    /// Query state at a specific path
    QueryState {
        path: Vec<String>,
        as_of: Option<Vec<automerge::ChangeHash>>,
    },
    
    
    /// Get current epoch
    GetEpoch,
    
    /// Get all active devices
    GetDevices,
    
    /// Check if operation has been applied
    HasOperation {
        op_id: OperationId,
    },
}

/// Result types for ledger effects
#[derive(Clone, Debug)]
pub enum LedgerValue {
    Changes(Vec<Change>),
    Merged,
    Query(serde_json::Value),
    EventEmitted,
    Epoch(u64),
    Devices(Vec<crate::DeviceMetadata>),
    Boolean(bool),
}

pub type LedgerResult = Result<LedgerValue>;

/// Operation logger trait for tracking applied operations
pub trait OperationLogger: Send + Sync {
    fn log(&self, op: &Operation, actor: &ActorId) -> Result<()>;
}

/// Default logger that stores operations in memory
pub struct VecOperationLogger {
    operations: Arc<RwLock<Vec<(Operation, ActorId)>>>,
}

impl VecOperationLogger {
    pub fn new() -> Self {
        Self {
            operations: Arc::new(RwLock::new(Vec::new())),
        }
    }
    
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
        
        // Use handle::spawn_blocking if inside async context  
        if Handle::try_current().is_ok() {
            let _ = tokio::task::block_in_place(|| {
                Handle::current().block_on(async move {
                    operations.write().await.push((op_clone, actor_clone));
                })
            });
        }
        Ok(())
    }
}

/// Effect handler that integrates with Automerge
pub struct LedgerHandler {
    state: Arc<RwLock<AccountState>>,
    operation_logger: Arc<dyn OperationLogger + Send + Sync>,
    applied_operations: Arc<RwLock<HashMap<OperationId, u64>>>, // op_id -> lamport_clock
}

impl LedgerHandler {
    /// Create a new ledger handler
    pub fn new(
        account_id: AccountId,
        group_public_key: aura_crypto::Ed25519VerifyingKey,
        operation_logger: Arc<dyn OperationLogger + Send + Sync>,
    ) -> Result<Self> {
        let state = AccountState::new(account_id, group_public_key)?;
        
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
            
            LedgerEffect::MergeRemoteChanges { changes, from_device } => {
                self.handle_merge_remote(changes, from_device).await
            }
            
            LedgerEffect::QueryState { path, as_of } => {
                self.handle_query_state(path, as_of).await
            }
            
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
    
    async fn handle_apply_operation(
        &mut self,
        op: Operation,
        actor_id: ActorId,
    ) -> LedgerResult {
        let op_id = op.id();
        
        // Check if already applied (idempotence)
        {
            let ops = self.applied_operations.read().await;
            if ops.contains_key(&op_id) && !op.is_idempotent() {
                return Ok(LedgerValue::Changes(vec![]));
            }
        }
        
        let mut state = self.state.write().await;
        
        // Set actor for this operation
        state.document_mut().set_actor(actor_id.0.clone());
        
        // Apply operation through Automerge
        let changes = match &op {
            Operation::AddDevice { device, .. } => {
                state.add_device(device.clone())?
            }
            Operation::RemoveDevice { device_id, .. } => {
                state.remove_device(*device_id)?
            }
            Operation::IncrementEpoch { .. } => {
                state.increment_epoch()?
            }
            Operation::AddGuardian { guardian, .. } => {
                state.add_guardian(guardian.clone())?
            }
            // TODO: Implement other operations
            _ => vec![],
        };
        
        // Record operation
        {
            let mut ops = self.applied_operations.write().await;
            ops.insert(op_id, state.get_lamport_clock());
        }
        
        // Log operation
        if !changes.is_empty() {
            self.operation_logger.log(&op, &actor_id).ok();
        }
        
        Ok(LedgerValue::Changes(changes))
    }
    
    async fn handle_merge_remote(
        &mut self,
        changes: Vec<Change>,
        _from_device: DeviceId,
    ) -> LedgerResult {
        let mut state = self.state.write().await;
        
        // Apply changes through Automerge
        state.apply_changes(changes.clone())?;
        
        // Changes are automatically merged by Automerge
        
        Ok(LedgerValue::Merged)
    }
    
    async fn handle_query_state(
        &self,
        path: Vec<String>,
        _as_of: Option<Vec<automerge::ChangeHash>>,
    ) -> LedgerResult {
        // For now, return a simple query result
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
    use aura_crypto::Effects;
    use aura_types::{AccountIdExt, DeviceIdExt};
    
    #[tokio::test]
    async fn test_ledger_handler_basic() {
        let effects = Effects::test(42);
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
        let effects = Effects::test(42);
        let account_id = AccountId::new_with_effects(&effects);
        let signing_key = aura_crypto::Ed25519SigningKey::from_bytes(&effects.random_bytes::<32>());
        let group_public_key = signing_key.verifying_key();
        let device_id = DeviceId::new_with_effects(&effects);
        
        let logger = Arc::new(VecOperationLogger::new());
        let mut handler = LedgerHandler::new(account_id, group_public_key, logger.clone()).unwrap();
        
        // Increment epoch
        let op = Operation::IncrementEpoch {
            device_id,
            reason: "test".to_string(),
        };
        
        let result = handler.handle(LedgerEffect::ApplyOperation {
            op: op.clone(),
            actor_id: device_id.into(),
        }).await.unwrap();
        
        match result {
            LedgerValue::Changes(changes) => assert!(!changes.is_empty()),
            _ => panic!("Expected changes"),
        }
        
        // Check epoch was incremented
        let result = handler.handle(LedgerEffect::GetEpoch).await.unwrap();
        match result {
            LedgerValue::Epoch(epoch) => assert_eq!(epoch, 1),
            _ => panic!("Expected epoch value"),
        }
        
        // Check idempotence
        let result = handler.handle(LedgerEffect::HasOperation { op_id: op.id() }).await.unwrap();
        match result {
            LedgerValue::Boolean(has) => assert!(has),
            _ => panic!("Expected boolean"),
        }
        
        // Check operation was logged
        let operations = logger.get_operations().await;
        assert_eq!(operations.len(), 1);
    }
}