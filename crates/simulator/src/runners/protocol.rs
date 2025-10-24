//! Protocol execution with simulation integration
//!
//! This module provides executors that integrate with tokio's runtime
//! to handle wake notifications from the simulation scheduler while supporting
//! both single and multi-protocol execution patterns.

use crate::{Result, SimError, SimulatedParticipant, Simulation};
use std::future::Future;
use std::sync::Arc;
use tokio::sync::{Notify, RwLock};
use tokio::task::JoinHandle;

/// Unified protocol executor that integrates simulation scheduling with tokio's runtime
pub struct ProtocolExecutor {
    simulation: Arc<RwLock<Simulation>>,
    /// Notify when simulation has made progress
    progress_notify: Arc<Notify>,
    /// Maximum ticks before timeout
    max_ticks: u64,
}

impl ProtocolExecutor {
    /// Create a new protocol executor with shared simulation access
    pub fn new(simulation: Arc<RwLock<Simulation>>) -> Self {
        Self {
            simulation,
            progress_notify: Arc::new(Notify::new()),
            max_ticks: 10000, // Default safety limit
        }
    }

    /// Create a protocol executor from a mutable reference to simulation
    pub fn from_simulation_ref(simulation: &mut Simulation) -> (Self, Arc<RwLock<Simulation>>) {
        // Move the simulation into an Arc<RwLock<>> for async access
        let sim_data = std::mem::replace(simulation, Simulation::new(0)); // Temporary placeholder
        let shared_sim = Arc::new(RwLock::new(sim_data));
        let executor = Self::new(shared_sim.clone());
        (executor, shared_sim)
    }

    /// Set maximum ticks before timeout
    pub fn with_max_ticks(mut self, max_ticks: u64) -> Self {
        self.max_ticks = max_ticks;
        self
    }

    /// Execute a single protocol with automatic simulation advancement
    pub async fn run<F, T>(&self, fut: F) -> Result<T>
    where
        F: Future<Output = Result<T>> + Send + 'static,
        T: Send + 'static,
    {
        let results = self.run_many(vec![fut]).await;
        results.into_iter().next().unwrap()
    }

    /// Execute multiple protocols concurrently with automatic simulation advancement
    pub async fn run_many<F, T>(&self, futures: Vec<F>) -> Vec<Result<T>>
    where
        F: Future<Output = Result<T>> + Send + 'static,
        T: Send + 'static,
    {
        if futures.is_empty() {
            return Vec::new();
        }

        // Spawn all futures as tokio tasks for proper waker support
        let task_handles: Vec<JoinHandle<Result<T>>> =
            futures.into_iter().map(|fut| tokio::spawn(fut)).collect();

        // Create a background task that advances the simulation
        let sim_ref = self.simulation.clone();
        let notify_ref = self.progress_notify.clone();
        let max_ticks = self.max_ticks;

        let simulation_task = tokio::spawn(async move {
            let mut tick_count = 0;
            const TICK_BATCH_SIZE: u32 = 5;

            loop {
                if tick_count >= max_ticks {
                    break;
                }

                // Advance simulation in small batches for better responsiveness
                let mut made_progress = false;
                for _ in 0..TICK_BATCH_SIZE {
                    let mut sim = sim_ref.write().await;

                    // Check if simulation has work or waiting contexts
                    let should_tick = {
                        let is_idle = sim.is_idle().await;
                        let scheduler = sim.scheduler();
                        let scheduler_guard = scheduler.read().await;
                        let has_waiting = scheduler_guard.has_waiting_contexts();
                        let has_active = scheduler_guard.has_active_contexts();

                        !is_idle || has_waiting || has_active
                    };

                    if should_tick {
                        if (sim.tick().await).is_err() {
                            // Stop simulation advancement on error
                            break;
                        }
                        made_progress = true;
                        tick_count += 1;
                    } else {
                        break;
                    }
                }

                if made_progress {
                    // Notify that we've made progress
                    notify_ref.notify_waiters();
                    // Yield to let other tasks run
                    tokio::task::yield_now().await;
                } else {
                    // No progress, wait a bit before checking again
                    tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
                }
            }
        });

        // Wait for all tasks to complete
        let results = futures::future::join_all(task_handles).await;

        // Cancel simulation task
        simulation_task.abort();

        // Convert join results to protocol results
        results
            .into_iter()
            .map(|res| match res {
                Ok(task_result) => task_result,
                Err(join_err) => {
                    if join_err.is_panic() {
                        // Convert panic to error since we can't re-panic in a collection
                        Err(SimError::RuntimeError("Task panicked".into()))
                    } else {
                        Err(SimError::RuntimeError("Task was cancelled".into()))
                    }
                }
            })
            .collect()
    }

