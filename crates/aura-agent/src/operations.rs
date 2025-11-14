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
    evaluate_authorization, AuthorizationContext, AuthorizationRequest,
};
use aura_verify::{verify_identity_proof, IdentityProof, KeyMaterial};
use aura_wot::{CapabilitySet, TreeAuthzContext, TreeOp, TreeOpKind};
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
    _device_id: DeviceId,
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
            _device_id: device_id,
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
        _namespace: &str,
        _context: &AgentOperationContext,
    ) -> AgentResult<AgentOperationResult> {
        match operation {
            StorageOperation::Store { key, data: _ } => {
                // TODO: Integrate with actual storage handler
                Ok(AgentOperationResult::Storage {
                    result: StorageResult::Stored { key: key.clone() },
                })
            }
            StorageOperation::Retrieve { key: _ } => {
                // TODO: Integrate with actual storage handler
                Ok(AgentOperationResult::Storage {
                    result: StorageResult::Retrieved { data: None },
                })
            }
            StorageOperation::Delete { key: _ } => {
                // TODO: Integrate with actual storage handler
                Ok(AgentOperationResult::Storage {
                    result: StorageResult::Deleted,
                })
            }
            StorageOperation::List { pattern: _ } => {
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
        _operation: &TreeOp,
        _context: &AgentOperationContext,
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
        _operation: &SessionOperation,
        _context: &AgentOperationContext,
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
        _operation: &AuthenticationOperation,
        _context: &AgentOperationContext,
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
