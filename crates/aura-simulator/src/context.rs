//! Simulation context for managing simulation state and configuration

use serde::{Deserialize, Serialize};

/// Context for simulation execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulatorContext {
    /// Name of the scenario being executed
    pub scenario_name: String,
    /// Unique run identifier
    pub run_id: String,
    /// Seed for deterministic simulation
    pub seed: u64,
    /// Number of participants in the simulation
    pub participants: Option<usize>,
    /// Threshold for operations requiring consensus
    pub threshold: Option<usize>,
}

impl SimulatorContext {
    /// Create a new simulator context
    pub fn new(scenario_name: String, run_id: String) -> Self {
        Self {
            scenario_name,
            run_id,
            seed: 42, // Default seed
            participants: None,
            threshold: None,
        }
    }

    /// Set the simulation seed
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    /// Set the number of participants and threshold
    pub fn with_participants(mut self, participants: usize, threshold: usize) -> Self {
        self.participants = Some(participants);
        self.threshold = Some(threshold);
        self
    }

    /// Get the scenario name
    pub fn scenario_name(&self) -> &str {
        &self.scenario_name
    }

    /// Get the run ID
    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    /// Get the simulation seed
    pub fn seed(&self) -> u64 {
        self.seed
    }
}
