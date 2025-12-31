//! Logical clock service (Layer 6 runtime-owned state).
//!
//! Provides a stateful implementation of LogicalClockEffects by persisting the
//! vector clock and Lamport counter across calls.

use async_trait::async_trait;
use aura_core::effects::time::{LogicalClockEffects, TimeError};
use aura_core::identifiers::DeviceId;
use aura_core::time::{LogicalTime, VectorClock};
use tokio::sync::RwLock;

/// Mutable logical clock state.
#[derive(Debug, Default, Clone)]
pub struct LogicalClockState {
    pub vector: VectorClock,
    pub lamport: u64,
}

/// Runtime-owned logical clock service.
#[derive(Debug)]
pub struct LogicalClockService {
    state: RwLock<LogicalClockState>,
    device_id: Option<DeviceId>,
}

impl LogicalClockService {
    /// Create a new logical clock service.
    pub fn new(device_id: Option<DeviceId>) -> Self {
        Self {
            state: RwLock::new(LogicalClockState::default()),
            device_id,
        }
    }

    /// Get the current logical clock state (snapshot).
    pub async fn snapshot(&self) -> LogicalClockState {
        self.state.read().await.clone()
    }

    /// Advance the logical clock using an observed vector clock.
    pub async fn advance(&self, observed: Option<&VectorClock>) -> LogicalTime {
        let mut state = self.state.write().await;
        let mut next_vector = state.vector.clone();
        let mut next_scalar = state.lamport;

        if let Some(obs) = observed {
            for (auth, val) in obs.iter() {
                let current_count = next_vector.get(auth).copied().unwrap_or(0);
                next_vector.insert(*auth, current_count.max(*val));
            }
            let obs_max = obs
                .iter()
                .map(|(_, v)| *v)
                .max()
                .unwrap_or(next_scalar);
            next_scalar = next_scalar.max(obs_max);
        }

        next_scalar = next_scalar.saturating_add(1);
        if let Some(auth) = self.device_id {
            let current_count = next_vector.get(&auth).copied().unwrap_or(0);
            next_vector.insert(auth, current_count.saturating_add(1));
        }

        let next = LogicalTime {
            vector: next_vector,
            lamport: next_scalar,
        };
        state.vector = next.vector.clone();
        state.lamport = next.lamport;
        next
    }
}

#[async_trait]
impl LogicalClockEffects for LogicalClockService {
    async fn logical_advance(
        &self,
        observed: Option<&VectorClock>,
    ) -> Result<LogicalTime, TimeError> {
        Ok(self.advance(observed).await)
    }

    async fn logical_now(&self) -> Result<LogicalTime, TimeError> {
        let state = self.state.read().await;
        Ok(LogicalTime {
            vector: state.vector.clone(),
            lamport: state.lamport,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_logical_clock_monotonicity() {
        let service = LogicalClockService::new(None);

        let t1 = service.logical_advance(None).await.unwrap();
        let t2 = service.logical_advance(None).await.unwrap();

        assert!(t2.lamport > t1.lamport);
    }

    #[tokio::test]
    async fn test_logical_clock_merges_observed() {
        let service = LogicalClockService::new(None);

        let mut observed = VectorClock::new();
        observed.insert(DeviceId::new_from_entropy([9u8; 32]), 5);

        let t = service.logical_advance(Some(&observed)).await.unwrap();
        assert!(t.lamport >= 6);
        assert!(t.vector.iter().any(|(_, v)| *v >= 5));
    }
}
