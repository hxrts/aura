//! Leakage Budget Tracking
//!
//! This module provides privacy budget tracking for choreographic protocols.
//! It ensures that protocols respect privacy contracts by tracking information
//! leakage and enforcing leakage bounds.
//!
//! # Syntax
//!
//! Leakage annotations use: `[leak: metadata]` where `metadata` describes
//! what information is being leaked to which observer.
//!
//! # Examples
//!
//! ```ignore
//! // Protocol with leakage tracking
//! choreography! {
//!     Alice[leak: metadata] -> Relay: ForwardMessage;
//!     Relay[leak: timing] -> Bob: DeliveredMessage;
//! }
//! ```

use aura_core::{AuraError, AuraResult, DeviceId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Type of information leakage
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum LeakageType {
    /// Message metadata (size, timing, etc.)
    Metadata,
    /// Timing information
    Timing,
    /// Communication patterns
    Patterns,
    /// Participant presence/absence
    Presence,
    /// Message frequency
    Frequency,
    /// Custom leakage type
    Custom(String),
}

impl std::fmt::Display for LeakageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LeakageType::Metadata => write!(f, "metadata"),
            LeakageType::Timing => write!(f, "timing"),
            LeakageType::Patterns => write!(f, "patterns"),
            LeakageType::Presence => write!(f, "presence"),
            LeakageType::Frequency => write!(f, "frequency"),
            LeakageType::Custom(s) => write!(f, "{}", s),
        }
    }
}

/// Leakage event record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeakageEvent {
    /// Type of leakage
    pub leak_type: LeakageType,
    /// Amount of budget consumed
    pub cost: u64,
    /// Observer who receives the information
    pub observer: DeviceId,
    /// When the leakage occurred
    pub timestamp: DateTime<Utc>,
    /// Description of what was leaked
    pub description: String,
}

impl LeakageEvent {
    /// Create a new leakage event
    #[allow(clippy::disallowed_methods)]
    pub fn new(
        leak_type: LeakageType,
        cost: u64,
        observer: DeviceId,
        description: impl Into<String>,
    ) -> Self {
        Self {
            leak_type,
            cost,
            observer,
            timestamp: Utc::now(),
            description: description.into(),
        }
    }
}

/// Leakage budget for a specific observer and leakage type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeakageBudget {
    /// Observer this budget applies to
    pub observer: DeviceId,
    /// Type of leakage this budget covers
    pub leak_type: LeakageType,
    /// Maximum allowed budget
    pub limit: u64,
    /// Currently consumed budget
    pub consumed: u64,
    /// Budget refresh period (if applicable)
    pub refresh_period: Option<chrono::Duration>,
    /// Last refresh time
    pub last_refresh: DateTime<Utc>,
}

impl LeakageBudget {
    /// Create a new leakage budget
    #[allow(clippy::disallowed_methods)]
    pub fn new(observer: DeviceId, leak_type: LeakageType, limit: u64) -> Self {
        Self {
            observer,
            leak_type,
            limit,
            consumed: 0,
            refresh_period: None,
            last_refresh: Utc::now(),
        }
    }

    /// Create budget with refresh period
    #[allow(clippy::disallowed_methods)]
    pub fn with_refresh(
        observer: DeviceId,
        leak_type: LeakageType,
        limit: u64,
        refresh_period: chrono::Duration,
    ) -> Self {
        Self {
            observer,
            leak_type,
            limit,
            consumed: 0,
            refresh_period: Some(refresh_period),
            last_refresh: Utc::now(),
        }
    }

    /// Check if budget allows the given cost
    pub fn can_afford(&self, cost: u64) -> bool {
        self.consumed + cost <= self.limit
    }

    /// Consume budget for a cost
    pub fn consume(&mut self, cost: u64) -> AuraResult<()> {
        if !self.can_afford(cost) {
            return Err(AuraError::permission_denied(format!(
                "Leakage budget exceeded for {} {} observer: {} + {} > {}",
                self.leak_type, self.observer, self.consumed, cost, self.limit
            )));
        }

        self.consumed += cost;
        Ok(())
    }

    /// Get remaining budget
    pub fn remaining(&self) -> u64 {
        self.limit.saturating_sub(self.consumed)
    }

    /// Refresh budget if refresh period has elapsed
    #[allow(clippy::disallowed_methods)]
    pub fn maybe_refresh(&mut self) {
        if let Some(period) = self.refresh_period {
            if Utc::now().signed_duration_since(self.last_refresh) >= period {
                self.consumed = 0;
                self.last_refresh = Utc::now();
            }
        }
    }
}

/// Leakage tracker for privacy contract enforcement
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeakageTracker {
    /// Budgets for each (observer, leak_type) pair
    budgets: HashMap<(DeviceId, LeakageType), LeakageBudget>,
    /// Event log for audit purposes
    events: Vec<LeakageEvent>,
    /// Maximum events to keep in memory
    max_events: usize,
}

impl LeakageTracker {
    /// Create a new leakage tracker
    pub fn new() -> Self {
        Self {
            budgets: HashMap::new(),
            events: Vec::new(),
            max_events: 1000, // Default limit
        }
    }

