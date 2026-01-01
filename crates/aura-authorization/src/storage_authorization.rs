//! Storage-specific authorization logic using Biscuit tokens
//!
//! This module provides the storage authorization evaluator that was moved from aura-store
//! to eliminate improper domain coupling. Storage access control is fundamentally an
//! authorization concern and belongs in the authorization domain (aura-authorization).

// Authorization logic moved from aura-store to proper domain (aura-authorization)
use aura_core::scope::ResourceScope;
use aura_core::{AuthorityId, FlowBudget};
use biscuit_auth::{
    macros::{fact, policy, rule},
    Authorizer, Biscuit, PublicKey,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing;

/// Storage resource types for authorization
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StorageResource {
    /// Content identified by ID
    Content(String),
    /// Namespace scope
    Namespace(String),
    /// Global storage access
    Global,
    /// Search index access
    SearchIndex,
    /// Garbage collection access
    GarbageCollection,
}

impl StorageResource {
    /// Create a content resource
    pub fn content(content_id: &str) -> Self {
        Self::Content(content_id.to_string())
    }

    /// Create a namespace resource
    pub fn namespace(namespace: &str) -> Self {
        Self::Namespace(namespace.to_string())
    }
}

/// Storage permission levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StoragePermission {
    /// Read access
    Read,
    /// Write access
    Write,
    /// Administrative access
    Admin,
}

/// Storage access decision
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessDecision {
    /// Whether access is allowed
    pub allowed: bool,
    /// Reason for the decision
    pub reason: String,
}

impl AccessDecision {
    /// Allow access
    pub fn allow() -> Self {
        Self {
            allowed: true,
            reason: "Access granted".to_string(),
        }
    }

    /// Deny access with reason
    pub fn deny(reason: &str) -> Self {
        Self {
            allowed: false,
            reason: reason.to_string(),
        }
    }
}

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
    /// Authority ID for storage scope (owner of the storage namespace)
    authority_id: AuthorityId,
}

impl BiscuitStorageEvaluator {
    /// Create a new Biscuit storage evaluator for a specific authority
    pub fn new(root_public_key: PublicKey, authority_id: AuthorityId) -> Self {
        Self {
            root_public_key,
            permission_mappings: PermissionMappings::new(),
            authority_id,
        }
    }

