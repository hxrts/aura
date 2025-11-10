//! Agent operations with authorization integration
//!
//! This module provides agent operations that integrate identity verification
//! with capability-based authorization using the bridge pattern from aura-protocol.
//!
//! # Overview
//!
//! The agent operations layer sits at the boundary between user-facing APIs and the
//! low-level Aura platform services. It provides:
//!
//! 1. **Unified Operation Interface**: A single entry point for all device operations
//! 2. **Integrated Authorization**: Every operation goes through authentication + authorization
//! 3. **Type-Safe Operations**: Strongly-typed operation definitions with capability mapping
//! 4. **Error Handling**: Comprehensive error reporting for failed operations
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
//! │   Application   │    │     Agent       │    │   Platform      │
//! │   API Layer     │───▶│   Operations    │───▶│   Services      │
//! │                 │    │                 │    │                 │
//! └─────────────────┘    └─────────────────┘    └─────────────────┘
//!                                │
//!                                │
//!          ┌─────────────────────┼─────────────────────┐
//!          │                     │                     │
//!          ▼                     ▼                     ▼
//! ┌─────────────────┐   ┌─────────────────┐   ┌─────────────────┐
//! │ Authentication  │   │ Authorization   │   │ Operation       │
//! │                 │   │    Bridge       │   │ Handlers        │
//! │ • Identity      │   │                 │   │                 │
//! │   Verification  │   │ • Capability    │   │ • Storage       │
//! │ • Signature     │   │   Evaluation    │   │ • Tree/Journal  │
//! │   Checking      │   │ • Policy        │   │ • Sessions      │
//! │ • Key Material  │   │   Enforcement   │   │ • Auth          │
//! └─────────────────┘   └─────────────────┘   └─────────────────┘
//! ```
//!
//! # Core Components
//!
//! ## Operation Types
//!
//! - **Storage Operations**: Key-value storage with namespace isolation
//! - **Tree Operations**: Journal/tree modifications (adding devices, rotating keys, etc.)
//! - **Session Operations**: Multi-device choreographic protocol coordination
//! - **Authentication Operations**: Identity verification and biometric enrollment
//!
//! ## Authorization Integration
//!
//! - **Capability Mapping**: Each operation type maps to specific capability requirements
//! - **Authorization Bridge**: Uses aura-protocol's authorization bridge for unified access control
//! - **Context Binding**: Operations are bound to specific accounts, devices, and sessions
//!
//! # Usage Patterns
//!
//! ## Basic Operation Execution
//!
//! ```rust,no_run
//! use aura_agent::operations::{
//!     AuthorizedAgentOperations, AgentOperationRequest, AgentOperation,
//!     AgentOperationContext, StorageOperation
//! };
//! use aura_verify::{IdentityProof, KeyMaterial};
//! use aura_wot::TreeAuthzContext;
//! use aura_core::{AccountId, DeviceId};
//! use std::collections::HashMap;
//!
//! async fn execute_storage_operation(
//!     agent: &AuthorizedAgentOperations,
//!     device_id: DeviceId,
//!     account_id: AccountId,
//! ) -> Result<(), Box<dyn std::error::Error>> {
//!     // 1. Create identity proof (would normally come from signature)
//!     let identity_proof = IdentityProof::Device {
//!         device_id,
//!         signature: get_device_signature(&device_id).await?,
//!     };
//!
//!     // 2. Define the storage operation
//!     let storage_op = AgentOperation::Storage {
//!         operation: StorageOperation::Store {
//!             key: "user_preferences".to_string(),
//!             data: b"dark_mode=true".to_vec(),
//!         },
//!         namespace: "settings".to_string(),
//!     };
//!
//!     // 3. Create operation context
//!     let context = AgentOperationContext {
//!         account_id,
//!         target_device: Some(device_id),
//!         session_id: None,
//!         metadata: HashMap::new(),
//!     };
//!
//!     // 4. Create operation request
//!     let request = AgentOperationRequest {
//!         identity_proof,
//!         operation: storage_op,
//!         signed_message: create_signed_message("store operation").await?,
//!         context,
//!     };
//!
//!     // 5. Execute with automatic authentication and authorization
//!     let result = agent.execute_operation(request).await?;
//!     println!("Operation result: {:?}", result);
//!
//!     Ok(())
//! }
//! # async fn get_device_signature(device_id: &DeviceId) -> Result<aura_crypto::Ed25519Signature, Box<dyn std::error::Error>> { unimplemented!() }
//! # async fn create_signed_message(msg: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> { unimplemented!() }
//! ```
//!
//! ## Multi-Device Session Coordination
//!
//! ```rust,no_run
//! use aura_agent::operations::{
//!     AuthorizedAgentOperations, AgentOperation, SessionOperation,
//!     AgentOperationRequest, AgentOperationContext
//! };
//! use aura_verify::IdentityProof;
//! use aura_core::{AccountId, DeviceId};
//!
//! async fn coordinate_multi_device_session(
//!     agent: &AuthorizedAgentOperations,
//!     organizer_device: DeviceId,
//!     participant_devices: Vec<DeviceId>,
//!     account_id: AccountId,
//! ) -> Result<String, Box<dyn std::error::Error>> {
//!     // Create session with multiple participants
//!     let create_session_op = AgentOperation::Session {
//!         operation: SessionOperation::Create {
//!             session_type: "key_rotation".to_string(),
//!             participants: participant_devices.clone(),
//!         },
//!     };
//!
//!     let context = AgentOperationContext {
//!         account_id,
//!         target_device: Some(organizer_device),
//!         session_id: None,
//!         metadata: [
//!             ("session_purpose".to_string(), serde_json::Value::String("FROST key rotation".to_string())),
//!             ("participant_count".to_string(), serde_json::Value::Number(participant_devices.len().into())),
//!         ].into_iter().collect(),
//!     };
//!
//!     let request = AgentOperationRequest {
//!         identity_proof: IdentityProof::Device {
//!             device_id: organizer_device,
//!             signature: get_device_signature(&organizer_device).await?,
//!         },
//!         operation: create_session_op,
//!         signed_message: create_session_message(&participant_devices).await?,
//!         context,
//!     };
//!
//!     // Execute session creation
//!     let result = agent.execute_operation(request).await?;
//!     
//!     match result {
//!         AgentOperationResult::Session { result } => match result {
//!             SessionResult::Success { session_id } => {
//!                 println!("Session created: {}", session_id);
//!                 Ok(session_id)
//!             },
//!             _ => Err("Unexpected session result".into()),
//!         },
//!         _ => Err("Expected session result".into()),
//!     }
//! }
//! # async fn get_device_signature(device_id: &DeviceId) -> Result<aura_crypto::Ed25519Signature, Box<dyn std::error::Error>> { unimplemented!() }
//! # async fn create_session_message(devices: &[DeviceId]) -> Result<Vec<u8>, Box<dyn std::error::Error>> { unimplemented!() }
//! ```
//!
//! ## Tree Operation with Guardian Approval
//!
//! ```rust,no_run
//! use aura_agent::operations::{
//!     AuthorizedAgentOperations, AgentOperation, AgentOperationRequest, AgentOperationContext
//! };
//! use aura_verify::IdentityProof;
//! use aura_wot::{TreeOp, TreeOpKind};
//! use aura_core::{AccountId, DeviceId, GuardianId};
//! use std::collections::BTreeSet;
//!
//! async fn add_device_with_guardian_approval(
//!     agent: &AuthorizedAgentOperations,
//!     requesting_device: DeviceId,
//!     new_device: DeviceId,
//!     guardian_approvals: Vec<GuardianId>,
//!     account_id: AccountId,
//! ) -> Result<(), Box<dyn std::error::Error>> {
//!     // Create tree operation to add new device
//!     let tree_op = TreeOp {
//!         parent_epoch: get_current_epoch(account_id).await?,
//!         parent_commitment: get_tree_commitment(account_id).await?,
//!         op: TreeOpKind::AddLeaf {
//!             leaf_id: get_next_leaf_id(account_id).await?,
//!             role: aura_wot::LeafRole::Device,
//!             under: 0, // Root node
//!         },
//!         version: 1,
//!     };
//!
//!     let context = AgentOperationContext {
//!         account_id,
//!         target_device: Some(new_device),
//!         session_id: None,
//!         metadata: [
//!             ("operation_type".to_string(), serde_json::Value::String("add_device".to_string())),
//!             ("guardian_count".to_string(), serde_json::Value::Number(guardian_approvals.len().into())),
//!         ].into_iter().collect(),
//!     };
//!
//!     let request = AgentOperationRequest {
//!         identity_proof: IdentityProof::Device {
//!             device_id: requesting_device,
//!             signature: get_device_signature(&requesting_device).await?,
//!         },
//!         operation: AgentOperation::TreeOperation { operation: tree_op },
//!         signed_message: create_tree_operation_message(&new_device).await?,
//!         context,
//!     };
//!
//!     // Execute tree operation (requires tree:write and tree:propose capabilities)
//!     let result = agent.execute_operation(request).await?;
//!     
//!     match result {
//!         AgentOperationResult::Tree { result } => {
//!             println!("Tree operation submitted: {:?}", result);
//!             Ok(())
//!         },
//!         _ => Err("Expected tree operation result".into()),
//!     }
//! }
//! # async fn get_current_epoch(account_id: AccountId) -> Result<u64, Box<dyn std::error::Error>> { Ok(1) }
//! # async fn get_tree_commitment(account_id: AccountId) -> Result<[u8; 32], Box<dyn std::error::Error>> { Ok([0u8; 32]) }
//! # async fn get_next_leaf_id(account_id: AccountId) -> Result<u32, Box<dyn std::error::Error>> { Ok(1) }
//! # async fn get_device_signature(device_id: &DeviceId) -> Result<aura_crypto::Ed25519Signature, Box<dyn std::error::Error>> { unimplemented!() }
//! # async fn create_tree_operation_message(device_id: &DeviceId) -> Result<Vec<u8>, Box<dyn std::error::Error>> { unimplemented!() }
//! ```
//!
//! ## Biometric Authentication Setup
//!
//! ```rust,no_run
//! use aura_agent::operations::{
//!     AuthorizedAgentOperations, AgentOperation, AuthenticationOperation,
//!     AgentOperationRequest, AgentOperationContext
//! };
//! use aura_verify::IdentityProof;
//! use aura_core::{AccountId, DeviceId};
//!
//! async fn setup_biometric_authentication(
//!     agent: &AuthorizedAgentOperations,
//!     device_id: DeviceId,
//!     account_id: AccountId,
//!     biometric_type: &str,
//! ) -> Result<(), Box<dyn std::error::Error>> {
//!     let auth_op = AgentOperation::Authentication {
//!         operation: AuthenticationOperation::EnrollBiometric {
//!             biometric_type: biometric_type.to_string(),
//!         },
//!     };
//!
//!     let context = AgentOperationContext {
//!         account_id,
//!         target_device: Some(device_id),
//!         session_id: None,
//!         metadata: [
//!             ("biometric_type".to_string(), serde_json::Value::String(biometric_type.to_string())),
//!             ("enrollment_timestamp".to_string(),
//!              serde_json::Value::Number(std::time::SystemTime::now()
//!                  .duration_since(std::time::UNIX_EPOCH)
//!                  .unwrap()
//!                  .as_secs().into())),
//!         ].into_iter().collect(),
//!     };
//!
//!     let request = AgentOperationRequest {
//!         identity_proof: IdentityProof::Device {
//!             device_id,
//!             signature: get_device_signature(&device_id).await?,
//!         },
//!         operation: auth_op,
//!         signed_message: create_biometric_enrollment_message(biometric_type).await?,
//!         context,
//!     };
//!
//!     // Authentication operations are self-authorizing (no additional capabilities required)
//!     let result = agent.execute_operation(request).await?;
//!     
//!     match result {
//!         AgentOperationResult::Authentication { result } => {
//!             println!("Biometric enrollment result: {:?}", result);
//!             Ok(())
//!         },
//!         _ => Err("Expected authentication result".into()),
//!     }
//! }
//! # async fn get_device_signature(device_id: &DeviceId) -> Result<aura_crypto::Ed25519Signature, Box<dyn std::error::Error>> { unimplemented!() }
//! # async fn create_biometric_enrollment_message(biometric_type: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> { unimplemented!() }
//! ```
//!
//! # Capability-Based Authorization
//!
//! ## Operation Capability Mapping
//!
//! Each operation type is automatically mapped to required capabilities:
//!
//! ```text
//! Storage Operations:
//! ├─ Store/Delete      → storage:write
//! ├─ Retrieve/List     → storage:read
//! └─ ClearNamespace    → storage:admin
//!
//! Tree Operations:
//! └─ All operations    → tree:write + tree:propose
//!
//! Session Operations:
//! ├─ Create            → session:create
//! ├─ Join              → session:join
//! └─ End/UpdateMeta    → session:manage
//!
//! Authentication Operations:
//! └─ All operations    → (self-authorizing)
//! ```
//!
//! ## Custom Capability Requirements
//!
//! ```rust,no_run
//! use aura_agent::operations::AuthorizedAgentOperations;
//! use aura_wot::CapabilitySet;
//!
//! // Example of how capability mapping works internally
//! impl AuthorizedAgentOperations {
//!     fn get_enhanced_operation_capabilities(&self, operation: &AgentOperation) -> CapabilitySet {
//!         let base_caps = self.get_operation_capabilities(operation);
//!         
//!         // Add additional capabilities based on operation context
//!         match operation {
//!             AgentOperation::Storage { namespace, .. } => {
//!                 if namespace == "system" {
//!                     // System namespace requires admin capabilities
//!                     base_caps.union(&CapabilitySet::from_permissions(&["storage:admin"]))
//!                 } else {
//!                     base_caps
//!                 }
//!             },
//!             AgentOperation::TreeOperation { operation } => {
//!                 match &operation.op {
//!                     TreeOpKind::AddLeaf { role, .. } => {
//!                         if matches!(role, aura_wot::LeafRole::Guardian) {
//!                             // Adding guardians requires special permissions
//!                             base_caps.union(&CapabilitySet::from_permissions(&["tree:admin"]))
//!                         } else {
//!                             base_caps
//!                         }
//!                     },
//!                     _ => base_caps,
//!                 }
//!             },
//!             _ => base_caps,
//!         }
//!     }
//! }
//! # use aura_agent::operations::AgentOperation;
//! # use aura_wot::TreeOpKind;
//! ```
//!
//! # Error Handling
//!
//! ```rust,no_run
//! use aura_agent::operations::{AuthorizedAgentOperations, AgentOperationRequest};
//! use aura_agent::errors::AuraError;
//!
//! async fn handle_operation_errors(
//!     agent: &AuthorizedAgentOperations,
//!     request: AgentOperationRequest,
//! ) {
//!     match agent.execute_operation(request).await {
//!         Ok(result) => {
//!             println!("Operation succeeded: {:?}", result);
//!         },
//!         Err(AuraError::AuthenticationFailed(msg)) => {
//!             eprintln!("Authentication failed: {}", msg);
//!             // Handle invalid signatures, expired keys, malformed proofs
//!         },
//!         Err(AuraError::AuthorizationFailed(msg)) => {
//!             eprintln!("Authorization failed: {}", msg);
//!             // Handle insufficient permissions, policy violations
//!         },
//!         Err(AuraError::PermissionDenied(msg)) => {
//!             eprintln!("Permission denied: {}", msg);
//!             // Handle capability evaluation failures
//!         },
//!         Err(AuraError::OperationFailed(msg)) => {
//!             eprintln!("Operation execution failed: {}", msg);
//!             // Handle storage errors, network failures, etc.
//!         },
//!         Err(e) => {
//!             eprintln!("Unexpected error: {:?}", e);
//!         }
//!     }
//! }
//! ```
//!
//! # Performance Optimization
//!
//! ## Capability Caching
//!
//! ```rust,no_run
//! use std::collections::HashMap;
//! use std::time::{SystemTime, Duration};
//! use aura_core::DeviceId;
//! use aura_wot::CapabilitySet;
//!
//! pub struct CapabilityCacheEntry {
//!     capabilities: CapabilitySet,
//!     cached_at: SystemTime,
//!     ttl: Duration,
//! }
//!
//! pub struct AgentCapabilityCache {
//!     device_capabilities: HashMap<DeviceId, CapabilityCacheEntry>,
//!     default_ttl: Duration,
//! }
//!
//! impl AgentCapabilityCache {
//!     pub fn get_capabilities(&self, device_id: &DeviceId) -> Option<&CapabilitySet> {
//!         if let Some(entry) = self.device_capabilities.get(device_id) {
//!             if entry.cached_at.elapsed().unwrap_or(Duration::MAX) < entry.ttl {
//!                 return Some(&entry.capabilities);
//!             }
//!         }
//!         None
//!     }
//!     
//!     pub fn cache_capabilities(&mut self, device_id: DeviceId, capabilities: CapabilitySet) {
//!         self.device_capabilities.insert(device_id, CapabilityCacheEntry {
//!             capabilities,
//!             cached_at: SystemTime::now(),
//!             ttl: self.default_ttl,
//!         });
//!     }
//! }
//! ```
//!
//! ## Batch Operation Processing
//!
//! ```rust,no_run
//! use aura_agent::operations::{AuthorizedAgentOperations, AgentOperationRequest};
//!
//! impl AuthorizedAgentOperations {
//!     pub async fn execute_batch_operations(
//!         &self,
//!         requests: Vec<AgentOperationRequest>,
//!     ) -> Vec<aura_agent::errors::Result<aura_agent::operations::AgentOperationResult>> {
//!         // Process operations in parallel where possible
//!         futures::future::join_all(
//!             requests.into_iter().map(|req| self.execute_operation(req))
//!         ).await
//!     }
//!     
//!     pub async fn execute_batch_with_transaction(
//!         &self,
//!         requests: Vec<AgentOperationRequest>,
//!     ) -> aura_agent::errors::Result<Vec<aura_agent::operations::AgentOperationResult>> {
//!         // Execute all operations as a single transaction
//!         // All succeed or all fail
//!         let mut results = Vec::new();
//!         
//!         // TODO: Implement transaction boundaries
//!         for request in requests {
//!             match self.execute_operation(request).await {
//!                 Ok(result) => results.push(result),
//!                 Err(e) => {
//!                     // Rollback all previous operations
//!                     return Err(e);
//!                 }
//!             }
//!         }
//!         
//!         Ok(results)
//!     }
//! }
//! ```
//!
//! # Security Considerations
//!
//! ## Operation Binding
//!
//! - **Message Binding**: All operations must be bound to signed messages
//! - **Context Validation**: Operation context must match identity proof context
//! - **Replay Protection**: Include operation nonces and timestamps
//!
//! ## Capability Isolation
//!
//! - **Namespace Isolation**: Storage operations are isolated by namespace
//! - **Session Scoping**: Session operations are scoped to specific session contexts
//! - **Device Authorization**: Operations are bound to specific device identities
//!
//! ## Audit Trail
//!
//! - **Operation Logging**: All operations should be logged with full context
//! - **Authorization Decisions**: Record all capability evaluation results
//! - **Error Forensics**: Include sufficient detail for security analysis
//!
//! **ZERO BACKWARDS COMPATIBILITY CODE. ZERO MIGRATION CODE. ZERO LEGACY CODE.**

