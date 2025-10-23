//! Choreographic test runner implementation
//!
//! This module provides the concrete implementation for running choreographic tests
//! with proper simulation coordination.

use crate::{Simulation, SimulatedParticipant, Result, SimError, tokio_integrated_executor::TokioIntegratedExecutor};
use std::future::Future;
use std::sync::Arc;
use tokio::sync::{RwLock, Mutex};
use std::collections::HashMap;

/// A choreographic test runner that manages simulation and protocol execution
pub struct ChoreographicRunner {
    pub simulation: Arc<RwLock<Simulation>>,
    active_protocols: Arc<Mutex<HashMap<String, ChoreographyState>>>,
}

#[derive(Debug)]
struct ChoreographyState {
    name: String,
    participants: Vec<String>,
    started: tokio::time::Instant,
    status: ChoreographyStatus,
}

#[derive(Debug, Clone)]
enum ChoreographyStatus {
    Running,
    Completed,
    Failed(String),
}

impl ChoreographicRunner {
    /// Create a new choreographic runner with a fresh simulation
    pub fn new(seed: u64) -> Self {
        Self {
            simulation: Arc::new(RwLock::new(Simulation::new(seed))),
            active_protocols: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    
    /// Create participants for the choreography
    pub async fn create_participants<const N: usize>(
        &self,
        names: [&str; N],
    ) -> Result<[Arc<SimulatedParticipant>; N]> {
        let mut sim = self.simulation.write().await;
        let (_account_id, device_info) = sim.add_account_with_devices(&names).await;
        
        let mut participants = Vec::with_capacity(N);
        for (participant_id, _) in &device_info {
            let participant = sim
                .get_participant(*participant_id)?;
            participants.push(participant);
        }
        
        participants.try_into()
            .map_err(|_| SimError::RuntimeError("Failed to create participants".into()))
    }
    
    /// Run a choreography with automatic simulation management
    pub async fn run_choreography<C, F, Fut>(
        &self,
        choreography_name: &str,
        choreography_fn: F,
    ) -> Result<C>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<C>> + Send + 'static,
        C: Send + 'static,
    {
        println!("[Runner] Starting choreography: {}", choreography_name);
        
        // Register choreography
        {
            let mut protocols = self.active_protocols.lock().await;
            protocols.insert(
                choreography_name.to_string(),
                ChoreographyState {
                    name: choreography_name.to_string(),
                    participants: vec![], // Could track actual participants
                    started: tokio::time::Instant::now(),
                    status: ChoreographyStatus::Running,
                },
            );
        }
        
        // Create the choreography future
        let choreography_future = choreography_fn();
        
        // Run with automatic simulation advancement
        let result = self.run_with_simulation(choreography_future).await;
        
        // Update status
        {
            let mut protocols = self.active_protocols.lock().await;
            if let Some(state) = protocols.get_mut(choreography_name) {
                state.status = match &result {
                    Ok(_) => ProtocolStatus::Completed,
                    Err(e) => ProtocolStatus::Failed(e.to_string()),
                };
            }
        }
        
        println!("[Runner] Completed choreography: {}", choreography_name);
        result
    }
    
    /// Run a protocol while automatically advancing the simulation
    async fn run_with_simulation<F, T>(&self, protocol_future: F) -> Result<T>
    where
        F: Future<Output = Result<T>> + Send + 'static,
        T: Send + 'static,
    {
        // Use the tokio-integrated executor
        let executor = TokioIntegratedExecutor::new(self.simulation.clone());
        executor.run_with_simulation(protocol_future).await
    }
    
    /// Get the current status of all protocols
    pub async fn protocol_status(&self) -> HashMap<String, ProtocolStatus> {
        let protocols = self.active_protocols.lock().await;
        protocols
            .iter()
            .map(|(name, state)| (name.clone(), state.status.clone()))
            .collect()
    }
}

/// Pre-built choreographies for common protocol patterns
pub mod patterns {
    use super::*;
    use aura_journal::DeviceId;
    use aura_coordination::SessionId;
    
    /// Run an N-party protocol where all parties execute the same logic
    pub async fn n_party_protocol<const N: usize, F, Fut, T>(
        runner: &ChoreographicRunner,
        participant_names: [&str; N],
        protocol_name: &str,
        protocol_fn: F,
    ) -> Result<[T; N]>
    where
        F: Fn(Arc<SimulatedParticipant>, Vec<DeviceId>, SessionId) -> Fut + Send + 'static,
        F: Clone,
        Fut: Future<Output = Result<T>> + Send + 'static,
        T: Send + 'static,
    {
        runner.run_choreography(protocol_name, || async move {
            // Create participants
            let participants = runner.create_participants(participant_names).await?;
            
            // Get device IDs
            let mut device_ids = Vec::with_capacity(N);
            for participant in &participants {
                let ledger = participant.ledger().await;
                let device_id = ledger.state().devices.keys().next().copied()
                    .ok_or_else(|| SimError::RuntimeError("No device ID".into()))?;
                device_ids.push(device_id);
            }
            
            // Generate session ID
            let session_id = SessionId({
                let sim = runner.simulation.read().await;
                sim.generate_uuid()
            });
            
            // Run all protocol instances concurrently
            let futures: Vec<_> = participants
                .into_iter()
                .map(|p| {
                    let fut = protocol_fn(p, device_ids.clone(), session_id);
                    async move { fut.await }
                })
                .collect();
            
            let results = futures::future::try_join_all(futures).await?;
            
            results.try_into()
                .map_err(|_| SimError::RuntimeError("Failed to convert results".into()))
        }).await
    }
    
    /// Run a threshold protocol with specified minimum participants
    pub async fn threshold_protocol<F, Fut, T>(
        runner: &ChoreographicRunner,
        total_participants: usize,
        threshold: usize,
        protocol_name: &str,
        protocol_fn: F,
    ) -> Result<Vec<T>>
    where
        F: Fn(Vec<Arc<SimulatedParticipant>>, usize) -> Fut,
        Fut: Future<Output = Result<Vec<T>>> + Send,
        T: Send,
    {
        runner.run_choreography(protocol_name, || async move {
            // Create participant names
            let names: Vec<&str> = (0..total_participants)
                .map(|i| match i {
                    0 => "alice",
                    1 => "bob",
                    2 => "carol",
                    3 => "dave",
                    4 => "eve",
                    _ => "participant",
                })
                .collect();
            
            // Create participants
            let mut participants = Vec::with_capacity(total_participants);
            {
                let mut sim = runner.simulation.write().await;
                let (_account_id, device_info) = sim.add_account_with_devices(&names).await;
                
                for (participant_id, _) in device_info {
                    let participant = sim.get_participant(participant_id)?;
                    participants.push(participant);
                }
            }
            
            // Run the threshold protocol
            protocol_fn(participants, threshold).await
        }).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_simple_choreography() {
        let runner = ChoreographicRunner::new(42);
        
        // Run a simple 2-party choreography
        let result = runner.run_choreography("simple_test", || async {
            let participants = runner.create_participants(["alice", "bob"]).await?;
            
            // Just verify we can create participants
            assert_eq!(participants.len(), 2);
            Ok(())
        }).await;
        
        assert!(result.is_ok());
    }
    
    #[tokio::test]
    async fn test_n_party_pattern() {
        let runner = ChoreographicRunner::new(42);
        
        // Test the n-party pattern
        let results = patterns::n_party_protocol(
            &runner,
            ["alice", "bob", "carol"],
            "test_protocol",
            |participant, _device_ids, _session_id| async move {
                // Simple protocol that returns participant's device ID
                let ledger = participant.ledger().await;
                let my_device_id = ledger.state().devices.keys().next().copied()
                    .ok_or_else(|| SimError::RuntimeError("No device ID".into()))?;
                Ok(my_device_id)
            },
        ).await.unwrap();
        
        assert_eq!(results.len(), 3);
        // Each participant should have a unique device ID
        assert_ne!(results[0], results[1]);
        assert_ne!(results[1], results[2]);
        assert_ne!(results[0], results[2]);
    }
}