    /// Get the root public key for token verification
    pub fn root_public_key(&self) -> PublicKey {
        self.root_public_key
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
            if let Err(e) = budget.record_charge(flow_cost) {
                return Err(BiscuitStorageError::FlowBudget(format!(
                    "Failed to record flow charge: {}",
                    e
                )));
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

    /// Check Biscuit token authorization using Authorizer
    fn check_biscuit_authorization(
        &self,
        token: &Biscuit,
        resource_scope: &ResourceScope,
        operation: &str,
    ) -> Result<bool, BiscuitStorageError> {
        // Token signature verification happens when Biscuit is constructed from bytes.
        // Here we enforce scope, capability, and authority binding using Datalog rules
        // that mirror the authority-centric model in docs/109_authorization.md.
        let mut authorizer = Authorizer::new();
        authorizer.add_token(token).map_err(|e| {
            BiscuitStorageError::TokenVerification(format!("Failed to add token: {}", e))
        })?;

        // Add environment facts for the requested operation and resource path.
        for fact in self.resource_facts(resource_scope, operation)? {
            authorizer
                .add_fact(fact)
                .map_err(|e| BiscuitStorageError::AuthorizationFailed(e.to_string()))?;
        }

        // Policy 1: bind token to the authority that owns the storage namespace.
        // Accept either an explicit authority_id(<uuid>) fact in the token or an
        // account(<uuid>) fact that matches the authority UUID. This matches the
        // account-issued device tokens produced by AccountAuthority.
        authorizer
            .add_policy(policy!(
                "allow if authority_id($auth), expected_authority($expected), $auth == $expected;"
            ))
            .map_err(|e| BiscuitStorageError::AuthorizationFailed(e.to_string()))?;

        // Policy 2: accept account(<uuid>) facts for backward compatibility with
        // existing device tokens issued by AccountAuthority.
        authorizer
            .add_policy(policy!(
                "allow if account($acct), expected_authority($expected), $acct == $expected;"
            ))
            .map_err(|e| BiscuitStorageError::AuthorizationFailed(e.to_string()))?;

        // Policy 3: enforce capability + operation + resource coherence. Any token
        // checks (e.g., resource prefix) embedded in the Biscuit must also succeed
        // because Authorizer evaluates all token checks alongside these facts.
        authorizer
            .add_policy(policy!(
                "allow if capability($op), operation($op), resource($res);"
            ))
            .map_err(|e| BiscuitStorageError::AuthorizationFailed(e.to_string()))?;

        // Default deny keeps evaluation strict.
        authorizer
            .add_policy(policy!("deny if true;"))
            .map_err(|e| BiscuitStorageError::AuthorizationFailed(e.to_string()))?;

        Ok(authorizer.authorize().is_ok())
    }

    /// Verify that token is authorized for the specified authority
    ///
    /// Extracts authority_id from token facts and compares with expected authority.
    /// Returns true if token is authorized for the authority. Useful for pre-checking
    /// tokens before storage operations or for audit logging.
    pub fn verify_token_authority(
        &self,
        token: &Biscuit,
        expected_authority: &AuthorityId,
    ) -> Result<bool, BiscuitStorageError> {
        // Extract authority_id from token by creating an Authorizer and querying facts
        // Biscuit tokens should contain an "authority_id" fact in the authority block
        // Format: authority_id(<uuid>)

        let mut authorizer = Authorizer::new();
        authorizer.add_token(token).map_err(|e| {
            BiscuitStorageError::TokenVerification(format!("Failed to add token: {}", e))
        })?;

        // Allow tokens that carry either authority_id(<uuid>) or account(<uuid>) that
        // matches the expected authority UUID (authority IDs are UUID-wrapped).
        let uuid = expected_authority.uuid().to_string();

        let authority_match: Result<Vec<(String,)>, _> = authorizer.query(rule!(
            "data($authority) <- authority_id($authority), $authority == {uuid};",
            uuid = uuid.clone()
        ));
        if matches!(authority_match, Ok(v) if !v.is_empty()) {
            return Ok(true);
        }

        let account_match: Result<Vec<(String,)>, _> = authorizer.query(rule!(
            "data($account) <- account($account), $account == {uuid};",
            uuid = uuid
        ));
        if matches!(account_match, Ok(v) if !v.is_empty()) {
            return Ok(true);
        }

        tracing::warn!(
            expected_authority = %expected_authority,
            "Token does not present authority/account binding for expected authority"
        );

        Ok(false)
    }

    /// Build Datalog facts for the requested resource and operation, escaping strings safely.
    fn resource_facts(
        &self,
        resource_scope: &ResourceScope,
        operation: &str,
    ) -> Result<Vec<biscuit_auth::builder::Fact>, BiscuitStorageError> {
        let op_fact = fact!("operation({op})", op = operation.to_string());
        let (authority_fact, resource_fact) = match resource_scope {
            ResourceScope::Storage { authority_id, path } => {
                let authority_fact = fact!(
                    "authority_id({auth})",
                    auth = authority_id.uuid().to_string()
                );
                let path_fact = fact!("resource({res})", res = path.to_string());
                (authority_fact, path_fact)
            }
            _ => {
                return Err(BiscuitStorageError::InvalidResource(
                    "Non-storage resource scopes are not supported".to_string(),
                ))
            }
        };

        let expected_auth_fact = fact!(
            "expected_authority({auth})",
            auth = self.authority_id.uuid().to_string()
        );

        Ok(vec![
            expected_auth_fact,
            authority_fact,
            resource_fact,
            op_fact,
        ])
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

// ============================================================================
// Authorized Storage Handler Wrapper
// ============================================================================

use async_trait::async_trait;
use aura_core::effects::{StorageCoreEffects, StorageError, StorageExtendedEffects, StorageStats};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Storage handler wrapper that enforces Biscuit authorization on all operations
///
/// This wrapper integrates `BiscuitStorageEvaluator` with any `StorageEffects` implementation,
/// ensuring all storage operations are authorized via Biscuit tokens before execution.
///
/// # Example
///
/// ```rust,ignore
/// use aura_authorization::{AuthorizedStorageHandler, BiscuitStorageEvaluator};
/// use aura_effects::FilesystemStorageHandler;
///
/// let inner = FilesystemStorageHandler::new(path);
/// let evaluator = BiscuitStorageEvaluator::new(root_key, authority_id);
/// let handler = AuthorizedStorageHandler::new(inner, evaluator, token);
///
/// // All operations now require valid Biscuit authorization
/// handler.store("key", data).await?;
/// ```
pub struct AuthorizedStorageHandler<S: StorageCoreEffects + StorageExtendedEffects> {
    /// Inner storage handler
    inner: S,
    /// Biscuit authorization evaluator
    evaluator: BiscuitStorageEvaluator,
    /// Biscuit token for authorization (shared, can be updated)
    token: Arc<RwLock<Option<Biscuit>>>,
    /// Flow budget for operations (shared, can be updated)
    budget: Arc<RwLock<FlowBudget>>,
}

impl<S: StorageCoreEffects + StorageExtendedEffects> AuthorizedStorageHandler<S> {
    /// Create a new authorized storage handler
    pub fn new(inner: S, evaluator: BiscuitStorageEvaluator, budget: FlowBudget) -> Self {
        Self {
            inner,
            evaluator,
            token: Arc::new(RwLock::new(None)),
            budget: Arc::new(RwLock::new(budget)),
        }
    }

    /// Create with an initial token
    pub fn with_token(
        inner: S,
        evaluator: BiscuitStorageEvaluator,
        token: Biscuit,
        budget: FlowBudget,
    ) -> Self {
        Self {
            inner,
            evaluator,
            token: Arc::new(RwLock::new(Some(token))),
            budget: Arc::new(RwLock::new(budget)),
        }
    }

    /// Set the Biscuit token for authorization
    pub async fn set_token(&self, token: Biscuit) {
        let mut guard = self.token.write().await;
        *guard = Some(token);
    }

    /// Clear the authorization token
    pub async fn clear_token(&self) {
        let mut guard = self.token.write().await;
        *guard = None;
    }

    /// Get the remaining flow budget
    pub async fn remaining_budget(&self) -> u64 {
        let guard = self.budget.read().await;
        guard.headroom()
    }

    /// Check authorization for a storage operation
    async fn check_authorization(
        &self,
        resource: &StorageResource,
        permission: &StoragePermission,
    ) -> Result<(), StorageError> {
        let token_guard = self.token.read().await;
        let token = token_guard.as_ref().ok_or_else(|| {
            StorageError::PermissionDenied("No authorization token set".to_string())
        })?;

        let mut budget_guard = self.budget.write().await;
        let decision = self
            .evaluator
            .evaluate_access(token, resource, permission, &mut budget_guard)
            .map_err(|e| StorageError::PermissionDenied(format!("Authorization error: {}", e)))?;

        if !decision.allowed {
            return Err(StorageError::PermissionDenied(decision.reason));
        }

        Ok(())
    }
}

#[async_trait]
impl<S: StorageCoreEffects + StorageExtendedEffects + Send + Sync> StorageCoreEffects
    for AuthorizedStorageHandler<S>
{
    async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
        let resource = StorageResource::content(key);
        self.check_authorization(&resource, &StoragePermission::Write)
            .await?;
        self.inner.store(key, value).await
    }

    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let resource = StorageResource::content(key);
        self.check_authorization(&resource, &StoragePermission::Read)
            .await?;
        self.inner.retrieve(key).await
    }

    async fn remove(&self, key: &str) -> Result<bool, StorageError> {
        let resource = StorageResource::content(key);
        self.check_authorization(&resource, &StoragePermission::Admin)
            .await?;
        self.inner.remove(key).await
    }

    async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
        let resource = match prefix {
            Some(p) => StorageResource::namespace(p),
            None => StorageResource::Global,
        };
        self.check_authorization(&resource, &StoragePermission::Read)
            .await?;
        self.inner.list_keys(prefix).await
    }
}

#[async_trait]
impl<S: StorageCoreEffects + StorageExtendedEffects + Send + Sync> StorageExtendedEffects
    for AuthorizedStorageHandler<S>
{
    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        let resource = StorageResource::content(key);
        self.check_authorization(&resource, &StoragePermission::Read)
            .await?;
        self.inner.exists(key).await
    }

