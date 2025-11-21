//! Relay Capability System
//!
//! Types for managing relay capabilities and flow budget enforcement
//! in the privacy-preserving communication system.

use aura_core::identifiers::DeviceId;
use aura_core::FlowBudget;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::Duration;

/// Capability granted to a relay device for forwarding messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayCapability {
    /// Device ID of the relay
    pub relay_device_id: DeviceId,
    /// Flow budget allocated to this relay
    pub flow_budget: FlowBudget,
    /// Maximum concurrent streams this relay can handle
    pub max_concurrent_streams: u32,
    /// Optional restriction on allowed destination devices
    pub allowed_destinations: Option<HashSet<DeviceId>>,
    /// Capability expiration timestamp (seconds since UNIX epoch)
    pub expires_at: u64,
    /// Decay policy for the flow budget
    pub decay_policy: BudgetDecayPolicy,
}

/// Policy for how flow budgets decay over time
#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
pub enum BudgetDecayPolicy {
    /// No decay - budget remains constant until manually reset
    #[default]
    NoDecay,
    /// Linear decay over time
    LinearDecay {
        /// Rate of decay per second (bytes/second)
        decay_rate: f64,
        /// Interval between decay applications
        decay_interval: Duration,
    },
    /// Exponential decay with half-life
    ExponentialDecay {
        /// Time for budget to decay to half value
        half_life: Duration,
    },
}


impl RelayCapability {
    /// Create new relay capability
    pub fn new(
        relay_device_id: DeviceId,
        flow_budget: FlowBudget,
        max_concurrent_streams: u32,
        expires_at: u64,
    ) -> Self {
        Self {
            relay_device_id,
            flow_budget,
            max_concurrent_streams,
            allowed_destinations: None,
            expires_at,
            decay_policy: BudgetDecayPolicy::default(),
        }
    }

    /// Create relay capability with destination restrictions
    pub fn with_restricted_destinations(
        relay_device_id: DeviceId,
        flow_budget: FlowBudget,
        max_concurrent_streams: u32,
        allowed_destinations: HashSet<DeviceId>,
        expires_at: u64,
    ) -> Self {
        Self {
            relay_device_id,
            flow_budget,
            max_concurrent_streams,
            allowed_destinations: Some(allowed_destinations),
            expires_at,
            decay_policy: BudgetDecayPolicy::default(),
        }
    }

    /// Consume flow budget for a message
    pub fn consume_flow_budget(&mut self, bytes: u64) -> Result<(), String> {
        if self.flow_budget.can_charge(bytes) {
            self.flow_budget.record_charge(bytes);
            Ok(())
        } else {
            Err(format!(
                "Insufficient flow budget: available {}, requested {}",
                self.flow_budget.headroom(),
                bytes
            ))
        }
    }

    /// Check if relay can forward to a destination
    pub fn can_forward_to(&self, destination: &DeviceId) -> bool {
        match &self.allowed_destinations {
            Some(allowed) => allowed.contains(destination),
            None => true, // No restrictions
        }
    }

    /// Check if capability has expired
    pub fn is_expired(&self, current_time: u64) -> bool {
        current_time > self.expires_at
    }

    /// Apply decay policy to the flow budget
    pub fn apply_decay(&mut self, current_time: u64, last_update_time: u64) {
        let elapsed = current_time.saturating_sub(last_update_time);

        match &self.decay_policy {
            BudgetDecayPolicy::NoDecay => {
                // No decay
            }
            BudgetDecayPolicy::LinearDecay {
                decay_rate,
                decay_interval,
            } => {
                let intervals_elapsed = elapsed / decay_interval.as_secs();
                let total_decay = (*decay_rate * intervals_elapsed as f64) as u64;

                if total_decay > 0 {
                    let current_spent = self.flow_budget.spent;
                    let new_spent = current_spent
                        .saturating_add(total_decay)
                        .min(self.flow_budget.limit);
                    self.flow_budget.spent = new_spent;
                }
            }
            BudgetDecayPolicy::ExponentialDecay { half_life } => {
                let half_life_secs = half_life.as_secs() as f64;
                if half_life_secs > 0.0 && elapsed > 0 {
                    let decay_factor = 2.0_f64.powf(-(elapsed as f64) / half_life_secs);
                    let remaining_budget = self.flow_budget.headroom() as f64;
                    let new_remaining = (remaining_budget * decay_factor) as u64;
                    let decay_amount = self.flow_budget.headroom().saturating_sub(new_remaining);

                    if decay_amount > 0 {
                        self.flow_budget.spent = self
                            .flow_budget
                            .spent
                            .saturating_add(decay_amount)
                            .min(self.flow_budget.limit);
                    }
                }
            }
        }
    }
}

