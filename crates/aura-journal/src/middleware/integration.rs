//! Integration between journal middleware and the existing LedgerHandler effect system

use super::{JournalContext, JournalHandler};
use crate::effects::{ActorId, LedgerEffect, LedgerResult, LedgerValue};
use crate::error::{Error, Result};
use crate::middleware::JournalMiddlewareStack;
use crate::operations::JournalOperation;
use crate::state::AccountState;
use aura_types::DeviceId;
use std::sync::{Arc, RwLock};

/// Handler that integrates journal operations with the effect system
pub struct EffectSystemHandler {
    /// Account state (protected by RwLock for concurrent access)
    state: Arc<RwLock<AccountState>>,

    /// Effect processor for handling ledger effects
    effect_processor: Arc<dyn EffectProcessor>,
}

impl EffectSystemHandler {
    /// Create a new effect system handler
    pub fn new(
        state: Arc<RwLock<AccountState>>,
        effect_processor: Arc<dyn EffectProcessor>,
    ) -> Self {
        Self {
            state,
            effect_processor,
        }
    }

    /// Create a complete middleware stack with effect system integration
    pub fn with_middleware_stack(
        state: Arc<RwLock<AccountState>>,
        effect_processor: Arc<dyn EffectProcessor>,
    ) -> JournalMiddlewareStack {
        use super::{
            AuditConfig, AuditMiddleware, AuthorizationConfig, AuthorizationMiddleware,
            CachingConfig, CachingMiddleware, ObservabilityConfig, ObservabilityMiddleware,
            RateLimitingConfig, RateLimitingMiddleware, RetryConfig, RetryMiddleware,
            ValidationConfig, ValidationMiddleware,
        };

        let handler = Arc::new(Self::new(state, effect_processor));

        JournalMiddlewareStack::new(handler)
            .with_middleware(Arc::new(ValidationMiddleware::new(
                ValidationConfig::default(),
            )))
            .with_middleware(Arc::new(AuthorizationMiddleware::with_default_checker(
                AuthorizationConfig::default(),
            )))
            .with_middleware(Arc::new(RateLimitingMiddleware::new(
                RateLimitingConfig::default(),
            )))
            .with_middleware(Arc::new(RetryMiddleware::new(RetryConfig::default())))
            .with_middleware(Arc::new(CachingMiddleware::new(
                CachingConfig::default(),
                Arc::new(aura_types::effects::SystemTimeEffects::new()),
            )))
            .with_middleware(Arc::new(ObservabilityMiddleware::new(
                ObservabilityConfig::default(),
            )))
            .with_middleware(Arc::new(AuditMiddleware::with_console_logger(
                AuditConfig::default(),
            )))
    }
}

impl JournalHandler for EffectSystemHandler {
    fn handle(
        &self,
        operation: JournalOperation,
        context: &JournalContext,
    ) -> Result<serde_json::Value> {
        // Convert journal operation to ledger effect
        let effect = self.journal_operation_to_effect(&operation, &context.device_id)?;

        // Process the effect through the effect system
        let result = self.effect_processor.process_effect(effect)?;

        // Convert the result back to JSON for middleware compatibility
        self.ledger_value_to_json(result, &operation)
    }
}

impl EffectSystemHandler {
    fn journal_operation_to_effect(
        &self,
        operation: &JournalOperation,
        device_id: &DeviceId,
    ) -> Result<LedgerEffect> {
        let actor_id = ActorId::from(device_id.clone());

        match operation {
            JournalOperation::AddDevice { device } => {
                let op = crate::Operation::AddDevice {
                    device: device.clone(),
                };
                Ok(LedgerEffect::ApplyOperation { op, actor_id })
            }

            JournalOperation::RemoveDevice { device_id } => {
                let op = crate::Operation::RemoveDevice {
                    device_id: device_id.clone(),
                };
                Ok(LedgerEffect::ApplyOperation { op, actor_id })
            }

            JournalOperation::AddGuardian { guardian } => {
                let op = crate::Operation::AddGuardian {
                    guardian: guardian.clone(),
                };
                Ok(LedgerEffect::ApplyOperation { op, actor_id })
            }

            JournalOperation::IncrementEpoch => {
                let op = crate::Operation::IncrementEpoch;
                Ok(LedgerEffect::ApplyOperation { op, actor_id })
            }

            JournalOperation::GetDevices => Ok(LedgerEffect::GetDevices),

            JournalOperation::GetEpoch => Ok(LedgerEffect::GetEpoch),
        }
    }

