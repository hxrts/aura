//! Leakage effects handlers for production and testing
//!
//! This module provides implementations of LeakageEffects for tracking
//! privacy leakage across different observer classes.

use async_trait::async_trait;
use aura_core::{
    effects::{LeakageBudget, LeakageEffects, LeakageEvent, ObserverClass},
    identifiers::ContextId,
    Result,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Production leakage handler with persistent storage
pub struct ProductionLeakageHandler {
    /// In-memory cache of budgets per context
    budgets: Arc<RwLock<HashMap<ContextId, LeakageBudget>>>,
    /// History of leakage events
    history: Arc<RwLock<Vec<LeakageEvent>>>,
    /// Budget limits per observer class
    limits: LeakageBudget,
}

impl ProductionLeakageHandler {
    /// Create a new production leakage handler
    pub fn new(limits: LeakageBudget) -> Self {
        Self {
            budgets: Arc::new(RwLock::new(HashMap::new())),
            history: Arc::new(RwLock::new(Vec::new())),
            limits,
        }
    }

    /// Create with default limits
    pub fn with_defaults() -> Self {
        let limits = LeakageBudget {
            external_consumed: 10000,  // 10K flow units for external
            neighbor_consumed: 50000,  // 50K for neighbors
            in_group_consumed: 100000, // 100K for in-group
        };
        Self::new(limits)
    }
}

#[async_trait]
impl LeakageEffects for ProductionLeakageHandler {
    async fn record_leakage(&self, event: LeakageEvent) -> Result<()> {
        // Update budget
        let mut budgets = self.budgets.write().await;
        let budget = budgets
            .entry(event.context_id)
            .or_insert_with(LeakageBudget::zero);

        let current = budget.for_observer(event.observer_class);
        budget.set_for_observer(event.observer_class, current + event.leakage_amount);

        // Record history
        let mut history = self.history.write().await;
        history.push(event);

        Ok(())
    }

    async fn get_leakage_budget(&self, context_id: ContextId) -> Result<LeakageBudget> {
        let budgets = self.budgets.read().await;
        Ok(budgets
            .get(&context_id)
            .cloned()
            .unwrap_or_else(LeakageBudget::zero))
    }

    async fn check_leakage_budget(
        &self,
        context_id: ContextId,
        observer: ObserverClass,
        amount: u64,
    ) -> Result<bool> {
        let budgets = self.budgets.read().await;
        let current = budgets
            .get(&context_id)
            .map(|b| b.for_observer(observer))
            .unwrap_or(0);

        let limit = self.limits.for_observer(observer);
        Ok(current + amount <= limit)
    }

    async fn get_leakage_history(
        &self,
        context_id: ContextId,
        since_timestamp: Option<u64>,
    ) -> Result<Vec<LeakageEvent>> {
        let history = self.history.read().await;
        let filtered: Vec<_> = history
            .iter()
            .filter(|e| e.context_id == context_id)
            .filter(|e| since_timestamp.map_or(true, |ts| e.timestamp_ms >= ts))
            .cloned()
            .collect();
        Ok(filtered)
    }
}

/// Test leakage handler for unit tests
pub struct TestLeakageHandler {
    /// Recorded events
    pub events: Arc<RwLock<Vec<LeakageEvent>>>,
    /// Configured to always allow or deny
    pub always_allow: bool,
}

impl TestLeakageHandler {
    /// Create a permissive test handler
    pub fn permissive() -> Self {
        Self {
            events: Arc::new(RwLock::new(Vec::new())),
            always_allow: true,
        }
    }

    /// Create a restrictive test handler
    pub fn restrictive() -> Self {
        Self {
            events: Arc::new(RwLock::new(Vec::new())),
            always_allow: false,
        }
    }

    /// Get recorded events
    pub async fn get_events(&self) -> Vec<LeakageEvent> {
        self.events.read().await.clone()
    }
}

#[async_trait]
impl LeakageEffects for TestLeakageHandler {
    async fn record_leakage(&self, event: LeakageEvent) -> Result<()> {
        self.events.write().await.push(event);
        Ok(())
    }

    async fn get_leakage_budget(&self, _context_id: ContextId) -> Result<LeakageBudget> {
        Ok(LeakageBudget::zero())
    }

    async fn check_leakage_budget(
        &self,
        _context_id: ContextId,
        _observer: ObserverClass,
        _amount: u64,
    ) -> Result<bool> {
        Ok(self.always_allow)
    }

    async fn get_leakage_history(
        &self,
        context_id: ContextId,
        since_timestamp: Option<u64>,
    ) -> Result<Vec<LeakageEvent>> {
        let events = self.events.read().await;
        let filtered: Vec<_> = events
            .iter()
            .filter(|e| e.context_id == context_id)
            .filter(|e| since_timestamp.map_or(true, |ts| e.timestamp_ms >= ts))
            .cloned()
            .collect();
        Ok(filtered)
    }
}

/// Mock leakage handler that does nothing
pub struct NoOpLeakageHandler;

#[async_trait]
impl LeakageEffects for NoOpLeakageHandler {
    async fn record_leakage(&self, _event: LeakageEvent) -> Result<()> {
        Ok(())
    }

    async fn get_leakage_budget(&self, _context_id: ContextId) -> Result<LeakageBudget> {
        Ok(LeakageBudget::zero())
    }

    async fn check_leakage_budget(
        &self,
        _context_id: ContextId,
        _observer: ObserverClass,
        _amount: u64,
    ) -> Result<bool> {
        Ok(true)
    }

    async fn get_leakage_history(
        &self,
        _context_id: ContextId,
        _since_timestamp: Option<u64>,
    ) -> Result<Vec<LeakageEvent>> {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::AuthorityId;

    #[tokio::test]
    async fn test_production_handler() {
        let handler = ProductionLeakageHandler::with_defaults();
        let context = ContextId::new();

        // Record some leakage
        let event = LeakageEvent {
            source: AuthorityId::new(),
            destination: AuthorityId::new(),
            context_id: context,
            leakage_amount: 100,
            observer_class: ObserverClass::External,
            operation: "test".to_string(),
            timestamp_ms: 0,
        };

        handler.record_leakage(event).await.unwrap();

        // Check budget
        let budget = handler.get_leakage_budget(context).await.unwrap();
        assert_eq!(budget.external_consumed, 100);

        // Check within limits
        let allowed = handler
            .check_leakage_budget(context, ObserverClass::External, 9900)
            .await
            .unwrap();
        assert!(allowed);
    }

    #[tokio::test]
    async fn test_test_handler() {
        let handler = TestLeakageHandler::permissive();
        let context = ContextId::new();

        let event = LeakageEvent {
            source: AuthorityId::new(),
            destination: AuthorityId::new(),
            context_id: context,
            leakage_amount: 100,
            observer_class: ObserverClass::Neighbor,
            operation: "test".to_string(),
            timestamp_ms: 100,
        };

        handler.record_leakage(event).await.unwrap();

        let events = handler.get_events().await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].leakage_amount, 100);
    }
}
