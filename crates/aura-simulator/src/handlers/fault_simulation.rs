#![allow(deprecated)]
//! Fault simulation effect handler for simulation
//!
//! This module provides simulation-specific fault injection capabilities through
//! the ChaosEffects trait. Replaces the former FaultSimulationMiddleware with
//! proper effect system integration.

use async_trait::async_trait;
use aura_core::effects::{ByzantineType, ChaosEffects, ChaosError, CorruptionType, ResourceType};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Simulation-specific fault injection handler
///
/// This handler implements the ChaosEffects trait to provide deterministic
/// fault injection for simulation testing.
pub struct SimulationFaultHandler {
    /// Active fault injections
    active_faults: std::sync::Mutex<HashMap<String, ActiveFault>>,
    /// Fault injection seed for deterministic behavior
    seed: u64,
    /// Maximum concurrent faults
    max_concurrent_faults: usize,
    /// Deterministic tick counter for fault timing
    clock: AtomicU64,
    /// Deterministic counter for fault IDs
    fault_counter: AtomicU64,
}

#[derive(Debug, Clone)]
struct ActiveFault {
    fault_type: String,
    start_tick: u64,
    duration_ms: Option<u64>,
    parameters: HashMap<String, String>,
}

// Mutex::lock().unwrap() is used throughout - simulation code doesn't handle poisoning
#[allow(clippy::unwrap_used)]
impl SimulationFaultHandler {
    /// Create a new simulation fault handler
    pub fn new(seed: u64) -> Self {
        Self {
            active_faults: std::sync::Mutex::new(HashMap::new()),
            seed,
            max_concurrent_faults: 10,
            clock: AtomicU64::new(0),
            fault_counter: AtomicU64::new(0),
        }
    }

    /// Create handler with maximum concurrent faults limit
    pub fn with_max_faults(seed: u64, max_faults: usize) -> Self {
        Self {
            active_faults: std::sync::Mutex::new(HashMap::new()),
            seed,
            max_concurrent_faults: max_faults,
            clock: AtomicU64::new(0),
            fault_counter: AtomicU64::new(0),
        }
    }

    /// Check if fault injection should be applied based on rate
    fn should_inject_fault(&self, rate: f64) -> bool {
        if rate <= 0.0 {
            return false;
        }
        if rate >= 1.0 {
            return true;
        }

        // Use deterministic pseudo-random based on seed
        let mut rng_state = self.seed;
        rng_state = rng_state.wrapping_mul(1103515245).wrapping_add(12345);
        let random_value = (rng_state >> 16) as f64 / u16::MAX as f64;

        random_value < rate
    }

    fn next_tick(&self) -> u64 {
        self.clock.fetch_add(1, Ordering::SeqCst)
    }

    fn next_fault_id(&self, prefix: &str) -> String {
        let id = self.fault_counter.fetch_add(1, Ordering::SeqCst);
        format!("{prefix}_{id}")
    }

    /// Add fault to active tracking
    fn track_fault(&self, fault_id: String, fault_type: String, duration: Option<Duration>) {
        let duration_ms = duration.map(|d| d.as_millis() as u64);
        let fault = ActiveFault {
            fault_type,
            start_tick: self.next_tick(),
            duration_ms,
            parameters: HashMap::new(),
        };

        let mut active_faults = self.active_faults.lock().unwrap();
        active_faults.insert(fault_id, fault);
    }

    /// Remove expired faults
    fn cleanup_expired_faults(&self) {
        let mut active_faults = self.active_faults.lock().unwrap();
        let now_tick = self.next_tick();

        active_faults.retain(|_, fault| {
            match fault.duration_ms {
                Some(duration_ms) => now_tick.saturating_sub(fault.start_tick) < duration_ms,
                None => true, // Permanent faults stay active
            }
        });
    }

    /// Check if we can inject more faults
    fn can_inject_more_faults(&self) -> bool {
        #[allow(clippy::unwrap_used)]
        // Simulation code - lock poisoning is not expected in test scenarios
        let active_faults = self.active_faults.lock().unwrap();
        active_faults.len() < self.max_concurrent_faults
    }
}

impl Default for SimulationFaultHandler {
    fn default() -> Self {
        Self::new(42) // Default deterministic seed
    }
}

#[async_trait]
impl ChaosEffects for SimulationFaultHandler {
    async fn inject_message_corruption(
        &self,
        corruption_rate: f64,
        corruption_type: CorruptionType,
    ) -> Result<(), ChaosError> {
        self.cleanup_expired_faults();

        if !self.can_inject_more_faults() {
            return Err(ChaosError::InjectionFailed {
                fault_type: "message_corruption".to_string(),
                reason: "Maximum concurrent faults reached".to_string(),
            });
        }

        if !(0.0..=1.0).contains(&corruption_rate) {
            return Err(ChaosError::InvalidConfiguration {
                reason: "Corruption rate must be between 0.0 and 1.0".to_string(),
            });
        }

        let fault_id = self.next_fault_id("corruption");
        self.track_fault(
            fault_id,
            format!("MessageCorruption({corruption_type:?})"),
            None, // Permanent until stopped
        );

        Ok(())
    }