    fn ledger_value_to_json(
        &self,
        value: LedgerValue,
        operation: &JournalOperation,
    ) -> Result<serde_json::Value> {
        match value {
            LedgerValue::Changes(changes) => Ok(serde_json::json!({
                "operation": format!("{:?}", operation),
                "changes_applied": changes.len(),
                "success": true
            })),

            LedgerValue::Merged => Ok(serde_json::json!({
                "operation": format!("{:?}", operation),
                "merged": true,
                "success": true
            })),

            LedgerValue::Query(result) => Ok(serde_json::json!({
                "operation": format!("{:?}", operation),
                "result": result,
                "success": true
            })),

            LedgerValue::EventEmitted => Ok(serde_json::json!({
                "operation": format!("{:?}", operation),
                "event_emitted": true,
                "success": true
            })),

            LedgerValue::Epoch(epoch) => Ok(serde_json::json!({
                "operation": format!("{:?}", operation),
                "epoch": epoch,
                "success": true
            })),

            LedgerValue::Devices(devices) => Ok(serde_json::json!({
                "operation": format!("{:?}", operation),
                "devices_count": devices.len(),
                "devices": devices.iter().map(|d| d.device_id.to_string()).collect::<Vec<_>>(),
                "success": true
            })),

            LedgerValue::Boolean(result) => Ok(serde_json::json!({
                "operation": format!("{:?}", operation),
                "result": result,
                "success": true
            })),
        }
    }
}

/// Trait for processing ledger effects
pub trait EffectProcessor: Send + Sync {
    /// Process a ledger effect and return the result
    fn process_effect(&self, effect: LedgerEffect) -> LedgerResult;
}

/// Default effect processor that delegates to AccountState methods
pub struct DefaultEffectProcessor {
    /// Account state
    state: Arc<RwLock<AccountState>>,
}

impl DefaultEffectProcessor {
    /// Create a new default effect processor
    pub fn new(state: Arc<RwLock<AccountState>>) -> Self {
        Self { state }
    }
}

impl EffectProcessor for DefaultEffectProcessor {
    fn process_effect(&self, effect: LedgerEffect) -> LedgerResult {
        match effect {
            LedgerEffect::ApplyOperation { op, actor_id: _ } => {
                let mut state = self.state.write().map_err(|_| {
                    Error::storage_failed("Failed to acquire write lock on account state")
                })?;

                match op {
                    crate::Operation::AddDevice { device } => {
                        let changes = state.add_device(device)?;
                        Ok(LedgerValue::Changes(changes))
                    }

                    crate::Operation::RemoveDevice { device_id } => {
                        let changes = state.remove_device(device_id)?;
                        Ok(LedgerValue::Changes(changes))
                    }

                    crate::Operation::AddGuardian { guardian } => {
                        let changes = state.add_guardian(guardian)?;
                        Ok(LedgerValue::Changes(changes))
                    }

                    crate::Operation::IncrementEpoch => {
                        let changes = state.increment_epoch()?;
                        Ok(LedgerValue::Changes(changes))
                    }

                    // TODO: Implement remaining operations
                    _ => {
                        // For now, return empty changes for unimplemented operations
                        Ok(LedgerValue::Changes(vec![]))
                    }
                }
            }

            LedgerEffect::MergeRemoteChanges {
                changes,
                from_device: _,
            } => {
                let mut state = self.state.write().map_err(|_| {
                    Error::storage_failed("Failed to acquire write lock on account state")
                })?;

                // Apply remote changes to the state
                for change in changes {
                    state.merge_change(change)?;
                }

                Ok(LedgerValue::Merged)
            }

            LedgerEffect::QueryState { path, as_of: _ } => {
                let state = self.state.read().map_err(|_| {
                    Error::storage_failed("Failed to acquire read lock on account state")
                })?;

                // Query the state at the specified path
                let result = state.query_path(&path)?;
                Ok(LedgerValue::Query(result))
            }

            LedgerEffect::GetEpoch => {
                let state = self.state.read().map_err(|_| {
                    Error::storage_failed("Failed to acquire read lock on account state")
                })?;

                let epoch = state.get_epoch();
                Ok(LedgerValue::Epoch(epoch))
            }

            LedgerEffect::GetDevices => {
                let state = self.state.read().map_err(|_| {
                    Error::storage_failed("Failed to acquire read lock on account state")
                })?;

                let devices = state.get_devices();
                Ok(LedgerValue::Devices(devices))
            }

            LedgerEffect::HasOperation { op_id } => {
                let state = self.state.read().map_err(|_| {
                    Error::storage_failed("Failed to acquire read lock on account state")
                })?;

                let has_op = state.has_operation(&op_id);
                Ok(LedgerValue::Boolean(has_op))
            }
        }
    }
}

/// Builder for creating middleware-enabled journal handlers
pub struct JournalHandlerBuilder {
    state: Arc<RwLock<AccountState>>,
    effect_processor: Option<Arc<dyn EffectProcessor>>,
    middleware_configs: MiddlewareConfigs,
}

impl JournalHandlerBuilder {
    /// Create a new builder
    pub fn new(state: Arc<RwLock<AccountState>>) -> Self {
        Self {
            state,
            effect_processor: None,
            middleware_configs: MiddlewareConfigs::default(),
        }
    }