/// Meet-semilattice operation for RelayCapability
/// The meet operation produces a capability with the most restrictive permissions
impl RelayCapability {
    /// Meet-semilattice operation (intersection of capabilities)
    /// Returns the more restrictive capability
    pub fn meet(&self, other: &Self) -> Self {
        // Can only meet capabilities for the same relay device
        if self.relay_device_id != other.relay_device_id {
            // Return the more restrictive one (self by convention)
            return self.clone();
        }

        // Take the more restrictive flow budget (lower limit)
        let flow_budget = if self.flow_budget.limit <= other.flow_budget.limit {
            self.flow_budget
        } else {
            other.flow_budget
        };

        // Take the more restrictive concurrent streams limit
        let max_concurrent_streams = self
            .max_concurrent_streams
            .min(other.max_concurrent_streams);

        // Intersect allowed destinations (more restrictive)
        let allowed_destinations = match (&self.allowed_destinations, &other.allowed_destinations) {
            (Some(self_dest), Some(other_dest)) => {
                // Intersection of both sets
                let intersection: HashSet<_> =
                    self_dest.intersection(other_dest).cloned().collect();
                Some(intersection)
            }
            (Some(dest), None) => Some(dest.clone()),
            (None, Some(dest)) => Some(dest.clone()),
            (None, None) => None,
        };

        // Take the earlier expiration time (more restrictive)
        let expires_at = self.expires_at.min(other.expires_at);

        // Use the more aggressive decay policy
        let decay_policy = match (&self.decay_policy, &other.decay_policy) {
            (BudgetDecayPolicy::NoDecay, other) => other.clone(),
            (self_policy, BudgetDecayPolicy::NoDecay) => self_policy.clone(),
            (
                BudgetDecayPolicy::LinearDecay { decay_rate: r1, .. },
                BudgetDecayPolicy::LinearDecay { decay_rate: r2, .. },
            ) => {
                if r1 > r2 {
                    self.decay_policy.clone()
                } else {
                    other.decay_policy.clone()
                }
            }
            (
                BudgetDecayPolicy::ExponentialDecay { half_life: h1 },
                BudgetDecayPolicy::ExponentialDecay { half_life: h2 },
            ) => {
                if h1 < h2 {
                    self.decay_policy.clone()
                } else {
                    other.decay_policy.clone()
                }
            }
            // Mixed policies - prefer exponential as more restrictive
            (BudgetDecayPolicy::ExponentialDecay { .. }, _) => self.decay_policy.clone(),
            (_, BudgetDecayPolicy::ExponentialDecay { .. }) => other.decay_policy.clone(),
        };

        Self {
            relay_device_id: self.relay_device_id,
            flow_budget,
            max_concurrent_streams,
            allowed_destinations,
            expires_at,
            decay_policy,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{FlowBudget, session_epochs::Epoch};

    #[test]
    fn test_relay_capability_creation() {
        let device_id = DeviceId::from("relay-1");
        let budget = FlowBudget::new(1000, Epoch::initial());
        let capability = RelayCapability::new(device_id, budget, 10, 1000000);

        assert_eq!(capability.max_concurrent_streams, 10);
        assert_eq!(capability.expires_at, 1000000);
        assert!(capability.allowed_destinations.is_none());
    }

    #[test]
    fn test_flow_budget_consumption() {
        let device_id = DeviceId::from("relay-1");
        let budget = FlowBudget::new(1000, Epoch::initial());
        let mut capability = RelayCapability::new(device_id, budget, 10, 1000000);

        // Should succeed
        assert!(capability.consume_flow_budget(100).is_ok());
        assert_eq!(capability.flow_budget.spent, 100);

        // Should fail when exceeding budget
        assert!(capability.consume_flow_budget(2000).is_err());
    }

    #[test]
    fn test_destination_restrictions() {
        let device_id = DeviceId::from("relay-1");
        let dest1 = DeviceId::from("dest-1");
        let dest2 = DeviceId::from("dest-2");
        let dest3 = DeviceId::from("dest-3");

        let budget = FlowBudget::new(1000, Epoch::initial());
        let mut allowed = HashSet::new();
        allowed.insert(dest1);
        allowed.insert(dest2);

        let capability =
            RelayCapability::with_restricted_destinations(device_id, budget, 10, allowed, 1000000);

        assert!(capability.can_forward_to(&dest1));
        assert!(capability.can_forward_to(&dest2));
        assert!(!capability.can_forward_to(&dest3));
    }

    #[test]
    fn test_capability_meet() {
        let device_id = DeviceId::from("relay-1");
        let budget1 = FlowBudget::new(1000, Epoch::initial());
        let budget2 = FlowBudget::new(500, Epoch::initial());

        let cap1 = RelayCapability::new(device_id, budget1, 10, 2000000);
        let cap2 = RelayCapability::new(device_id, budget2, 5, 1000000);

        let meet_result = cap1.meet(&cap2);

        // Should take the more restrictive values
        assert_eq!(meet_result.flow_budget.limit, 500); // Lower limit
        assert_eq!(meet_result.max_concurrent_streams, 5); // Lower streams
        assert_eq!(meet_result.expires_at, 1000000); // Earlier expiration
    }

    #[test]
    fn test_expiration() {
        let device_id = DeviceId::from("relay-1");
        let budget = FlowBudget::new(1000, Epoch::initial());
        let capability = RelayCapability::new(device_id, budget, 10, 1000000);

        assert!(!capability.is_expired(500000));
        assert!(capability.is_expired(2000000));
    }
}