    async fn inject_network_delay(
        &self,
        delay_range: (Duration, Duration),
        affected_peers: Option<Vec<String>>,
    ) -> Result<(), ChaosError> {
        self.cleanup_expired_faults();

        if !self.can_inject_more_faults() {
            return Err(ChaosError::InjectionFailed {
                fault_type: "network_delay".to_string(),
                reason: "Maximum concurrent faults reached".to_string(),
            });
        }

        if delay_range.0 > delay_range.1 {
            return Err(ChaosError::InvalidConfiguration {
                reason: "Min delay cannot be greater than max delay".to_string(),
            });
        }

        let fault_id = self.next_fault_id("delay");
        let peers_desc = match affected_peers {
            Some(ref peers) => format!("peers: {peers:?}"),
            None => "all peers".to_string(),
        };

        self.track_fault(
            fault_id,
            format!("NetworkDelay({delay_range:?}, {peers_desc})"),
            None,
        );

        Ok(())
    }

    async fn inject_network_partition(
        &self,
        partition_groups: Vec<Vec<String>>,
        duration: Duration,
    ) -> Result<(), ChaosError> {
        self.cleanup_expired_faults();

        if !self.can_inject_more_faults() {
            return Err(ChaosError::InjectionFailed {
                fault_type: "network_partition".to_string(),
                reason: "Maximum concurrent faults reached".to_string(),
            });
        }

        if partition_groups.is_empty() {
            return Err(ChaosError::InvalidConfiguration {
                reason: "Partition groups cannot be empty".to_string(),
            });
        }

        let fault_id = self.next_fault_id("partition");
        self.track_fault(
            fault_id,
            format!("NetworkPartition({} groups)", partition_groups.len()),
            Some(duration),
        );

        Ok(())
    }

    async fn inject_byzantine_behavior(
        &self,
        byzantine_peers: Vec<String>,
        behavior_type: ByzantineType,
    ) -> Result<(), ChaosError> {
        self.cleanup_expired_faults();

        if !self.can_inject_more_faults() {
            return Err(ChaosError::InjectionFailed {
                fault_type: "byzantine_behavior".to_string(),
                reason: "Maximum concurrent faults reached".to_string(),
            });
        }

        if byzantine_peers.is_empty() {
            return Err(ChaosError::InvalidConfiguration {
                reason: "Byzantine peers list cannot be empty".to_string(),
            });
        }

        let fault_id = self.next_fault_id("byzantine");
        self.track_fault(
            fault_id,
            format!(
                "Byzantine({:?}, {} peers)",
                behavior_type,
                byzantine_peers.len()
            ),
            None,
        );

        Ok(())
    }

    async fn inject_resource_exhaustion(
        &self,
        resource_type: ResourceType,
        constraint_level: f64,
    ) -> Result<(), ChaosError> {
        self.cleanup_expired_faults();

        if !self.can_inject_more_faults() {
            return Err(ChaosError::InjectionFailed {
                fault_type: "resource_exhaustion".to_string(),
                reason: "Maximum concurrent faults reached".to_string(),
            });
        }

        if !(0.0..=1.0).contains(&constraint_level) {
            return Err(ChaosError::InvalidConfiguration {
                reason: "Constraint level must be between 0.0 and 1.0".to_string(),
            });
        }

        let fault_id = self.next_fault_id("resource");
        self.track_fault(
            fault_id,
            format!("ResourceExhaustion({resource_type:?}, {constraint_level:.2})"),
            None,
        );

        Ok(())
    }

    async fn inject_timing_faults(
        &self,
        time_skew: Duration,
        clock_drift_rate: f64,
    ) -> Result<(), ChaosError> {
        self.cleanup_expired_faults();

        if !self.can_inject_more_faults() {
            return Err(ChaosError::InjectionFailed {
                fault_type: "timing_faults".to_string(),
                reason: "Maximum concurrent faults reached".to_string(),
            });
        }

        if clock_drift_rate < 0.0 {
            return Err(ChaosError::InvalidConfiguration {
                reason: "Clock drift rate cannot be negative".to_string(),
            });
        }

        let fault_id = self.next_fault_id("timing");
        self.track_fault(
            fault_id,
            format!("TimingFaults(skew: {time_skew:?}, drift: {clock_drift_rate:.2})"),
            None,
        );

        Ok(())
    }

    async fn stop_all_injections(&self) -> Result<(), ChaosError> {
        #[allow(clippy::unwrap_used)]
        // Simulation code - lock poisoning is not expected in test scenarios
        let mut active_faults = self.active_faults.lock().unwrap();
        active_faults.clear();
        Ok(())
    }
}