    /// Set a custom effect processor
    pub fn with_effect_processor(mut self, processor: Arc<dyn EffectProcessor>) -> Self {
        self.effect_processor = Some(processor);
        self
    }

    /// Configure observability middleware
    pub fn with_observability(mut self, config: super::ObservabilityConfig) -> Self {
        self.middleware_configs.observability = Some(config);
        self
    }

    /// Configure authorization middleware
    pub fn with_authorization(mut self, config: super::AuthorizationConfig) -> Self {
        self.middleware_configs.authorization = Some(config);
        self
    }

    /// Configure audit middleware
    pub fn with_audit(mut self, config: super::AuditConfig) -> Self {
        self.middleware_configs.audit = Some(config);
        self
    }

    /// Configure validation middleware
    pub fn with_validation(mut self, config: super::ValidationConfig) -> Self {
        self.middleware_configs.validation = Some(config);
        self
    }

    /// Configure caching middleware
    pub fn with_caching(mut self, config: super::CachingConfig) -> Self {
        self.middleware_configs.caching = Some(config);
        self
    }

    /// Configure retry middleware
    pub fn with_retry(mut self, config: super::RetryConfig) -> Self {
        self.middleware_configs.retry = Some(config);
        self
    }

    /// Configure rate limiting middleware
    pub fn with_rate_limiting(mut self, config: super::RateLimitingConfig) -> Self {
        self.middleware_configs.rate_limiting = Some(config);
        self
    }

    /// Build the middleware stack
    pub fn build(self) -> JournalMiddlewareStack {
        let effect_processor = self
            .effect_processor
            .unwrap_or_else(|| Arc::new(DefaultEffectProcessor::new(self.state.clone())));

        let handler = Arc::new(EffectSystemHandler::new(self.state, effect_processor));
        let mut stack = JournalMiddlewareStack::new(handler);

        // Add middleware in execution order (first added = first executed)
        if let Some(config) = self.middleware_configs.validation {
            stack = stack.with_middleware(Arc::new(super::ValidationMiddleware::new(config)));
        }

        if let Some(config) = self.middleware_configs.authorization {
            stack = stack.with_middleware(Arc::new(
                super::AuthorizationMiddleware::with_default_checker(config),
            ));
        }

        if let Some(config) = self.middleware_configs.rate_limiting {
            stack = stack.with_middleware(Arc::new(super::RateLimitingMiddleware::new(config)));
        }

        if let Some(config) = self.middleware_configs.retry {
            stack = stack.with_middleware(Arc::new(super::RetryMiddleware::new(config)));
        }

        if let Some(config) = self.middleware_configs.caching {
            stack = stack.with_middleware(Arc::new(super::CachingMiddleware::new(
                config,
                Arc::new(aura_types::effects::SystemTimeEffects::new()),
            )));
        }

        if let Some(config) = self.middleware_configs.observability {
            stack = stack.with_middleware(Arc::new(super::ObservabilityMiddleware::new(config)));
        }

        if let Some(config) = self.middleware_configs.audit {
            stack = stack.with_middleware(Arc::new(super::AuditMiddleware::with_console_logger(
                config,
            )));
        }

        stack
    }
}

/// Configuration container for all middleware types
#[derive(Default)]
struct MiddlewareConfigs {
    observability: Option<super::ObservabilityConfig>,
    authorization: Option<super::AuthorizationConfig>,
    audit: Option<super::AuditConfig>,
    validation: Option<super::ValidationConfig>,
    caching: Option<super::CachingConfig>,
    retry: Option<super::RetryConfig>,
    rate_limiting: Option<super::RateLimitingConfig>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::JournalOperation;
    use aura_crypto::Effects;
    use aura_types::{AccountIdExt, DeviceIdExt};

    #[test]
    fn test_effect_system_integration() {
        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);

        // Create account state
        let state = Arc::new(RwLock::new(
            AccountState::new(account_id, effects.ed25519_keypair().public).unwrap(),
        ));

        // Create handler with default effect processor
        let handler =
            EffectSystemHandler::new(state.clone(), Arc::new(DefaultEffectProcessor::new(state)));

        let context = super::JournalContext::new(account_id, device_id, "test".to_string());
        let operation = JournalOperation::GetEpoch;

        let result = handler.handle(operation, &context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_builder_pattern() {
        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);

        let state = Arc::new(RwLock::new(
            AccountState::new(account_id, effects.ed25519_keypair().public).unwrap(),
        ));

        let stack = JournalHandlerBuilder::new(state)
            .with_observability(super::super::ObservabilityConfig::default())
            .with_validation(super::super::ValidationConfig::default())
            .build();

        let context = super::JournalContext::new(
            account_id,
            aura_types::DeviceId::new_with_effects(&effects),
            "test".to_string(),
        );
        let operation = JournalOperation::GetEpoch;

        let result = stack.process(operation, &context);
        assert!(result.is_ok());
    }
}
