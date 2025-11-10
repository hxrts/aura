//! Simulation effect system implementation

use async_trait::async_trait;

use aura_core::{
    handlers::{context::AuraContext, AuraHandler, AuraHandlerError, EffectType, ExecutionMode},
    identifiers::DeviceId,
    sessions::LocalSessionType,
};

/// Simulation-specific effect system (TODO fix - Simplified)
pub struct SimulationEffectSystem {
    device_id: DeviceId,
    execution_mode: ExecutionMode,
    simulation_seed: u64,
}

impl SimulationEffectSystem {
    /// Create a new simulation effect system
    pub fn new(
        device_id: DeviceId,
        execution_mode: ExecutionMode,
    ) -> Result<Self, AuraHandlerError> {
        // Extract simulation seed if available
        let simulation_seed = match execution_mode {
            ExecutionMode::Simulation { seed } => seed,
            _ => 42, // Default seed for non-simulation modes
        };

        Ok(Self {
            device_id,
            execution_mode,
            simulation_seed,
        })
    }

    /// Get the device ID this simulation system is configured for
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Get the simulation seed
    pub fn simulation_seed(&self) -> u64 {
        self.simulation_seed
    }

    /// Check if this system supports simulation-specific effects
    pub fn supports_simulation_effect(&self, effect_type: EffectType) -> bool {
        matches!(
            effect_type,
            EffectType::FaultInjection
                | EffectType::TimeControl
                | EffectType::StateInspection
                | EffectType::PropertyChecking
                | EffectType::ChaosCoordination
        )
    }
}

#[async_trait]
impl AuraHandler for SimulationEffectSystem {
    async fn execute_effect(
        &mut self,
        effect_type: EffectType,
        operation: &str,
        parameters: &[u8],
        ctx: &mut AuraContext,
    ) -> Result<Vec<u8>, AuraHandlerError> {
        // For simulation effects, return mock data
        if self.supports_simulation_effect(effect_type) {
            match effect_type {
                EffectType::FaultInjection => Ok(b"fault_injected".to_vec()),
                EffectType::TimeControl => Ok(b"time_controlled".to_vec()),
                EffectType::StateInspection => Ok(b"state_inspected".to_vec()),
                EffectType::PropertyChecking => Ok(b"properties_checked".to_vec()),
                EffectType::ChaosCoordination => Ok(b"chaos_coordinated".to_vec()),
                _ => Err(AuraHandlerError::UnsupportedEffect { effect_type }),
            }
        } else {
            // For other effects, return default responses
            Ok(b"simulation_default".to_vec())
        }
    }

    async fn execute_session(
        &mut self,
        _session: LocalSessionType,
        _ctx: &mut AuraContext,
    ) -> Result<(), AuraHandlerError> {
        // TODO fix - Simplified session handling for simulation
        Ok(())
    }

    fn supports_effect(&self, effect_type: EffectType) -> bool {
        self.supports_simulation_effect(effect_type)
            || matches!(
                effect_type,
                EffectType::Crypto
                    | EffectType::Network
                    | EffectType::Storage
                    | EffectType::Time
                    | EffectType::Console
                    | EffectType::Random
            )
    }

    fn execution_mode(&self) -> ExecutionMode {
        self.execution_mode
    }
}

/// Factory for creating simulation effect systems
pub struct SimulationEffectSystemFactory;

impl SimulationEffectSystemFactory {
    /// Create a simulation effect system handler
    pub fn create(
        device_id: DeviceId,
        execution_mode: ExecutionMode,
    ) -> Result<Box<dyn AuraHandler>, AuraHandlerError> {
        let system = SimulationEffectSystem::new(device_id, execution_mode)?;
        Ok(Box::new(system))
    }

    /// Create a simulation effect system for testing
    pub fn for_testing(device_id: DeviceId) -> Result<Box<dyn AuraHandler>, AuraHandlerError> {
        Self::create(device_id, ExecutionMode::Testing)
    }

    /// Create a simulation effect system for simulation mode
    pub fn for_simulation(
        device_id: DeviceId,
        seed: u64,
    ) -> Result<Box<dyn AuraHandler>, AuraHandlerError> {
        Self::create(device_id, ExecutionMode::Simulation { seed })
    }
}

/// Simulation effect system statistics
#[derive(Debug, Clone)]
pub struct SimulationEffectSystemStats {
    /// Device ID for this system
    pub device_id: DeviceId,
    /// Seed used for deterministic simulation
    pub simulation_seed: u64,
    /// Whether the system is in deterministic mode
    pub deterministic_mode: bool,
    /// Current execution mode
    pub execution_mode: ExecutionMode,
    /// Number of middleware components
    pub middleware_count: usize,
    /// List of effect types supported by this system
    pub supported_effect_types: Vec<EffectType>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::identifiers::DeviceId;

    #[tokio::test]
    async fn test_simulation_system_creation() {
        let device_id = DeviceId::new();
        let system =
            SimulationEffectSystem::new(device_id, ExecutionMode::Simulation { seed: 12345 });
        assert!(system.is_ok());

        let system = system.unwrap();
        assert_eq!(system.device_id(), device_id);
        assert_eq!(system.simulation_seed(), 12345);
        assert_eq!(
            system.execution_mode(),
            ExecutionMode::Simulation { seed: 12345 }
        );
    }

    #[tokio::test]
    async fn test_simulation_effect_support() {
        let device_id = DeviceId::new();
        let system =
            SimulationEffectSystem::new(device_id, ExecutionMode::Simulation { seed: 42 }).unwrap();

        // Should support simulation-specific effects
        assert!(system.supports_simulation_effect(EffectType::FaultInjection));
        assert!(system.supports_simulation_effect(EffectType::TimeControl));
        assert!(system.supports_simulation_effect(EffectType::StateInspection));
        assert!(system.supports_simulation_effect(EffectType::PropertyChecking));
        assert!(system.supports_simulation_effect(EffectType::ChaosCoordination));

        // Should not support non-simulation effects directly via simulation middleware
        assert!(!system.supports_simulation_effect(EffectType::Crypto));
        assert!(!system.supports_simulation_effect(EffectType::Network));

        // But should support them via the overall system
        assert!(system.supports_effect(EffectType::Crypto));
        assert!(system.supports_effect(EffectType::Network));
    }

    #[test]
    fn test_factory_creation() {
        let device_id = DeviceId::new();

        // Testing mode should work
        let handler = SimulationEffectSystemFactory::for_testing(device_id);
        assert!(handler.is_ok());

        // Simulation mode should work
        let handler = SimulationEffectSystemFactory::for_simulation(device_id, 42);
        assert!(handler.is_ok());
    }
}
