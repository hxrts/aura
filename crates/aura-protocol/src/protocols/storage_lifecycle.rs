//! Storage Protocol Lifecycle Implementation
//!
//! Implements storage operations as first-class protocol lifecycles integrated
//! with the journal system. Storage operations (store, retrieve, delete) generate
//! events in the ledger and coordinate through capability-based access control.
//!
//! ## Architecture
//!
//! Storage operations are implemented as simple state machines:
//! - **Journaled**: All storage operations create events in the ledger
//! - **Capability-Based**: Access control enforced through authorization layer
//! - **Coordinated**: Integration with existing protocol infrastructure
//!
//! ## Usage
//!
//! Storage operations are triggered through protocol events and coordinated
//! through the existing protocol infrastructure.

use crate::core::capabilities::ProtocolCapabilities;
use crate::core::lifecycle::{ProtocolDescriptor, ProtocolInput, ProtocolLifecycle, ProtocolStep};
use crate::core::metadata::{ProtocolPriority, ProtocolType};
use crate::core::typestate::SessionState;
use crate::types::SessionId;
use aura_authorization::CapabilityToken;
use aura_crypto::Effects;
use aura_journal::{DeleteDataEvent, Event, EventType, RetrieveDataEvent, StoreDataEvent};
use aura_types::{AccountId, AuraError, AuraResult, Cid, DeviceId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Debug;
use uuid::Uuid;

/// Storage protocol state machine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StorageState {
    /// Initiating storage operation with metadata validation
    Initiating {
        blob_id: Cid,
        metadata: BlobMetadata,
        operation_type: StorageOperationType,
    },
    /// Encrypting data before upload (store operations only)
    Encrypting {
        blob_id: Cid,
        chunks: Vec<ChunkInfo>,
    },
    /// Uploading data chunks to storage nodes
    Uploading {
        blob_id: Cid,
        uploaded_chunks: usize,
        total_chunks: usize,
        storage_nodes: Vec<DeviceId>,
    },
    /// Replicating data across storage nodes for durability
    Replicating {
        blob_id: Cid,
        replicas: Vec<ReplicaInfo>,
        target_replication_factor: u8,
    },
    /// Operation completed successfully
    Completed {
        blob_id: Cid,
        locations: Vec<DeviceId>,
        operation_result: StorageOperationResult,
    },
    /// Operation failed with error details
    Failed {
        blob_id: Cid,
        reason: String,
        recovery_options: Vec<RecoveryOption>,
    },
}

/// Type of storage operation being performed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StorageOperationType {
    Store,
    Retrieve,
    Delete,
}

/// Metadata for storage objects
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlobMetadata {
    /// Size of the blob in bytes
    pub size_bytes: u64,
    /// Content type hint
    pub content_type: Option<String>,
    /// Application-specific metadata
    pub app_metadata: BTreeMap<String, String>,
    /// Required capabilities for access
    pub required_capabilities: Vec<String>,
    /// Desired replication factor
    pub replication_factor: u8,
    /// Encryption specification
    pub encryption_spec: EncryptionSpec,
}

/// Information about encrypted chunks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkInfo {
    /// Index of this chunk
    pub index: u32,
    /// Size of encrypted chunk
    pub size: u64,
    /// Hash of encrypted content
    pub hash: [u8; 32],
    /// Encryption key derivation info
    pub key_info: KeyInfo,
}

/// Replica placement information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicaInfo {
    /// Storage node hosting this replica
    pub node_id: DeviceId,
    /// Timestamp when replica was created
    pub created_at: u64,
    /// Health status of this replica
    pub status: ReplicaStatus,
}

/// Status of a storage replica
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReplicaStatus {
    Healthy,
    Degraded,
    Failed,
    Unknown,
}

/// Result of a storage operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageOperationResult {
    /// Bytes transferred
    pub bytes_transferred: u64,
    /// Duration of operation in milliseconds
    pub duration_ms: u64,
    /// Nodes involved in the operation
    pub nodes_used: Vec<DeviceId>,
    /// Operation-specific metadata
    pub metadata: BTreeMap<String, String>,
}

/// Recovery options when storage operation fails
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecoveryOption {
    Retry,
    UseAlternativeNodes(Vec<DeviceId>),
    ReduceReplicationFactor(u8),
    SplitIntoSmallerChunks,
}

