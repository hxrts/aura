//! DKD protocol choreography using session types
//!
//! This module implements the DKD protocol tests using our choreographic
//! programming framework with session type guarantees.

use crate::{
    choreographic_runner::{ChoreographicRunner, patterns},
    SimulatedParticipant,
    Result,
    SimError,
};
use aura_coordination::SessionId;

/// Run the DKD protocol with choreographic correctness
pub async fn run_dkd_choreography(
    runner: &ChoreographicRunner,
    participant_count: usize,
    threshold: usize,
) -> Result<Vec<Vec<u8>>> {
    patterns::threshold_protocol(
        runner,
        participant_count,
        threshold,
        "dkd_protocol",
        |participants, threshold| async move {
            // Generate session ID
            let session_id = {
                let sim = runner.simulation.read().await;
                sim.generate_uuid()
            };
            
            // Get device IDs
            let mut device_ids = Vec::new();
            for participant in &participants {
                let ledger = participant.ledger().await;
                let device_id = ledger.state().devices.keys().next().copied()
                    .ok_or_else(|| SimError::RuntimeError("No device ID".into()))?;
                device_ids.push(device_id);
            }
            
            // Execute DKD for all participants concurrently
            let futures: Vec<_> = participants
                .iter()
                .map(|p| {
                    let p = p.clone();
                    let ids = device_ids.clone();
                    async move {
                        p.initiate_dkd_with_session(session_id.0, ids, threshold).await
                    }
                })
                .collect();
            
            futures::future::try_join_all(futures).await
        },
    ).await
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_dkd_three_party_choreographic() {
        let runner = ChoreographicRunner::new(42);
        
        // Run 3-party DKD with threshold 2
        let keys = run_dkd_choreography(&runner, 3, 2).await.unwrap();
        
        // Verify results
        assert_eq!(keys.len(), 3);
        assert_eq!(keys[0], keys[1], "Alice and Bob should derive same key");
        assert_eq!(keys[1], keys[2], "Bob and Carol should derive same key");
        assert!(!keys[0].is_empty(), "Derived key should not be empty");
        assert_eq!(keys[0].len(), 32, "Derived key should be 32 bytes");
    }
    
    #[tokio::test]
    async fn test_dkd_five_party_choreographic() {
        let runner = ChoreographicRunner::new(123);
        
        // Run 5-party DKD with threshold 3
        let keys = run_dkd_choreography(&runner, 5, 3).await.unwrap();
        
        // Verify all participants derived the same key
        assert_eq!(keys.len(), 5);
        for i in 1..5 {
            assert_eq!(keys[0], keys[i], "All participants should derive same key");
        }
        assert!(!keys[0].is_empty(), "Derived key should not be empty");
    }
    
    #[tokio::test]
    async fn test_dkd_sequential_sessions() {
        let runner = ChoreographicRunner::new(456);
        
        // Run two sequential DKD sessions
        let keys1 = run_dkd_choreography(&runner, 3, 2).await.unwrap();
        let keys2 = run_dkd_choreography(&runner, 3, 2).await.unwrap();
        
        // Verify both sessions succeeded
        assert_eq!(keys1.len(), 3);
        assert_eq!(keys2.len(), 3);
        
        // Keys should be consistent within each session
        assert_eq!(keys1[0], keys1[1]);
        assert_eq!(keys1[1], keys1[2]);
        assert_eq!(keys2[0], keys2[1]);
        assert_eq!(keys2[1], keys2[2]);
        
        // Note: Without context_id, same participants produce same key
        // This is expected behavior
        assert_eq!(keys1[0], keys2[0], "Same participants produce same key without context_id");
    }
    
    #[tokio::test]
    async fn test_dkd_concurrent_sessions() {
        let runner = ChoreographicRunner::new(789);
        
        // Run two DKD sessions concurrently with different participant sets
        let session1 = run_dkd_choreography(&runner, 3, 2);
        let session2 = run_dkd_choreography(&runner, 4, 3);
        
        let (keys1, keys2) = tokio::join!(session1, session2);
        
        let keys1 = keys1.unwrap();
        let keys2 = keys2.unwrap();
        
        // Verify both sessions completed successfully
        assert_eq!(keys1.len(), 3);
        assert_eq!(keys2.len(), 4);
        
        // Each session should have consistent keys
        for i in 1..3 {
            assert_eq!(keys1[0], keys1[i]);
        }
        for i in 1..4 {
            assert_eq!(keys2[0], keys2[i]);
        }
    }
    
    #[tokio::test]
    async fn test_dkd_pattern_abstraction() {
        let runner = ChoreographicRunner::new(999);
        
        // Use the n_party_protocol pattern directly
        let results = patterns::n_party_protocol(
            &runner,
            ["alice", "bob", "carol"],
            "dkd_test",
            |participant, device_ids, session_id| async move {
                participant.initiate_dkd_with_session(
                    session_id,
                    device_ids,
                    2, // threshold
                ).await
            },
        ).await.unwrap();
        
        // Verify results
        assert_eq!(results[0], results[1]);
        assert_eq!(results[1], results[2]);
        assert!(!results[0].is_empty());
    }
}