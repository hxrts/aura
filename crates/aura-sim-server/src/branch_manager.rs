//! Branch management for simulation instances
//!
//! Manages multiple simulation branches, allowing clients to fork simulations,
//! switch between branches, and maintain isolated execution contexts.

use crate::simulation_wrapper::SimulationWrapper;
use anyhow::{anyhow, Result};
use aura_console_types::{BranchInfo, ConsoleCommand, SimulationInfo, TraceEvent};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use tracing::{info, warn};
use uuid::Uuid;

use crate::scenario_export::ScenarioExporter;

/// Unique branch identifier
pub type BranchId = Uuid;

/// Unique simulation identifier
pub type SimulationId = Uuid;

/// Branch management system for simulation instances
#[allow(dead_code)]
#[derive(Debug)]
pub struct BranchManager {
    /// Active simulation branches
    branches: HashMap<BranchId, SimulationBranch>,
    /// Default branch for new connections
    default_branch: Option<BranchId>,
    /// Branch metadata and relationships
    branch_metadata: HashMap<BranchId, BranchMetadata>,
}

/// Individual simulation branch containing a running simulation
#[derive(Debug)]
pub struct SimulationBranch {
    /// Unique branch identifier
    pub id: BranchId,
    /// The simulation wrapper for this branch
    pub simulation: Arc<Mutex<SimulationWrapper>>,
    /// Branch creation metadata
    pub metadata: BranchMetadata,
    /// Whether this branch is currently active
    pub is_active: bool,
    /// Accumulated events for streaming to clients
    pub event_buffer: Vec<TraceEvent>,
    /// Scenario exporter for this branch
    pub scenario_exporter: ScenarioExporter,
}

/// Branch metadata and relationship information
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct BranchMetadata {
    /// Branch identifier
    pub id: BranchId,
    /// Optional branch name/label
    pub name: Option<String>,
    /// Parent branch (if forked)
    pub parent_branch: Option<BranchId>,
    /// Simulation this branch contains
    pub simulation_id: SimulationId,
    /// When this branch was created
    pub created_at: SystemTime,
    /// Last activity time
    pub last_activity: SystemTime,
    /// Branch description
    pub description: Option<String>,
    /// Whether branch is marked for auto-cleanup
    pub auto_cleanup: bool,
}

impl BranchManager {
    /// Create a new branch manager
    pub fn new() -> Self {
        Self {
            branches: HashMap::new(),
            default_branch: None,
            branch_metadata: HashMap::new(),
        }
    }

    /// Get or create the default branch
    pub fn get_or_create_default_branch(&mut self) -> BranchId {
        if let Some(default_id) = self.default_branch {
            if self.branches.contains_key(&default_id) {
                return default_id;
            }
        }

        // Create a new default branch
        let branch_id = self
            .create_branch_from_simulation(
                SimulationWrapper::new(42), // Default seed
                Some("main".to_string()),
                None,
            )
            .expect("Failed to create default branch");

        self.default_branch = Some(branch_id);

        info!("Created default branch: {}", branch_id);
        branch_id
    }

    /// Create a new branch from a simulation
    pub fn create_branch_from_simulation(
        &mut self,
        simulation: SimulationWrapper,
        name: Option<String>,
        parent_branch: Option<BranchId>,
    ) -> Result<BranchId> {
        let branch_id = Uuid::new_v4();
        let simulation_id = Uuid::new_v4(); // Generate new simulation ID

        let metadata = BranchMetadata {
            id: branch_id,
            name: name.clone(),
            parent_branch,
            simulation_id,
            created_at: std::time::SystemTime::now(),
            last_activity: std::time::SystemTime::now(),
            description: None,
            auto_cleanup: false,
        };

        let branch = SimulationBranch {
            id: branch_id,
            simulation: Arc::new(Mutex::new(simulation)),
            metadata: metadata.clone(),
            is_active: true,
            event_buffer: Vec::new(),
            scenario_exporter: ScenarioExporter::new(),
        };

        self.branches.insert(branch_id, branch);
        self.branch_metadata.insert(branch_id, metadata);

        info!(
            "Created branch {} (simulation: {}) {}",
            branch_id,
            simulation_id,
            name.map(|n| format!("'{}'", n)).unwrap_or_default()
        );

        Ok(branch_id)
    }

