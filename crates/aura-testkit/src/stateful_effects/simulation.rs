//! Mock simulation effect handlers for testing

use aura_core::effects::simulation::*;
use aura_core::Result;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;

/// Mock simulation storage
#[derive(Debug)]
pub struct MockSimulationStorage {
    pub data: HashMap<String, Vec<u8>>,
    pub scenarios: HashMap<String, SimulationScenario>,
    pub checkpoints: HashMap<String, SimulationCheckpoint>,
    pub metrics: SimulationMetrics,
    pub custom_metrics: HashMap<String, f64>,
    pub operation_stats: HashMap<String, OperationStats>,
    pub active_faults: Vec<FaultInjectionConfig>,
    pub simulation_time: SimulationTime,
}

/// Mock simulation handler for testing
#[derive(Debug)]
pub struct MockSimulationHandler {
    storage: Arc<RwLock<MockSimulationStorage>>,
}

impl Default for MockSimulationHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl MockSimulationHandler {
    pub fn new() -> Self {
        Self {
            storage: Arc::new(RwLock::new(MockSimulationStorage {
                data: HashMap::new(),
                scenarios: HashMap::new(),
                checkpoints: HashMap::new(),
                metrics: SimulationMetrics::default(),
                custom_metrics: HashMap::new(),
                operation_stats: HashMap::new(),
                active_faults: Vec::new(),
                simulation_time: SimulationTime::new(SystemTime::now()),
            })),
        }
    }
}

#[async_trait::async_trait]
impl SimulationControlEffects for MockSimulationHandler {
    async fn create_scenario(
        &self,
        name: String,
        description: String,
        parameters: HashMap<String, serde_json::Value>,
    ) -> Result<ScenarioId> {
        let mut storage = self.storage.write().await;
        let id = ScenarioId(format!("scenario_{}", storage.scenarios.len()));
        let scenario = SimulationScenario {
            id: id.clone(),
            name,
            description,
            parameters,
            duration: None,
            state: ScenarioState::Initializing,
        };
        storage.scenarios.insert(id.0.clone(), scenario);
        Ok(id)
    }

    async fn start_scenario(&self, scenario_id: &ScenarioId) -> Result<()> {
        let mut storage = self.storage.write().await;
        if let Some(scenario) = storage.scenarios.get_mut(&scenario_id.0) {
            scenario.state = ScenarioState::Running;
        }
        Ok(())
    }

    async fn pause_scenario(&self, scenario_id: &ScenarioId) -> Result<()> {
        let mut storage = self.storage.write().await;
        if let Some(scenario) = storage.scenarios.get_mut(&scenario_id.0) {
            scenario.state = ScenarioState::Paused;
        }
        Ok(())
    }

    async fn resume_scenario(&self, scenario_id: &ScenarioId) -> Result<()> {
        let mut storage = self.storage.write().await;
        if let Some(scenario) = storage.scenarios.get_mut(&scenario_id.0) {
            scenario.state = ScenarioState::Running;
        }
        Ok(())
    }

    async fn stop_scenario(&self, scenario_id: &ScenarioId) -> Result<()> {
        let mut storage = self.storage.write().await;
        if let Some(scenario) = storage.scenarios.get_mut(&scenario_id.0) {
            scenario.state = ScenarioState::Completed;
        }
        Ok(())
    }

    async fn get_scenario(&self, scenario_id: &ScenarioId) -> Result<Option<SimulationScenario>> {
        let storage = self.storage.read().await;
        Ok(storage.scenarios.get(&scenario_id.0).cloned())
    }

    async fn list_scenarios(&self) -> Result<Vec<SimulationScenario>> {
        let storage = self.storage.read().await;
        Ok(storage.scenarios.values().cloned().collect())
    }

    async fn advance_time(&self, duration: Duration) -> Result<()> {
        let mut storage = self.storage.write().await;
        storage.simulation_time.current += duration;
        Ok(())
    }

