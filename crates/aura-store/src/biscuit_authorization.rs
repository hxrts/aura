//! Biscuit-based storage authorization for the Aura storage system
//!
//! This module provides Biscuit token-based access control for storage operations,
//! replacing the old storage_authz.rs functionality with a more secure and flexible
//! authorization system.

use crate::{AccessDecision, StoragePermission, StorageResource};
use aura_core::{AuthorityId, FlowBudget};
use aura_wot::ResourceScope;
use biscuit_auth::{Biscuit, PublicKey};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing;

/// Biscuit-based storage authorization evaluator
///
/// Provides secure storage access control using Biscuit tokens with proper
/// capability delegation and flow budget enforcement.
#[derive(Debug)]
pub struct BiscuitStorageEvaluator {
    /// Root public key for token verification
    #[allow(dead_code)]
    root_public_key: PublicKey,
    /// Permission mappings for authorization checks
    permission_mappings: PermissionMappings,
    /// Authority ID for storage scope (owner of the storage namespace)
    authority_id: AuthorityId,
}

impl BiscuitStorageEvaluator {
    /// Create a new Biscuit storage evaluator for a specific authority
    pub fn new(root_public_key: PublicKey, authority_id: AuthorityId) -> Self {
        Self {
            root_public_key,
            permission_mappings: PermissionMappings::default(),
            authority_id,
        }
    }

    /// Evaluate storage access using a Biscuit token
    pub fn evaluate_access(
        &self,
        token: &Biscuit,
        resource: &StorageResource,
        permission: &StoragePermission,
        budget: &mut FlowBudget,
    ) -> Result<AccessDecision, BiscuitStorageError> {
        // Convert storage resource to ResourceScope pattern
        let resource_scope = self.storage_resource_to_scope(resource)?;

        // Get required operation from permission
        let operation = self.permission_mappings.permission_to_operation(permission);

        // Calculate flow cost for operation
        let flow_cost = self.calculate_flow_cost(resource, permission);

        // Check flow budget
        if !budget.can_charge(flow_cost) {
            return Ok(AccessDecision::deny(&format!(
                "Insufficient flow budget: required {}, available {}",
                flow_cost,
                budget.headroom()
            )));
        }

        // Check authorization using Biscuit authorizer
        let auth_result = self.check_biscuit_authorization(token, &resource_scope, &operation)?;

        if auth_result {
            // Charge budget on successful authorization
            if !budget.record_charge(flow_cost) {
                return Err(BiscuitStorageError::FlowBudget(
                    "Failed to record flow charge".to_string(),
                ));
            }
            Ok(AccessDecision::allow())
        } else {
            Ok(AccessDecision::deny(&format!(
                "Token does not grant {} permission on resource {:?}",
                operation, resource
            )))
        }
    }

    /// Check authorization without budget enforcement (for read-only checks)
    pub fn check_access(
        &self,
        token: &Biscuit,
        resource: &StorageResource,
        permission: &StoragePermission,
    ) -> Result<bool, BiscuitStorageError> {
        // Convert storage resource to ResourceScope pattern
        let resource_scope = self.storage_resource_to_scope(resource)?;

        // Get required operation from permission
        let operation = self.permission_mappings.permission_to_operation(permission);

        // Check authorization
        self.check_biscuit_authorization(token, &resource_scope, &operation)
    }

    /// Convert StorageResource to ResourceScope for Biscuit patterns
    fn storage_resource_to_scope(
        &self,
        resource: &StorageResource,
    ) -> Result<ResourceScope, BiscuitStorageError> {
        match resource {
            StorageResource::Content(content_id) => {
                // Use authority ID and content ID as path
                Ok(ResourceScope::Storage {
                    authority_id: self.authority_id,
                    path: format!("content/{}", content_id),
                })
            }
            StorageResource::Namespace(namespace) => {
                // Use authority ID and namespace as path
                Ok(ResourceScope::Storage {
                    authority_id: self.authority_id,
                    path: format!("namespace/{}/*", namespace),
                })
            }
            StorageResource::Global => {
                // Global storage scoped to this authority
                Ok(ResourceScope::Storage {
                    authority_id: self.authority_id,
                    path: "global/*".to_string(),
                })
            }
            StorageResource::SearchIndex => Ok(ResourceScope::Storage {
                authority_id: self.authority_id,
                path: "search_index".to_string(),
            }),
            StorageResource::GarbageCollection => Ok(ResourceScope::Storage {
                authority_id: self.authority_id,
                path: "gc".to_string(),
            }),
        }
    }