    /// Fork an existing branch at its current state
    pub fn fork_branch(
        &mut self,
        parent_branch_id: BranchId,
        name: Option<String>,
    ) -> Result<BranchId> {
        // Get the parent branch
        let parent_simulation = {
            let parent_branch = self
                .branches
                .get(&parent_branch_id)
                .ok_or_else(|| anyhow!("Parent branch not found: {}", parent_branch_id))?;

            let _parent_sim = parent_branch.simulation.lock().unwrap();

            // Create a new simulation with the same state
            // For now, we'll create a new simulation with the same seed
            // In a full implementation, we would copy the exact state
            SimulationWrapper::new(42) // TODO: Get seed from parent simulation
        };

        // Create the forked branch
        let fork_id =
            self.create_branch_from_simulation(parent_simulation, name, Some(parent_branch_id))?;

        info!("Forked branch {} from {}", fork_id, parent_branch_id);
        Ok(fork_id)
    }

    /// Get branch information for API responses
    pub fn get_branch_info(&self, branch_id: BranchId) -> Option<BranchInfo> {
        let metadata = self.branch_metadata.get(&branch_id)?;
        let branch = self.branches.get(&branch_id)?;

        let simulation_info = {
            let sim = branch.simulation.lock().unwrap();
            SimulationInfo {
                id: sim.id,
                current_tick: sim.current_tick(),
                current_time: sim.current_time(),
                seed: sim.seed,
                is_recording: sim.is_recording_enabled(),
            }
        };

        Some(BranchInfo {
            id: branch_id,
            name: metadata.name.clone(),
            parent_branch: metadata.parent_branch,
            simulation_info,
            created_at: metadata.created_at,
            last_activity: metadata.last_activity,
            is_active: branch.is_active,
            event_count: branch.event_buffer.len() as u64,
        })
    }

    /// List all branches
    pub fn list_branches(&self) -> Vec<BranchInfo> {
        self.branches
            .keys()
            .filter_map(|&branch_id| self.get_branch_info(branch_id))
            .collect()
    }

    /// Get a branch for command execution
    pub fn get_branch(&mut self, branch_id: BranchId) -> Option<&mut SimulationBranch> {
        if let Some(branch) = self.branches.get_mut(&branch_id) {
            // Update last activity
            if let Some(metadata) = self.branch_metadata.get_mut(&branch_id) {
                metadata.last_activity = SystemTime::now();
            }
            Some(branch)
        } else {
            None
        }
    }

    /// Remove a branch
    #[allow(dead_code)]
    pub fn remove_branch(&mut self, branch_id: BranchId) -> Result<()> {
        if Some(branch_id) == self.default_branch {
            return Err(anyhow!("Cannot remove the default branch"));
        }

        if self.branches.remove(&branch_id).is_some() {
            self.branch_metadata.remove(&branch_id);
            info!("Removed branch: {}", branch_id);
            Ok(())
        } else {
            Err(anyhow!("Branch not found: {}", branch_id))
        }
    }

    /// Get the default branch ID
    #[allow(dead_code)]
    pub fn get_default_branch(&self) -> Option<BranchId> {
        self.default_branch
    }

    /// Set branch name/description
    #[allow(dead_code)]
    pub fn set_branch_metadata(
        &mut self,
        branch_id: BranchId,
        name: Option<String>,
        description: Option<String>,
    ) -> Result<()> {
        let metadata = self
            .branch_metadata
            .get_mut(&branch_id)
            .ok_or_else(|| anyhow!("Branch not found: {}", branch_id))?;

        if let Some(name) = name {
            metadata.name = Some(name);
        }
        if let Some(description) = description {
            metadata.description = Some(description);
        }
        metadata.last_activity = SystemTime::now();

        Ok(())
    }

    /// Get branch count
    pub fn get_branch_count(&self) -> usize {
        self.branches.len()
    }

    /// Get active simulation count
    pub fn get_active_simulation_count(&self) -> usize {
        self.branches.values().filter(|b| b.is_active).count()
    }

    /// Clean up inactive branches
    #[allow(dead_code)]
    pub fn cleanup_inactive_branches(&mut self, max_age_seconds: u64) {
        let now = SystemTime::now();
        let mut to_remove = Vec::new();

        for (branch_id, metadata) in &self.branch_metadata {
            if metadata.auto_cleanup {
                if let Ok(elapsed) = now.duration_since(metadata.last_activity) {
                    if elapsed.as_secs() > max_age_seconds {
                        to_remove.push(*branch_id);
                    }
                }
            }
        }

        for branch_id in to_remove {
            if let Err(e) = self.remove_branch(branch_id) {
                warn!("Failed to cleanup branch {}: {}", branch_id, e);
            }
        }
    }