    async fn set_time_rate(&self, rate: f64) -> Result<()> {
        let mut storage = self.storage.write().await;
        storage.simulation_time.rate = rate;
        Ok(())
    }

    async fn get_simulation_time(&self) -> Result<SimulationTime> {
        let storage = self.storage.read().await;
        Ok(storage.simulation_time.clone())
    }

    async fn set_manual_time_control(&self, enabled: bool) -> Result<()> {
        let mut storage = self.storage.write().await;
        storage.simulation_time.manual_control = enabled;
        Ok(())
    }

    async fn create_checkpoint(&self, name: String) -> Result<CheckpointId> {
        let mut storage = self.storage.write().await;
        let id = CheckpointId(name);
        let checkpoint = SimulationCheckpoint {
            id: id.clone(),
            timestamp: SystemTime::now(),
            simulation_time: storage.simulation_time.clone(),
            scenario_id: None,
            metadata: HashMap::new(),
            size_bytes: 1024, // Mock size
        };
        storage.checkpoints.insert(id.0.clone(), checkpoint);
        storage.metrics.checkpoints_created += 1;
        Ok(id)
    }

    async fn restore_checkpoint(&self, _checkpoint_id: &CheckpointId) -> Result<()> {
        // Mock implementation - in real use would restore state
        Ok(())
    }

    async fn get_checkpoint(
        &self,
        checkpoint_id: &CheckpointId,
    ) -> Result<Option<SimulationCheckpoint>> {
        let storage = self.storage.read().await;
        Ok(storage.checkpoints.get(&checkpoint_id.0).cloned())
    }

    async fn list_checkpoints(&self) -> Result<Vec<SimulationCheckpoint>> {
        let storage = self.storage.read().await;
        Ok(storage.checkpoints.values().cloned().collect())
    }

    async fn delete_checkpoint(&self, checkpoint_id: &CheckpointId) -> Result<()> {
        let mut storage = self.storage.write().await;
        storage.checkpoints.remove(&checkpoint_id.0);
        Ok(())
    }

    async fn get_metrics(&self) -> Result<SimulationMetrics> {
        let storage = self.storage.read().await;
        Ok(storage.metrics.clone())
    }

    async fn reset_metrics(&self) -> Result<()> {
        let mut storage = self.storage.write().await;
        storage.metrics = SimulationMetrics::default();
        Ok(())
    }
}

#[async_trait::async_trait]
impl FaultInjectionEffects for MockSimulationHandler {
    async fn inject_network_partition(&self, groups: Vec<Vec<String>>) -> Result<()> {
        let mut storage = self.storage.write().await;
        let config = FaultInjectionConfig {
            fault_type: FaultType::Network(NetworkFault::Partition { groups }),
            parameters: HashMap::new(),
            duration: None,
            probability: 1.0,
        };
        storage.active_faults.push(config);
        storage.metrics.faults_injected += 1;
        Ok(())
    }

    async fn inject_packet_loss(&self, probability: f64) -> Result<()> {
        let mut storage = self.storage.write().await;
        let config = FaultInjectionConfig {
            fault_type: FaultType::Network(NetworkFault::PacketLoss { probability }),
            parameters: HashMap::new(),
            duration: None,
            probability,
        };
        storage.active_faults.push(config);
        storage.metrics.faults_injected += 1;
        Ok(())
    }

    async fn inject_network_latency(&self, delay: Duration) -> Result<()> {
        let mut storage = self.storage.write().await;
        let config = FaultInjectionConfig {
            fault_type: FaultType::Network(NetworkFault::Latency { delay }),
            parameters: HashMap::new(),
            duration: None,
            probability: 1.0,
        };
        storage.active_faults.push(config);
        storage.metrics.faults_injected += 1;
        Ok(())
    }

    async fn inject_storage_failure(&self, probability: f64) -> Result<()> {
        let mut storage = self.storage.write().await;
        let config = FaultInjectionConfig {
            fault_type: FaultType::Storage(StorageFault::Failure { probability }),
            parameters: HashMap::new(),
            duration: None,
            probability,
        };
        storage.active_faults.push(config);
        storage.metrics.faults_injected += 1;
        Ok(())
    }