use crate::errors::{AuraError, Result as AgentResult};
use aura_core::{AccountId, DeviceId, GuardianId};
use aura_protocol::authorization_bridge::{
    evaluate_authorization, AuthorizationContext, AuthorizationError, AuthorizationRequest,
    AuthorizedEvent,
};
use aura_verify::{verify_identity_proof, IdentityProof, KeyMaterial};
use aura_wot::{CapabilitySet, NodeIndex, TreeAuthzContext, TreeOp, TreeOpKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Agent operation request with identity proof
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentOperationRequest {
    /// Identity proof for the requesting entity
    pub identity_proof: IdentityProof,
    /// The operation to perform
    pub operation: AgentOperation,
    /// Message that was signed (for verification)
    pub signed_message: Vec<u8>,
    /// Additional context for authorization
    pub context: AgentOperationContext,
}

/// Context for agent operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentOperationContext {
    /// Account this operation pertains to
    pub account_id: AccountId,
    /// Target device (if applicable)
    pub target_device: Option<DeviceId>,
    /// Session ID (if part of a session)
    pub session_id: Option<String>,
    /// Additional metadata
    pub metadata: std::collections::HashMap<String, serde_json::Value>,
}

/// Agent operations that require authorization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentOperation {
    /// Storage operations
    Storage {
        operation: StorageOperation,
        namespace: String,
    },
    /// Tree/Journal operations
    TreeOperation { operation: TreeOp },
    /// Session operations
    Session { operation: SessionOperation },
    /// Authentication operations
    Authentication { operation: AuthenticationOperation },
}