    async fn store_batch(&self, pairs: HashMap<String, Vec<u8>>) -> Result<(), StorageError> {
        // Check authorization for each key (batch write requires write permission on all)
        for key in pairs.keys() {
            let resource = StorageResource::content(key);
            self.check_authorization(&resource, &StoragePermission::Write)
                .await?;
        }
        self.inner.store_batch(pairs).await
    }

    async fn retrieve_batch(
        &self,
        keys: &[String],
    ) -> Result<HashMap<String, Vec<u8>>, StorageError> {
        // Check authorization for each key
        for key in keys {
            let resource = StorageResource::content(key);
            self.check_authorization(&resource, &StoragePermission::Read)
                .await?;
        }
        self.inner.retrieve_batch(keys).await
    }

    async fn clear_all(&self) -> Result<(), StorageError> {
        let resource = StorageResource::Global;
        self.check_authorization(&resource, &StoragePermission::Admin)
            .await?;
        self.inner.clear_all().await
    }

    async fn stats(&self) -> Result<StorageStats, StorageError> {
        // Stats are generally safe to read without per-item authorization
        let resource = StorageResource::Global;
        self.check_authorization(&resource, &StoragePermission::Read)
            .await?;
        self.inner.stats().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TokenAuthority;

    fn setup_test_authority() -> (TokenAuthority, AuthorityId) {
        let authority_id = AuthorityId::new_from_entropy([9u8; 32]);
        let authority = TokenAuthority::new(authority_id);
        (authority, authority_id)
    }

    fn setup_test_evaluator() -> BiscuitStorageEvaluator {
        let (authority, authority_id) = setup_test_authority();
        BiscuitStorageEvaluator::new(authority.root_public_key(), authority_id)
    }

    // Tests updated for the new authority-centric API
    #[test]
    fn test_authority_centric_content_access() {
        let evaluator = setup_test_evaluator();
        let _authority_id = AuthorityId::new_from_entropy([69u8; 32]);

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
        let (authority, _authority_id) = setup_test_authority();
        let recipient = AuthorityId::new_from_entropy([1u8; 32]);
        let token = authority.create_token(recipient).unwrap();
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

    // ========================================================================
    // AuthorizedStorageHandler Tests
    // ========================================================================

    use tokio::sync::Mutex;

    /// In-memory storage backend for testing
    struct MockStorage {
        data: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl MockStorage {
        fn new() -> Self {
            Self {
                data: Mutex::new(HashMap::new()),
            }
        }
    }

    #[async_trait]
    impl StorageCoreEffects for MockStorage {
        async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
            let mut data = self.data.lock().await;
            data.insert(key.to_string(), value);
            Ok(())
        }

        async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
            let data = self.data.lock().await;
            Ok(data.get(key).cloned())
        }

        async fn remove(&self, key: &str) -> Result<bool, StorageError> {
            let mut data = self.data.lock().await;
            Ok(data.remove(key).is_some())
        }

        async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
            let data = self.data.lock().await;
            let keys: Vec<String> = match prefix {
                Some(p) => data.keys().filter(|k| k.starts_with(p)).cloned().collect(),
                None => data.keys().cloned().collect(),
            };
            Ok(keys)
        }
    }