    /// Add a budget for an observer and leak type
    pub fn add_budget(&mut self, budget: LeakageBudget) {
        let key = (budget.observer, budget.leak_type.clone());
        self.budgets.insert(key, budget);
    }

    /// Record a leakage event
    pub fn record_leakage(
        &mut self,
        leak_type: LeakageType,
        cost: u64,
        observer: DeviceId,
        description: impl Into<String>,
    ) -> AuraResult<()> {
        // Check budget
        let key = (observer, leak_type.clone());
        if let Some(budget) = self.budgets.get_mut(&key) {
            budget.maybe_refresh();
            budget.consume(cost)?;
        } else {
            // No budget defined - allow but warn
            tracing::warn!(
                "No leakage budget defined for {} {} observer",
                leak_type,
                observer
            );
        }

        // Record event
        let event = LeakageEvent::new(leak_type, cost, observer, description);
        self.events.push(event);

        // Trim events if necessary
        if self.events.len() > self.max_events {
            self.events.remove(0);
        }

        Ok(())
    }

    /// Get budget for observer and leak type
    pub fn get_budget(
        &self,
        observer: DeviceId,
        leak_type: &LeakageType,
    ) -> Option<&LeakageBudget> {
        self.budgets.get(&(observer, leak_type.clone()))
    }

    /// Get remaining budget
    pub fn remaining_budget(&self, observer: DeviceId, leak_type: &LeakageType) -> Option<u64> {
        self.get_budget(observer, leak_type).map(|b| b.remaining())
    }

    /// Check if operation is allowed
    pub fn check_leakage(&self, leak_type: &LeakageType, cost: u64, observer: DeviceId) -> bool {
        match self.get_budget(observer, leak_type) {
            Some(budget) => budget.can_afford(cost),
            None => true, // No budget means unlimited (TODO fix - For now)
        }
    }

    /// Get all events for an observer
    pub fn events_for_observer(&self, observer: DeviceId) -> Vec<&LeakageEvent> {
        self.events
            .iter()
            .filter(|e| e.observer == observer)
            .collect()
    }

    /// Get total consumption for observer and leak type
    pub fn total_consumption(&self, observer: DeviceId, leak_type: &LeakageType) -> u64 {
        self.events
            .iter()
            .filter(|e| e.observer == observer && e.leak_type == *leak_type)
            .map(|e| e.cost)
            .sum()
    }
}

impl Default for LeakageTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Privacy contract specifying leakage bounds
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyContract {
    /// Name of the contract
    pub name: String,
    /// Leakage budgets defined by this contract
    pub budgets: Vec<LeakageBudget>,
    /// Contract description
    pub description: Option<String>,
}

impl PrivacyContract {
    /// Create a new privacy contract
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            budgets: Vec::new(),
            description: None,
        }
    }

    /// Add description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Add a budget to the contract
    pub fn add_budget(mut self, budget: LeakageBudget) -> Self {
        self.budgets.push(budget);
        self
    }

    /// Apply this contract to a tracker
    pub fn apply_to(&self, tracker: &mut LeakageTracker) {
        for budget in &self.budgets {
            tracker.add_budget(budget.clone());
        }
    }

    /// Validate that the contract is consistent
    pub fn validate(&self) -> AuraResult<()> {
        // Check for duplicate budgets
        let mut seen = std::collections::HashSet::new();
        for budget in &self.budgets {
            let key = (budget.observer, budget.leak_type.clone());
            if seen.contains(&key) {
                return Err(AuraError::invalid(format!(
                    "Duplicate budget for {} {} observer in contract '{}'",
                    budget.leak_type, budget.observer, self.name
                )));
            }
            seen.insert(key);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::DeviceId;
    use uuid::Uuid;

    #[test]
    fn test_leakage_budget() {
        let observer = DeviceId::new();
        let mut budget = LeakageBudget::new(observer, LeakageType::Metadata, 100);

        assert!(budget.can_afford(50));
        assert!(budget.consume(50).is_ok());
        assert_eq!(budget.remaining(), 50);
        assert!(!budget.can_afford(60));
    }

    #[test]
    fn test_leakage_tracker() {
        let observer = DeviceId::new();
        let mut tracker = LeakageTracker::new();

        let budget = LeakageBudget::new(observer, LeakageType::Metadata, 100);
        tracker.add_budget(budget);

        assert!(tracker.check_leakage(&LeakageType::Metadata, 50, observer));
        assert!(tracker
            .record_leakage(LeakageType::Metadata, 50, observer, "test")
            .is_ok());
        assert!(!tracker.check_leakage(&LeakageType::Metadata, 60, observer));
    }

    #[test]
    fn test_privacy_contract() {
        let observer = DeviceId::new();
        let budget = LeakageBudget::new(observer, LeakageType::Metadata, 100);

        let contract = PrivacyContract::new("test_contract")
            .with_description("Test privacy contract")
            .add_budget(budget);

        assert!(contract.validate().is_ok());
        assert_eq!(contract.budgets.len(), 1);
    }

    #[test]
    fn test_leakage_types() {
        assert_eq!(LeakageType::Metadata.to_string(), "metadata");
        assert_eq!(LeakageType::Timing.to_string(), "timing");
        assert_eq!(LeakageType::Custom("test".to_string()).to_string(), "test");
    }
}