/// Storage operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StorageOperation {
    Store { key: String, data: Vec<u8> },
    Retrieve { key: String },
    Delete { key: String },
    List { pattern: Option<String> },
    ClearNamespace,
}

/// Session operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionOperation {
    Create {
        session_type: String,
        participants: Vec<DeviceId>,
    },
    Join {
        session_id: String,
    },
    End {
        session_id: String,
    },
    UpdateMetadata {
        session_id: String,
        metadata: std::collections::HashMap<String, serde_json::Value>,
    },
}

/// Authentication operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuthenticationOperation {
    Authenticate,
    Verify { capability: Vec<u8> },
    EnrollBiometric { biometric_type: String },
    RemoveBiometric { biometric_type: String },
}

/// Agent operations handler with authorization integration
pub struct AuthorizedAgentOperations {
    /// Key material for identity verification
    key_material: KeyMaterial,
    /// Tree authorization context
    tree_context: TreeAuthzContext,
    /// Device ID for this agent
    device_id: DeviceId,
}

impl AuthorizedAgentOperations {
    /// Create new authorized agent operations handler
    pub fn new(
        key_material: KeyMaterial,
        tree_context: TreeAuthzContext,
        device_id: DeviceId,
    ) -> Self {
        Self {
            key_material,
            tree_context,
            device_id,
        }
    }

