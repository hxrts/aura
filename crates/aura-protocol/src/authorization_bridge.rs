//! Authorization Bridge - Connecting Authentication with Authorization
//!
//! This module provides the bridge layer that connects pure identity verification
//! from aura-verify with capability-based authorization from aura-wot.
//!
//! # Overview
//!
//! The authorization bridge combines two critical security layers:
//!
//! 1. **Verification** (aura-verify): Proves "who you are" through cryptographic identity verification
//! 2. **Authorization** (aura-wot): Proves "what you can do" through capability-based access control
//!
//! The bridge ensures that both layers work together seamlessly, providing complete access control
//! for all operations in the Aura platform.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
//! │   Application   │    │ Authorization   │    │  Tree Operation │
//! │   Operation     │───▶│    Bridge       │───▶│   Execution     │
//! │                 │    │                 │    │                 │
//! └─────────────────┘    └─────────────────┘    └─────────────────┘
//!                                │
//!                                │
//!          ┌─────────────────────┼─────────────────────┐
//!          │                     │                     │
//!          ▼                     ▼                     ▼
//! ┌─────────────────┐   ┌─────────────────┐   ┌─────────────────┐
//! │ aura-verify       │ │   aura-wot      │   │   Tree State    │
//! │                 │   │                 │   │                 │
//! │ • Identity      │   │ • Capabilities  │   │ • Tree Context  │
//! │   Verification  │   │ • Policy Meet   │   │ • Node Roles    │
//! │ • Cryptographic │   │ • Authorization │   │ • Epochs        │
//! │   Proof         │   │   Evaluation    │   │                 │
//! └─────────────────┘   └─────────────────┘   └─────────────────┘
//! ```
//!
//! # Core Components
//!
//! ## Authentication Integration
//!
//! - **Identity Verification**: Validates cryptographic proofs (Ed25519 signatures, threshold signatures)
//! - **Message Binding**: Ensures proof is bound to specific operation message
//! - **Key Material**: Manages public keys and verification contexts
//!
//! ## Authorization Integration
//!
//! - **Capability Evaluation**: Checks fine-grained permissions using meet-semilattice operations
//! - **Policy Enforcement**: Applies local and global policies
//! - **Context Binding**: Connects authorization decisions to tree operations
//!
//! # Usage Patterns
//!
//! ## Basic Authorization Flow
//!
//! ```rust,no_run
//! use aura_protocol::authorization_bridge::{
//!     evaluate_authorization, AuthorizationRequest, AuthorizationContext,
//! };
//! use aura_verify::{IdentityProof, VerifiedIdentity};
//! use aura_wot::{CapabilitySet, TreeAuthzContext, TreeOp, TreeOpKind};
//! use aura_core::{AccountId, DeviceId};
//! use std::collections::BTreeSet;
//!
//! async fn authorize_tree_operation(
//!     device_id: DeviceId,
//!     account_id: AccountId,
//!     operation: TreeOp,
//! ) -> Result<(), Box<dyn std::error::Error>> {
//!     // 1. Create verified identity (from prior authentication)
//!     let verified_identity = VerifiedIdentity {
//!         proof: IdentityProof::Device {
//!             device_id,
//!             signature: get_signature_for_operation(&operation).await?,
//!         },
//!         message_hash: hash_operation(&operation),
//!     };
//!
//!     // 2. Set up authorization context
//!     let base_capabilities = CapabilitySet::from_permissions(&[
//!         "tree:read",
//!         "tree:propose",
//!         "tree:modify",
//!     ]);
//!
//!     let tree_context = TreeAuthzContext::new(account_id, operation.parent_epoch);
//!     let authz_context = AuthorizationContext::new(
//!         account_id,
//!         base_capabilities,
//!         tree_context,
//!     );
//!
//!     // 3. Create authorization request
//!     let request = AuthorizationRequest {
//!         verified_identity,
//!         operation,
//!         context: authz_context,
//!         additional_signers: BTreeSet::new(),
//!         guardian_signers: BTreeSet::new(),
//!     };
//!
//!     // 4. Evaluate authorization
//!     let grant = evaluate_authorization(request)?;
//!
//!     if grant.authorized {
//!         println!("Operation authorized with capabilities: {:?}",
//!                  grant.effective_capabilities);
//!         Ok(())
//!     } else {
//!         Err(format!("Authorization denied: {}",
//!                     grant.denial_reason.unwrap_or_default()).into())
//!     }
//! }
//! # async fn get_signature_for_operation(op: &TreeOp) -> Result<aura_crypto::Ed25519Signature, Box<dyn std::error::Error>> { unimplemented!() }
//! # fn hash_operation(op: &TreeOp) -> [u8; 32] { [0u8; 32] }
//! ```
//!
//! ## Combined Authentication and Authorization
//!
//! ```rust,no_run
//! use aura_protocol::authorization_bridge::{authenticate_and_authorize, AuthorizationContext};
//! use aura_verify::{IdentityProof, KeyMaterial};
//! use aura_wot::{CapabilitySet, TreeAuthzContext, TreeOp};
//! use aura_core::{AccountId, DeviceId, GuardianId};
//! use std::collections::BTreeSet;
//!
//! async fn complete_authorization_flow(
//!     identity_proof: IdentityProof,
//!     operation_message: &[u8],
//!     key_material: &KeyMaterial,
//!     account_id: AccountId,
//!     operation: TreeOp,
//! ) -> Result<(), Box<dyn std::error::Error>> {
//!     // Set up authorization context
//!     let base_capabilities = CapabilitySet::device_default();
//!     let tree_context = TreeAuthzContext::new(account_id, operation.parent_epoch);
//!     let authz_context = AuthorizationContext::new(
//!         account_id,
//!         base_capabilities,
//!         tree_context,
//!     );
//!
//!     // Perform combined authentication and authorization
//!     let grant = authenticate_and_authorize(
//!         identity_proof,
//!         operation_message,
//!         key_material,
//!         authz_context,
//!         operation,
//!         BTreeSet::new(), // No additional signers
//!         BTreeSet::new(), // No guardian signers
//!     )?;
//!
//!     if grant.authorized {
//!         println!("Both authentication and authorization succeeded!");
//!         println!("Identity: {:?}", grant.authorized_identity);
//!         println!("Capabilities: {:?}", grant.effective_capabilities);
//!         Ok(())
//!     } else {
//!         Err(format!("Access denied: {}",
//!                     grant.denial_reason.unwrap_or_default()).into())
//!     }
//! }
//! ```
//!
//! ## Guardian-Based Recovery Authorization
//!
//! ```rust,no_run
//! use aura_protocol::authorization_bridge::{AuthorizationRequest, AuthorizationContext};
//! use aura_verify::{IdentityProof, VerifiedIdentity};
//! use aura_wot::{CapabilitySet, TreeAuthzContext, TreeOp, TreeOpKind};
//! use aura_core::{AccountId, DeviceId, GuardianId};
//! use std::collections::BTreeSet;
//!
//! async fn authorize_recovery_operation(
//!     guardian_identities: Vec<VerifiedIdentity>,
//!     account_id: AccountId,
//!     recovery_operation: TreeOp,
//! ) -> Result<(), Box<dyn std::error::Error>> {
//!     // Extract guardian signers
//!     let mut guardian_signers = BTreeSet::new();
//!     let mut primary_identity = None;
//!
//!     for identity in guardian_identities {
//!         if let IdentityProof::Guardian { guardian_id, .. } = &identity.proof {
//!             guardian_signers.insert(*guardian_id);
//!             if primary_identity.is_none() {
//!                 primary_identity = Some(identity);
//!             }
//!         }
//!     }
//!
//!     let primary_identity = primary_identity.ok_or("No guardian identity found")?;
//!
//!     // Set up recovery capabilities
//!     let recovery_capabilities = CapabilitySet::from_permissions(&[
//!         "tree:recovery",
//!         "tree:rotate_epoch",
//!         "tree:modify",
//!     ]);
//!
//!     let tree_context = TreeAuthzContext::recovery_context(account_id);
//!     let authz_context = AuthorizationContext::new(
//!         account_id,
//!         recovery_capabilities,
//!         tree_context,
//!     );
//!
//!     // Create recovery authorization request
//!     let request = AuthorizationRequest {
//!         verified_identity: primary_identity,
//!         operation: recovery_operation,
//!         context: authz_context,
//!         additional_signers: BTreeSet::new(),
//!         guardian_signers,
//!     };
//!
//!     let grant = evaluate_authorization(request)?;
//!
//!     if grant.authorized {
//!         println!("Recovery operation authorized by {} guardians",
//!                  request.guardian_signers.len());
//!         Ok(())
//!     } else {
//!         Err("Recovery authorization failed".into())
//!     }
//! }
//! ```
//!
//! ## Threshold Signature Authorization
//!
//! ```rust,no_run
//! use aura_protocol::authorization_bridge::{AuthorizationContext, evaluate_authorization};
//! use aura_verify::{IdentityProof, VerifiedIdentity, ThresholdSig};
//! use aura_wot::{CapabilitySet, TreeAuthzContext, TreeOp};
//! use aura_core::{AccountId, DeviceId};
//! use std::collections::BTreeSet;
//!
//! async fn authorize_threshold_operation(
//!     threshold_proof: ThresholdSig,
//!     participating_devices: BTreeSet<DeviceId>,
//!     account_id: AccountId,
//!     operation: TreeOp,
//! ) -> Result<(), Box<dyn std::error::Error>> {
//!     // Create verified threshold identity
//!     let verified_identity = VerifiedIdentity {
//!         proof: IdentityProof::Threshold(threshold_proof),
//!         message_hash: hash_operation(&operation),
//!     };
//!
//!     // Elevated capabilities for threshold operations
//!     let threshold_capabilities = CapabilitySet::from_permissions(&[
//!         "tree:read",
//!         "tree:modify",
//!         "tree:rotate_epoch",
//!         "tree:add_device",
//!         "tree:remove_device",
//!     ]);
//!
//!     let tree_context = TreeAuthzContext::new(account_id, operation.parent_epoch);
//!     let authz_context = AuthorizationContext::new(
//!         account_id,
//!         threshold_capabilities,
//!         tree_context,
//!     );
//!
//!     // Note: For threshold signatures, we specify additional signers
//!     // since the identity extraction may not handle threshold proofs
//!     let request = AuthorizationRequest {
//!         verified_identity,
//!         operation,
//!         context: authz_context,
//!         additional_signers: participating_devices,
//!         guardian_signers: BTreeSet::new(),
//!     };
//!
//!     let grant = evaluate_authorization(request)?;
//!
//!     if grant.authorized {
//!         println!("Threshold operation authorized");
//!         Ok(())
//!     } else {
//!         Err("Threshold authorization failed".into())
//!     }
//! }
//! # fn hash_operation(op: &TreeOp) -> [u8; 32] { [0u8; 32] }
//! ```
//!
//! ## Policy-Based Authorization
//!
//! ```rust,no_run
//! use aura_protocol::authorization_bridge::{AuthorizationContext, evaluate_authorization};
//! use aura_wot::{CapabilitySet, TreeAuthzContext};
//! use aura_core::AccountId;
//!
//! async fn authorize_with_local_policy(
//!     account_id: AccountId,
//! ) -> Result<AuthorizationContext, Box<dyn std::error::Error>> {
//!     // Define base capabilities for this device
//!     let base_capabilities = CapabilitySet::from_permissions(&[
//!         "tree:read",
//!         "tree:propose",
//!         "tree:modify",
//!         "storage:read",
//!         "storage:write",
//!     ]);
//!
//!     // Define local policy constraints (more restrictive)
//!     let local_policy = CapabilitySet::from_permissions(&[
//!         "tree:read",        // Allow reading
//!         "tree:propose",     // Allow proposing
//!         // "tree:modify",   // Deny modification locally
//!         "storage:read",     // Allow storage read
//!         // "storage:write", // Deny storage write locally
//!     ]);
//!
//!     let tree_context = TreeAuthzContext::new(account_id, 1);
//!
//!     // Create context with local policy restrictions
//!     let authz_context = AuthorizationContext::new(
//!         account_id,
//!         base_capabilities,
//!         tree_context,
//!     ).with_local_policy(local_policy);
//!
//!     // The effective capabilities will be the meet of base and local policy
//!     // Result: ["tree:read", "tree:propose", "storage:read"]
//!
//!     Ok(authz_context)
//! }
//! ```
//!
//! # Error Handling
//!
//! ```rust,no_run
//! use aura_protocol::authorization_bridge::{
//!     authenticate_and_authorize, AuthorizationError, AuthorizationContext
//! };
//! use aura_verify::IdentityProof;
//!
//! async fn handle_authorization_errors(
//!     identity_proof: IdentityProof,
//!     message: &[u8],
//!     // ... other parameters
//! ) {
//!     match authenticate_and_authorize(
//!         identity_proof,
//!         message,
//!         &key_material,
//!         authz_context,
//!         operation,
//!         additional_signers,
//!         guardian_signers,
//!     ) {
//!         Ok(grant) => {
//!             if grant.authorized {
//!                 println!("Success: {:?}", grant.effective_capabilities);
//!             } else {
//!                 println!("Denied: {}", grant.denial_reason.unwrap_or_default());
//!             }
//!         }
//!         Err(AuthorizationError::AuthenticationFailed(auth_err)) => {
//!             eprintln!("Authentication failed: {}", auth_err);
//!             // Handle invalid signatures, expired keys, etc.
//!         }
//!         Err(AuthorizationError::CapabilityEvaluationFailed(cap_err)) => {
//!             eprintln!("Authorization failed: {}", cap_err);
//!             // Handle insufficient permissions, policy violations, etc.
//!         }
//!         Err(AuthorizationError::UnsupportedIdentityType(msg)) => {
//!             eprintln!("Unsupported identity: {}", msg);
//!             // Handle threshold signatures or other complex identity types
//!         }
//!         Err(AuthorizationError::InvalidRequest(msg)) => {
//!             eprintln!("Invalid request: {}", msg);
//!             // Handle malformed requests or missing context
//!         }
//!     }
//! }
//! # let key_material = unimplemented!();
//! # let authz_context: AuthorizationContext = unimplemented!();
//! # let operation = unimplemented!();
//! # let additional_signers = unimplemented!();
//! # let guardian_signers = unimplemented!();
//! ```
//!
//! # Security Considerations
//!
//! ## Time-Based Attacks
//!
//! - **Replay Protection**: All authorization requests should include operation-specific nonces
//! - **Temporal Validity**: Consider implementing time-bounded authorization grants
//! - **Epoch Validation**: Ensure operations reference current tree epoch
//!
//! ## Privilege Escalation
//!
//! - **Capability Meet**: Always use meet operation for policy combination (intersection, not union)
//! - **Context Isolation**: Different operation contexts should have isolated capability evaluation
//! - **Guardian Thresholds**: Enforce minimum guardian requirements for recovery operations
//!
//! ## Side-Channel Resistance
//!
//! - **Constant-Time Evaluation**: Capability evaluation should not leak timing information
//! - **Error Uniformity**: Authorization errors should not reveal internal state
//! - **Audit Logging**: All authorization decisions should be logged for security analysis
//!
//! # Performance Optimization
//!
//! ## Capability Caching
//!
//! ```rust,no_run
//! use std::collections::HashMap;
//! use aura_core::DeviceId;
//! use aura_wot::CapabilitySet;
//!
//! pub struct CapabilityCache {
//!     device_capabilities: HashMap<DeviceId, (CapabilitySet, std::time::SystemTime)>,
//!     cache_ttl: std::time::Duration,
//! }
//!
//! impl CapabilityCache {
//!     pub fn get_capabilities(&self, device_id: &DeviceId) -> Option<&CapabilitySet> {
//!         if let Some((caps, cached_at)) = self.device_capabilities.get(device_id) {
//!             if cached_at.elapsed().ok()? < self.cache_ttl {
//!                 return Some(caps);
//!             }
//!         }
//!         None
//!     }
//! }
//! ```
//!
//! ## Batch Authorization
//!
//! For multiple operations, batch authorization requests to reduce overhead:
//!
//! ```rust,no_run
//! use aura_protocol::authorization_bridge::AuthorizationRequest;
//! use aura_wot::TreeOp;
//!
//! async fn batch_authorize(
//!     requests: Vec<AuthorizationRequest>
//! ) -> Vec<Result<aura_protocol::authorization_bridge::PermissionGrant,
//!                aura_protocol::authorization_bridge::AuthorizationError>> {
//!     // Process requests in parallel where possible
//!     futures::future::join_all(
//!         requests.into_iter().map(evaluate_authorization)
//!     ).await
//! }
//! # use aura_protocol::authorization_bridge::evaluate_authorization;
//! ```
//!
//! **ZERO BACKWARDS COMPATIBILITY CODE. ZERO MIGRATION CODE. ZERO LEGACY CODE.**

