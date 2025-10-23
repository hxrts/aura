//! Choreography-based simulation infrastructure
//!
//! This module provides a declarative API for testing distributed protocols.
//! Instead of manually coordinating protocol execution and simulation ticking,
//! tests describe choreographies that are automatically executed.

use crate::{ParticipantId, Simulation, SimError, Result};
use std::future::Future;
use std::sync::Arc;
use tokio::sync::RwLock;

/// A choreography is a distributed protocol test scenario
pub trait Choreography: Send {
    /// The result type of the choreography
    type Output: Send;
    
    /// Execute the choreography with the given participants
    fn run(
        self,
        participants: Vec<Arc<crate::SimulatedParticipant>>,
        runtime: ChoreographyRuntime,
    ) -> impl Future<Output = Result<Self::Output>> + Send;
}

/// Runtime support for executing choreographies
pub struct ChoreographyRuntime {
    simulation: Arc<RwLock<Simulation>>,
}

impl ChoreographyRuntime {
    /// Create a new choreography runtime
    pub fn new(simulation: Arc<RwLock<Simulation>>) -> Self {
        Self { simulation }
    }
    
    /// Wait for all participants to reach a synchronization point
    pub async fn barrier(&self, name: &str) {
        // TODO: Implement barrier synchronization
        println!("Choreography barrier: {}", name);
    }
    
    /// Run a step of the choreography with automatic simulation advancement
    pub async fn step<F, Fut, T>(&self, name: &str, f: F) -> Result<T>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        println!("Choreography step: {}", name);
        
        // Start the protocol operation
        let protocol_future = f();
        
        // Run simulation alongside the protocol
        let result = self.run_with_simulation(protocol_future).await?;
        
        Ok(result)
    }
    
    /// Run a future while advancing the simulation
    async fn run_with_simulation<F, T>(&self, protocol_future: F) -> Result<T>
    where
        F: Future<Output = Result<T>>,
    {
        // Pin the protocol future so we can poll it
        let mut protocol_future = Box::pin(protocol_future);
        
        loop {
            // Try to make progress on the protocol
            match futures::poll!(protocol_future.as_mut()) {
                std::task::Poll::Ready(result) => return result,
                std::task::Poll::Pending => {
                    // Protocol is waiting, advance simulation
                    let mut sim = self.simulation.write().await;
                    
                    // Check if simulation has work to do
                    if !sim.is_idle().await {
                        sim.tick().await?;
                    } else {
                        // Check if protocols are waiting
                        let scheduler = sim.scheduler().read().await;
                        if scheduler.has_waiting_contexts() {
                            // Advance time to wake waiting contexts
                            drop(scheduler);
                            sim.tick().await?;
                        } else {
                            // Nothing to do, yield to let async runtime progress
                            tokio::task::yield_now().await;
                        }
                    }
                }
            }
        }
    }
}

/// Extension trait for Simulation to support choreography execution
impl Simulation {
    /// Run a choreography with the specified number of participants
    pub async fn run_choreography<C, F, Fut>(
        &mut self,
        participant_count: usize,
        choreography_fn: F,
    ) -> Result<C::Output>
    where
        C: Choreography,
        F: FnOnce(Vec<Arc<crate::SimulatedParticipant>>) -> Fut,
        Fut: Future<Output = C>,
    {
        // Create participants
        let (_account_id, device_info) = self
            .add_account_with_devices(
                &(0..participant_count)
                    .map(|i| format!("participant_{}", i).as_str())
                    .collect::<Vec<_>>()
            )
            .await;
        
        // Get participant references
        let participants: Vec<_> = device_info
            .iter()
            .map(|(id, _)| self.get_participant(*id).unwrap())
            .collect();
        
        // Create choreography
        let choreography = choreography_fn(participants.clone()).await;
        
        // Create runtime
        let runtime = ChoreographyRuntime::new(Arc::new(RwLock::new(self)));
        
        // Execute choreography
        choreography.run(participants, runtime).await
    }
}

/// Helper macro to define choreographies
#[macro_export]
macro_rules! choreography {
    ($name:ident, $participants:tt, $body:expr) => {
        struct $name;
        
        impl $crate::choreography::Choreography for $name {
            type Output = ();
            
            async fn run(
                self,
                participants: Vec<std::sync::Arc<$crate::SimulatedParticipant>>,
                runtime: $crate::choreography::ChoreographyRuntime,
            ) -> $crate::Result<Self::Output> {
                let $participants = &participants[..];
                $body(runtime).await
            }
        }
    };
}

/// Example choreography for DKD protocol
pub struct DkdChoreography {
    threshold: usize,
}

impl DkdChoreography {
    pub fn new(threshold: usize) -> Self {
        Self { threshold }
    }
}

impl Choreography for DkdChoreography {
    type Output = Vec<Vec<u8>>;
    
    async fn run(
        self,
        participants: Vec<Arc<crate::SimulatedParticipant>>,
        runtime: ChoreographyRuntime,
    ) -> Result<Self::Output> {
        let session_id = uuid::Uuid::new_v4();
        let participant_ids: Vec<_> = participants
            .iter()
            .map(|p| {
                // Get device ID from participant
                // This is a bit hacky - we should have a better API
                todo!("Get device ID from participant")
            })
            .collect();
        
        // Start all DKD protocols concurrently
        runtime.step("initiate_dkd", || async {
            let futures: Vec<_> = participants
                .iter()
                .map(|p| {
                    p.initiate_dkd_with_session(
                        session_id,
                        participant_ids.clone(),
                        self.threshold,
                    )
                })
                .collect();
            
            // Wait for all to complete
            let results = futures::future::try_join_all(futures).await?;
            Ok(results)
        }).await
    }
}

/// Simplified test API
pub mod testing {
    use super::*;
    
    /// Run a simple N-party protocol test
    pub async fn run_protocol_test<F, Fut, T>(
        participant_count: usize,
        test_fn: F,
    ) -> Result<T>
    where
        F: FnOnce(Vec<Arc<crate::SimulatedParticipant>>) -> Fut,
        Fut: Future<Output = Result<T>> + Send,
        T: Send,
    {
        let mut sim = Simulation::new(42);
        
        // Create shared account with participants
        let (_account_id, device_info) = sim
            .add_account_with_devices(
                &(0..participant_count)
                    .map(|i| format!("participant_{}", i).as_str())
                    .collect::<Vec<_>>()
            )
            .await;
        
        // Get participants
        let participants: Vec<_> = device_info
            .iter()
            .map(|(id, _)| sim.get_participant(*id).unwrap())
            .collect();
        
        // Create simulation runtime
        let sim_arc = Arc::new(RwLock::new(sim));
        let runtime = ChoreographyRuntime::new(sim_arc.clone());
        
        // Run test with automatic simulation management
        runtime.run_with_simulation(test_fn(participants)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_choreography_api() {
        // This is what tests should look like
        testing::run_protocol_test(3, |participants| async move {
            let alice = &participants[0];
            let bob = &participants[1];
            let carol = &participants[2];
            
            // The actual test logic is simple and declarative
            println!("Running DKD with Alice, Bob, and Carol");
            
            // TODO: Actually run DKD
            Ok(())
        }).await.unwrap();
    }
}