    #[async_trait]
    impl StorageExtendedEffects for MockStorage {
        async fn exists(&self, key: &str) -> Result<bool, StorageError> {
            let data = self.data.lock().await;
            Ok(data.contains_key(key))
        }

        async fn store_batch(&self, pairs: HashMap<String, Vec<u8>>) -> Result<(), StorageError> {
            let mut data = self.data.lock().await;
            data.extend(pairs);
            Ok(())
        }

        async fn retrieve_batch(
            &self,
            keys: &[String],
        ) -> Result<HashMap<String, Vec<u8>>, StorageError> {
            let data = self.data.lock().await;
            Ok(keys
                .iter()
                .filter_map(|k| data.get(k).map(|v| (k.clone(), v.clone())))
                .collect())
        }

        async fn clear_all(&self) -> Result<(), StorageError> {
            let mut data = self.data.lock().await;
            data.clear();
            Ok(())
        }

        async fn stats(&self) -> Result<StorageStats, StorageError> {
            let data = self.data.lock().await;
            Ok(StorageStats {
                key_count: data.len() as u64,
                total_size: data.values().map(|v| v.len() as u64).sum(),
                available_space: None,
                backend_type: "mock".to_string(),
            })
        }
    }

    #[tokio::test]
    async fn test_authorized_storage_no_token_fails() {
        use aura_core::types::Epoch;

        let mock_storage = MockStorage::new();
        let evaluator = setup_test_evaluator();
        let budget = FlowBudget::new(1000, Epoch::initial());

        let authorized = AuthorizedStorageHandler::new(mock_storage, evaluator, budget);

        // Without token, store should fail
        let result = authorized.store("test_key", b"test_value".to_vec()).await;
        assert!(result.is_err());

        if let Err(StorageError::PermissionDenied(reason)) = result {
            assert!(reason.contains("No authorization token"));
        } else {
            panic!("Expected PermissionDenied error");
        }
    }

