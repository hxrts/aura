//! Chaos coordination middleware implementation
//!
//! Provides chaos engineering capabilities for simulation including coordinated
//! chaos experiments, failure scenarios, and distributed system stress testing.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use aura_core::identifiers::DeviceId;
use aura_core::LocalSessionType;
use aura_protocol::handlers::{AuraHandler, AuraHandlerError, EffectType, ExecutionMode};

/// Chaos experiment configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChaosConfig {
    /// Device ID for the chaos experiments
    pub device_id: DeviceId,
    /// Seed for deterministic chaos generation
    pub chaos_seed: u64,
    /// List of active experiment names
    pub active_experiments: Vec<String>,
}

/// Chaos coordination middleware for simulation effect system
pub struct ChaosCoordinationMiddleware {
    device_id: DeviceId,
    chaos_seed: u64,
    execution_mode: ExecutionMode,
    active_experiments: HashMap<String, serde_json::Value>,
}

impl ChaosCoordinationMiddleware {
    /// Create new chaos coordination middleware
    pub fn new(device_id: DeviceId, chaos_seed: u64) -> Self {
        Self {
            device_id,
            chaos_seed,
            execution_mode: ExecutionMode::Simulation { seed: chaos_seed },
            active_experiments: HashMap::new(),
        }
    }

    /// Create for simulation mode
    pub fn for_simulation(device_id: DeviceId, seed: u64) -> Self {
        Self::new(device_id, seed)
    }

    /// Check if this middleware handles chaos coordination effects
    fn handles_effect(&self, effect_type: EffectType) -> bool {
        matches!(effect_type, EffectType::ChaosCoordination)
    }

    /// Start a chaos experiment
    pub fn start_experiment(&mut self, name: String, config: serde_json::Value) {
        self.active_experiments.insert(name, config);
    }

    /// Stop a chaos experiment
    pub fn stop_experiment(&mut self, name: &str) -> Option<serde_json::Value> {
        self.active_experiments.remove(name)
    }

    /// Get active experiments
    pub fn active_experiments(&self) -> &HashMap<String, serde_json::Value> {
        &self.active_experiments
    }

    /// Get the device ID
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }
}

#[async_trait]
impl AuraHandler for ChaosCoordinationMiddleware {
    async fn execute_effect(
        &self,
        effect_type: EffectType,
        operation: &str,
        parameters: &[u8],
        _ctx: &aura_protocol::handlers::context_immutable::AuraContext,
    ) -> Result<Vec<u8>, AuraHandlerError> {
        if !self.handles_effect(effect_type) {
            return Err(AuraHandlerError::UnsupportedEffect { effect_type });
        }

        match operation {
            "start_experiment" => {
                // Parse experiment parameters
                let experiment_name = format!("chaos_experiment_{}", self.chaos_seed);
                let _config = serde_json::json!({
                    "type": "network_partition",
                    "duration": 30,
                    "affected_nodes": [self.device_id.to_string()]
                });

                // In immutable context, just return the experiment name without actual mutation
                Ok(serde_json::to_vec(&experiment_name).unwrap_or_default())
            }
            "stop_experiment" => {
                let _experiment_name = String::from_utf8_lossy(parameters);
                // In immutable context, just return success without actual mutation
                Ok(serde_json::to_vec(&true).unwrap_or_default())
            }
            "list_experiments" => {
                let experiment_names: Vec<_> = self.active_experiments.keys().collect();
                Ok(serde_json::to_vec(&experiment_names).unwrap_or_default())
            }
            "inject_chaos" => {
                // Simulate chaos injection based on seed
                let chaos_value = (self.chaos_seed % 100) as u8;
                Ok(vec![chaos_value])
            }
            _ => Err(AuraHandlerError::UnknownOperation {
                effect_type,
                operation: operation.to_string(),
            }),
        }
    }

    async fn execute_session(
        &self,
        _session: LocalSessionType,
        _ctx: &aura_protocol::handlers::context_immutable::AuraContext,
    ) -> Result<(), AuraHandlerError> {
        // Chaos coordination doesn't handle sessions directly
        Ok(())
    }

    fn supports_effect(&self, effect_type: EffectType) -> bool {
        self.handles_effect(effect_type)
    }

    fn execution_mode(&self) -> ExecutionMode {
        self.execution_mode
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_protocol::handlers::context_immutable::AuraContext;

    #[tokio::test]
    async fn test_chaos_coordination_creation() {
        let device_id = DeviceId::new();
        let middleware = ChaosCoordinationMiddleware::for_simulation(device_id, 42);

        assert_eq!(middleware.device_id(), device_id);
        assert_eq!(middleware.chaos_seed, 42);
        assert_eq!(
            middleware.execution_mode(),
            ExecutionMode::Simulation { seed: 42 }
        );
    }

    #[tokio::test]
    async fn test_chaos_effect_support() {
        let device_id = DeviceId::new();
        let middleware = ChaosCoordinationMiddleware::for_simulation(device_id, 42);

        assert!(middleware.supports_effect(EffectType::ChaosCoordination));
        assert!(!middleware.supports_effect(EffectType::Crypto));
        assert!(!middleware.supports_effect(EffectType::Network));
    }

    #[tokio::test]
    async fn test_chaos_operations() {
        let device_id = DeviceId::new();
        let middleware = ChaosCoordinationMiddleware::for_simulation(device_id, 42);
        let ctx = AuraContext::for_testing(device_id);

        // Test start experiment
        let result = middleware
            .execute_effect(
                EffectType::ChaosCoordination,
                "start_experiment",
                b"test_experiment",
                &ctx,
            )
            .await;
        assert!(result.is_ok());

        // Test list experiments
        let result = middleware
            .execute_effect(EffectType::ChaosCoordination, "list_experiments", b"", &ctx)
            .await;
        assert!(result.is_ok());

        // Test inject chaos
        let result = middleware
            .execute_effect(EffectType::ChaosCoordination, "inject_chaos", b"", &ctx)
            .await;
        assert!(result.is_ok());
        let chaos_value = result.unwrap();
        assert_eq!(chaos_value.len(), 1);
        assert_eq!(chaos_value[0], 42); // Based on seed % 100
    }

    #[test]
    fn test_experiment_management() {
        let device_id = DeviceId::new();
        let mut middleware = ChaosCoordinationMiddleware::for_simulation(device_id, 42);

        // Start experiment
        let config = serde_json::json!({"type": "test"});
        middleware.start_experiment("test_exp".to_string(), config.clone());
        assert_eq!(middleware.active_experiments().len(), 1);

        // Stop experiment
        let stopped = middleware.stop_experiment("test_exp");
        assert!(stopped.is_some());
        assert_eq!(stopped.unwrap(), config);
        assert_eq!(middleware.active_experiments().len(), 0);
    }
}