/// Encryption specification for storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionSpec {
    /// Algorithm used (e.g., "AES-256-GCM")
    pub algorithm: String,
    /// Key derivation context
    pub key_context: String,
    /// Additional parameters
    pub params: BTreeMap<String, String>,
}

/// Key information for chunk decryption
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyInfo {
    /// Key derivation context
    pub context: String,
    /// Algorithm used
    pub algorithm: String,
    /// Additional key parameters
    pub params: BTreeMap<String, String>,
}

/// Session state marker for storage protocol
#[derive(Debug, Clone)]
pub struct StorageSessionState;

impl SessionState for StorageSessionState {
    const NAME: &'static str = "storage_session";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = true;
}

/// Storage protocol lifecycle implementation
pub struct StorageLifecycle {
    session_id: Uuid,
    account_id: AccountId,
    device_id: DeviceId,
    state: StorageState,
    effects: Effects,
    descriptor: ProtocolDescriptor,
}

impl StorageLifecycle {
    /// Create a new storage operation for storing data
    pub fn new_store(
        session_id: Uuid,
        account_id: AccountId,
        device_id: DeviceId,
        blob_id: Cid,
        metadata: BlobMetadata,
        effects: Effects,
    ) -> Self {
        let descriptor = ProtocolDescriptor::new(
            Uuid::new_v4(),
            SessionId::new(),
            device_id,
            ProtocolType::Storage,
        )
        .with_priority(ProtocolPriority::Normal);

        Self {
            session_id,
            account_id,
            device_id,
            state: StorageState::Initiating {
                blob_id,
                metadata,
                operation_type: StorageOperationType::Store,
            },
            effects,
            descriptor,
        }
    }

    /// Create a new storage operation for retrieving data
    pub fn new_retrieve(
        session_id: Uuid,
        account_id: AccountId,
        device_id: DeviceId,
        blob_id: Cid,
        _capability_proof: CapabilityToken,
        effects: Effects,
    ) -> Self {
        let metadata = BlobMetadata {
            size_bytes: 0, // Unknown for retrieve operations
            content_type: None,
            app_metadata: BTreeMap::new(),
            required_capabilities: vec![], // Will be validated against proof
            replication_factor: 1,
            encryption_spec: EncryptionSpec {
                algorithm: "unknown".to_string(),
                key_context: "retrieve".to_string(),
                params: BTreeMap::new(),
            },
        };

        let descriptor = ProtocolDescriptor::new(
            Uuid::new_v4(),
            SessionId::new(),
            device_id,
            ProtocolType::Storage,
        )
        .with_priority(ProtocolPriority::Normal);

        Self {
            session_id,
            account_id,
            device_id,
            state: StorageState::Initiating {
                blob_id,
                metadata,
                operation_type: StorageOperationType::Retrieve,
            },
            effects,
            descriptor,
        }
    }

    /// Create a new storage operation for deleting data
    pub fn new_delete(
        session_id: Uuid,
        account_id: AccountId,
        device_id: DeviceId,
        blob_id: Cid,
        _reason: Option<String>,
        effects: Effects,
    ) -> Self {
        let metadata = BlobMetadata {
            size_bytes: 0, // Not relevant for delete operations
            content_type: None,
            app_metadata: BTreeMap::new(),
            required_capabilities: vec![], // Will be validated during execution
            replication_factor: 0,
            encryption_spec: EncryptionSpec {
                algorithm: "none".to_string(),
                key_context: "delete".to_string(),
                params: BTreeMap::new(),
            },
        };

        let descriptor = ProtocolDescriptor::new(
            Uuid::new_v4(),
            SessionId::new(),
            device_id,
            ProtocolType::Storage,
        )
        .with_priority(ProtocolPriority::Normal);

        Self {
            session_id,
            account_id,
            device_id,
            state: StorageState::Initiating {
                blob_id,
                metadata,
                operation_type: StorageOperationType::Delete,
            },
            effects,
            descriptor,
        }
    }

    /// Get current storage state
    pub fn current_state(&self) -> &StorageState {
        &self.state
    }

    /// Get blob ID for this operation
    pub fn blob_id(&self) -> Cid {
        match &self.state {
            StorageState::Initiating { blob_id, .. } => blob_id.clone(),
            StorageState::Encrypting { blob_id, .. } => blob_id.clone(),
            StorageState::Uploading { blob_id, .. } => blob_id.clone(),
            StorageState::Replicating { blob_id, .. } => blob_id.clone(),
            StorageState::Completed { blob_id, .. } => blob_id.clone(),
            StorageState::Failed { blob_id, .. } => blob_id.clone(),
        }
    }