use aura_core::{AccountId, DeviceId, GuardianId};
use aura_verify::{IdentityProof, KeyMaterial, VerifiedIdentity};
use aura_wot::{
    evaluate_tree_operation_capabilities, CapabilityEvaluationContext, CapabilitySet, EntityId,
    TreeAuthzContext, TreeCapabilityRequest, TreeOp, TreeOpKind, WotError,
};
use std::collections::BTreeSet;

/// Authorization request connecting identity proof with operation
#[derive(Debug, Clone)]
pub struct AuthorizationRequest {
    /// Verified identity from authentication layer
    pub verified_identity: VerifiedIdentity,
    /// Tree operation to authorize
    pub operation: TreeOp,
    /// Context for authorization evaluation
    pub context: AuthorizationContext,
    /// Additional signers for threshold operations
    pub additional_signers: BTreeSet<DeviceId>,
    /// Guardian signers for recovery operations
    pub guardian_signers: BTreeSet<GuardianId>,
}

/// Context for authorization evaluation
#[derive(Debug, Clone)]
pub struct AuthorizationContext {
    /// Account this operation affects
    pub account_id: AccountId,
    /// Base capabilities for the requesting entity
    pub base_capabilities: CapabilitySet,
    /// Tree authorization context
    pub tree_context: TreeAuthzContext,
    /// Local policy constraints (optional)
    pub local_policy: Option<CapabilitySet>,
}