    /// Execute an agent operation with authorization
    pub async fn execute_operation(
        &self,
        request: AgentOperationRequest,
    ) -> AgentResult<AgentOperationResult> {
        // Step 1: Verify identity proof
        let verified_identity = verify_identity_proof(
            &request.identity_proof,
            &request.signed_message,
            &self.key_material,
        )
        .map_err(|e| {
            AuraError::permission_denied(format!("Identity verification failed: {}", e))
        })?;

        // Step 2: Check if operation requires authorization
        let required_capabilities = self.get_operation_capabilities(&request.operation);

        // Step 3: Evaluate authorization using bridge pattern
        if required_capabilities.capabilities().next().is_some() {
            let authz_result = self
                .authorize_operation(
                    &verified_identity,
                    &request.operation,
                    &request.context,
                    required_capabilities,
                )
                .await?;

            if !authz_result.authorized {
                return Err(AuraError::permission_denied(format!(
                    "Operation not authorized: {}",
                    authz_result
                        .denial_reason
                        .unwrap_or_else(|| "Permission denied".to_string())
                )));
            }
        }

        // Step 4: Execute the operation
        self.perform_operation(&request.operation, &request.context)
            .await
    }

    /// Get required capabilities for an operation
    fn get_operation_capabilities(&self, operation: &AgentOperation) -> CapabilitySet {
        match operation {
            AgentOperation::Storage { operation, .. } => match operation {
                StorageOperation::Store { .. } => {
                    CapabilitySet::from_permissions(&["storage:write"])
                }
                StorageOperation::Retrieve { .. } => {
                    CapabilitySet::from_permissions(&["storage:read"])
                }
                StorageOperation::Delete { .. } => {
                    CapabilitySet::from_permissions(&["storage:write"])
                }
                StorageOperation::List { .. } => CapabilitySet::from_permissions(&["storage:read"]),
                StorageOperation::ClearNamespace => {
                    CapabilitySet::from_permissions(&["storage:admin"])
                }
            },
            AgentOperation::TreeOperation { .. } => {
                CapabilitySet::from_permissions(&["tree:write", "tree:propose"])
            }
            AgentOperation::Session { operation } => match operation {
                SessionOperation::Create { .. } => {
                    CapabilitySet::from_permissions(&["session:create"])
                }
                SessionOperation::Join { .. } => CapabilitySet::from_permissions(&["session:join"]),
                SessionOperation::End { .. } => {
                    CapabilitySet::from_permissions(&["session:manage"])
                }
                SessionOperation::UpdateMetadata { .. } => {
                    CapabilitySet::from_permissions(&["session:manage"])
                }
            },
            AgentOperation::Authentication { .. } => {
                // Authentication operations typically don't require additional authorization
                // as they are self-authorizing through identity proof
                CapabilitySet::empty()
            }
        }
    }

