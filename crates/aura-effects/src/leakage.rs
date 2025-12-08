//! Layer 3: Leakage Effect Handlers
//!
//! Stateless single-party implementation of LeakageEffects from aura-core (Layer 1).
//! This handler provides production leakage tracking by composing StorageEffects.
//!
//! **Layer Constraint**: NO mock handlers - those belong in aura-testkit (Layer 8).
//! This module contains only production-grade stateless handlers.
//!
//! **Architecture**: This handler composes `StorageEffects` for persistence rather than
//! using direct filesystem access. This enables deterministic simulation and WASM compatibility.

use async_trait::async_trait;
use aura_core::{
    effects::{LeakageBudget, LeakageEffects, LeakageEvent, ObserverClass, StorageEffects},
    identifiers::ContextId,
    AuraError, Result,
};
use serde_json::{from_str, to_string};
use std::sync::Arc;

/// Production leakage handler that composes StorageEffects
///
/// This handler tracks privacy leakage by delegating to injected storage effects.
/// It is stateless and does not maintain in-memory state.
///
/// **Architecture**: Composes `StorageEffects` for persistence, enabling:
/// - Deterministic simulation (inject mock storage)
/// - WASM compatibility (no direct filesystem access)
/// - Testability (inject in-memory storage for tests)
///
/// **Note**: Complex leakage aggregation and multi-context coordination has been
/// moved to `LeakageCoordinator` in aura-protocol (Layer 4). This handler provides
/// only stateless storage operations. For coordination capabilities, wrap this handler
/// with `aura_protocol::handlers::LeakageCoordinator`.
#[derive(Debug, Clone)]
pub struct ProductionLeakageHandler<S: StorageEffects> {
    /// Injected storage effects for persistence
    storage: Arc<S>,
    /// Budget limits per observer class
    limits: LeakageBudget,
}

impl<S: StorageEffects> ProductionLeakageHandler<S> {
    /// Create a new production leakage handler with injected storage
    pub fn new(storage: Arc<S>, limits: LeakageBudget) -> Self {
        Self { storage, limits }
    }

    /// Create with default limits and injected storage
    pub fn with_storage(storage: Arc<S>) -> Self {
        let limits = LeakageBudget {
            external_consumed: 10000,  // 10K flow units for external
            neighbor_consumed: 50000,  // 50K for neighbors
            in_group_consumed: 100000, // 100K for in-group
        };
        Self::new(storage, limits)
    }

    /// Get the storage key for a context's leakage events
    fn storage_key(context_id: ContextId) -> String {
        format!("leakage/{}", context_id)
    }

    /// Load events from storage for a context
    async fn load_events(&self, context_id: ContextId) -> Result<Vec<LeakageEvent>> {
        let key = Self::storage_key(context_id);

        match self.storage.retrieve(&key).await {
            Ok(Some(data)) => {
                let content =
                    String::from_utf8(data).map_err(|e| AuraError::serialization(e.to_string()))?;

                let mut events = Vec::new();
                for line in content.lines() {
                    if !line.is_empty() {
                        let evt: LeakageEvent =
                            from_str(line).map_err(|e| AuraError::serialization(e.to_string()))?;
                        events.push(evt);
                    }
                }
                Ok(events)
            }
            Ok(None) => Ok(Vec::new()),
            Err(e) => Err(AuraError::storage(e.to_string())),
        }
    }

    /// Append an event to storage (read-modify-write pattern)
    async fn append_event(&self, context_id: ContextId, event: &LeakageEvent) -> Result<()> {
        let key = Self::storage_key(context_id);

        // Read existing content
        let existing = match self.storage.retrieve(&key).await {
            Ok(Some(data)) => {
                String::from_utf8(data).map_err(|e| AuraError::serialization(e.to_string()))?
            }
            Ok(None) => String::new(),
            Err(e) => return Err(AuraError::storage(e.to_string())),
        };

        // Serialize new event
        let line = to_string(event).map_err(|e| AuraError::serialization(e.to_string()))?;

        // Append new line
        let updated = if existing.is_empty() {
            format!("{}\n", line)
        } else {
            format!("{}{}\n", existing, line)
        };

        // Write back
        self.storage
            .store(&key, updated.into_bytes())
            .await
            .map_err(|e| AuraError::storage(e.to_string()))?;

        Ok(())
    }
}