    /// Get the shared simulation reference for manual operations
    pub fn simulation(&self) -> &Arc<RwLock<Simulation>> {
        &self.simulation
    }
}

/// Extension trait for Simulation to add protocol execution
impl Simulation {
    /// Execute protocols with automatic simulation advancement
    ///
    /// This creates a temporary executor and runs the protocol function.
    /// For multiple protocol executions, prefer creating a ProtocolExecutor directly.
    pub async fn execute_protocols<F, Fut, T>(&mut self, protocol_fn: F) -> Result<T>
    where
        F: FnOnce(&ProtocolExecutor) -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let (executor, _shared_sim) = ProtocolExecutor::from_simulation_ref(self);
        let future = protocol_fn(&executor);

        // Execute and return result
        let result = future.await;

        // Note: The simulation state remains in the shared Arc<RwLock<>>
        // This is a bit awkward but preserves the API. In practice,
        // users should prefer using ProtocolExecutor directly for better control.

        result
    }
}

/// Helper for running N-party protocols
///
/// This is a convenience function that creates a simulation, adds participants,
/// and runs protocols with automatic advancement.
pub async fn run_n_party_protocol<F, T>(participant_count: usize, protocol_fn: F) -> Result<Vec<T>>
where
    F: FnOnce(
        Vec<Arc<SimulatedParticipant>>,
    ) -> Vec<std::pin::Pin<Box<dyn Future<Output = Result<T>> + Send>>>,
    T: Send + 'static,
{
    let sim = Arc::new(RwLock::new(Simulation::new(42)));

    // Create participants
    let names: Vec<&str> = (0..participant_count)
        .map(|i| match i {
            0 => "alice",
            1 => "bob",
            2 => "carol",
            3 => "dave",
            4 => "eve",
            _ => "participant",
        })
        .collect();

    let (_account_id, device_info) = {
        let mut sim_guard = sim.write().await;
        sim_guard.add_account_with_devices(&names).await
    };

    // Get participant references
    let participants: Vec<_> = {
        let sim_guard = sim.read().await;
        device_info
            .iter()
            .map(|(id, _)| sim_guard.get_participant(*id).unwrap())
            .collect()
    };

    // Create protocol futures
    let protocol_futures = protocol_fn(participants);

    // Execute with automatic simulation advancement
    let executor = ProtocolExecutor::new(sim);
    let results = executor.run_many(protocol_futures).await;

    // Convert Vec<Result<T>> to Result<Vec<T>>
    results.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_protocol_executor_single() {
        let sim = Arc::new(RwLock::new(Simulation::new(42)));
        let executor = ProtocolExecutor::new(sim.clone());

        // Run a simple future
        let result = executor
            .run(async {
                // Simulate some async work
                tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
                Ok(42)
            })
            .await;

        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_protocol_executor_multiple() {
        let sim = Arc::new(RwLock::new(Simulation::new(42)));
        let executor = ProtocolExecutor::new(sim.clone());

        // Run multiple futures concurrently
        let futures = vec![
            Box::pin(async { Ok(1) })
                as std::pin::Pin<Box<dyn Future<Output = Result<i32>> + Send>>,
            Box::pin(async { Ok(2) })
                as std::pin::Pin<Box<dyn Future<Output = Result<i32>> + Send>>,
            Box::pin(async { Ok(3) })
                as std::pin::Pin<Box<dyn Future<Output = Result<i32>> + Send>>,
        ];

        let results = executor.run_many(futures).await;

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].as_ref().unwrap(), &1);
        assert_eq!(results[1].as_ref().unwrap(), &2);
        assert_eq!(results[2].as_ref().unwrap(), &3);
    }

    #[tokio::test]
    async fn test_simulation_extension_trait() {
        let mut sim = Simulation::new(42);

        // Test the extension trait method
        let result = sim
            .execute_protocols(|_executor| {
                Box::pin(async move {
                    // Just return a value
                    Ok(42)
                })
            })
            .await;

        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_n_party_protocol_helper() {
        let result = run_n_party_protocol(2, |participants| {
            assert_eq!(participants.len(), 2);

            vec![
                Box::pin(async move { Ok(1) })
                    as std::pin::Pin<Box<dyn Future<Output = Result<i32>> + Send>>,
                Box::pin(async move { Ok(2) })
                    as std::pin::Pin<Box<dyn Future<Output = Result<i32>> + Send>>,
            ]
        })
        .await;

        let results = result.unwrap();
        assert_eq!(results, vec![1, 2]);
    }
}