impl AuthorizationContext {
    /// Create new authorization context
    pub fn new(
        account_id: AccountId,
        base_capabilities: CapabilitySet,
        tree_context: TreeAuthzContext,
    ) -> Self {
        Self {
            account_id,
            base_capabilities,
            tree_context,
            local_policy: None,
        }
    }

    /// Add local policy constraints
    pub fn with_local_policy(mut self, policy: CapabilitySet) -> Self {
        self.local_policy = Some(policy);
        self
    }
}

/// Result of authorization evaluation
#[derive(Debug, Clone)]
pub struct PermissionGrant {
    /// Whether the operation is authorized
    pub authorized: bool,
    /// Effective capabilities after all policy meets
    pub effective_capabilities: CapabilitySet,
    /// Identity that was authorized
    pub authorized_identity: VerifiedIdentity,
    /// Reason for denial if not authorized
    pub denial_reason: Option<String>,
}

impl PermissionGrant {
    /// Create successful permission grant
    pub fn granted(
        effective_capabilities: CapabilitySet,
        authorized_identity: VerifiedIdentity,
    ) -> Self {
        Self {
            authorized: true,
            effective_capabilities,
            authorized_identity,
            denial_reason: None,
        }
    }

    /// Create denied permission grant
    pub fn denied(reason: String, identity: VerifiedIdentity) -> Self {
        Self {
            authorized: false,
            effective_capabilities: CapabilitySet::empty(),
            authorized_identity: identity,
            denial_reason: Some(reason),
        }
    }
}

