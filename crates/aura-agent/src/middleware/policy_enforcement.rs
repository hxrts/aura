//! Policy enforcement middleware for access control and compliance

use super::{AgentContext, AgentHandler, AgentMiddleware};
use crate::error::Result;
use crate::middleware::AgentOperation;
use crate::utils::time::AgentTimeProvider;
use aura_types::AuraError;
use aura_types::DeviceId;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

/// Policy enforcement middleware that validates operations against policies
pub struct PolicyEnforcementMiddleware {
    /// Policy store
    policies: Arc<RwLock<PolicyStore>>,

    /// Configuration
    config: PolicyConfig,

    /// Time provider for timestamp generation
    time_provider: Arc<AgentTimeProvider>,
}

impl PolicyEnforcementMiddleware {
    /// Create new policy enforcement middleware with production time provider
    pub fn new(config: PolicyConfig) -> Self {
        Self {
            policies: Arc::new(RwLock::new(PolicyStore::new())),
            config,
            time_provider: Arc::new(AgentTimeProvider::production()),
        }
    }

    /// Create new policy enforcement middleware with custom time provider
    pub fn with_time_provider(config: PolicyConfig, time_provider: Arc<AgentTimeProvider>) -> Self {
        Self {
            policies: Arc::new(RwLock::new(PolicyStore::new())),
            config,
            time_provider,
        }
    }

    /// Add a policy rule
    pub fn add_policy(&self, policy: PolicyRule) -> Result<()> {
        let mut policies = self.policies.write().map_err(|_| {
            AuraError::internal_error("Failed to acquire write lock on policies".to_string())
        })?;

        policies.add_policy(policy);
        Ok(())
    }

    /// Remove a policy rule
    pub fn remove_policy(&self, policy_id: &str) -> Result<bool> {
        let mut policies = self.policies.write().map_err(|_| {
            AuraError::internal_error("Failed to acquire write lock on policies".to_string())
        })?;

        Ok(policies.remove_policy(policy_id))
    }

    /// Get policy statistics
    pub fn stats(&self) -> PolicyStats {
        let policies = self.policies.read().unwrap();
        policies.stats()
    }
}

impl AgentMiddleware for PolicyEnforcementMiddleware {
    fn process(
        &self,
        operation: AgentOperation,
        context: &AgentContext,
        next: &dyn AgentHandler,
    ) -> Result<serde_json::Value> {
        // Skip policy enforcement if disabled
        if !self.config.enable_policy_enforcement {
            return next.handle(operation, context);
        }

        // Evaluate policies for this operation
        self.evaluate_policies(&operation, context)?;

        // Policies passed, proceed with operation
        next.handle(operation, context)
    }

    fn name(&self) -> &str {
        "policy_enforcement"
    }
}

impl PolicyEnforcementMiddleware {
    fn evaluate_policies(&self, operation: &AgentOperation, context: &AgentContext) -> Result<()> {
        let policies = self.policies.read().map_err(|_| {
            AuraError::internal_error("Failed to acquire read lock on policies".to_string())
        })?;

        let operation_type = self.operation_to_type(operation);
        let applicable_policies =
            policies.get_applicable_policies(&operation_type, &context.device_id);

        for policy in applicable_policies {
            self.evaluate_policy(policy, operation, context)?;
        }

        Ok(())
    }

    fn evaluate_policy(
        &self,
        policy: &PolicyRule,
        operation: &AgentOperation,
        context: &AgentContext,
    ) -> Result<()> {
        match &policy.policy_type {
            PolicyType::DeviceRestriction { allowed_devices } => {
                if !allowed_devices.contains(&context.device_id) {
                    return Err(AuraError::policy_violation(format!(
                        "Device {} not allowed for operation by policy {}",
                        context.device_id, policy.id
                    )));
                }
            }

            PolicyType::TimeRestriction { allowed_hours } => {
                let current_hour = self.get_current_hour();
                if !allowed_hours.contains(&current_hour) {
                    return Err(AuraError::policy_violation(format!(
                        "Operation not allowed at hour {} by policy {}",
                        current_hour, policy.id
                    )));
                }
            }

            PolicyType::OperationLimit {
                max_operations,
                window_seconds,
            } => {
                if !self.check_operation_limit(
                    &context.device_id,
                    &policy.id,
                    *max_operations,
                    *window_seconds,
                )? {
                    return Err(AuraError::policy_violation(format!(
                        "Operation limit exceeded by policy {}",
                        policy.id
                    )));
                }
            }

            PolicyType::SessionRequirement {
                required_session_types: _,
            } => {
                if let Some(session_id) = &context.session_id {
                    // TODO: Check session type - for now we just check if session exists
                    if session_id.is_empty() {
                        return Err(AuraError::policy_violation(format!(
                            "Valid session required by policy {}",
                            policy.id
                        )));
                    }
                } else {
                    return Err(AuraError::policy_violation(format!(
                        "Session required by policy {}",
                        policy.id
                    )));
                }
            }

            PolicyType::DataSizeLimit { max_size_bytes } => {
                if let AgentOperation::StoreData { data, .. } = operation {
                    if data.len() > *max_size_bytes {
                        return Err(AuraError::policy_violation(format!(
                            "Data size {} exceeds limit {} by policy {}",
                            data.len(),
                            max_size_bytes,
                            policy.id
                        )));
                    }
                }
            }

            PolicyType::CapabilityRequirement {
                required_capabilities,
            } => {
                // Check if device has required capabilities (simplified check)
                for capability in required_capabilities {
                    if !self.device_has_capability(&context.device_id, capability)? {
                        return Err(AuraError::policy_violation(format!(
                            "Device lacks required capability '{}' by policy {}",
                            capability, policy.id
                        )));
                    }
                }
            }
        }

        Ok(())
    }