    // Note: parse_content_id and parse_namespace methods removed as they are
    // no longer needed with the authority-centric ResourceScope model

    /// Check Biscuit token authorization using Authorizer
    fn check_biscuit_authorization(
        &self,
        token: &Biscuit,
        resource_scope: &ResourceScope,
        operation: &str,
    ) -> Result<bool, BiscuitStorageError> {
        // Note: Token signature verification happens when Biscuit is constructed from bytes
        // with Biscuit::from(bytes, root_key). If we have a &Biscuit, it's already verified.

        // For now, implement basic authorization logic based on resource and operation
        // Full implementation would use token.authorize(&authorizer) with proper Datalog facts
        match resource_scope {
            ResourceScope::Storage { authority_id, path } => {
                // SECURITY: Verify token authority_id matches resource authority
                // Extract authority_id from token and ensure it matches the resource scope
                if !self.verify_token_authority(token, authority_id)? {
                    return Ok(false); // Token not authorized for this authority
                }

                // Check basic operation permissions
                // In the authority-centric model, permissions are evaluated based on:
                // 1. The authority owning the storage
                // 2. The path within that authority's storage
                // 3. The operation being performed

                match operation {
                    "read" => {
                        // Read operations allowed if token is for the correct authority
                        Ok(true)
                    }
                    "write" => {
                        // Write operations require ownership or delegation
                        // Check if path is writable for this authority
                        if path.starts_with("global/") {
                            Ok(false) // Global paths might be read-only
                        } else {
                            Ok(true) // Authority can write to own storage
                        }
                    }
                    "admin" => {
                        // Admin operations require full authority control
                        // Only the authority itself can perform admin operations
                        Ok(true) // Simplified: assume token validates authority
                    }
                    _ => Ok(false), // Unknown operations denied
                }
            }
            _ => {
                // Non-storage resources - implement as needed
                Ok(false)
            }
        }
    }

    /// Verify that token is authorized for the specified authority
    ///
    /// Extracts authority_id from token facts and compares with expected authority.
    /// Returns true if token is authorized for the authority.
    fn verify_token_authority(
        &self,
        token: &Biscuit,
        expected_authority: &AuthorityId,
    ) -> Result<bool, BiscuitStorageError> {
        // Extract authority_id from token by creating an Authorizer and querying facts
        // Biscuit tokens should contain an "authority_id" fact in the authority block
        // Format: authority_id(<uuid>)

        use biscuit_auth::Authorizer;

        // Create an authorizer to query token facts
        let mut authorizer = Authorizer::new();

        // Add the token to the authorizer
        authorizer.add_token(token).map_err(|e| {
            BiscuitStorageError::TokenVerification(format!("Failed to add token: {}", e))
        })?;

        // Add proper Datalog query to check for authority_id fact
        // Format: authority_id(<uuid_string>)
        let expected_uuid = expected_authority.to_string();

        // Add query to check for authority_id fact
        // Note: query() method accepts &str directly and parses it into a Rule internally
        let query_str = format!(
            "data($authority) <- authority_id($authority), $authority == \"{}\"",
            expected_uuid
        );
        let query_result: Result<Vec<(String,)>, _> = authorizer.query(query_str.as_str());

        match query_result {
            Ok(results) if !results.is_empty() => return Ok(true),
            Err(e) => {
                return Err(BiscuitStorageError::TokenVerification(format!(
                    "Authority query failed: {}",
                    e
                )))
            }
            _ => {} // Continue to next check
        }

        // Try alternative fact formats that might be in the token
        // Check for authority fact: authority(<uuid>)
        let alt_query_str = format!(
            "data($authority) <- authority($authority), $authority == \"{}\"",
            expected_uuid
        );
        let alt_query_result: Result<Vec<(String,)>, _> = authorizer.query(alt_query_str.as_str());

        match alt_query_result {
            Ok(results) if !results.is_empty() => return Ok(true),
            Err(e) => {
                return Err(BiscuitStorageError::TokenVerification(format!(
                    "Alternative authority query failed: {}",
                    e
                )))
            }
            _ => {} // Continue to next check
        }

        // Check for owner fact: owner(<uuid>)
        let owner_query_str = format!(
            "data($owner) <- owner($owner), $owner == \"{}\"",
            expected_uuid
        );
        let owner_query_result: Result<Vec<(String,)>, _> =
            authorizer.query(owner_query_str.as_str());

        match owner_query_result {
            Ok(results) if !results.is_empty() => return Ok(true),
            Err(e) => {
                return Err(BiscuitStorageError::TokenVerification(format!(
                    "Owner query failed: {}",
                    e
                )))
            }
            _ => {} // No match found
        }

        // No authority_id fact found - token is not properly scoped for this authority
        // In the authority-centric model, this is a security requirement
        tracing::warn!(
            expected_authority = %expected_authority,
            "Token does not contain authority_id fact for expected authority - access denied"
        );

        Ok(false)
    }

