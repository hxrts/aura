//! Fault injection middleware implementation
//!
//! Provides fault injection capabilities for simulation including message drops,
//! delays, corruption, Byzantine behavior, network partitions, and node crashes.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use aura_protocol::handlers::{
    AuraContext, AuraHandler, AuraHandlerError, EffectType, ExecutionMode,
};
use aura_core::identifiers::DeviceId;
use aura_core::LocalSessionType;

/// Fault injection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaultConfig {
    /// ID of the device this configuration applies to
    pub device_id: DeviceId,
    /// Seed value for deterministic fault generation
    pub fault_seed: u64,
    /// List of currently active fault names
    pub active_faults: Vec<String>,
}

/// Fault injection middleware for simulation effect system
pub struct FaultInjectionMiddleware {
    device_id: DeviceId,
    fault_seed: u64,
    execution_mode: ExecutionMode,
    active_faults: HashMap<String, f64>, // fault_name -> probability
}

impl FaultInjectionMiddleware {
    /// Create new fault injection middleware
    pub fn new(device_id: DeviceId, fault_seed: u64) -> Self {
        Self {
            device_id,
            fault_seed,
            execution_mode: ExecutionMode::Simulation { seed: fault_seed },
            active_faults: HashMap::new(),
        }
    }

    /// Create for simulation mode
    pub fn for_simulation(device_id: DeviceId, seed: u64) -> Self {
        Self::new(device_id, seed)
    }

    /// Check if this middleware handles fault injection effects
    fn handles_effect(&self, effect_type: EffectType) -> bool {
        matches!(effect_type, EffectType::FaultInjection)
    }

    /// Inject a fault with probability
    pub fn inject_fault(&mut self, fault_name: String, probability: f64) {
        self.active_faults.insert(fault_name, probability);
    }

    /// Remove a fault
    pub fn remove_fault(&mut self, fault_name: &str) -> Option<f64> {
        self.active_faults.remove(fault_name)
    }

    /// Get active faults
    pub fn active_faults(&self) -> &HashMap<String, f64> {
        &self.active_faults
    }

    /// Get the device ID
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Check if a fault should trigger based on seed and probability
    fn should_trigger_fault(&self, fault_name: &str, probability: f64) -> bool {
        let hash_input = format!("{}{}{}", self.fault_seed, fault_name, self.device_id);
        let hash = blake3::hash(hash_input.as_bytes());
        let hash_value = u64::from_le_bytes([
            hash.as_bytes()[0],
            hash.as_bytes()[1],
            hash.as_bytes()[2],
            hash.as_bytes()[3],
            hash.as_bytes()[4],
            hash.as_bytes()[5],
            hash.as_bytes()[6],
            hash.as_bytes()[7],
        ]);
        (hash_value as f64 / u64::MAX as f64) < probability
    }
}

#[async_trait]
impl AuraHandler for FaultInjectionMiddleware {
    async fn execute_effect(
        &mut self,
        effect_type: EffectType,
        operation: &str,
        parameters: &[u8],
        _ctx: &mut AuraContext,
    ) -> Result<Vec<u8>, AuraHandlerError> {
        if !self.handles_effect(effect_type) {
            return Err(AuraHandlerError::UnsupportedEffect { effect_type });
        }

        match operation {
            "inject_message_drop" => {
                let probability = 0.1; // 10% drop probability for simulation
                self.inject_fault("message_drop".to_string(), probability);
                Ok(serde_json::to_vec(&true).unwrap_or_default())
            }
            "inject_message_delay" => {
                let probability = 0.05; // 5% delay probability
                self.inject_fault("message_delay".to_string(), probability);
                Ok(serde_json::to_vec(&true).unwrap_or_default())
            }
            "inject_byzantine_behavior" => {
                let probability = 0.02; // 2% byzantine probability
                self.inject_fault("byzantine".to_string(), probability);
                Ok(serde_json::to_vec(&true).unwrap_or_default())
            }
            "should_drop_message" => {
                let should_drop = if let Some(&prob) = self.active_faults.get("message_drop") {
                    self.should_trigger_fault("message_drop", prob)
                } else {
                    false
                };
                Ok(serde_json::to_vec(&should_drop).unwrap_or_default())
            }
            "should_delay_message" => {
                let should_delay = if let Some(&prob) = self.active_faults.get("message_delay") {
                    self.should_trigger_fault("message_delay", prob)
                } else {
                    false
                };
                Ok(serde_json::to_vec(&should_delay).unwrap_or_default())
            }
            "get_active_faults" => {
                let fault_names: Vec<_> = self.active_faults.keys().collect();
                Ok(serde_json::to_vec(&fault_names).unwrap_or_default())
            }
            "remove_fault" => {
                let fault_name = String::from_utf8_lossy(parameters);
                let removed = self.remove_fault(&fault_name);
                Ok(serde_json::to_vec(&removed.is_some()).unwrap_or_default())
            }
            _ => Err(AuraHandlerError::UnsupportedOperation {
                effect_type,
                operation: operation.to_string(),
            }),
        }
    }

