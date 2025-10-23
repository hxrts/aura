//! Protocol execution support for simulations
//!
//! This module provides utilities to run protocols with automatic
//! simulation advancement, solving the coordination problem between
//! async protocol futures and tick-based simulation.

use crate::{Simulation, SimError, Result};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::sync::Arc;

/// Executes protocols while automatically advancing the simulation
pub struct ProtocolExecutor<'a> {
    simulation: &'a mut Simulation,
    max_ticks: u64,
}

impl<'a> ProtocolExecutor<'a> {
    /// Create a new protocol executor
    pub fn new(simulation: &'a mut Simulation) -> Self {
        Self {
            simulation,
            max_ticks: 10000, // Default safety limit
        }
    }
    
    /// Set maximum ticks before timeout
    pub fn with_max_ticks(mut self, max_ticks: u64) -> Self {
        self.max_ticks = max_ticks;
        self
    }
    
    /// Execute a single protocol with simulation advancement
    pub async fn run<F, T>(&mut self, protocol_future: F) -> Result<T>
    where
        F: Future<Output = Result<T>>,
    {
        let results = self.run_many(vec![protocol_future]).await;
        results.into_iter().next().unwrap()
    }
    
    /// Execute multiple protocols concurrently with simulation advancement
    pub async fn run_many<F, T>(&mut self, protocol_futures: Vec<F>) -> Vec<Result<T>>
    where
        F: Future<Output = Result<T>>,
    {
        // Convert to pinned futures
        let mut futures: Vec<Pin<Box<dyn Future<Output = Result<T>>>>> = protocol_futures
            .into_iter()
            .map(|f| Box::pin(f) as Pin<Box<dyn Future<Output = Result<T>>>>)
            .collect();
        
        let mut results: Vec<Option<Result<T>>> = Vec::new();
        for _ in 0..futures.len() {
            results.push(None);
        }
        let mut ticks = 0;
        
        // Run until all complete or timeout
        while results.iter().any(|r| r.is_none()) && ticks < self.max_ticks {
            // Poll all incomplete futures
            let waker = futures::task::noop_waker();
            let mut cx = Context::from_waker(&waker);
            
            let mut made_progress = false;
            for (i, future) in futures.iter_mut().enumerate() {
                if results[i].is_none() {
                    match future.as_mut().poll(&mut cx) {
                        Poll::Ready(result) => {
                            results[i] = Some(result);
                            made_progress = true;
                        }
                        Poll::Pending => {}
                    }
                }
            }
            
            // If no progress, advance simulation
            if !made_progress {
                match self.simulation.tick().await {
                    Ok(_) => {},
                    Err(e) => {
                        // Return error for all incomplete futures
                        return results.into_iter()
                            .map(|r| r.unwrap_or(Err(e.clone())))
                            .collect();
                    }
                }
                ticks += 1;
                
                // Yield to allow async runtime to process tasks
                tokio::task::yield_now().await;
            }
        }
        
        if ticks >= self.max_ticks {
            // Fill in errors for any incomplete futures
            for result in &mut results {
                if result.is_none() {
                    *result = Some(Err(SimError::RuntimeError(
                        format!("Protocol execution timed out after {} ticks", self.max_ticks)
                    )));
                }
            }
        }
        
        // Unwrap all results
        results.into_iter().map(|r| r.unwrap()).collect()
    }
}

/// Extension trait for Simulation to add protocol execution
impl Simulation {
    /// Execute protocols with automatic simulation advancement
    pub async fn execute_protocols<F, Fut, T>(&mut self, protocol_fn: F) -> Result<T>
    where
        F: FnOnce(&mut ProtocolExecutor) -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let mut executor = ProtocolExecutor::new(self);
        let future = protocol_fn(&mut executor);
        future.await
    }
}

/// Helper for running N-party protocols
pub async fn run_n_party_protocol<F, T>(
    participant_count: usize,
    protocol_fn: F,
) -> Result<Vec<T>>
where
    F: FnOnce(Vec<Arc<crate::SimulatedParticipant>>) -> Vec<Pin<Box<dyn Future<Output = Result<T>> + Send>>>,
    T: Send + 'static,
{
    let mut sim = Simulation::new(42);
    
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
    
    let (_account_id, device_info) = sim.add_account_with_devices(&names).await;
    
    // Get participant references
    let participants: Vec<_> = device_info
        .iter()
        .map(|(id, _)| sim.get_participant(*id).unwrap())
        .collect();
    
    // Create protocol futures
    let protocol_futures = protocol_fn(participants);
    
    // Execute with automatic simulation advancement
    let mut executor = ProtocolExecutor::new(&mut sim);
    executor.run_many(protocol_futures).await.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_protocol_executor() {
        let mut sim = Simulation::new(42);
        
        // Simple test that executor can run
        let result = sim.execute_protocols(|_executor| {
            Box::pin(async move {
                // Just return a value
                Ok(42)
            })
        }).await;
        
        assert_eq!(result.unwrap(), 42);
    }
}