    /// Transition to next state in the storage protocol
    pub fn transition_to(&mut self, new_state: StorageState) -> AuraResult<()> {
        // Validate state transition
        self.validate_transition(&self.state, &new_state)?;
        self.state = new_state;
        Ok(())
    }

    /// Validate that a state transition is legal
    fn validate_transition(&self, from: &StorageState, to: &StorageState) -> AuraResult<()> {
        use StorageState::*;

        let valid = match (from, to) {
            // From Initiating
            (Initiating { .. }, Encrypting { .. }) => true,
            (Initiating { .. }, Uploading { .. }) => true,
            (Initiating { .. }, Failed { .. }) => true,

            // From Encrypting
            (Encrypting { .. }, Uploading { .. }) => true,
            (Encrypting { .. }, Failed { .. }) => true,

            // From Uploading
            (Uploading { .. }, Replicating { .. }) => true,
            (Uploading { .. }, Completed { .. }) => true, // Single replica case
            (Uploading { .. }, Failed { .. }) => true,

            // From Replicating
            (Replicating { .. }, Completed { .. }) => true,
            (Replicating { .. }, Failed { .. }) => true,

            // Terminal states
            (Completed { .. }, _) => false, // No transitions from completed
            (Failed { .. }, Initiating { .. }) => true, // Retry allowed
            (Failed { .. }, _) => false,

            _ => false,
        };

        if !valid {
            return Err(AuraError::agent_invalid_state(format!(
                "Invalid storage state transition: {:?} -> {:?}",
                from, to
            )));
        }

        Ok(())
    }
}

impl StorageLifecycle {
    /// Generate journal events for storage operations
    pub fn generate_events(&self) -> AuraResult<Vec<Event>> {
        let mut events = Vec::new();

        match &self.state {
            StorageState::Initiating {
                blob_id,
                metadata,
                operation_type,
            } => {
                match operation_type {
                    StorageOperationType::Store => {
                        let store_event = StoreDataEvent {
                            blob_id: blob_id.clone(),
                            size_bytes: metadata.size_bytes,
                            required_capabilities: metadata.required_capabilities.clone(),
                            replication_factor: metadata.replication_factor,
                            encryption_key_spec: aura_journal::KeyDerivationSpec {
                                context: metadata.encryption_spec.key_context.clone(),
                                algorithm: metadata.encryption_spec.algorithm.clone(),
                                params: metadata.encryption_spec.params.clone(),
                            },
                        };

                        let event = Event::new(
                            self.account_id,
                            1,    // TODO: Use proper nonce
                            None, // TODO: Use proper parent hash
                            0,    // TODO: Use proper epoch
                            EventType::StoreData(store_event),
                            aura_authentication::EventAuthorization::LifecycleInternal,
                            &self.effects,
                        )
                        .map_err(|e| AuraError::serialization_failed(e))?;

                        events.push(event);
                    }
                    StorageOperationType::Retrieve => {
                        // Create a placeholder capability token for testing
                        let placeholder_token = CapabilityToken::new(
                            aura_authorization::Subject::Device(self.device_id),
                            aura_authorization::Resource::StorageObject {
                                object_id: uuid::Uuid::new_v4(),
                                owner: self.account_id,
                            },
                            vec![aura_authorization::Action::Read],
                            self.device_id,
                            false, // not delegatable
                            0,     // no delegation depth
                        );

                        let retrieve_event = RetrieveDataEvent {
                            blob_id: blob_id.clone(),
                            requester: self.device_id,
                            capability_proof: placeholder_token, // TODO: Use actual proof
                        };

                        let event = Event::new(
                            self.account_id,
                            1,    // TODO: Use proper nonce
                            None, // TODO: Use proper parent hash
                            0,    // TODO: Use proper epoch
                            EventType::RetrieveData(retrieve_event),
                            aura_authentication::EventAuthorization::LifecycleInternal,
                            &self.effects,
                        )
                        .map_err(|e| AuraError::serialization_failed(e))?;

                        events.push(event);
                    }
                    StorageOperationType::Delete => {
                        let delete_event = DeleteDataEvent {
                            blob_id: blob_id.clone(),
                            deleted_by: self.device_id,
                            reason: Some("User requested deletion".to_string()), // TODO: Use actual reason
                        };

                        let event = Event::new(
                            self.account_id,
                            1,    // TODO: Use proper nonce
                            None, // TODO: Use proper parent hash
                            0,    // TODO: Use proper epoch
                            EventType::DeleteData(delete_event),
                            aura_authentication::EventAuthorization::LifecycleInternal,
                            &self.effects,
                        )
                        .map_err(|e| AuraError::serialization_failed(e))?;

                        events.push(event);
                    }
                }
            }
            _ => {
                // Other states don't generate events in this basic implementation
            }
        }

        Ok(events)
    }