    fn operation_to_type(&self, operation: &AgentOperation) -> OperationType {
        match operation {
            AgentOperation::Initialize { .. } => OperationType::Initialize,
            AgentOperation::DeriveIdentity { .. } => OperationType::DeriveIdentity,
            AgentOperation::StartSession { .. } => OperationType::StartSession,
            AgentOperation::StoreData { .. } => OperationType::StoreData,
            AgentOperation::RetrieveData { .. } => OperationType::RetrieveData,
            AgentOperation::InitiateBackup { .. } => OperationType::InitiateBackup,
            AgentOperation::GetStatus => OperationType::GetStatus,
        }
    }

    fn get_current_hour(&self) -> u8 {
        let timestamp = self.time_provider.timestamp_secs();
        // Simplified hour calculation (UTC)
        ((timestamp / 3600) % 24) as u8
    }

    fn check_operation_limit(
        &self,
        device_id: &DeviceId,
        policy_id: &str,
        max_operations: usize,
        window_seconds: u64,
    ) -> Result<bool> {
        let mut policies = self.policies.write().map_err(|_| {
            AuraError::internal_error("Failed to acquire write lock on policies".to_string())
        })?;

        let now = self.time_provider.timestamp_secs();
        let key = format!("{}:{}", device_id, policy_id);

        let operations = policies
            .operation_history
            .entry(key)
            .or_insert_with(Vec::new);

        // Remove old operations outside the window
        operations.retain(|&timestamp| now - timestamp < window_seconds);

        // Check if adding this operation would exceed the limit
        if operations.len() >= max_operations {
            Ok(false)
        } else {
            // Record this operation
            operations.push(now);
            Ok(true)
        }
    }

    fn device_has_capability(&self, device_id: &DeviceId, capability: &str) -> Result<bool> {
        // Simplified capability check - in real implementation this would
        // check device certificates, attestations, etc.
        let policies = self.policies.read().map_err(|_| {
            AuraError::internal_error("Failed to acquire read lock on policies".to_string())
        })?;

        Ok(policies
            .device_capabilities
            .get(&device_id.to_string())
            .map(|caps| caps.contains(capability))
            .unwrap_or(false))
    }
}

/// Configuration for policy enforcement middleware
#[derive(Debug, Clone)]
pub struct PolicyConfig {
    /// Whether policy enforcement is enabled
    pub enable_policy_enforcement: bool,

    /// Whether to log policy violations
    pub log_policy_violations: bool,

    /// Whether to allow admin bypass
    pub allow_admin_bypass: bool,

    /// Default device capabilities
    pub default_device_capabilities: Vec<String>,
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            enable_policy_enforcement: true,
            log_policy_violations: true,
            allow_admin_bypass: true,
            default_device_capabilities: vec![
                "basic_operations".to_string(),
                "secure_storage".to_string(),
            ],
        }
    }
}

/// Policy rule definition
#[derive(Debug, Clone)]
pub struct PolicyRule {
    /// Unique policy identifier
    pub id: String,

    /// Human-readable policy name
    pub name: String,

    /// Policy description
    pub description: String,

    /// Operations this policy applies to
    pub applies_to: Vec<OperationType>,

    /// Policy type and parameters
    pub policy_type: PolicyType,

    /// Whether this policy is enabled
    pub enabled: bool,

    /// Policy priority (higher = evaluated first)
    pub priority: u32,
}

/// Types of policies that can be enforced
#[derive(Debug, Clone)]
pub enum PolicyType {
    /// Restrict operations to specific devices
    DeviceRestriction { allowed_devices: HashSet<DeviceId> },

    /// Restrict operations to specific time windows
    TimeRestriction {
        allowed_hours: HashSet<u8>, // 0-23
    },

    /// Limit number of operations per time window
    OperationLimit {
        max_operations: usize,
        window_seconds: u64,
    },

    /// Require specific session types
    SessionRequirement { required_session_types: Vec<String> },

    /// Limit data size for storage operations
    DataSizeLimit { max_size_bytes: usize },

    /// Require specific device capabilities
    CapabilityRequirement { required_capabilities: Vec<String> },
}

/// Operation types for policy matching
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum OperationType {
    Initialize,
    DeriveIdentity,
    StartSession,
    StoreData,
    RetrieveData,
    InitiateBackup,
    GetStatus,
}