/// Evaluate authorization combining identity proof with capability evaluation
pub fn evaluate_authorization(
    request: AuthorizationRequest,
) -> Result<PermissionGrant, AuthorizationError> {
    // Step 1: Extract entity ID from verified identity
    let entity_id = extract_entity_id(&request.verified_identity)?;

    // Step 2: Build capability evaluation context
    let capability_context = CapabilityEvaluationContext::new(
        request.context.base_capabilities,
        request.context.tree_context,
    );

    let capability_context = if let Some(local_policy) = request.context.local_policy {
        capability_context.with_local_policy(local_policy)
    } else {
        capability_context
    };

    // Step 3: Create tree capability request
    let mut signers = request.additional_signers;

    // Add the requesting identity as a signer
    match &request.verified_identity.proof {
        IdentityProof::Device { device_id, .. } => {
            signers.insert(*device_id);
        }
        IdentityProof::Guardian { .. } => {
            // Guardian signatures are handled separately
        }
        IdentityProof::Threshold(_) => {
            // Threshold signatures contain multiple signers
            // In a full implementation, we would extract the signer set
        }
    }

    let tree_request = TreeCapabilityRequest {
        operation: request.operation,
        requester: entity_id,
        signers,
        guardian_signers: request.guardian_signers,
    };

    // Step 4: Evaluate tree operation capabilities
    let evaluation_result = evaluate_tree_operation_capabilities(tree_request, capability_context)
        .map_err(AuthorizationError::CapabilityEvaluationFailed)?;

    // Step 5: Convert result to permission grant
    if evaluation_result.permitted {
        Ok(PermissionGrant::granted(
            evaluation_result.effective_capabilities,
            request.verified_identity,
        ))
    } else {
        Ok(PermissionGrant::denied(
            evaluation_result
                .denial_reason
                .unwrap_or_else(|| "Authorization denied for unknown reason".to_string()),
            request.verified_identity,
        ))
    }
}