    /// Authorize operation using bridge pattern
    async fn authorize_operation(
        &self,
        verified_identity: &aura_verify::VerifiedIdentity,
        operation: &AgentOperation,
        context: &AgentOperationContext,
        required_capabilities: CapabilitySet,
    ) -> AgentResult<aura_protocol::authorization_bridge::PermissionGrant> {
        // Convert agent operation to tree operation if applicable
        let tree_op = match operation {
            AgentOperation::TreeOperation { operation } => operation.clone(),
            _ => {
                // For non-tree operations, create a synthetic tree operation
                // This allows us to use the tree authorization model for all operations
                TreeOp {
                    parent_epoch: 1,
                    parent_commitment: [0u8; 32],
                    op: TreeOpKind::AddLeaf {
                        leaf_id: 0,
                        role: aura_wot::LeafRole::Device,
                        under: 0,
                    },
                    version: 1,
                }
            }
        };

        // Create authorization context
        let authz_context = AuthorizationContext::new(
            context.account_id,
            required_capabilities,
            self.tree_context.clone(),
        );

        // Create authorization request
        let authz_request = AuthorizationRequest {
            verified_identity: verified_identity.clone(),
            operation: tree_op,
            context: authz_context,
            additional_signers: BTreeSet::new(),
            guardian_signers: BTreeSet::new(),
        };

        // Evaluate authorization
        evaluate_authorization(authz_request).map_err(|e| {
            AuraError::permission_denied(format!("Authorization evaluation failed: {:?}", e))
        })
    }

