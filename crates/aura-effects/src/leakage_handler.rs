//! Layer 3: Leakage Effect Handlers - Production Only
//!
//! Stateless single-party implementation of LeakageEffects from aura-core (Layer 1).
//! This handler provides production leakage tracking delegating to storage APIs.
//!
//! **Layer Constraint**: NO mock handlers - those belong in aura-testkit (Layer 8).
//! This module contains only production-grade stateless handlers.

use async_trait::async_trait;
use aura_core::{
    effects::{LeakageBudget, LeakageEffects, LeakageEvent, ObserverClass},
    identifiers::ContextId,
    AuraError, Result,
};
use serde_json::to_string;
use std::path::PathBuf;
use std::{fs, io::Write};

/// Production leakage handler for production use
///
/// This handler tracks privacy leakage by delegating to persistent storage.
/// It is stateless and does not maintain in-memory state.
///
/// **Note**: Complex leakage aggregation and multi-context coordination has been
/// moved to `LeakageCoordinator` in aura-protocol (Layer 4). This handler provides
/// only stateless storage operations. For coordination capabilities, wrap this handler
/// with `aura_protocol::handlers::LeakageCoordinator`.
#[derive(Debug, Clone)]
pub struct ProductionLeakageHandler {
    /// Storage configuration for leakage data
    _storage_path: PathBuf,
    /// Budget limits per observer class
    limits: LeakageBudget,
}

impl ProductionLeakageHandler {
    /// Create a new production leakage handler
    pub fn new(storage_path: PathBuf, limits: LeakageBudget) -> Self {
        Self {
            _storage_path: storage_path,
            limits,
        }
    }

    /// Create with default configuration
    pub fn with_defaults() -> Self {
        let limits = LeakageBudget {
            external_consumed: 10000,  // 10K flow units for external
            neighbor_consumed: 50000,  // 50K for neighbors
            in_group_consumed: 100000, // 100K for in-group
        };
        Self::new(PathBuf::from("./leakage"), limits)
    }

    /// Create a new production leakage handler
    pub fn new_real(storage_path: PathBuf, limits: LeakageBudget) -> Self {
        Self::new(storage_path, limits)
    }
}

#[async_trait]
impl LeakageEffects for ProductionLeakageHandler {
    async fn record_leakage(&self, event: LeakageEvent) -> Result<()> {
        let path = self.event_file(event.context_id);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| AuraError::storage(e.to_string()))?;
        }
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| AuraError::storage(e.to_string()))?;

        let line = to_string(&event).map_err(|e| AuraError::serialization(e.to_string()))?;
        file.write_all(line.as_bytes())
            .and_then(|_| file.write_all(b"\n"))
            .map_err(|e| AuraError::storage(e.to_string()))?;

        Ok(())
    }

    async fn get_leakage_budget(&self, context_id: ContextId) -> Result<LeakageBudget> {
        let events = self.load_events(context_id)?;
        let mut budget = LeakageBudget::zero();
        for event in events {
            let consumed = budget.for_observer(event.observer_class) + event.leakage_amount;
            budget.set_for_observer(event.observer_class, consumed);
        }
        Ok(budget)
    }

    async fn check_leakage_budget(
        &self,
        context_id: ContextId,
        observer: ObserverClass,
        amount: u64,
    ) -> Result<bool> {
        let current_budget = self.get_leakage_budget(context_id).await?;
        let consumed = current_budget.for_observer(observer);
        let limit = self.limits.for_observer(observer);
        let allowed = consumed.saturating_add(amount) <= limit;

        tracing::debug!(
            context_id = ?context_id,
            observer_class = ?observer,
            amount,
            limit,
            consumed,
            allowed,
            "Checking leakage budget via production handler"
        );

        Ok(allowed)
    }

    async fn get_leakage_history(
        &self,
        context_id: ContextId,
        since_timestamp: Option<u64>,
    ) -> Result<Vec<LeakageEvent>> {
        let mut events = self.load_events(context_id)?;
        if let Some(since) = since_timestamp {
            events.retain(|e| e.timestamp_ms >= since);
        }
        Ok(events)
    }
}

impl ProductionLeakageHandler {
    fn event_file(&self, context_id: ContextId) -> PathBuf {
        self._storage_path
            .join(context_id.to_string())
            .with_extension("jsonl")
    }

    fn load_events(&self, context_id: ContextId) -> Result<Vec<LeakageEvent>> {
        let path = self.event_file(context_id);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let data = fs::read_to_string(&path).map_err(|e| AuraError::storage(e.to_string()))?;
        let mut events = Vec::new();
        for line in data.lines() {
            let evt: LeakageEvent =
                serde_json::from_str(line).map_err(|e| AuraError::serialization(e.to_string()))?;
            events.push(evt);
        }
        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::AuthorityId;

    #[tokio::test]
    async fn test_production_handler_creation() {
        let handler = ProductionLeakageHandler::with_defaults();
        // ProductionLeakageHandler should be created successfully
        assert_eq!(handler.limits.external_consumed, 10000);
    }

    #[tokio::test]
    async fn test_leakage_operations() {
        let handler = ProductionLeakageHandler::with_defaults();
        let context = ContextId::new();

        // Test record leakage (currently a placeholder)
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

        // Test budget check against limits
        let allowed = handler
            .check_leakage_budget(context, ObserverClass::External, 5000)
            .await
            .unwrap();
        assert!(allowed);

        // Test exceeding limits
        let denied = handler
            .check_leakage_budget(context, ObserverClass::External, 15000)
            .await
            .unwrap();
        assert!(!denied);
    }
}
