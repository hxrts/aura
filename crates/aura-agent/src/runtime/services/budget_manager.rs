//! Flow budget management service
//!
//! Provides isolated management of flow budgets with atomic charging
//! and consistent budget enforcement without lock contention.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use aura_core::ContextId;
use aura_core::session_epochs::Epoch;
use aura_core::{AuraError, AuraResult, DeviceId, FlowBudget};

/// Key for identifying a unique budget
#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct BudgetKey {
    pub context: ContextId,
    pub peer: DeviceId,
}

/// Manages flow budgets in isolation from effect execution
#[derive(Clone)]
pub struct FlowBudgetManager {
    /// Flow budgets indexed by context-peer pair
    budgets: Arc<RwLock<HashMap<BudgetKey, FlowBudget>>>,
}

impl FlowBudgetManager {
    /// Create a new flow budget manager
    pub fn new() -> Self {
        Self {
            budgets: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Check if a charge can be made without actually charging
    ///
    /// This method takes a brief read lock to check the budget state
    /// and returns immediately.
    pub async fn can_charge(
        &self,
        context: &ContextId,
        peer: &DeviceId,
        cost: u32,
    ) -> AuraResult<bool> {
        let key = BudgetKey {
            context: context.clone(),
            peer: *peer,
        };

        let budgets = self.budgets.read().await;
        if let Some(budget) = budgets.get(&key) {
            Ok(budget.headroom() >= cost as u64)
        } else {
            // No budget exists yet - would need initialization
            Ok(false)
        }
    }

    /// Atomically charge a budget if possible
    ///
    /// Returns the updated budget if successful, or an error if the charge
    /// would exceed the limit.
    pub async fn charge(
        &self,
        context: &ContextId,
        peer: &DeviceId,
        cost: u32,
    ) -> AuraResult<FlowBudget> {
        let key = BudgetKey {
            context: context.clone(),
            peer: *peer,
        };

        let mut budgets = self.budgets.write().await;
        let budget = budgets
            .get_mut(&key)
            .ok_or_else(|| AuraError::not_found("Flow budget not initialized"))?;

        if !budget.record_charge(cost as u64) {
            return Err(AuraError::permission_denied(format!(
                "Flow budget exceeded: limit={}, spent={}, cost={}",
                budget.limit, budget.spent, cost
            )));
        }

        Ok(*budget)
    }

    /// Initialize or update a budget
    pub async fn set_budget(
        &self,
        context: &ContextId,
        peer: &DeviceId,
        budget: FlowBudget,
    ) -> AuraResult<()> {
        let key = BudgetKey {
            context: context.clone(),
            peer: *peer,
        };

        let mut budgets = self.budgets.write().await;
        budgets.insert(key, budget);
        Ok(())
    }

    /// Get a snapshot of a budget
    pub async fn get_budget(
        &self,
        context: &ContextId,
        peer: &DeviceId,
    ) -> AuraResult<Option<FlowBudget>> {
        let key = BudgetKey {
            context: context.clone(),
            peer: *peer,
        };

        let budgets = self.budgets.read().await;
        Ok(budgets.get(&key).cloned())
    }

    /// Initialize a budget with default values if it doesn't exist
    pub async fn initialize_budget(
        &self,
        context: &ContextId,
        peer: &DeviceId,
        limit: u64,
        epoch: Epoch,
    ) -> AuraResult<FlowBudget> {
        let key = BudgetKey {
            context: context.clone(),
            peer: *peer,
        };

        let mut budgets = self.budgets.write().await;
        let budget = budgets
            .entry(key)
            .or_insert_with(|| FlowBudget::new(limit, epoch));
        Ok(*budget)
    }

    /// Reset all budgets for a new epoch
    pub async fn rotate_epoch(&self, new_epoch: Epoch) -> AuraResult<()> {
        let mut budgets = self.budgets.write().await;
        for budget in budgets.values_mut() {
            budget.rotate_epoch(new_epoch);
        }
        Ok(())
    }

    /// Remove a specific budget
    pub async fn remove_budget(
        &self,
        context: &ContextId,
        peer: &DeviceId,
    ) -> AuraResult<Option<FlowBudget>> {
        let key = BudgetKey {
            context: context.clone(),
            peer: *peer,
        };

        let mut budgets = self.budgets.write().await;
        Ok(budgets.remove(&key))
    }

    /// Clear all budgets (useful for testing)
    pub async fn clear(&self) {
        let mut budgets = self.budgets.write().await;
        budgets.clear();
    }

    /// Get the number of managed budgets
    pub async fn len(&self) -> usize {
        let budgets = self.budgets.read().await;
        budgets.len()
    }

    /// Check if the manager is empty
    pub async fn is_empty(&self) -> bool {
        let budgets = self.budgets.read().await;
        budgets.is_empty()
    }

    /// Get all budget keys
    pub async fn budget_keys(&self) -> Vec<BudgetKey> {
        let budgets = self.budgets.read().await;
        budgets.keys().cloned().collect()
    }

    /// Atomically charge if possible, initializing budget if needed
    pub async fn charge_or_init(
        &self,
        context: &ContextId,
        peer: &DeviceId,
        cost: u32,
        default_limit: u64,
        epoch: Epoch,
    ) -> AuraResult<FlowBudget> {
        let key = BudgetKey {
            context: context.clone(),
            peer: *peer,
        };

        let mut budgets = self.budgets.write().await;
        let budget = budgets
            .entry(key)
            .or_insert_with(|| FlowBudget::new(default_limit, epoch));

        if !budget.record_charge(cost as u64) {
            return Err(AuraError::permission_denied(format!(
                "Flow budget exceeded: limit={}, spent={}, cost={}",
                budget.limit, budget.spent, cost
            )));
        }

        Ok(*budget)
    }
}

impl Default for FlowBudgetManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_macros::aura_test;
    use aura_testkit::{ TestFixture};

    #[aura_test]
    async fn test_budget_charging() -> AuraResult<()> {
        let fixture = TestFixture::new().await?;
        let manager = FlowBudgetManager::new();
        let context = ContextId::from("test-context");
        let peer = fixture.device_id();
        let epoch = Epoch::from(1);

        // Initialize budget with limit of 1000
        manager
            .initialize_budget(&context, &peer, 1000, epoch)
            .await?;

        // Check we can charge
        assert!(manager.can_charge(&context, &peer, 100).await?);

        // Charge 100
        let budget = manager.charge(&context, &peer, 100).await?;
        assert_eq!(budget.spent, 100);
        assert_eq!(budget.limit, 1000);

        // Charge another 400
        let budget = manager.charge(&context, &peer, 400).await?;
        assert_eq!(budget.spent, 500);

        // Try to charge more than remaining
        let result = manager.charge(&context, &peer, 600).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Flow budget exceeded"));
        Ok(())
    }

    #[aura_test]
    async fn test_epoch_rotation() -> AuraResult<()> {
        let fixture = TestFixture::new().await?;
        let manager = FlowBudgetManager::new();
        let context = ContextId::from("test-context");
        let peer = fixture.device_id();
        let epoch1 = Epoch::from(1);
        let epoch2 = Epoch::from(2);

        // Initialize and charge budget
        manager
            .initialize_budget(&context, &peer, 1000, epoch1)
            .await?;
        manager.charge(&context, &peer, 800).await?;

        // Verify spent
        let budget = manager.get_budget(&context, &peer).await?.unwrap();
        assert_eq!(budget.spent, 800);

        // Rotate epoch
        manager.rotate_epoch(epoch2).await?;

        // Verify budget was reset
        let budget = manager.get_budget(&context, &peer).await?.unwrap();
        assert_eq!(budget.spent, 0);
        assert_eq!(budget.epoch, epoch2);
        Ok(())
    }

    #[aura_test]
    async fn test_charge_or_init() -> AuraResult<()> {
        let fixture = TestFixture::new().await?;
        let manager = FlowBudgetManager::new();
        let context = ContextId::from("test-context");
        let peer = fixture.device_id();
        let epoch = Epoch::from(1);

        // Charge without initialization
        let budget = manager
            .charge_or_init(&context, &peer, 100, 1000, epoch)
            .await?;

        assert_eq!(budget.spent, 100);
        assert_eq!(budget.limit, 1000);

        // Charge again - should use existing budget
        let budget = manager
            .charge_or_init(&context, &peer, 200, 2000, epoch)
            .await?;

        assert_eq!(budget.spent, 300);
        assert_eq!(budget.limit, 1000); // Original limit preserved
        Ok(())
    }

    #[aura_test]
    async fn test_concurrent_charging() -> AuraResult<()> {
        let fixture = TestFixture::new().await?;
        let manager = FlowBudgetManager::new();
        let context = ContextId::from("test-context");
        let peer = fixture.device_id();
        let epoch = Epoch::from(1);

        // Initialize budget
        manager
            .initialize_budget(&context, &peer, 1000, epoch)
            .await?;

        // Spawn concurrent charges
        let mut handles = vec![];
        for _ in 0..10 {
            let mgr = manager.clone();
            let ctx = context.clone();
            let handle = tokio::spawn(async move { mgr.charge(&ctx, &peer, 50).await });
            handles.push(handle);
        }

        // Collect results
        let mut successes = 0;
        for handle in handles {
            if handle.await.unwrap().is_ok() {
                successes += 1;
            }
        }

        // All charges should succeed (10 * 50 = 500 < 1000)
        assert_eq!(successes, 10);

        // Verify final state
        let budget = manager.get_budget(&context, &peer).await?.unwrap();
        assert_eq!(budget.spent, 500);
        Ok(())
    }
}