/// Policy store for managing rules and operation history
struct PolicyStore {
    policies: HashMap<String, PolicyRule>,
    operation_history: HashMap<String, Vec<u64>>, // device:policy -> timestamps
    device_capabilities: HashMap<String, HashSet<String>>, // device_id -> capabilities
    policy_evaluations: u64,
    policy_violations: u64,
}

impl PolicyStore {
    fn new() -> Self {
        Self {
            policies: HashMap::new(),
            operation_history: HashMap::new(),
            device_capabilities: HashMap::new(),
            policy_evaluations: 0,
            policy_violations: 0,
        }
    }

    fn add_policy(&mut self, policy: PolicyRule) {
        self.policies.insert(policy.id.clone(), policy);
    }

    fn remove_policy(&mut self, policy_id: &str) -> bool {
        self.policies.remove(policy_id).is_some()
    }

    fn get_applicable_policies(
        &self,
        operation_type: &OperationType,
        _device_id: &DeviceId,
    ) -> Vec<&PolicyRule> {
        let mut applicable: Vec<&PolicyRule> = self
            .policies
            .values()
            .filter(|policy| {
                policy.enabled
                    && (policy.applies_to.is_empty() || policy.applies_to.contains(operation_type))
            })
            .collect();

        // Sort by priority (higher first)
        applicable.sort_by(|a, b| b.priority.cmp(&a.priority));
        applicable
    }

    fn stats(&self) -> PolicyStats {
        let active_policies = self.policies.values().filter(|p| p.enabled).count();

        PolicyStats {
            total_policies: self.policies.len(),
            active_policies,
            policy_evaluations: self.policy_evaluations,
            policy_violations: self.policy_violations,
        }
    }
}

/// Policy statistics
#[derive(Debug, Clone)]
pub struct PolicyStats {
    /// Total number of policies
    pub total_policies: usize,

    /// Number of active policies
    pub active_policies: usize,

    /// Total policy evaluations
    pub policy_evaluations: u64,

    /// Total policy violations
    pub policy_violations: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::handler::NoOpHandler;
    use aura_crypto::Effects;
    use aura_types::{AccountIdExt, DeviceIdExt};

    #[test]
    fn test_policy_enforcement_middleware() {
        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);

        let middleware = PolicyEnforcementMiddleware::new(PolicyConfig::default());
        let handler = NoOpHandler;
        let context = AgentContext::new(account_id, device_id, "test".to_string());
        let operation = AgentOperation::GetStatus;

        let result = middleware.process(operation, &context, &handler);
        assert!(result.is_ok());
    }

    #[test]
    fn test_device_restriction_policy() {
        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let allowed_device = aura_types::DeviceId::new_with_effects(&effects);
        let blocked_device = aura_types::DeviceId::new_with_effects(&effects);

        let middleware = PolicyEnforcementMiddleware::new(PolicyConfig::default());

        // Add device restriction policy
        let mut allowed_devices = HashSet::new();
        allowed_devices.insert(allowed_device.clone());

        let policy = PolicyRule {
            id: "device-restriction".to_string(),
            name: "Device Restriction".to_string(),
            description: "Only allow specific devices".to_string(),
            applies_to: vec![OperationType::GetStatus],
            policy_type: PolicyType::DeviceRestriction { allowed_devices },
            enabled: true,
            priority: 100,
        };

        middleware.add_policy(policy).unwrap();

        let handler = NoOpHandler;
        let operation = AgentOperation::GetStatus;

        // Allowed device should succeed
        let allowed_context = AgentContext::new(account_id, allowed_device, "test".to_string());
        let result = middleware.process(operation.clone(), &allowed_context, &handler);
        assert!(result.is_ok());

        // Blocked device should fail
        let blocked_context = AgentContext::new(account_id, blocked_device, "test".to_string());
        let result = middleware.process(operation, &blocked_context, &handler);
        assert!(result.is_err());
    }

    #[test]
    fn test_data_size_limit_policy() {
        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);

        let middleware = PolicyEnforcementMiddleware::new(PolicyConfig::default());

        // Add data size limit policy
        let policy = PolicyRule {
            id: "data-size-limit".to_string(),
            name: "Data Size Limit".to_string(),
            description: "Limit data size to 100 bytes".to_string(),
            applies_to: vec![OperationType::StoreData],
            policy_type: PolicyType::DataSizeLimit {
                max_size_bytes: 100,
            },
            enabled: true,
            priority: 100,
        };

        middleware.add_policy(policy).unwrap();

        let handler = NoOpHandler;
        let context = AgentContext::new(account_id, device_id, "test".to_string());

        // Small data should succeed
        let small_operation = AgentOperation::StoreData {
            data: vec![0u8; 50], // 50 bytes
            capabilities: vec!["read".to_string()],
        };
        let result = middleware.process(small_operation, &context, &handler);
        assert!(result.is_ok());

        // Large data should fail
        let large_operation = AgentOperation::StoreData {
            data: vec![0u8; 200], // 200 bytes
            capabilities: vec!["read".to_string()],
        };
        let result = middleware.process(large_operation, &context, &handler);
        assert!(result.is_err());
    }
}