/// Extract entity ID from verified identity
fn extract_entity_id(verified_identity: &VerifiedIdentity) -> Result<EntityId, AuthorizationError> {
    match &verified_identity.proof {
        IdentityProof::Device { device_id, .. } => Ok(EntityId::Device(*device_id)),
        IdentityProof::Guardian { guardian_id, .. } => Ok(EntityId::Guardian(*guardian_id)),
        IdentityProof::Threshold(_) => {
            // For threshold signatures, we could extract the account ID
            // TODO fix - For now, return an error since this requires more context
            Err(AuthorizationError::UnsupportedIdentityType(
                "Threshold identity requires account context".to_string(),
            ))
        }
    }
}

/// Combined authentication and authorization in one step
pub fn authenticate_and_authorize(
    identity_proof: IdentityProof,
    message: &[u8],
    key_material: &KeyMaterial,
    authz_context: AuthorizationContext,
    operation: TreeOp,
    additional_signers: BTreeSet<DeviceId>,
    guardian_signers: BTreeSet<GuardianId>,
) -> Result<PermissionGrant, AuthorizationError> {
    // Step 1: Authenticate identity
    let verified_identity =
        aura_verify::verify_identity_proof(&identity_proof, message, key_material)
            .map_err(AuthorizationError::AuthenticationFailed)?;

    // Step 2: Create authorization request
    let authz_request = AuthorizationRequest {
        verified_identity,
        operation,
        context: authz_context,
        additional_signers,
        guardian_signers,
    };

    // Step 3: Evaluate authorization
    evaluate_authorization(authz_request)
}