    /// Process storage operation input
    pub fn process_input(&mut self, input: StorageInput) -> AuraResult<Option<StorageOutput>> {
        match (&self.state, input) {
            // TODO: Implement input processing for storage operations
            // This would include:
            // - Processing chunk data for encryption
            // - Handling storage node responses
            // - Managing replication coordination
            // - Processing capability validations
            _ => {
                tracing::warn!("Unhandled storage input in state: {:?}", self.state);
                Ok(None)
            }
        }
    }
}

/// Input types for storage protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StorageInput {
    /// Raw data to be stored
    StoreData(Vec<u8>),
    /// Encrypted chunk data
    EncryptedChunk { index: u32, data: Vec<u8> },
    /// Storage node response
    NodeResponse {
        node_id: DeviceId,
        success: bool,
        message: String,
    },
    /// Capability validation result
    CapabilityValidation {
        valid: bool,
        capabilities: Vec<String>,
    },
    /// Replication status update
    ReplicationUpdate { replica_info: ReplicaInfo },
}

/// Output types for storage protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StorageOutput {
    /// Storage operation completed
    OperationComplete(StorageOperationResult),
    /// Retrieved data
    RetrievedData(Vec<u8>),
    /// Progress update
    ProgressUpdate {
        completed_chunks: usize,
        total_chunks: usize,
    },
    /// Error occurred
    Error(String),
}

impl ProtocolLifecycle for StorageLifecycle {
    type State = StorageSessionState;
    type Output = StorageOperationResult;
    type Error = AuraError;

    fn descriptor(&self) -> &ProtocolDescriptor {
        &self.descriptor
    }

    fn step(
        &mut self,
        input: ProtocolInput<'_>,
        _caps: &mut ProtocolCapabilities<'_>,
    ) -> ProtocolStep<Self::Output, Self::Error> {
        match input {
            ProtocolInput::LocalSignal { signal, data: _ } => {
                match signal {
                    "start_operation" => {
                        // Process storage operation start
                        let effects = vec![];

                        ProtocolStep {
                            effects,
                            transition: None,
                            outcome: None,
                        }
                    }
                    "complete_operation" => {
                        // Mark operation as completed
                        match &self.state {
                            StorageState::Uploading { blob_id, .. }
                            | StorageState::Replicating { blob_id, .. } => {
                                let result = StorageOperationResult {
                                    bytes_transferred: 1024, // TODO: Use actual values
                                    duration_ms: 100,
                                    nodes_used: vec![self.device_id],
                                    metadata: BTreeMap::new(),
                                };

                                self.state = StorageState::Completed {
                                    blob_id: blob_id.clone(),
                                    locations: vec![self.device_id],
                                    operation_result: result.clone(),
                                };

                                ProtocolStep {
                                    effects: vec![],
                                    transition: None,
                                    outcome: Some(Ok(result)),
                                }
                            }
                            _ => ProtocolStep {
                                effects: vec![],
                                transition: None,
                                outcome: Some(Err(AuraError::agent_invalid_state(
                                    "Cannot complete operation in current state".to_string(),
                                ))),
                            },
                        }
                    }
                    _ => {
                        // Unknown signal
                        ProtocolStep {
                            effects: vec![],
                            transition: None,
                            outcome: None,
                        }
                    }
                }
            }
            ProtocolInput::Timer {
                timer_id: _,
                timeout: _,
            } => {
                // Handle timeout - mark as failed
                let blob_id = self.blob_id();
                self.state = StorageState::Failed {
                    blob_id,
                    reason: "Operation timeout".to_string(),
                    recovery_options: vec![RecoveryOption::Retry],
                };

                ProtocolStep {
                    effects: vec![],
                    transition: None,
                    outcome: Some(Err(AuraError::timeout_error("Storage operation timed out"))),
                }
            }
            ProtocolInput::Message(_msg) => {
                // Handle protocol messages (not implemented in basic version)
                ProtocolStep {
                    effects: vec![],
                    transition: None,
                    outcome: None,
                }
            }
            ProtocolInput::Journal {
                event_type: _,
                payload: _,
            } => {
                // Handle journal events (not implemented in basic version)
                ProtocolStep {
                    effects: vec![],
                    transition: None,
                    outcome: None,
                }
            }
        }
    }

