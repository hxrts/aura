//! Effect system composer for simulation
//!
//! This module provides effect-based composition patterns to replace the former
//! middleware stack architecture. Enables clean composition of simulation effects
//! without middleware wrapper patterns.

use std::sync::Arc;
use aura_core::DeviceId;
use aura_core::effects::{ChaosEffects, TestingEffects, TimeEffects};
use aura_protocol::effects::{AuraEffectSystem, EffectSystemConfig};
use super::{SimulationTimeHandler, SimulationFaultHandler, SimulationScenarioHandler};

/// Effect-based simulation composer
///
/// Replaces the former middleware stack pattern with direct effect composition.
/// Provides clean, explicit dependency injection for simulation effects.
pub struct SimulationEffectComposer {
    device_id: DeviceId,
    effect_system: Option<Arc<AuraEffectSystem>>,
    time_handler: Option<Arc<SimulationTimeHandler>>,
    fault_handler: Option<Arc<SimulationFaultHandler>>,
    scenario_handler: Option<Arc<SimulationScenarioHandler>>,
    seed: u64,
}

impl SimulationEffectComposer {
    /// Create a new effect composer for the given device
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            device_id,
            effect_system: None,
            time_handler: None,
            fault_handler: None,
            scenario_handler: None,
            seed: 42, // Default deterministic seed
        }
    }

    /// Set the seed for deterministic simulation
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    /// Add core effect system
    pub fn with_effect_system(mut self, config: EffectSystemConfig) -> Result<Self, SimulationComposerError> {
        let effect_system = Arc::new(AuraEffectSystem::new(config).map_err(|e| {
            SimulationComposerError::EffectSystemCreationFailed(e.to_string())
        })?);
        
        self.effect_system = Some(effect_system);
        Ok(self)
    }

    /// Add simulation-specific time control
    pub fn with_time_control(mut self) -> Self {
        self.time_handler = Some(Arc::new(SimulationTimeHandler::new()));
        self
    }

    /// Add fault injection capabilities  
    pub fn with_fault_injection(mut self) -> Self {
        self.fault_handler = Some(Arc::new(SimulationFaultHandler::new(self.seed)));
        self
    }

    /// Add scenario management capabilities
    pub fn with_scenario_management(mut self) -> Self {
        self.scenario_handler = Some(Arc::new(SimulationScenarioHandler::new(self.seed)));
        self
    }

    /// Build the composed simulation environment
    pub fn build(self) -> Result<ComposedSimulationEnvironment, SimulationComposerError> {
        let effect_system = self.effect_system.ok_or(
            SimulationComposerError::MissingRequiredComponent("effect_system".to_string())
        )?;

        Ok(ComposedSimulationEnvironment {
            device_id: self.device_id,
            effect_system,
            time_handler: self.time_handler,
            fault_handler: self.fault_handler,
            scenario_handler: self.scenario_handler,
        })
    }

    /// Create a typical testing environment with all handlers
    pub fn for_testing(device_id: DeviceId) -> Result<ComposedSimulationEnvironment, SimulationComposerError> {
        let config = EffectSystemConfig::for_testing(device_id);
        
        Self::new(device_id)
            .with_seed(42) // Deterministic for testing
            .with_effect_system(config)?
            .with_time_control()
            .with_fault_injection()
            .with_scenario_management()
            .build()
    }

    /// Create a simulation environment with specific seed
    pub fn for_simulation(device_id: DeviceId, seed: u64) -> Result<ComposedSimulationEnvironment, SimulationComposerError> {
        let config = EffectSystemConfig::for_simulation(device_id, seed);
        
        Self::new(device_id)
            .with_seed(seed)
            .with_effect_system(config)?
            .with_time_control()
            .with_fault_injection()
            .with_scenario_management()
            .build()
    }
}

/// Composed simulation environment with effect handlers
///
/// Provides unified access to all simulation capabilities through
/// proper effect system composition.
pub struct ComposedSimulationEnvironment {
    device_id: DeviceId,
    effect_system: Arc<AuraEffectSystem>,
    time_handler: Option<Arc<SimulationTimeHandler>>,
    fault_handler: Option<Arc<SimulationFaultHandler>>,
    scenario_handler: Option<Arc<SimulationScenarioHandler>>,
}

impl ComposedSimulationEnvironment {
    /// Get the device ID
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Get the core effect system
    pub fn effect_system(&self) -> &Arc<AuraEffectSystem> {
        &self.effect_system
    }

    /// Get time effects handler (if available)
    pub fn time_effects(&self) -> Option<&Arc<SimulationTimeHandler>> {
        self.time_handler.as_ref()
    }