impl SimulationFaultHandler {
    /// Get information about currently active faults
    pub fn get_active_faults(&self) -> Vec<String> {
        self.cleanup_expired_faults();
        #[allow(clippy::unwrap_used)]
        // Simulation code - lock poisoning is not expected in test scenarios
        let active_faults = self.active_faults.lock().unwrap();
        active_faults
            .values()
            .map(|fault| fault.fault_type.clone())
            .collect()
    }

    /// Get count of active faults
    pub fn active_fault_count(&self) -> usize {
        self.cleanup_expired_faults();
        #[allow(clippy::unwrap_used)]
        // Simulation code - lock poisoning is not expected in test scenarios
        let active_faults = self.active_faults.lock().unwrap();
        active_faults.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_message_corruption_injection() {
        let handler = SimulationFaultHandler::new(123);

        let result = handler
            .inject_message_corruption(0.1, CorruptionType::BitFlip)
            .await;
        assert!(result.is_ok());

        assert_eq!(handler.active_fault_count(), 1);
        assert!(handler.get_active_faults()[0].contains("MessageCorruption"));
    }

    #[tokio::test]
    async fn test_invalid_corruption_rate() {
        let handler = SimulationFaultHandler::new(123);

        let result = handler
            .inject_message_corruption(1.5, CorruptionType::BitFlip)
            .await;
        assert!(result.is_err());

        if let Err(ChaosError::InvalidConfiguration { reason }) = result {
            assert!(reason.contains("between 0.0 and 1.0"));
        } else {
            panic!("Expected InvalidConfiguration error");
        }
    }

    #[tokio::test]
    async fn test_network_delay_injection() {
        let handler = SimulationFaultHandler::new(123);

        let delay_range = (Duration::from_millis(10), Duration::from_millis(100));
        let result = handler.inject_network_delay(delay_range, None).await;
        assert!(result.is_ok());

        assert_eq!(handler.active_fault_count(), 1);
    }

    #[tokio::test]
    async fn test_network_partition() {
        let handler = SimulationFaultHandler::new(123);

        let groups = vec![
            vec!["peer1".to_string(), "peer2".to_string()],
            vec!["peer3".to_string(), "peer4".to_string()],
        ];
        let result = handler
            .inject_network_partition(groups, Duration::from_secs(10))
            .await;
        assert!(result.is_ok());

        assert_eq!(handler.active_fault_count(), 1);
    }

    #[tokio::test]
    async fn test_byzantine_behavior() {
        let handler = SimulationFaultHandler::new(123);

        let peers = vec!["byzantine_peer".to_string()];
        let result = handler
            .inject_byzantine_behavior(peers, ByzantineType::Equivocation)
            .await;
        assert!(result.is_ok());

        assert_eq!(handler.active_fault_count(), 1);
    }

    #[tokio::test]
    async fn test_resource_exhaustion() {
        let handler = SimulationFaultHandler::new(123);

        let result = handler
            .inject_resource_exhaustion(ResourceType::Memory, 0.8)
            .await;
        assert!(result.is_ok());

        assert_eq!(handler.active_fault_count(), 1);
    }

    #[tokio::test]
    async fn test_timing_faults() {
        let handler = SimulationFaultHandler::new(123);

        let result = handler
            .inject_timing_faults(Duration::from_millis(50), 0.1)
            .await;
        assert!(result.is_ok());

        assert_eq!(handler.active_fault_count(), 1);
    }

    #[tokio::test]
    async fn test_stop_all_injections() {
        let handler = SimulationFaultHandler::new(123);

        // Inject multiple faults
        let _ = handler
            .inject_message_corruption(0.1, CorruptionType::BitFlip)
            .await;
        let _ = handler
            .inject_network_delay(
                (Duration::from_millis(10), Duration::from_millis(100)),
                None,
            )
            .await;

        assert_eq!(handler.active_fault_count(), 2);

        // Stop all injections
        let result = handler.stop_all_injections().await;
        assert!(result.is_ok());
        assert_eq!(handler.active_fault_count(), 0);
    }

    #[tokio::test]
    async fn test_max_concurrent_faults() {
        let handler = SimulationFaultHandler::with_max_faults(123, 2);

        // Inject up to the limit
        let _ = handler
            .inject_message_corruption(0.1, CorruptionType::BitFlip)
            .await;
        let _ = handler
            .inject_network_delay(
                (Duration::from_millis(10), Duration::from_millis(100)),
                None,
            )
            .await;

        // Try to inject one more (should fail)
        let result = handler
            .inject_byzantine_behavior(vec!["peer".to_string()], ByzantineType::Silent)
            .await;
        assert!(result.is_err());

        if let Err(ChaosError::InjectionFailed { reason, .. }) = result {
            assert!(reason.contains("Maximum concurrent faults reached"));
        } else {
            panic!("Expected InjectionFailed error");
        }
    }
}
