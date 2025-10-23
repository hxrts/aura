//! Choreographic tests for the DKG (Distributed Key Generation) protocol
//!
//! These tests demonstrate the choreographic approach to testing distributed protocols.

use crate::choreographic::{Choreography, ChoreographyBuilder, run_choreography};
use crate::{Simulation, SimulatedParticipant, Result, SimError};
use std::sync::Arc;
use std::future::Future;
use std::pin::Pin;

/// DKG protocol choreography for testing
pub struct DkgChoreography {
    threshold: usize,
}

impl DkgChoreography {
    pub fn new(threshold: usize) -> Self {
        Self { threshold }
    }
}

impl Choreography for DkgChoreography {
    type Output = DkgResult;
    
    fn execute(
        self,
        participants: Vec<Arc<SimulatedParticipant>>,
        session_id: uuid::Uuid,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Output>> + Send>> {
        Box::pin(async move {
            // Get device IDs
            let mut device_ids = Vec::new();
            for participant in &participants {
                let ledger = participant.ledger().await;
                let device_id = ledger.state().devices.keys().next().copied()
                    .ok_or_else(|| SimError::RuntimeError("No device ID".into()))?;
                device_ids.push(device_id);
            }
            
            // Execute DKG for all participants
            let futures: Vec<_> = participants
                .iter()
                .map(|p| {
                    let p = p.clone();
                    let ids = device_ids.clone();
                    async move {
                        p.initiate_dkd_with_session(session_id, ids, self.threshold).await
                    }
                })
                .collect();
            
            let keys = futures::future::try_join_all(futures).await?;
            
            // Verify consistency and return result
            let public_key = keys[0].clone();
            let consistent = keys.iter().all(|k| k == &public_key);
            
            Ok(DkgResult {
                keys,
                public_key,
                consistent,
                threshold: self.threshold,
                participant_count: participants.len(),
            })
        })
    }
}

/// Result of a DKG protocol execution
#[derive(Debug)]
pub struct DkgResult {
    pub keys: Vec<Vec<u8>>,
    pub public_key: Vec<u8>,
    pub consistent: bool,
    pub threshold: usize,
    pub participant_count: usize,
}

/// Byzantine DKG choreography where one participant misbehaves
pub struct ByzantineDkgChoreography {
    threshold: usize,
    byzantine_index: usize,
}

impl ByzantineDkgChoreography {
    pub fn new(threshold: usize, byzantine_index: usize) -> Self {
        Self { threshold, byzantine_index }
    }
}

impl Choreography for ByzantineDkgChoreography {
    type Output = DkgResult;
    
    fn execute(
        self,
        participants: Vec<Arc<SimulatedParticipant>>,
        session_id: uuid::Uuid,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Output>> + Send>> {
        // For now, just run normal DKG since Byzantine behavior isn't implemented
        // In the future, this would inject faults at the specified index
        DkgChoreography::new(self.threshold).execute(participants, session_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_dkg_three_party() {
        let choreography = DkgChoreography::new(2);
        
        let result = ChoreographyBuilder::new(choreography, 3)
            .with_seed(42)
            .run()
            .await
            .unwrap();
        
        assert!(result.consistent, "All parties should derive the same key");
        assert_eq!(result.keys.len(), 3);
        assert!(!result.public_key.is_empty());
        assert_eq!(result.public_key.len(), 32);
        assert_eq!(result.threshold, 2);
    }
    
    #[tokio::test]
    async fn test_dkg_five_party_threshold_three() {
        let choreography = DkgChoreography::new(3);
        
        let result = ChoreographyBuilder::new(choreography, 5)
            .with_seed(123)
            .with_latency(2, 10) // Higher latency
            .run()
            .await
            .unwrap();
        
        assert!(result.consistent);
        assert_eq!(result.keys.len(), 5);
        assert_eq!(result.threshold, 3);
    }
    
    #[tokio::test]
    async fn test_dkg_minimum_threshold() {
        // Test with minimum threshold (1-of-2)
        let choreography = DkgChoreography::new(1);
        
        let result = ChoreographyBuilder::new(choreography, 2)
            .run()
            .await
            .unwrap();
        
        assert!(result.consistent);
        assert_eq!(result.keys.len(), 2);
        assert_eq!(result.threshold, 1);
    }
    
    #[tokio::test]
    async fn test_dkg_maximum_threshold() {
        // Test with maximum threshold (N-of-N)
        let choreography = DkgChoreography::new(4);
        
        let result = ChoreographyBuilder::new(choreography, 4)
            .run()
            .await
            .unwrap();
        
        assert!(result.consistent);
        assert_eq!(result.keys.len(), 4);
        assert_eq!(result.threshold, 4);
    }
    
    #[tokio::test]
    async fn test_dkg_deterministic() {
        // Same seed should produce same results
        let choreography1 = DkgChoreography::new(2);
        let choreography2 = DkgChoreography::new(2);
        
        let result1 = ChoreographyBuilder::new(choreography1, 3)
            .with_seed(999)
            .run()
            .await
            .unwrap();
        
        let result2 = ChoreographyBuilder::new(choreography2, 3)
            .with_seed(999)
            .run()
            .await
            .unwrap();
        
        assert_eq!(result1.public_key, result2.public_key, "Same seed should produce same key");
    }
    
    #[tokio::test]
    async fn test_dkg_with_state_verification() {
        // Test with full state verification
        let mut sim = Simulation::new(42);
        sim.set_latency_range(1, 5);
        
        let choreography = DkgChoreography::new(2);
        let (result, sim) = run_choreography(sim, 3, choreography).await.unwrap();
        
        assert!(result.consistent);
        
        // Verify all participants have consistent state
        let participants = sim.get_all_participants();
        let mut epochs = Vec::new();
        
        for p_id in participants {
            let participant = sim.get_participant(p_id).unwrap();
            let ledger = participant.ledger_snapshot().await;
            epochs.push(ledger.state().session_epoch);
        }
        
        // All should have the same epoch
        assert!(epochs.windows(2).all(|w| w[0] == w[1]), "All participants should have same epoch");
    }
}