    /// Get chaos/fault effects handler (if available) 
    pub fn chaos_effects(&self) -> Option<&Arc<SimulationFaultHandler>> {
        self.fault_handler.as_ref()
    }

    /// Get testing effects handler (if available)
    pub fn testing_effects(&self) -> Option<&Arc<SimulationScenarioHandler>> {
        self.scenario_handler.as_ref()
    }

    /// Access time effects through trait
    pub async fn current_timestamp(&self) -> Result<u64, SimulationComposerError> {
        match &self.time_handler {
            Some(handler) => {
                handler.current_timestamp().await.map_err(|e| {
                    SimulationComposerError::EffectOperationFailed(format!("Time effect failed: {}", e))
                })
            }
            None => Err(SimulationComposerError::MissingRequiredComponent("time_handler".to_string()))
        }
    }

    /// Inject faults through chaos effects
    pub async fn inject_network_delay(
        &self,
        delay_range: (std::time::Duration, std::time::Duration),
        affected_peers: Option<Vec<String>>,
    ) -> Result<(), SimulationComposerError> {
        match &self.fault_handler {
            Some(handler) => {
                handler.inject_network_delay(delay_range, affected_peers).await.map_err(|e| {
                    SimulationComposerError::EffectOperationFailed(format!("Chaos effect failed: {}", e))
                })
            }
            None => Err(SimulationComposerError::MissingRequiredComponent("fault_handler".to_string()))
        }
    }

    /// Record testing events
    pub async fn record_test_event(
        &self,
        event_type: &str,
        event_data: std::collections::HashMap<String, String>,
    ) -> Result<(), SimulationComposerError> {
        match &self.scenario_handler {
            Some(handler) => {
                handler.record_event(event_type, event_data).await.map_err(|e| {
                    SimulationComposerError::EffectOperationFailed(format!("Testing effect failed: {}", e))
                })
            }
            None => Err(SimulationComposerError::MissingRequiredComponent("scenario_handler".to_string()))
        }
    }
}

/// Errors that can occur during simulation composition
#[derive(Debug, thiserror::Error)]
pub enum SimulationComposerError {
    /// Failed to create effect system
    #[error("Effect system creation failed: {0}")]
    EffectSystemCreationFailed(String),

    /// Missing required component
    #[error("Missing required component: {0}")]
    MissingRequiredComponent(String),

    /// Effect operation failed
    #[error("Effect operation failed: {0}")]
    EffectOperationFailed(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::DeviceId;

    #[tokio::test]
    async fn test_effect_composer_basic() {
        let device_id = DeviceId::new();
        let environment = SimulationEffectComposer::for_testing(device_id).unwrap();
        
        assert_eq!(environment.device_id(), device_id);
        assert!(environment.time_effects().is_some());
        assert!(environment.chaos_effects().is_some());
        assert!(environment.testing_effects().is_some());
    }

    #[tokio::test]
    async fn test_effect_composition_manual() {
        let device_id = DeviceId::new();
        let config = EffectSystemConfig::for_testing(device_id);
        
        let environment = SimulationEffectComposer::new(device_id)
            .with_seed(123)
            .with_effect_system(config).unwrap()
            .with_time_control()
            .with_fault_injection()
            .build().unwrap();
        
        assert_eq!(environment.device_id(), device_id);
        assert!(environment.time_effects().is_some());
        assert!(environment.chaos_effects().is_some());
    }

    #[tokio::test]
    async fn test_time_effects_integration() {
        let device_id = DeviceId::new();
        let environment = SimulationEffectComposer::for_testing(device_id).unwrap();
        
        let timestamp = environment.current_timestamp().await.unwrap();
        assert!(timestamp >= 0);
    }

    #[tokio::test]
    async fn test_fault_injection_integration() {
        let device_id = DeviceId::new();
        let environment = SimulationEffectComposer::for_testing(device_id).unwrap();
        
        let result = environment.inject_network_delay(
            (std::time::Duration::from_millis(10), std::time::Duration::from_millis(100)),
            None
        ).await;
        
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_scenario_management_integration() {
        let device_id = DeviceId::new();
        let environment = SimulationEffectComposer::for_testing(device_id).unwrap();
        
        let mut event_data = std::collections::HashMap::new();
        event_data.insert("test_key".to_string(), "test_value".to_string());
        
        let result = environment.record_test_event("test_event", event_data).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_missing_component_error() {
        let device_id = DeviceId::new();
        
        let result = SimulationEffectComposer::new(device_id).build();
        assert!(result.is_err());
        
        if let Err(SimulationComposerError::MissingRequiredComponent(component)) = result {
            assert_eq!(component, "effect_system");
        } else {
            panic!("Expected MissingRequiredComponent error");
        }
    }
}