#[async_trait]
impl<S: StorageEffects> LeakageEffects for ProductionLeakageHandler<S> {
    async fn record_leakage(&self, event: LeakageEvent) -> Result<()> {
        self.append_event(event.context_id, &event).await
    }

    async fn get_leakage_budget(&self, context_id: ContextId) -> Result<LeakageBudget> {
        let events = self.load_events(context_id).await?;
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
        since_timestamp: Option<&aura_core::time::PhysicalTime>,
    ) -> Result<Vec<LeakageEvent>> {
        let mut events = self.load_events(context_id).await?;
        if let Some(since) = since_timestamp {
            events.retain(|e| e.timestamp.ts_ms >= since.ts_ms);
        }
        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::effects::StorageStats;
    use aura_core::AuthorityId;
    use std::collections::HashMap;
    use tokio::sync::RwLock;

    /// Simple in-memory storage for tests (mirrors aura-testkit's MemoryStorageHandler)
    #[derive(Debug, Default)]
    struct TestStorage {
        data: RwLock<HashMap<String, Vec<u8>>>,
    }

    impl TestStorage {
        fn new() -> Self {
            Self::default()
        }
    }

    #[async_trait]
    impl StorageEffects for TestStorage {
        async fn store(
            &self,
            key: &str,
            value: Vec<u8>,
        ) -> std::result::Result<(), aura_core::effects::StorageError> {
            self.data.write().await.insert(key.to_string(), value);
            Ok(())
        }

        async fn retrieve(
            &self,
            key: &str,
        ) -> std::result::Result<Option<Vec<u8>>, aura_core::effects::StorageError> {
            Ok(self.data.read().await.get(key).cloned())
        }

        async fn remove(
            &self,
            key: &str,
        ) -> std::result::Result<bool, aura_core::effects::StorageError> {
            Ok(self.data.write().await.remove(key).is_some())
        }

        async fn list_keys(
            &self,
            prefix: Option<&str>,
        ) -> std::result::Result<Vec<String>, aura_core::effects::StorageError> {
            let data = self.data.read().await;
            let keys: Vec<String> = match prefix {
                Some(p) => data.keys().filter(|k| k.starts_with(p)).cloned().collect(),
                None => data.keys().cloned().collect(),
            };
            Ok(keys)
        }

        async fn exists(
            &self,
            key: &str,
        ) -> std::result::Result<bool, aura_core::effects::StorageError> {
            Ok(self.data.read().await.contains_key(key))
        }

        async fn store_batch(
            &self,
            pairs: HashMap<String, Vec<u8>>,
        ) -> std::result::Result<(), aura_core::effects::StorageError> {
            let mut data = self.data.write().await;
            for (k, v) in pairs {
                data.insert(k, v);
            }
            Ok(())
        }

        async fn retrieve_batch(
            &self,
            keys: &[String],
        ) -> std::result::Result<HashMap<String, Vec<u8>>, aura_core::effects::StorageError>
        {
            let data = self.data.read().await;
            let mut result = HashMap::new();
            for key in keys {
                if let Some(value) = data.get(key) {
                    result.insert(key.clone(), value.clone());
                }
            }
            Ok(result)
        }

        async fn clear_all(&self) -> std::result::Result<(), aura_core::effects::StorageError> {
            self.data.write().await.clear();
            Ok(())
        }

        async fn stats(
            &self,
        ) -> std::result::Result<StorageStats, aura_core::effects::StorageError> {
            let data = self.data.read().await;
            Ok(StorageStats {
                key_count: data.len() as u64,
                total_size: data.values().map(|v| v.len() as u64).sum(),
                available_space: None,
                backend_type: "test".to_string(),
            })
        }
    }

    #[tokio::test]
    async fn test_production_handler_creation() {
        let storage = Arc::new(TestStorage::new());
        let handler = ProductionLeakageHandler::with_storage(storage);
        // ProductionLeakageHandler should be created successfully
        assert_eq!(handler.limits.external_consumed, 10000);
    }

    #[tokio::test]
    async fn test_leakage_record_and_retrieve() {
        let storage = Arc::new(TestStorage::new());
        let handler = ProductionLeakageHandler::with_storage(storage);
        let context = ContextId::new_from_entropy([1u8; 32]);

        // Record a leakage event
        let event = LeakageEvent::with_timestamp_ms(
            AuthorityId::new_from_entropy([2u8; 32]),
            AuthorityId::new_from_entropy([3u8; 32]),
            context,
            100,
            ObserverClass::External,
            "test".to_string(),
            1000,
        );

        handler.record_leakage(event.clone()).await.unwrap();

        // Retrieve history
        let history = handler.get_leakage_history(context, None).await.unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].leakage_amount, 100);
        assert_eq!(history[0].operation, "test");
    }

    #[tokio::test]
    async fn test_leakage_budget_accumulation() {
        let storage = Arc::new(TestStorage::new());
        let handler = ProductionLeakageHandler::with_storage(storage);
        let context = ContextId::new_from_entropy([1u8; 32]);

        // Record multiple events
        for i in 0..5 {
            let event = LeakageEvent::with_timestamp_ms(
                AuthorityId::new_from_entropy([2u8; 32]),
                AuthorityId::new_from_entropy([3u8; 32]),
                context,
                100,
                ObserverClass::External,
                format!("test_{}", i),
                i as u64 * 1000,
            );
            handler.record_leakage(event).await.unwrap();
        }

        // Check accumulated budget
        let budget = handler.get_leakage_budget(context).await.unwrap();
        assert_eq!(budget.external_consumed, 500); // 5 * 100
    }

    #[tokio::test]
    async fn test_leakage_budget_check() {
        let storage = Arc::new(TestStorage::new());
        let handler = ProductionLeakageHandler::with_storage(storage);
        let context = ContextId::new_from_entropy([1u8; 32]);

        // Record initial leakage
        let event = LeakageEvent::with_timestamp_ms(
            AuthorityId::new_from_entropy([2u8; 32]),
            AuthorityId::new_from_entropy([3u8; 32]),
            context,
            100,
            ObserverClass::External,
            "test".to_string(),
            0,
        );
        handler.record_leakage(event).await.unwrap();

        // Check budget - should allow 5000 more (limit is 10000, consumed is 100)
        let allowed = handler
            .check_leakage_budget(context, ObserverClass::External, 5000)
            .await
            .unwrap();
        assert!(allowed);

        // Check exceeding limits - should deny 15000 (would exceed 10000 limit)
        let denied = handler
            .check_leakage_budget(context, ObserverClass::External, 15000)
            .await
            .unwrap();
        assert!(!denied);
    }

    #[tokio::test]
    async fn test_leakage_history_filtering() {
        use aura_core::time::PhysicalTime;

        let storage = Arc::new(TestStorage::new());
        let handler = ProductionLeakageHandler::with_storage(storage);
        let context = ContextId::new_from_entropy([1u8; 32]);

        // Record events at different timestamps
        for ts in [1000u64, 2000, 3000, 4000, 5000] {
            let event = LeakageEvent::with_timestamp_ms(
                AuthorityId::new_from_entropy([2u8; 32]),
                AuthorityId::new_from_entropy([3u8; 32]),
                context,
                100,
                ObserverClass::External,
                format!("test_{}", ts),
                ts,
            );
            handler.record_leakage(event).await.unwrap();
        }

        // Get all history
        let all_history = handler.get_leakage_history(context, None).await.unwrap();
        assert_eq!(all_history.len(), 5);

        // Get history since timestamp 3000
        let since = PhysicalTime {
            ts_ms: 3000,
            uncertainty: None,
        };
        let filtered = handler
            .get_leakage_history(context, Some(&since))
            .await
            .unwrap();
        assert_eq!(filtered.len(), 3); // 3000, 4000, 5000
    }

    #[tokio::test]
    async fn test_empty_context_returns_empty() {
        let storage = Arc::new(TestStorage::new());
        let handler = ProductionLeakageHandler::with_storage(storage);
        let context = ContextId::new_from_entropy([1u8; 32]);

        // No events recorded
        let history = handler.get_leakage_history(context, None).await.unwrap();
        assert!(history.is_empty());

        let budget = handler.get_leakage_budget(context).await.unwrap();
        assert_eq!(budget.external_consumed, 0);
        assert_eq!(budget.neighbor_consumed, 0);
        assert_eq!(budget.in_group_consumed, 0);
    }
}
