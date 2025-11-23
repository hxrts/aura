//! Layer 3: Simulation Effect Handlers (stubbed)
//!
//! Minimal implementations to satisfy trait bounds; real simulation lives in higher layers.

use async_trait::async_trait;
use aura_core::effects::{
    ByzantineFault, CheckpointId, FaultInjectionEffects, FaultInjectionConfig, FaultType,
    OperationStats, ScenarioId, SimulationCheckpoint, SimulationControlEffects,
    SimulationMetrics, SimulationObservationEffects, SimulationScenario, SimulationTime,
    StorageEffects, TimeEffects,
};
use aura_core::Result;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct StatelessSimulationHandler<S, T> {
    #[allow(dead_code)]
    storage: Arc<S>,
    #[allow(dead_code)]
    time: Arc<T>,
}

impl<S, T> StatelessSimulationHandler<S, T>
where
    S: StorageEffects,
    T: TimeEffects,
{
    pub fn new(storage: Arc<S>, time: Arc<T>) -> Self {
        Self { storage, time }
    }
}

// Effect handler implementation - allowed to use impure functions
#[allow(clippy::disallowed_methods)]
#[async_trait]
impl<S, T> SimulationControlEffects for StatelessSimulationHandler<S, T>
where
    S: StorageEffects + Send + Sync,
    T: TimeEffects + Send + Sync,
{
    async fn create_scenario(
        &self,
        name: String,
        _description: String,
        _parameters: HashMap<String, serde_json::Value>,
    ) -> Result<ScenarioId> {
        Ok(ScenarioId(format!("scenario-{}", name)))
    }

    async fn start_scenario(&self, _scenario_id: &ScenarioId) -> Result<()> {
        Ok(())
    }

    async fn pause_scenario(&self, _scenario_id: &ScenarioId) -> Result<()> {
        Ok(())
    }

    async fn resume_scenario(&self, _scenario_id: &ScenarioId) -> Result<()> {
        Ok(())
    }

    async fn stop_scenario(&self, _scenario_id: &ScenarioId) -> Result<()> {
        Ok(())
    }

    async fn get_scenario(&self, _scenario_id: &ScenarioId) -> Result<Option<SimulationScenario>> {
        Ok(None)
    }

    async fn list_scenarios(&self) -> Result<Vec<SimulationScenario>> {
        Ok(Vec::new())
    }

    async fn advance_time(&self, _duration: Duration) -> Result<()> {
        Ok(())
    }

    async fn set_time_rate(&self, _rate: f64) -> Result<()> {
        Ok(())
    }

    async fn get_simulation_time(&self) -> Result<SimulationTime> {
        Ok(SimulationTime::new(std::time::SystemTime::now()))
    }

    async fn set_manual_time_control(&self, _enabled: bool) -> Result<()> {
        Ok(())
    }

    async fn create_checkpoint(&self, name: String) -> Result<CheckpointId> {
        Ok(CheckpointId(format!("checkpoint-{}", name)))
    }

    async fn restore_checkpoint(&self, _checkpoint_id: &CheckpointId) -> Result<()> {
        Ok(())
    }

    async fn get_checkpoint(
        &self,
        _checkpoint_id: &CheckpointId,
    ) -> Result<Option<SimulationCheckpoint>> {
        Ok(None)
    }

    async fn list_checkpoints(&self) -> Result<Vec<SimulationCheckpoint>> {
        Ok(Vec::new())
    }

    async fn delete_checkpoint(&self, _checkpoint_id: &CheckpointId) -> Result<()> {
        Ok(())
    }

    async fn get_metrics(&self) -> Result<SimulationMetrics> {
        Ok(SimulationMetrics::default())
    }

    async fn reset_metrics(&self) -> Result<()> {
        Ok(())
    }
}

#[async_trait]
impl<S, T> FaultInjectionEffects for StatelessSimulationHandler<S, T>
where
    S: StorageEffects + Send + Sync,
    T: TimeEffects + Send + Sync,
{
    async fn inject_fault(&self, _fault: FaultInjectionConfig) -> Result<()> {
        Ok(())
    }

    async fn clear_faults(&self) -> Result<()> {
        Ok(())
    }

    async fn inject_byzantine_fault(&self, _fault: ByzantineFault) -> Result<()> {
        Ok(())
    }
    async fn inject_network_partition(&self, _groups: Vec<Vec<String>>) -> Result<()> {
        Ok(())
    }

    async fn inject_packet_loss(&self, _probability: f64) -> Result<()> {
        Ok(())
    }

    async fn inject_network_latency(&self, _latency: Duration) -> Result<()> {
        Ok(())
    }

    async fn inject_storage_failure(&self, _probability: f64) -> Result<()> {
        Ok(())
    }

    async fn inject_computation_slowness(&self, _factor: f64) -> Result<()> {
        Ok(())
    }

    async fn clear_fault_type(&self, _fault: FaultType) -> Result<()> {
        Ok(())
    }

    async fn list_active_faults(&self) -> Result<Vec<FaultInjectionConfig>> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl<S, T> SimulationObservationEffects for StatelessSimulationHandler<S, T>
where
    S: StorageEffects + Send + Sync,
    T: TimeEffects + Send + Sync,
{
    async fn record_metric(&self, _name: String, _value: f64) -> Result<()> {
        Ok(())
    }

    async fn get_metric(&self, _name: &str) -> Result<Option<f64>> {
        Ok(None)
    }

    async fn get_all_metrics(&self) -> Result<HashMap<String, f64>> {
        Ok(HashMap::new())
    }

    async fn record_operation(&self, _operation_name: &str, _duration: Duration) -> Result<()> {
        Ok(())
    }

    async fn get_operation_stats(&self, _operation_name: &str) -> Result<Option<OperationStats>> {
        Ok(None)
    }

    async fn export_simulation_data(
        &self,
        _format: aura_core::effects::simulation::ExportFormat,
    ) -> Result<Vec<u8>> {
        Ok(Vec::new())
    }
}