    /// Record an event for a branch
    #[allow(dead_code)]
    pub fn record_branch_event(&mut self, branch_id: BranchId, event: TraceEvent) {
        if let Some(branch) = self.branches.get_mut(&branch_id) {
            branch.event_buffer.push(event);

            // Limit event buffer size to prevent memory issues
            if branch.event_buffer.len() > 10000 {
                branch.event_buffer.drain(0..5000); // Keep most recent 5000 events
            }
        }
    }

    /// Get buffered events for a branch
    pub fn get_branch_events(
        &self,
        branch_id: BranchId,
        since_event: Option<u64>,
    ) -> Vec<TraceEvent> {
        if let Some(branch) = self.branches.get(&branch_id) {
            if let Some(since) = since_event {
                branch
                    .event_buffer
                    .iter()
                    .filter(|event| event.event_id > since)
                    .cloned()
                    .collect()
            } else {
                branch.event_buffer.clone()
            }
        } else {
            Vec::new()
        }
    }

    /// Record a command execution for scenario export
    pub fn record_command_execution(
        &mut self,
        branch_id: BranchId,
        command: ConsoleCommand,
        executed_at_tick: u64,
    ) {
        if let Some(branch) = self.branches.get_mut(&branch_id) {
            branch
                .scenario_exporter
                .record_command(command, executed_at_tick);
        }
    }

    /// Export a branch as a TOML scenario
    pub fn export_branch_as_scenario(
        &self,
        branch_id: BranchId,
        name: Option<String>,
        description: Option<String>,
    ) -> Result<String> {
        let branch = self
            .branches
            .get(&branch_id)
            .ok_or_else(|| anyhow!("Branch not found: {}", branch_id))?;

        branch
            .scenario_exporter
            .export_branch_as_scenario(branch, name, description)
    }
}

impl Default for BranchManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_branch_manager_creation() {
        let manager = BranchManager::new();
        assert_eq!(manager.get_branch_count(), 0);
        assert_eq!(manager.get_active_simulation_count(), 0);
    }

    #[test]
    fn test_default_branch_creation() {
        let mut manager = BranchManager::new();
        let default_id = manager.get_or_create_default_branch();

        assert_eq!(manager.get_branch_count(), 1);
        assert_eq!(manager.get_default_branch(), Some(default_id));

        // Second call should return the same branch
        let default_id2 = manager.get_or_create_default_branch();
        assert_eq!(default_id, default_id2);
        assert_eq!(manager.get_branch_count(), 1);
    }

    #[test]
    fn test_branch_forking() {
        let mut manager = BranchManager::new();
        let parent_id = manager.get_or_create_default_branch();

        let fork_id = manager
            .fork_branch(parent_id, Some("test_fork".to_string()))
            .unwrap();

        assert_eq!(manager.get_branch_count(), 2);

        let fork_info = manager.get_branch_info(fork_id).unwrap();
        assert_eq!(fork_info.name, Some("test_fork".to_string()));
        assert_eq!(fork_info.parent_branch, Some(parent_id));
    }

    #[test]
    fn test_branch_metadata_update() {
        let mut manager = BranchManager::new();
        let branch_id = manager.get_or_create_default_branch();

        manager
            .set_branch_metadata(
                branch_id,
                Some("updated_name".to_string()),
                Some("Test description".to_string()),
            )
            .unwrap();

        let info = manager.get_branch_info(branch_id).unwrap();
        assert_eq!(info.name, Some("updated_name".to_string()));
    }

    #[test]
    fn test_event_recording() {
        let mut manager = BranchManager::new();
        let branch_id = manager.get_or_create_default_branch();

        let test_event = TraceEvent {
            tick: 0,
            event_id: 1,
            event_type: aura_console_types::EventType::EffectExecuted {
                effect_type: "test".to_string(),
                effect_data: vec![],
            },
            participant: "test_participant".to_string(),
            causality: aura_console_types::CausalityInfo {
                parent_events: vec![],
                happens_before: vec![],
                concurrent_with: vec![],
            },
        };

        manager.record_branch_event(branch_id, test_event.clone());

        let events = manager.get_branch_events(branch_id, None);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_id, test_event.event_id);
    }
}