/// Authorization errors combining authentication and capability evaluation failures
#[derive(Debug, thiserror::Error)]
pub enum AuthorizationError {
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(#[from] aura_verify::AuthenticationError),

    #[error("Capability evaluation failed: {0}")]
    CapabilityEvaluationFailed(#[from] WotError),

    #[error("Unsupported identity type: {0}")]
    UnsupportedIdentityType(String),

    #[error("Invalid authorization request: {0}")]
    InvalidRequest(String),
}

/// Authorized event combining identity proof with permission grant
#[derive(Debug, Clone)]
pub struct AuthorizedEvent {
    /// The verified identity who authorized this event
    pub identity_proof: VerifiedIdentity,
    /// The permission grant that authorized this operation
    pub permission_grant: PermissionGrant,
    /// The tree operation that was authorized
    pub operation: TreeOp,
    /// Timestamp when authorization was granted
    pub authorized_at: std::time::SystemTime,
}

impl AuthorizedEvent {
    /// Create new authorized event
    pub fn new(
        identity_proof: VerifiedIdentity,
        permission_grant: PermissionGrant,
        operation: TreeOp,
    ) -> Self {
        Self {
            identity_proof,
            permission_grant,
            operation,
            authorized_at: std::time::SystemTime::now(),
        }
    }

    /// Check if this event is properly authorized
    pub fn is_authorized(&self) -> bool {
        self.permission_grant.authorized
    }

    /// Get the authorized identity
    pub fn authorized_identity(&self) -> &VerifiedIdentity {
        &self.identity_proof
    }

    /// Get effective capabilities for this authorization
    pub fn effective_capabilities(&self) -> &CapabilitySet {
        &self.permission_grant.effective_capabilities
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_crypto::Ed25519Signature;
    use aura_verify::ThresholdSig;

    #[test]
    fn test_authorization_request_creation() {
        let account_id = AccountId::from_bytes([1u8; 32]);
        let device_id = DeviceId::from_bytes([1u8; 32]);

        let identity_proof = IdentityProof::Device {
            device_id,
            signature: Ed25519Signature::from_bytes(&[0u8; 64]),
        };

        let verified_identity = VerifiedIdentity {
            proof: identity_proof,
            message_hash: [0u8; 32],
        };

        let tree_context = aura_wot::TreeAuthzContext::new(account_id, 1);
        let base_capabilities =
            CapabilitySet::from_permissions(&["tree:read", "tree:propose", "tree:modify"]);

        let authz_context = AuthorizationContext::new(account_id, base_capabilities, tree_context);

        let operation = TreeOp {
            parent_epoch: 1,
            parent_commitment: [0u8; 32],
            op: TreeOpKind::AddLeaf {
                leaf_id: 2,
                role: aura_wot::LeafRole::Device,
                under: 0,
            },
            version: 1,
        };

        let request = AuthorizationRequest {
            verified_identity,
            operation,
            context: authz_context,
            additional_signers: BTreeSet::new(),
            guardian_signers: BTreeSet::new(),
        };

        assert_eq!(request.context.account_id, account_id);
        assert!(request.context.base_capabilities.permits("tree:read"));
    }

    #[test]
    fn test_entity_id_extraction() {
        let device_id = DeviceId::from_bytes([1u8; 32]);
        let guardian_id = DeviceId::from_bytes([2u8; 32]);

        // Test device identity
        let device_identity = VerifiedIdentity {
            proof: IdentityProof::Device {
                device_id,
                signature: Ed25519Signature::from_bytes(&[0u8; 64]),
            },
            message_hash: [0u8; 32],
        };

        let entity_id = extract_entity_id(&device_identity).unwrap();
        assert_eq!(entity_id, EntityId::Device(device_id));

        // Test guardian identity
        let guardian_identity = VerifiedIdentity {
            proof: IdentityProof::Guardian {
                guardian_id,
                signature: Ed25519Signature::from_bytes(&[0u8; 64]),
            },
            message_hash: [0u8; 32],
        };

        let entity_id = extract_entity_id(&guardian_identity).unwrap();
        assert_eq!(entity_id, EntityId::Guardian(guardian_id));

        // Test threshold identity (should fail)
        let threshold_identity = VerifiedIdentity {
            proof: IdentityProof::Threshold(ThresholdSig {
                signature: Ed25519Signature::from_bytes(&[0u8; 64]),
                signers: vec![0, 1],
                signature_shares: vec![],
            }),
            message_hash: [0u8; 32],
        };

        let result = extract_entity_id(&threshold_identity);
        assert!(result.is_err());
    }

    #[test]
    fn test_permission_grant_creation() {
        let device_id = DeviceId::from_bytes([1u8; 32]);
        let identity = VerifiedIdentity {
            proof: IdentityProof::Device {
                device_id,
                signature: Ed25519Signature::from_bytes(&[0u8; 64]),
            },
            message_hash: [0u8; 32],
        };

        let capabilities = CapabilitySet::from_permissions(&["tree:read", "tree:modify"]);

        let grant = PermissionGrant::granted(capabilities.clone(), identity.clone());
        assert!(grant.authorized);
        assert_eq!(grant.effective_capabilities, capabilities);

        let denial = PermissionGrant::denied("Test denial".to_string(), identity);
        assert!(!denial.authorized);
        assert_eq!(denial.denial_reason, Some("Test denial".to_string()));
    }

    #[test]
    fn test_authorized_event_creation() {
        let device_id = DeviceId::from_bytes([1u8; 32]);
        let identity = VerifiedIdentity {
            proof: IdentityProof::Device {
                device_id,
                signature: Ed25519Signature::from_bytes(&[0u8; 64]),
            },
            message_hash: [0u8; 32],
        };

        let capabilities = CapabilitySet::from_permissions(&["tree:read"]);
        let grant = PermissionGrant::granted(capabilities, identity.clone());

        let operation = TreeOp {
            parent_epoch: 1,
            parent_commitment: [0u8; 32],
            op: TreeOpKind::RotateEpoch { affected: vec![0] },
            version: 1,
        };

        let event = AuthorizedEvent::new(identity.clone(), grant, operation);
        assert!(event.is_authorized());
        // TODO: Implement PartialEq for VerifiedIdentity
        // assert_eq!(event.authorized_identity(), &identity);
        assert!(event.effective_capabilities().permits("tree:read"));
    }
}
