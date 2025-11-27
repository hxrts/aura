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

use aura_core::{identifiers::DeviceId, AuraError, AuraResult};
// Time system moved to Aura unified time system
use aura_core::time::TimeStamp;
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
    pub timestamp: TimeStamp,
    /// Description of what was leaked
    pub description: String,
}

impl LeakageEvent {
    /// Create a new leakage event with explicit timestamp
    ///
    /// # Arguments
    /// * `timestamp` - The timestamp when the leakage occurred (from TimeEffects)
    pub fn new(
        leak_type: LeakageType,
        cost: u64,
        observer: DeviceId,
        timestamp: TimeStamp,
        description: impl Into<String>,
    ) -> Self {
        Self {
            leak_type,
            cost,
            observer,
            timestamp,
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
    /// Budget refresh period in milliseconds (if applicable)
    pub refresh_period_ms: Option<u64>,
    /// Last refresh time (using Aura unified time system)
    pub last_refresh: TimeStamp,
}

impl LeakageBudget {
    /// Create a new leakage budget with explicit timestamp
    ///
    /// # Arguments
    /// * `now` - Current timestamp (from TimeEffects)
    pub fn new(observer: DeviceId, leak_type: LeakageType, limit: u64, now: TimeStamp) -> Self {
        Self {
            observer,
            leak_type,
            limit,
            consumed: 0,
            refresh_period_ms: None,
            last_refresh: now,
        }
    }

    /// Create budget with refresh period and explicit timestamp
    ///
    /// # Arguments
    /// * `now` - Current timestamp (from TimeEffects)
    pub fn with_refresh(
        observer: DeviceId,
        leak_type: LeakageType,
        limit: u64,
        refresh_period_ms: u64,
        now: TimeStamp,
    ) -> Self {
        Self {
            observer,
            leak_type,
            limit,
            consumed: 0,
            refresh_period_ms: Some(refresh_period_ms),
            last_refresh: now,
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
    ///
    /// # Arguments
    /// * `now` - Current timestamp (from TimeEffects)
    pub fn maybe_refresh(&mut self, now: TimeStamp) {
        if let Some(period_ms) = self.refresh_period_ms {
            // Use unified time system with explicit ordering policy
            use aura_core::time::{OrderingPolicy, TimeOrdering};

            let ordering = now.compare(&self.last_refresh, OrderingPolicy::Native);
            if let TimeOrdering::After = ordering {
                // Check if enough time has elapsed for refresh
                // For refresh timing, we need physical time comparison
                if let (
                    aura_core::time::TimeStamp::PhysicalClock(now_phys),
                    aura_core::time::TimeStamp::PhysicalClock(last_phys),
                ) = (&now, &self.last_refresh)
                {
                    let elapsed_ms = now_phys.ts_ms.saturating_sub(last_phys.ts_ms);

                    if elapsed_ms >= period_ms {
                        self.consumed = 0;
                        self.last_refresh = now;
                    }
                }
            }
        }
    }
}

/// Policy for handling undefined budgets
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum UndefinedBudgetPolicy {
    /// Allow unlimited access (legacy behavior, less secure)
    Allow,
    /// Deny access (secure default)
    #[default]
    Deny,
    /// Use a default budget with specified limit
    DefaultBudget(u64),
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
    /// Policy for handling undefined budgets
    undefined_budget_policy: UndefinedBudgetPolicy,
}

impl LeakageTracker {
    /// Create a new leakage tracker with secure defaults
    pub fn new() -> Self {
        Self {
            budgets: HashMap::new(),
            events: Vec::new(),
            max_events: 1000, // Default limit
            undefined_budget_policy: UndefinedBudgetPolicy::default(),
        }
    }

    /// Create a new leakage tracker with specified policy for undefined budgets
    pub fn with_undefined_policy(policy: UndefinedBudgetPolicy) -> Self {
        Self {
            budgets: HashMap::new(),
            events: Vec::new(),
            max_events: 1000,
            undefined_budget_policy: policy,
        }
    }

    /// Create a legacy-compatible tracker (allows undefined budgets)
    pub fn legacy_permissive() -> Self {
        Self::with_undefined_policy(UndefinedBudgetPolicy::Allow)
    }

    /// Add a budget for an observer and leak type
    pub fn add_budget(&mut self, budget: LeakageBudget) {
        let key = (budget.observer, budget.leak_type.clone());
        self.budgets.insert(key, budget);
    }

    /// Record a leakage event
    ///
    /// # Arguments
    /// * `now` - Current timestamp (from TimeEffects)
    pub fn record_leakage(
        &mut self,
        leak_type: LeakageType,
        cost: u64,
        observer: DeviceId,
        now: TimeStamp,
        description: impl Into<String>,
    ) -> AuraResult<()> {
        // Check budget
        let key = (observer, leak_type.clone());
        if let Some(budget) = self.budgets.get_mut(&key) {
            budget.maybe_refresh(now.clone());
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
        let event = LeakageEvent::new(leak_type, cost, observer, now, description);
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
            None => {
                // No budget defined - apply configured policy
                match &self.undefined_budget_policy {
                    UndefinedBudgetPolicy::Allow => {
                        tracing::warn!(
                            "No leakage budget defined for {} {} observer - allowing (permissive mode)",
                            leak_type,
                            observer
                        );
                        true
                    }
                    UndefinedBudgetPolicy::Deny => {
                        tracing::warn!(
                            "No leakage budget defined for {} {} observer - denying operation",
                            leak_type,
                            observer
                        );
                        false
                    }
                    UndefinedBudgetPolicy::DefaultBudget(limit) => {
                        tracing::info!(
                            "No leakage budget defined for {} {} observer - using default budget of {}",
                            leak_type,
                            observer,
                            limit
                        );
                        cost <= *limit
                    }
                }
            }
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
    use aura_core::identifiers::DeviceId;
    use aura_core::time::{PhysicalTime, TimeStamp};

    #[test]
    fn test_leakage_budget() {
        let now = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 1000,
            uncertainty: None,
        });
        let observer = DeviceId::new();
        let mut budget = LeakageBudget::new(observer, LeakageType::Metadata, 100, now.clone());

        assert!(budget.can_afford(50));
        assert!(budget.consume(50).is_ok());
        assert_eq!(budget.remaining(), 50);
        assert!(!budget.can_afford(60));
    }

    #[test]
    fn test_leakage_tracker() {
        let now = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 1000,
            uncertainty: None,
        });
        let observer = DeviceId::new();
        let mut tracker = LeakageTracker::new();

        let budget = LeakageBudget::new(observer, LeakageType::Metadata, 100, now.clone());
        tracker.add_budget(budget);

        assert!(tracker.check_leakage(&LeakageType::Metadata, 50, observer));
        assert!(tracker
            .record_leakage(LeakageType::Metadata, 50, observer, now, "test")
            .is_ok());
        assert!(!tracker.check_leakage(&LeakageType::Metadata, 60, observer));
    }

    #[test]
    fn test_privacy_contract() {
        let now = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 1000,
            uncertainty: None,
        });
        let observer = DeviceId::new();
        let budget = LeakageBudget::new(observer, LeakageType::Metadata, 100, now.clone());

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