    fn is_final(&self) -> bool {
        matches!(
            self.state,
            StorageState::Completed { .. } | StorageState::Failed { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_types::{AccountIdExt, DeviceIdExt};

    #[test]
    fn test_storage_lifecycle_creation() {
        let effects = Effects::test();
        let session_id = Uuid::new_v4();
        let account_id = AccountId::new_with_effects(&effects);
        let device_id = DeviceId::new_with_effects(&effects);
        let blob_id = Cid::new("test-blob");

        let metadata = BlobMetadata {
            size_bytes: 1024,
            content_type: Some("text/plain".to_string()),
            app_metadata: BTreeMap::new(),
            required_capabilities: vec!["read".to_string()],
            replication_factor: 3,
            encryption_spec: EncryptionSpec {
                algorithm: "AES-256-GCM".to_string(),
                key_context: "user-data".to_string(),
                params: BTreeMap::new(),
            },
        };

        let lifecycle = StorageLifecycle::new_store(
            session_id,
            account_id,
            device_id,
            blob_id.clone(),
            metadata,
            effects,
        );

        assert_eq!(lifecycle.session_id, session_id);
        assert_eq!(lifecycle.blob_id(), blob_id);
        assert!(matches!(
            lifecycle.current_state(),
            StorageState::Initiating { .. }
        ));
    }

    #[test]
    fn test_state_transitions() {
        let effects = Effects::test();
        let session_id = Uuid::new_v4();
        let account_id = AccountId::new_with_effects(&effects);
        let device_id = DeviceId::new_with_effects(&effects);
        let blob_id = Cid::new("test-blob");

        let metadata = BlobMetadata {
            size_bytes: 1024,
            content_type: None,
            app_metadata: BTreeMap::new(),
            required_capabilities: vec![],
            replication_factor: 1,
            encryption_spec: EncryptionSpec {
                algorithm: "AES-256-GCM".to_string(),
                key_context: "test".to_string(),
                params: BTreeMap::new(),
            },
        };

        let mut lifecycle = StorageLifecycle::new_store(
            session_id,
            account_id,
            device_id,
            blob_id.clone(),
            metadata,
            effects,
        );

        // Valid transition: Initiating -> Encrypting
        let encrypting_state = StorageState::Encrypting {
            blob_id: blob_id.clone(),
            chunks: vec![],
        };
        assert!(lifecycle.transition_to(encrypting_state).is_ok());

        // Valid transition: Encrypting -> Uploading
        let uploading_state = StorageState::Uploading {
            blob_id: blob_id.clone(),
            uploaded_chunks: 0,
            total_chunks: 1,
            storage_nodes: vec![device_id],
        };
        assert!(lifecycle.transition_to(uploading_state).is_ok());

        // Valid transition: Uploading -> Completed
        let completed_state = StorageState::Completed {
            blob_id: blob_id.clone(),
            locations: vec![device_id],
            operation_result: StorageOperationResult {
                bytes_transferred: 1024,
                duration_ms: 100,
                nodes_used: vec![device_id],
                metadata: BTreeMap::new(),
            },
        };
        assert!(lifecycle.transition_to(completed_state).is_ok());

        // Invalid transition: Completed -> Initiating
        let initiating_state = StorageState::Initiating {
            blob_id,
            metadata: BlobMetadata {
                size_bytes: 0,
                content_type: None,
                app_metadata: BTreeMap::new(),
                required_capabilities: vec![],
                replication_factor: 1,
                encryption_spec: EncryptionSpec {
                    algorithm: "none".to_string(),
                    key_context: "test".to_string(),
                    params: BTreeMap::new(),
                },
            },
            operation_type: StorageOperationType::Store,
        };
        assert!(lifecycle.transition_to(initiating_state).is_err());
    }
}
