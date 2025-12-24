//! Simulation context for deterministic testing
//!
//! Immutable context for simulation operations, including fault injection,
//! time control, and property checking.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

/// Immutable context for simulation operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationContext {
    /// Random seed for deterministic execution
    pub seed: u64,
    /// Current simulation time
    pub simulation_time: Duration,
    /// Whether time is being controlled
    pub time_controlled: bool,
    /// Active fault injection settings
    pub fault_injection: FaultInjectionSettings,
    /// Checkpoint state for time travel
    pub checkpoint_state: Option<Arc<Vec<u8>>>,
    /// Property checking configuration
    pub property_checking: PropertyCheckingConfig,
}

/// Fault injection settings for simulation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaultInjectionSettings {
    /// Probability of network faults (0.0 to 1.0)
    pub network_fault_rate: f64,
    /// Probability of Byzantine behavior (0.0 to 1.0)
    pub byzantine_fault_rate: f64,
    /// Whether to inject timing faults
    pub timing_faults_enabled: bool,
    /// Maximum delay for timing faults
    pub max_timing_delay: Duration,
}

impl Default for FaultInjectionSettings {
    fn default() -> Self {
        Self {
            network_fault_rate: 0.0,
            byzantine_fault_rate: 0.0,
            timing_faults_enabled: false,
            max_timing_delay: Duration::from_millis(100),
        }
    }
}

/// Property checking configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyCheckingConfig {
    /// Whether to check safety properties
    pub check_safety: bool,
    /// Whether to check liveness properties
    pub check_liveness: bool,
    /// Maximum execution time before liveness violation
    pub liveness_timeout: Duration,
}

impl Default for PropertyCheckingConfig {
    fn default() -> Self {
        Self {
            check_safety: true,
            check_liveness: true,
            liveness_timeout: Duration::from_secs(30),
        }
    }
}

impl SimulationContext {
    /// Create a new simulation context
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            simulation_time: Duration::ZERO,
            time_controlled: false,
            fault_injection: FaultInjectionSettings::default(),
            checkpoint_state: None,
            property_checking: PropertyCheckingConfig::default(),
        }
    }

    /// Create context with advanced time
    pub fn with_time_advanced(&self, duration: Duration) -> Self {
        Self {
            seed: self.seed,
            simulation_time: self.simulation_time + duration,
            time_controlled: self.time_controlled,
            fault_injection: self.fault_injection.clone(),
            checkpoint_state: self.checkpoint_state.clone(),
            property_checking: self.property_checking.clone(),
        }
    }

    /// Create context with checkpoint
    pub fn with_checkpoint(&self, state: Vec<u8>) -> Self {
        Self {
            seed: self.seed,
            simulation_time: self.simulation_time,
            time_controlled: self.time_controlled,
            fault_injection: self.fault_injection.clone(),
            checkpoint_state: Some(Arc::new(state)),
            property_checking: self.property_checking.clone(),
        }
    }

    /// Create context with time control enabled
    pub fn with_time_control(&self) -> Self {
        Self {
            seed: self.seed,
            simulation_time: self.simulation_time,
            time_controlled: true,
            fault_injection: self.fault_injection.clone(),
            checkpoint_state: self.checkpoint_state.clone(),
            property_checking: self.property_checking.clone(),
        }
    }

    /// Check if a network fault should be injected
    pub fn should_inject_network_fault(&self, rng_value: f64) -> bool {
        rng_value < self.fault_injection.network_fault_rate
    }

    /// Check if Byzantine behavior should be injected
    pub fn should_inject_byzantine_fault(&self, rng_value: f64) -> bool {
        rng_value < self.fault_injection.byzantine_fault_rate
    }
}