    async fn execute_session(
        &mut self,
        _session: LocalSessionType,
        _ctx: &mut AuraContext,
    ) -> Result<(), AuraHandlerError> {
        // Fault injection doesn't handle sessions directly
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

    #[tokio::test]
    async fn test_fault_injection_creation() {
        let device_id = DeviceId::new();
        let middleware = FaultInjectionMiddleware::for_simulation(device_id, 42);

        assert_eq!(middleware.device_id(), device_id);
        assert_eq!(middleware.fault_seed, 42);
        assert_eq!(
            middleware.execution_mode(),
            ExecutionMode::Simulation { seed: 42 }
        );
    }

    #[tokio::test]
    async fn test_fault_effect_support() {
        let device_id = DeviceId::new();
        let middleware = FaultInjectionMiddleware::for_simulation(device_id, 42);

        assert!(middleware.supports_effect(EffectType::FaultInjection));
        assert!(!middleware.supports_effect(EffectType::Crypto));
        assert!(!middleware.supports_effect(EffectType::ChaosCoordination));
    }

    #[tokio::test]
    async fn test_fault_operations() {
        let device_id = DeviceId::new();
        let mut middleware = FaultInjectionMiddleware::for_simulation(device_id, 42);
        let mut ctx = AuraContext::for_testing(device_id);

        // Test inject message drop
        let result = middleware
            .execute_effect(
                EffectType::FaultInjection,
                "inject_message_drop",
                b"",
                &mut ctx,
            )
            .await;
        assert!(result.is_ok());

        // Test should drop message
        let result = middleware
            .execute_effect(
                EffectType::FaultInjection,
                "should_drop_message",
                b"",
                &mut ctx,
            )
            .await;
        assert!(result.is_ok());

        // Test get active faults
        let result = middleware
            .execute_effect(
                EffectType::FaultInjection,
                "get_active_faults",
                b"",
                &mut ctx,
            )
            .await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_fault_management() {
        let device_id = DeviceId::new();
        let mut middleware = FaultInjectionMiddleware::for_simulation(device_id, 42);

        // Inject fault
        middleware.inject_fault("test_fault".to_string(), 0.5);
        assert_eq!(middleware.active_faults().len(), 1);
        assert_eq!(middleware.active_faults().get("test_fault"), Some(&0.5));

        // Remove fault
        let removed = middleware.remove_fault("test_fault");
        assert_eq!(removed, Some(0.5));
        assert_eq!(middleware.active_faults().len(), 0);
    }

    #[test]
    fn test_fault_triggering() {
        let device_id = DeviceId::new();
        let middleware = FaultInjectionMiddleware::for_simulation(device_id, 42);

        // Test deterministic fault triggering
        // With the same seed and fault name, should always return the same result
        let should_trigger1 = middleware.should_trigger_fault("test_fault", 0.5);
        let should_trigger2 = middleware.should_trigger_fault("test_fault", 0.5);
        assert_eq!(should_trigger1, should_trigger2);

        // With probability 0, should never trigger
        assert!(!middleware.should_trigger_fault("never_fault", 0.0));

        // With probability 1, should always trigger
        assert!(middleware.should_trigger_fault("always_fault", 1.0));
    }
}