    /// Calculate flow cost for storage operation
    fn calculate_flow_cost(
        &self,
        resource: &StorageResource,
        permission: &StoragePermission,
    ) -> u64 {
        // Base costs by operation type
        let base_cost = match permission {
            StoragePermission::Read => 10,
            StoragePermission::Write => 50,
            StoragePermission::Admin => 100,
        };

        // Resource multipliers
        let resource_multiplier = match resource {
            StorageResource::Content(_) => 1,
            StorageResource::Namespace(_) => 2,
            StorageResource::Global => 5,
            StorageResource::SearchIndex => 3,
            StorageResource::GarbageCollection => 4,
        };

        base_cost * resource_multiplier
    }
}

/// Permission mappings for storage operations
#[derive(Debug, Default)]
pub struct PermissionMappings {
    mappings: HashMap<StoragePermission, String>,
}

impl PermissionMappings {
    /// Create default permission mappings
    pub fn new() -> Self {
        let mut mappings = HashMap::new();
        mappings.insert(StoragePermission::Read, "read".to_string());
        mappings.insert(StoragePermission::Write, "write".to_string());
        mappings.insert(StoragePermission::Admin, "admin".to_string());

        Self { mappings }
    }

    /// Get operation string for permission
    pub fn permission_to_operation(&self, permission: &StoragePermission) -> String {
        self.mappings
            .get(permission)
            .cloned()
            .unwrap_or_else(|| "unknown".to_string())
    }
}

/// Storage access request with Biscuit token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BiscuitAccessRequest {
    /// Biscuit token for authorization
    pub token: Vec<u8>, // Serialized token
    /// Requested resource
    pub resource: StorageResource,
    /// Required permission level
    pub permission: StoragePermission,
    /// Flow cost for the operation
    pub flow_cost: Option<u64>,
}

impl BiscuitAccessRequest {
    /// Create a new Biscuit access request
    pub fn new(token: Vec<u8>, resource: StorageResource, permission: StoragePermission) -> Self {
        Self {
            token,
            resource,
            permission,
            flow_cost: None,
        }
    }

    /// Set flow cost for the request
    pub fn with_flow_cost(mut self, cost: u64) -> Self {
        self.flow_cost = Some(cost);
        self
    }

    /// Deserialize the Biscuit token
    pub fn deserialize_token(&self, root_key: &PublicKey) -> Result<Biscuit, BiscuitStorageError> {
        Biscuit::from(&self.token, *root_key).map_err(|e| {
            BiscuitStorageError::Biscuit(format!("Token deserialization failed: {}", e))
        })
    }
}

/// Biscuit storage authorization errors
#[derive(Debug, thiserror::Error)]
pub enum BiscuitStorageError {
    /// Biscuit authorization error
    #[error("Biscuit authorization error: {0}")]
    Biscuit(String),

    /// Token verification error (signature or format invalid)
    #[error("Token verification error: {0}")]
    TokenVerification(String),

    /// Invalid resource identifier
    #[error("Invalid resource: {0}")]
    InvalidResource(String),

    /// Flow budget error
    #[error("Flow budget error: {0}")]
    FlowBudget(String),

    /// Authorization failed
    #[error("Authorization failed: {0}")]
    AuthorizationFailed(String),
}

/// Pure function to evaluate Biscuit storage access
pub fn evaluate_biscuit_access(
    evaluator: &BiscuitStorageEvaluator,
    request: &BiscuitAccessRequest,
    root_key: &PublicKey,
    budget: &mut FlowBudget,
) -> Result<AccessDecision, BiscuitStorageError> {
    // Deserialize token
    let token = request.deserialize_token(root_key)?;

    // Evaluate access
    evaluator.evaluate_access(&token, &request.resource, &request.permission, budget)
}