    async fn inject_computation_slowness(&self, factor: f64) -> Result<()> {
        let mut storage = self.storage.write().await;
        let config = FaultInjectionConfig {
            fault_type: FaultType::Computation(ComputationFault::CpuSlowness { factor }),
            parameters: HashMap::new(),
            duration: None,
            probability: 1.0,
        };
        storage.active_faults.push(config);
        storage.metrics.faults_injected += 1;
        Ok(())
    }

    async fn inject_byzantine_fault(&self, fault: ByzantineFault) -> Result<()> {
        let mut storage = self.storage.write().await;
        let config = FaultInjectionConfig {
            fault_type: FaultType::Byzantine(fault),
            parameters: HashMap::new(),
            duration: None,
            probability: 1.0,
        };
        storage.active_faults.push(config);
        storage.metrics.faults_injected += 1;
        Ok(())
    }

    async fn inject_fault(&self, config: FaultInjectionConfig) -> Result<()> {
        let mut storage = self.storage.write().await;
        storage.active_faults.push(config);
        storage.metrics.faults_injected += 1;
        Ok(())
    }

    async fn clear_faults(&self) -> Result<()> {
        let mut storage = self.storage.write().await;
        storage.active_faults.clear();
        Ok(())
    }

    async fn clear_fault_type(&self, fault_type: FaultType) -> Result<()> {
        let mut storage = self.storage.write().await;
        storage.active_faults.retain(|f| f.fault_type != fault_type);
        Ok(())
    }

    async fn list_active_faults(&self) -> Result<Vec<FaultInjectionConfig>> {
        let storage = self.storage.read().await;
        Ok(storage.active_faults.clone())
    }
}

#[async_trait::async_trait]
impl SimulationObservationEffects for MockSimulationHandler {
    async fn record_metric(&self, name: String, value: f64) -> Result<()> {
        let mut storage = self.storage.write().await;
        storage.custom_metrics.insert(name, value);
        Ok(())
    }

    async fn get_metric(&self, name: &str) -> Result<Option<f64>> {
        let storage = self.storage.read().await;
        Ok(storage.custom_metrics.get(name).copied())
    }

    async fn get_all_metrics(&self) -> Result<HashMap<String, f64>> {
        let storage = self.storage.read().await;
        Ok(storage.custom_metrics.clone())
    }

    async fn record_operation(&self, operation_name: &str, duration: Duration) -> Result<()> {
        let mut storage = self.storage.write().await;
        let stats = storage
            .operation_stats
            .entry(operation_name.to_string())
            .or_insert_with(|| OperationStats {
                operation_name: operation_name.to_string(),
                execution_count: 0,
                total_duration: Duration::ZERO,
                avg_duration: Duration::ZERO,
                min_duration: duration,
                max_duration: duration,
                std_deviation: Duration::ZERO,
            });

        stats.execution_count += 1;
        stats.total_duration += duration;
        stats.avg_duration = stats.total_duration / stats.execution_count as u32;
        stats.min_duration = stats.min_duration.min(duration);
        stats.max_duration = stats.max_duration.max(duration);

        storage.metrics.operations_count += 1;
        Ok(())
    }

    async fn get_operation_stats(&self, operation_name: &str) -> Result<Option<OperationStats>> {
        let storage = self.storage.read().await;
        Ok(storage.operation_stats.get(operation_name).cloned())
    }

    async fn export_simulation_data(&self, _format: ExportFormat) -> Result<Vec<u8>> {
        // Mock implementation - return empty data
        Ok(Vec::new())
    }
}

/// Stateless simulation handler for testing
#[derive(Debug)]
pub struct StatelessSimulationHandler;

impl Default for StatelessSimulationHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl StatelessSimulationHandler {
    pub fn new() -> Self {
        Self
    }
}
