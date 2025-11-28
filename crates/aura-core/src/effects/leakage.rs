//! Leakage tracking effects
//!
//! This module defines the effect traits for privacy leakage tracking
//! as specified in docs/002_theoretical_model.md §2.4 and docs/003_information_flow.md
//!
//! # Effect Classification
//!
//! - **Category**: Application Effect
//! - **Implementation**: `aura-protocol/guards` (Layer 4)
//! - **Usage**: Metadata leakage tracking and privacy budget enforcement
//!
//! This is an application effect implemented in orchestration layer by composing
//! infrastructure effects with privacy-specific business logic.

use crate::types::identifiers::ContextId;
use crate::{AuthorityId, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Observer classes for privacy leakage analysis
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ObserverClass {
    /// External observer (network adversary)
    External,
    /// Neighbor observer (local network)
    Neighbor,
    /// In-group observer (within context)
    InGroup,
}

/// Leakage event for privacy tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeakageEvent {
    /// Source authority
    pub source: AuthorityId,
    /// Destination authority
    pub destination: AuthorityId,
    /// Context where leakage occurred
    pub context_id: ContextId,
    /// Amount of leakage (flow units)
    pub leakage_amount: u64,
    /// Observer class that could see this
    pub observer_class: ObserverClass,
    /// Operation that caused leakage
    pub operation: String,
    /// Timestamp
    pub timestamp_ms: u64,
}

/// Leakage budget state
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LeakageBudget {
    /// Budget consumed per observer class
    pub external_consumed: u64,
    pub neighbor_consumed: u64,
    pub in_group_consumed: u64,
}

impl LeakageBudget {
    /// Create a zero budget
    pub fn zero() -> Self {
        Self::default()
    }

    /// Add leakage to budget
    pub fn add(&self, other: &LeakageBudget) -> Self {
        Self {
            external_consumed: self.external_consumed + other.external_consumed,
            neighbor_consumed: self.neighbor_consumed + other.neighbor_consumed,
            in_group_consumed: self.in_group_consumed + other.in_group_consumed,
        }
    }

    /// Check if within limits
    pub fn is_within_limits(&self, limits: &LeakageBudget) -> bool {
        self.external_consumed <= limits.external_consumed
            && self.neighbor_consumed <= limits.neighbor_consumed
            && self.in_group_consumed <= limits.in_group_consumed
    }

    /// Get leakage for observer class
    pub fn for_observer(&self, observer: ObserverClass) -> u64 {
        match observer {
            ObserverClass::External => self.external_consumed,
            ObserverClass::Neighbor => self.neighbor_consumed,
            ObserverClass::InGroup => self.in_group_consumed,
        }
    }

    /// Set leakage for observer class
    pub fn set_for_observer(&mut self, observer: ObserverClass, amount: u64) {
        match observer {
            ObserverClass::External => self.external_consumed = amount,
            ObserverClass::Neighbor => self.neighbor_consumed = amount,
            ObserverClass::InGroup => self.in_group_consumed = amount,
        }
    }
}

/// Effect trait for leakage tracking
#[async_trait]
pub trait LeakageEffects: Send + Sync {
    /// Record a leakage event
    async fn record_leakage(&self, event: LeakageEvent) -> Result<()>;

    /// Get current leakage budget for a context
    async fn get_leakage_budget(&self, context_id: ContextId) -> Result<LeakageBudget>;

    /// Check if operation would exceed budget
    async fn check_leakage_budget(
        &self,
        context_id: ContextId,
        observer: ObserverClass,
        amount: u64,
    ) -> Result<bool>;

    /// Get leakage history for analysis
    async fn get_leakage_history(
        &self,
        context_id: ContextId,
        since_timestamp: Option<u64>,
    ) -> Result<Vec<LeakageEvent>>;
}

/// Extension trait for choreography integration
#[async_trait]
pub trait LeakageChoreographyExt: LeakageEffects {
    /// Record leakage for a send operation
    ///
    /// # Arguments
    /// * `timestamp_ms` - Current timestamp in milliseconds since UNIX epoch.
    ///   Should be obtained from `PhysicalTimeEffects` to maintain effect system boundaries.
    async fn record_send_leakage(
        &self,
        source: AuthorityId,
        destination: AuthorityId,
        context_id: ContextId,
        flow_cost: u64,
        observer_classes: &[ObserverClass],
        timestamp_ms: u64,
    ) -> Result<()> {
        for observer in observer_classes {
            let event = LeakageEvent {
                source,
                destination,
                context_id,
                leakage_amount: flow_cost,
                observer_class: *observer,
                operation: "send".to_string(),
                timestamp_ms,
            };
            self.record_leakage(event).await?;
        }
        Ok(())
    }

    /// Record leakage for a receive operation
    ///
    /// # Arguments
    /// * `timestamp_ms` - Current timestamp in milliseconds since UNIX epoch.
    ///   Should be obtained from `PhysicalTimeEffects` to maintain effect system boundaries.
    async fn record_recv_leakage(
        &self,
        source: AuthorityId,
        destination: AuthorityId,
        context_id: ContextId,
        flow_cost: u64,
        observer_classes: &[ObserverClass],
        timestamp_ms: u64,
    ) -> Result<()> {
        for observer in observer_classes {
            let event = LeakageEvent {
                source,
                destination,
                context_id,
                leakage_amount: flow_cost,
                observer_class: *observer,
                operation: "recv".to_string(),
                timestamp_ms,
            };
            self.record_leakage(event).await?;
        }
        Ok(())
    }
}

// Blanket implementation
impl<T: LeakageEffects> LeakageChoreographyExt for T {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_leakage_budget() {
        let mut budget = LeakageBudget::zero();
        assert_eq!(budget.for_observer(ObserverClass::External), 0);

        budget.set_for_observer(ObserverClass::External, 100);
        assert_eq!(budget.for_observer(ObserverClass::External), 100);

        let other = LeakageBudget {
            external_consumed: 50,
            neighbor_consumed: 25,
            in_group_consumed: 10,
        };

        let combined = budget.add(&other);
        assert_eq!(combined.external_consumed, 150);
        assert_eq!(combined.neighbor_consumed, 25);
        assert_eq!(combined.in_group_consumed, 10);
    }
}