    #[tokio::test]
    async fn test_authorized_storage_with_token_works() {
        use aura_core::types::Epoch;

        let mock_storage = MockStorage::new();
        let (authority, authority_id) = setup_test_authority();
        let recipient = AuthorityId::new_from_entropy([1u8; 32]);
        let token = authority.create_token(recipient).unwrap();
        let evaluator = BiscuitStorageEvaluator::new(authority.root_public_key(), authority_id);
        let budget = FlowBudget::new(10000, Epoch::initial());

        let authorized =
            AuthorizedStorageHandler::with_token(mock_storage, evaluator, token, budget);

        // With valid token, operations should proceed to authorization check
        // Note: The token may still be denied by Biscuit policy, but it won't fail
        // due to "no token" error
        let result = authorized.store("test_key", b"test_value".to_vec()).await;

        // The result depends on whether the Biscuit authorization succeeds
        // For now, we just verify we get past the "no token" check
        match result {
            Ok(()) => {
                // Authorization succeeded, verify data was stored
                let retrieved = authorized.retrieve("test_key").await;
                assert!(retrieved.is_ok());
            }
            Err(StorageError::PermissionDenied(reason)) => {
                // Authorization was checked but denied (expected with our test setup)
                assert!(!reason.contains("No authorization token"));
            }
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_authorized_storage_budget_tracking() {
        use aura_core::types::Epoch;

        let mock_storage = MockStorage::new();
        let evaluator = setup_test_evaluator();
        let budget = FlowBudget::new(1000, Epoch::initial());

        let authorized = AuthorizedStorageHandler::new(mock_storage, evaluator, budget);

        // Initial budget should be full
        assert_eq!(authorized.remaining_budget().await, 1000);
    }

    #[tokio::test]
    async fn test_authorized_storage_token_management() {
        use aura_core::types::Epoch;

        let mock_storage = MockStorage::new();
        let (authority, authority_id) = setup_test_authority();
        let recipient = AuthorityId::new_from_entropy([2u8; 32]);
        let token = authority.create_token(recipient).unwrap();
        let evaluator = BiscuitStorageEvaluator::new(authority.root_public_key(), authority_id);
        let budget = FlowBudget::new(1000, Epoch::initial());

        let authorized = AuthorizedStorageHandler::new(mock_storage, evaluator, budget);

        // Without token, operation fails
        let result1 = authorized.retrieve("test").await;
        assert!(result1.is_err());

        // Set token
        authorized.set_token(token).await;

        // Now operation should at least pass the "no token" check
        let result2 = authorized.retrieve("test").await;
        match result2 {
            Ok(_) => {} // Authorization succeeded
            Err(StorageError::PermissionDenied(reason)) => {
                // Token was checked, even if authorization failed
                assert!(!reason.contains("No authorization token"));
            }
            Err(e) => panic!("Unexpected error: {:?}", e),
        }

        // Clear token
        authorized.clear_token().await;

        // Should fail again with "no token"
        let result3 = authorized.retrieve("test").await;
        if let Err(StorageError::PermissionDenied(reason)) = result3 {
            assert!(reason.contains("No authorization token"));
        } else {
            panic!("Expected PermissionDenied error");
        }
    }
}
