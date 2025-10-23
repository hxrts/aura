//! Tokio-based choreographic execution that avoids noop_waker issues
//!
//! This module provides a simpler approach that leverages tokio's task spawning
//! to ensure proper wake notification handling.

use crate::{Simulation, SimulatedParticipant, Result, SimError};
use std::future::Future;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

/// Run a choreography using tokio tasks for proper wake handling
pub async fn run_choreography_tokio<F, Fut, T>(
    mut sim: Simulation,
    participant_count: usize,
    choreography_fn: F,
) -> Result<T>
where
    F: FnOnce(Vec<Arc<SimulatedParticipant>>, uuid::Uuid) -> Fut + Send + 'static,
    Fut: Future<Output = Result<T>> + Send + 'static,
    T: Send + 'static,
{
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
    
    let participants: Vec<_> = device_info
        .iter()
        .map(|(id, _)| sim.get_participant(*id).unwrap())
        .collect();
    
    let session_id = sim.generate_uuid();
    
    // Wrap simulation in Arc<RwLock> for shared access
    let sim_arc = Arc::new(RwLock::new(sim));
    
    // Create the choreography future as a spawned task
    let participants_clone = participants.clone();
    let choreography_task: JoinHandle<Result<T>> = tokio::spawn(async move {
        choreography_fn(participants_clone, session_id).await
    });
    
    // Create a task that advances the simulation
    let sim_ref = sim_arc.clone();
    let mut sim_task = tokio::spawn(async move {
        let mut tick_count = 0;
        const MAX_TICKS: u64 = 10000;
        let mut idle_count = 0;
        
        loop {
            if tick_count >= MAX_TICKS {
                eprintln!("Simulation hit max ticks: {}", MAX_TICKS);
                break;
            }
            
            // Check if we should advance
            let (should_tick, is_idle, has_waiting, has_active) = {
                let sim = sim_ref.read().await;
                let is_idle = sim.is_idle().await;
                let scheduler = sim.scheduler();
                let scheduler_guard = scheduler.read().await;
                let has_waiting = scheduler_guard.has_waiting_contexts();
                let has_active = scheduler_guard.has_active_contexts();
                
                (!is_idle || has_waiting || has_active, is_idle, has_waiting, has_active)
            };
            
            if tick_count % 100 == 0 {
                println!("[Sim] Tick {}: is_idle={}, has_waiting={}, has_active={}", 
                    tick_count, is_idle, has_waiting, has_active);
            }
            
            if should_tick {
                let mut sim = sim_ref.write().await;
                if let Err(e) = sim.tick().await {
                    eprintln!("Simulation tick error: {:?}", e);
                    break;
                }
                tick_count += 1;
                idle_count = 0;
                
                // Yield frequently to let protocol tasks run
                tokio::task::yield_now().await;
            } else {
                // No work, wait a bit
                idle_count += 1;
                if idle_count > 100 {
                    eprintln!("[Sim] Simulation appears stuck - idle for {} iterations", idle_count);
                    eprintln!("[Sim] is_idle={}, has_waiting={}, has_active={}", is_idle, has_waiting, has_active);
                    // Force a tick to try to unstick
                    let mut sim = sim_ref.write().await;
                    if let Err(e) = sim.tick().await {
                        eprintln!("Forced tick error: {:?}", e);
                    }
                    tick_count += 1;
                    idle_count = 0;
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            }
        }
        
        println!("Simulation task completed after {} ticks", tick_count);
    });
    
    // Wait for choreography to complete
    let result = tokio::select! {
        res = choreography_task => {
            // Choreography completed
            match res {
                Ok(choreography_result) => choreography_result,
                Err(join_err) => {
                    if join_err.is_panic() {
                        std::panic::resume_unwind(join_err.into_panic());
                    } else {
                        Err(SimError::RuntimeError("Choreography task cancelled".into()))
                    }
                }
            }
        }
        _ = &mut sim_task => {
            // Simulation task finished first (shouldn't happen)
            Err(SimError::RuntimeError("Simulation task finished before choreography".into()))
        }
    };
    
    // Clean up
    sim_task.abort();
    
    result
}

/// Run DKD choreography with tokio tasks
pub async fn run_dkd_choreography_tokio(
    participant_count: usize,
    threshold: usize,
    seed: u64,
) -> Result<Vec<Vec<u8>>> {
    let sim = Simulation::new(seed);
    
    run_choreography_tokio(sim, participant_count, move |participants, session_id| {
        async move {
            // Get device IDs
            let mut device_ids = Vec::new();
            for participant in &participants {
                let ledger = participant.ledger().await;
                let device_id = ledger.state().devices.keys().next().copied()
                    .ok_or_else(|| SimError::RuntimeError("No device ID".into()))?;
                device_ids.push(device_id);
            }
            
            // Spawn all DKD instances as separate tasks
            let mut handles = Vec::new();
            
            for participant in participants {
                let ids = device_ids.clone();
                let handle = tokio::spawn(async move {
                    participant.initiate_dkd_with_session(session_id, ids, threshold).await
                });
                handles.push(handle);
            }
            
            // Wait for all to complete
            let mut results = Vec::new();
            for handle in handles {
                match handle.await {
                    Ok(dkd_result) => results.push(dkd_result?),
                    Err(join_err) => {
                        if join_err.is_panic() {
                            std::panic::resume_unwind(join_err.into_panic());
                        } else {
                            return Err(SimError::RuntimeError("DKD task cancelled".into()));
                        }
                    }
                }
            }
            
            Ok(results)
        }
    }).await
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    #[ignore] // TODO: Complete tokio choreographic implementation
    async fn test_tokio_dkd_three_party() {
        let keys = run_dkd_choreography_tokio(3, 2, 42).await.unwrap();
        
        assert_eq!(keys.len(), 3);
        assert_eq!(keys[0], keys[1]);
        assert_eq!(keys[1], keys[2]);
        assert!(!keys[0].is_empty());
    }
    
    #[tokio::test]
    #[ignore] // TODO: Complete tokio choreographic implementation
    async fn test_tokio_dkd_five_party() {
        let keys = run_dkd_choreography_tokio(5, 3, 123).await.unwrap();
        
        assert_eq!(keys.len(), 5);
        for i in 1..5 {
            assert_eq!(keys[0], keys[i]);
        }
    }
}