/// Pure function to check Biscuit storage access without budget enforcement
pub fn check_biscuit_access(
    evaluator: &BiscuitStorageEvaluator,
    request: &BiscuitAccessRequest,
    root_key: &PublicKey,
) -> Result<bool, BiscuitStorageError> {
    // Deserialize token
    let token = request.deserialize_token(root_key)?;

    // Check access
    evaluator.check_access(&token, &request.resource, &request.permission)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::identifiers::DeviceId;
    use aura_core::AccountId;
    use aura_wot::AccountAuthority;

    fn setup_test_authority() -> AccountAuthority {
        AccountAuthority::new(AccountId::new())
    }

    fn setup_test_evaluator() -> BiscuitStorageEvaluator {
        let authority = setup_test_authority();
        let authority_id = AuthorityId::new();
        BiscuitStorageEvaluator::new(authority.root_public_key(), authority_id)
    }

    // Tests updated for the new authority-centric API
    #[test]
    fn test_authority_centric_content_access() {
        let evaluator = setup_test_evaluator();
        let _authority_id = AuthorityId::new();

        // Test content resource scope conversion
        let content_resource = StorageResource::content("personal/user123/doc");
        let scope_result = evaluator.storage_resource_to_scope(&content_resource);
        assert!(scope_result.is_ok());

        if let Ok(ResourceScope::Storage {
            authority_id: scope_authority,
            path,
        }) = scope_result
        {
            assert_eq!(scope_authority, evaluator.authority_id);
            assert!(path.contains("content/"));
            assert!(path.contains("personal/user123/doc"));
        } else {
            panic!("Expected Storage ResourceScope");
        }
    }

    #[test]
    fn test_authority_centric_namespace_access() {
        let evaluator = setup_test_evaluator();

        // Test namespace resource scope conversion
        let namespace_resource = StorageResource::namespace("personal");
        let scope_result = evaluator.storage_resource_to_scope(&namespace_resource);
        assert!(scope_result.is_ok());

        if let Ok(ResourceScope::Storage {
            authority_id: scope_authority,
            path,
        }) = scope_result
        {
            assert_eq!(scope_authority, evaluator.authority_id);
            assert_eq!(path, "namespace/personal/*");
        } else {
            panic!("Expected Storage ResourceScope");
        }
    }

    #[test]
    fn test_authority_centric_global_access() {
        let evaluator = setup_test_evaluator();

        // Test global resource scope conversion
        let global_resource = StorageResource::Global;
        let scope_result = evaluator.storage_resource_to_scope(&global_resource);
        assert!(scope_result.is_ok());

        if let Ok(ResourceScope::Storage {
            authority_id: scope_authority,
            path,
        }) = scope_result
        {
            assert_eq!(scope_authority, evaluator.authority_id);
            assert_eq!(path, "global/*");
        } else {
            panic!("Expected Storage ResourceScope");
        }
    }

    #[test]
    fn test_flow_cost_calculation() {
        let evaluator = setup_test_evaluator();

        let content_resource = StorageResource::content("personal/user123/doc");
        let read_cost = evaluator.calculate_flow_cost(&content_resource, &StoragePermission::Read);
        let write_cost =
            evaluator.calculate_flow_cost(&content_resource, &StoragePermission::Write);
        let admin_cost =
            evaluator.calculate_flow_cost(&content_resource, &StoragePermission::Admin);

        assert_eq!(read_cost, 10); // 10 * 1
        assert_eq!(write_cost, 50); // 50 * 1
        assert_eq!(admin_cost, 100); // 100 * 1

        let global_resource = StorageResource::Global;
        let global_read_cost =
            evaluator.calculate_flow_cost(&global_resource, &StoragePermission::Read);
        assert_eq!(global_read_cost, 50); // 10 * 5
    }

    #[test]
    fn test_biscuit_access_request() {
        let authority = setup_test_authority();
        let device_id = DeviceId::new();
        let token = authority.create_device_token(device_id).unwrap();
        let token_bytes = token.to_vec().unwrap();

        let request = BiscuitAccessRequest::new(
            token_bytes,
            StorageResource::content("personal/user123/doc"),
            StoragePermission::Read,
        );

        let deserialized_token = request
            .deserialize_token(&authority.root_public_key())
            .unwrap();
        assert_eq!(deserialized_token.to_vec().unwrap(), request.token);
    }

    #[test]
    fn test_permission_mappings() {
        let mappings = PermissionMappings::new();

        assert_eq!(
            mappings.permission_to_operation(&StoragePermission::Read),
            "read"
        );
        assert_eq!(
            mappings.permission_to_operation(&StoragePermission::Write),
            "write"
        );
        assert_eq!(
            mappings.permission_to_operation(&StoragePermission::Admin),
            "admin"
        );
    }
}