    /// Perform the actual operation (placeholder implementation)
    async fn perform_operation(
        &self,
        operation: &AgentOperation,
        context: &AgentOperationContext,
    ) -> AgentResult<AgentOperationResult> {
        match operation {
            AgentOperation::Storage {
                operation,
                namespace,
            } => {
                self.perform_storage_operation(operation, namespace, context)
                    .await
            }
            AgentOperation::TreeOperation { operation } => {
                self.perform_tree_operation(operation, context).await
            }
            AgentOperation::Session { operation } => {
                self.perform_session_operation(operation, context).await
            }
            AgentOperation::Authentication { operation } => {
                self.perform_authentication_operation(operation, context)
                    .await
            }
        }
    }

    /// Perform storage operation (placeholder)
    async fn perform_storage_operation(
        &self,
        operation: &StorageOperation,
        namespace: &str,
        context: &AgentOperationContext,
    ) -> AgentResult<AgentOperationResult> {
        match operation {
            StorageOperation::Store { key, data } => {
                // TODO: Integrate with actual storage handler
                Ok(AgentOperationResult::Storage {
                    result: StorageResult::Stored { key: key.clone() },
                })
            }
            StorageOperation::Retrieve { key } => {
                // TODO: Integrate with actual storage handler
                Ok(AgentOperationResult::Storage {
                    result: StorageResult::Retrieved { data: None },
                })
            }
            StorageOperation::Delete { key } => {
                // TODO: Integrate with actual storage handler
                Ok(AgentOperationResult::Storage {
                    result: StorageResult::Deleted,
                })
            }
            StorageOperation::List { pattern } => {
                // TODO: Integrate with actual storage handler
                Ok(AgentOperationResult::Storage {
                    result: StorageResult::Listed { keys: vec![] },
                })
            }
            StorageOperation::ClearNamespace => {
                // TODO: Integrate with actual storage handler
                Ok(AgentOperationResult::Storage {
                    result: StorageResult::Cleared { count: 0 },
                })
            }
        }
    }

