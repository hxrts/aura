//! Tokio-integrated executor for choreographic tests
//!
//! This module provides an executor that properly integrates with tokio's runtime
//! to handle wake notifications from the simulation scheduler.

use crate::{Simulation, SimError, Result};
use std::future::Future;
use std::sync::Arc;
use tokio::sync::{RwLock, Notify};
use tokio::task::JoinHandle;

/// An executor that integrates simulation scheduling with tokio's runtime
pub struct TokioIntegratedExecutor {
    simulation: Arc<RwLock<Simulation>>,
    /// Notify when simulation has made progress
    progress_notify: Arc<Notify>,
}

impl TokioIntegratedExecutor {
    pub fn new(simulation: Arc<RwLock<Simulation>>) -> Self {
        Self {
            simulation,
            progress_notify: Arc::new(Notify::new()),
        }
    }
    
    /// Run a future with automatic simulation advancement
    pub async fn run_with_simulation<F, T>(&self, fut: F) -> Result<T>
    where
        F: Future<Output = Result<T>> + Send + 'static,
        T: Send + 'static,
    {
        // Spawn the future as a tokio task so it gets proper waker support
        let task_handle: JoinHandle<Result<T>> = tokio::spawn(fut);
        
        // Create a background task that advances the simulation
        let sim_ref = self.simulation.clone();
        let notify_ref = self.progress_notify.clone();
        let mut simulation_task = tokio::spawn(async move {
            let mut tick_count = 0;
            const MAX_TICKS: u64 = 10000;
            const TICK_BATCH_SIZE: u32 = 5;
            
            loop {
                // Check if the main task is done
                if tick_count >= MAX_TICKS {
                    break;
                }
                
                // Advance simulation in small batches
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
                            // Ignore tick errors in background task
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
        
        // Wait for the main task to complete
        let result = tokio::select! {
            res = task_handle => {
                // Task completed
                match res {
                    Ok(task_result) => task_result,
                    Err(join_err) => {
                        if join_err.is_panic() {
                            // Re-panic to preserve the panic
                            std::panic::resume_unwind(join_err.into_panic());
                        } else {
                            Err(SimError::RuntimeError("Task was cancelled".into()))
                        }
                    }
                }
            }
            _ = &mut simulation_task => {
                // Simulation task finished (shouldn't happen)
                Err(SimError::RuntimeError("Simulation task unexpectedly finished".into()))
            }
        };
        
        // Cancel whichever task is still running
        simulation_task.abort();
        
        result
    }
    
    /// Run multiple futures concurrently with simulation advancement
    pub async fn run_many_with_simulation<F, T>(&self, futures: Vec<F>) -> Vec<Result<T>>
    where
        F: Future<Output = Result<T>> + Send + 'static,
        T: Send + 'static,
    {
        // Spawn all futures as tokio tasks
        let task_handles: Vec<JoinHandle<Result<T>>> = futures
            .into_iter()
            .map(|fut| tokio::spawn(fut))
            .collect();
        
        // Create a background task that advances the simulation
        let sim_ref = self.simulation.clone();
        let notify_ref = self.progress_notify.clone();
        let simulation_task = tokio::spawn(async move {
            let mut tick_count = 0;
            const MAX_TICKS: u64 = 10000;
            const TICK_BATCH_SIZE: u32 = 5;
            
            loop {
                if tick_count >= MAX_TICKS {
                    break;
                }
                
                // Advance simulation
                let mut made_progress = false;
                for _ in 0..TICK_BATCH_SIZE {
                    let mut sim = sim_ref.write().await;
                    
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
                            break;
                        }
                        made_progress = true;
                        tick_count += 1;
                    } else {
                        break;
                    }
                }
                
                if made_progress {
                    notify_ref.notify_waiters();
                    tokio::task::yield_now().await;
                } else {
                    tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
                }
            }
        });
        
        // Wait for all tasks to complete
        let results = futures::future::join_all(task_handles).await;
        
        // Cancel simulation task
        simulation_task.abort();
        
        // Convert results
        results
            .into_iter()
            .map(|res| match res {
                Ok(task_result) => task_result,
                Err(join_err) => {
                    if join_err.is_panic() {
                        // Can't re-panic in a collection, so convert to error
                        Err(SimError::RuntimeError("Task panicked".into()))
                    } else {
                        Err(SimError::RuntimeError("Task was cancelled".into()))
                    }
                }
            })
            .collect()
    }
}

// Helper function to check if scheduler has contexts to wake would go here
// but we can't extend SimulationScheduler from another crate

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Simulation;
    
    #[tokio::test]
    async fn test_tokio_integrated_executor() {
        let sim = Arc::new(RwLock::new(Simulation::new(42)));
        let executor = TokioIntegratedExecutor::new(sim.clone());
        
        // Run a simple future
        let result = executor.run_with_simulation(async {
            // Simulate some async work
            tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
            Ok(42)
        }).await;
        
        assert_eq!(result.unwrap(), 42);
    }
    
    #[tokio::test]
    async fn test_concurrent_futures() {
        let sim = Arc::new(RwLock::new(Simulation::new(42)));
        let executor = TokioIntegratedExecutor::new(sim.clone());
        
        // Run multiple futures concurrently
        let futures = vec![
            Box::pin(async { Ok(1) }) as std::pin::Pin<Box<dyn Future<Output = Result<i32>> + Send>>,
            Box::pin(async { Ok(2) }) as std::pin::Pin<Box<dyn Future<Output = Result<i32>> + Send>>,
            Box::pin(async { Ok(3) }) as std::pin::Pin<Box<dyn Future<Output = Result<i32>> + Send>>,
        ];
        
        let results = executor.run_many_with_simulation(futures).await;
        
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].as_ref().unwrap(), &1);
        assert_eq!(results[1].as_ref().unwrap(), &2);
        assert_eq!(results[2].as_ref().unwrap(), &3);
    }
}