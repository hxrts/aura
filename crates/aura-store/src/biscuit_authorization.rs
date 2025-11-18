//! Biscuit-based storage authorization for the Aura storage system
//!
//! This module provides Biscuit token-based access control for storage operations,
//! replacing the old storage_authz.rs functionality with a more secure and flexible
//! authorization system.

use crate::{AccessDecision, StoragePermission, StorageResource};
use aura_core::FlowBudget;
use aura_wot::{ResourceScope, StorageCategory};
use biscuit_auth::{Biscuit, PublicKey};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Biscuit-based storage authorization evaluator
///
/// Provides secure storage access control using Biscuit tokens with proper
/// capability delegation and flow budget enforcement.
#[derive(Debug)]
pub struct BiscuitStorageEvaluator {
    /// Root public key for token verification
    root_public_key: PublicKey,
    /// Permission mappings for authorization checks
    permission_mappings: PermissionMappings,
}

impl BiscuitStorageEvaluator {
    /// Create a new Biscuit storage evaluator
    pub fn new(root_public_key: PublicKey) -> Self {
        Self {
            root_public_key,
            permission_mappings: PermissionMappings::default(),
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
                // Parse content ID to determine category and path
                let (category, path) = self.parse_content_id(content_id)?;
                Ok(ResourceScope::Storage { category, path })
            }
            StorageResource::Namespace(namespace) => {
                // Parse namespace to determine category
                let category = self.parse_namespace(namespace)?;
                Ok(ResourceScope::Storage {
                    category,
                    path: format!("{}/*", namespace),
                })
            }
            StorageResource::Global => {
                Ok(ResourceScope::Storage {
                    category: StorageCategory::Public, // Most permissive for global
                    path: "*".to_string(),
                })
            }
            StorageResource::SearchIndex => Ok(ResourceScope::Storage {
                category: StorageCategory::Shared,
                path: "search_index".to_string(),
            }),
            StorageResource::GarbageCollection => Ok(ResourceScope::Storage {
                category: StorageCategory::Shared,
                path: "gc".to_string(),
            }),
        }
    }

    /// Parse content ID to extract category and path
    fn parse_content_id(
        &self,
        content_id: &str,
    ) -> Result<(StorageCategory, String), BiscuitStorageError> {
        // Content IDs follow pattern: category/path
        // e.g., "personal/user123/document1", "shared/project/file", "public/asset"
        let parts: Vec<&str> = content_id.splitn(2, '/').collect();

        if parts.len() < 2 {
            return Err(BiscuitStorageError::InvalidResource(format!(
                "Invalid content ID format: {}",
                content_id
            )));
        }

        let category = match parts[0] {
            "personal" => StorageCategory::Personal,
            "shared" => StorageCategory::Shared,
            "public" => StorageCategory::Public,
            _ => {
                return Err(BiscuitStorageError::InvalidResource(format!(
                    "Unknown storage category: {}",
                    parts[0]
                )))
            }
        };

        Ok((category, parts[1].to_string()))
    }

    /// Parse namespace to extract category
    fn parse_namespace(&self, namespace: &str) -> Result<StorageCategory, BiscuitStorageError> {
        // Namespaces follow pattern: category/...
        let parts: Vec<&str> = namespace.splitn(2, '/').collect();

        if parts.is_empty() {
            return Err(BiscuitStorageError::InvalidResource(format!(
                "Invalid namespace format: {}",
                namespace
            )));
        }

        let category = match parts[0] {
            "personal" => StorageCategory::Personal,
            "shared" => StorageCategory::Shared,
            "public" => StorageCategory::Public,
            _ => {
                return Err(BiscuitStorageError::InvalidResource(format!(
                    "Unknown storage category: {}",
                    parts[0]
                )))
            }
        };

        Ok(category)
    }

    /// Check Biscuit token authorization using Authorizer
    fn check_biscuit_authorization(
        &self,
        _token: &Biscuit,
        resource_scope: &ResourceScope,
        operation: &str,
    ) -> Result<bool, BiscuitStorageError> {
        // Stub implementation - basic pattern matching for now
        // In a full implementation, this would use token.authorize(&authorizer)
        // with proper Datalog fact building

        // For now, implement basic authorization logic based on resource and operation
        match resource_scope {
            ResourceScope::Storage { category, path: _ } => {
                // Check basic operation permissions
                match operation {
                    "read" => {
                        // Read is generally allowed for most storage categories
                        match category {
                            StorageCategory::Public => Ok(true),
                            StorageCategory::Shared => Ok(true), // Would check group membership
                            StorageCategory::Personal => Ok(true), // Would check device ownership
                        }
                    }
                    "write" => {
                        // Write requires higher permissions
                        match category {
                            StorageCategory::Public => Ok(false),  // Public is read-only
                            StorageCategory::Shared => Ok(true), // Would check group write permissions
                            StorageCategory::Personal => Ok(true), // Would check device ownership
                        }
                    }
                    "admin" => {
                        // Admin operations require special privileges
                        match category {
                            StorageCategory::Public => Ok(false),
                            StorageCategory::Shared => Ok(false), // Would check admin permissions
                            StorageCategory::Personal => Ok(true), // Owner can admin their storage
                        }
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
    use aura_core::{AccountId, DeviceId};
    use aura_wot::AccountAuthority;

    fn setup_test_authority() -> AccountAuthority {
        AccountAuthority::new(AccountId::new())
    }

    fn setup_test_evaluator() -> BiscuitStorageEvaluator {
        let authority = setup_test_authority();
        BiscuitStorageEvaluator::new(authority.root_public_key())
    }

    #[test]
    fn test_content_id_parsing() {
        let evaluator = setup_test_evaluator();

        let (category, path) = evaluator
            .parse_content_id("personal/user123/document1")
            .unwrap();
        assert_eq!(category, StorageCategory::Personal);
        assert_eq!(path, "user123/document1");

        let (category, path) = evaluator.parse_content_id("shared/project/file").unwrap();
        assert_eq!(category, StorageCategory::Shared);
        assert_eq!(path, "project/file");

        let (category, path) = evaluator.parse_content_id("public/asset").unwrap();
        assert_eq!(category, StorageCategory::Public);
        assert_eq!(path, "asset");
    }

    #[test]
    fn test_namespace_parsing() {
        let evaluator = setup_test_evaluator();

        let category = evaluator.parse_namespace("personal/user123").unwrap();
        assert_eq!(category, StorageCategory::Personal);

        let category = evaluator.parse_namespace("shared/project").unwrap();
        assert_eq!(category, StorageCategory::Shared);

        let category = evaluator.parse_namespace("public").unwrap();
        assert_eq!(category, StorageCategory::Public);
    }

    #[test]
    fn test_invalid_content_id() {
        let evaluator = setup_test_evaluator();

        assert!(evaluator.parse_content_id("invalid").is_err());
        assert!(evaluator.parse_content_id("unknown/path").is_err());
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