    /// Perform tree operation (placeholder)
    async fn perform_tree_operation(
        &self,
        operation: &TreeOp,
        context: &AgentOperationContext,
    ) -> AgentResult<AgentOperationResult> {
        // TODO: Integrate with actual journal/tree handler
        Ok(AgentOperationResult::Tree {
            result: TreeResult::OperationSubmitted {
                intent_id: "placeholder".to_string(),
            },
        })
    }

    /// Perform session operation (placeholder)
    async fn perform_session_operation(
        &self,
        operation: &SessionOperation,
        context: &AgentOperationContext,
    ) -> AgentResult<AgentOperationResult> {
        // TODO: Integrate with actual session handler
        Ok(AgentOperationResult::Session {
            result: SessionResult::Success {
                session_id: "placeholder".to_string(),
            },
        })
    }

    /// Perform authentication operation (placeholder)
    async fn perform_authentication_operation(
        &self,
        operation: &AuthenticationOperation,
        context: &AgentOperationContext,
    ) -> AgentResult<AgentOperationResult> {
        // TODO: Integrate with actual authentication handler
        Ok(AgentOperationResult::Authentication {
            result: AuthResult::Success,
        })
    }

    /// Update key material with new device key
    pub fn add_device_key(
        &mut self,
        device_id: DeviceId,
        public_key: aura_crypto::Ed25519VerifyingKey,
    ) {
        self.key_material.add_device_key(device_id, public_key);
    }

    /// Update key material with new guardian key
    pub fn add_guardian_key(
        &mut self,
        guardian_id: GuardianId,
        public_key: aura_crypto::Ed25519VerifyingKey,
    ) {
        self.key_material.add_guardian_key(guardian_id, public_key);
    }
}

/// Result of an agent operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentOperationResult {
    Storage { result: StorageResult },
    Tree { result: TreeResult },
    Session { result: SessionResult },
    Authentication { result: AuthResult },
}

/// Storage operation results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StorageResult {
    Stored { key: String },
    Retrieved { data: Option<Vec<u8>> },
    Deleted,
    Listed { keys: Vec<String> },
    Cleared { count: usize },
}

/// Tree operation results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TreeResult {
    OperationSubmitted { intent_id: String },
    DeviceAdded { leaf_index: u32 },
    DeviceRemoved,
    KeysRotated,
}

/// Session operation results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionResult {
    Success { session_id: String },
    Joined { role: String },
    Ended,
    Updated,
}

/// Authentication operation results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuthResult {
    Success,
    TokenGenerated { token: Vec<u8> },
    Verified { valid: bool },
    BiometricEnrolled,
    BiometricRemoved,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_operation_capabilities_mapping() {
        let auth_ops = create_test_handler();

        // Test storage operations
        let store_op = AgentOperation::Storage {
            operation: StorageOperation::Store {
                key: "test".to_string(),
                data: vec![1, 2, 3],
            },
            namespace: "test".to_string(),
        };
        let caps = auth_ops.get_operation_capabilities(&store_op);
        assert!(caps.permits("storage:write"));

        // Test tree operations
        let tree_op = AgentOperation::TreeOperation {
            operation: TreeOp {
                parent_epoch: 1,
                parent_commitment: [0u8; 32],
                op: TreeOpKind::AddLeaf {
                    leaf_id: 0,
                    role: aura_wot::LeafRole::Device,
                    under: 0,
                },
                version: 1,
            },
        };
        let caps = auth_ops.get_operation_capabilities(&tree_op);
        assert!(caps.permits("tree:write"));
        assert!(caps.permits("tree:propose"));

        // Test authentication operations (should be empty)
        let auth_op = AgentOperation::Authentication {
            operation: AuthenticationOperation::Authenticate,
        };
        let caps = auth_ops.get_operation_capabilities(&auth_op);
        assert_eq!(caps, CapabilitySet::empty());
    }

    fn create_test_handler() -> AuthorizedAgentOperations {
        let device_id = DeviceId::from_bytes([1u8; 32]);
        let account_id = AccountId::from_bytes([1u8; 32]);

        AuthorizedAgentOperations::new(
            KeyMaterial::new(),
            TreeAuthzContext::new(account_id, 1),
            device_id,
        )
    }
}
