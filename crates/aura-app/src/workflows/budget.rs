//! Budget Workflow - Portable Business Logic
//!
//! This module contains budget-related operations that are portable
//! across all frontends. It follows the reactive signal pattern and
//! returns domain types (not UI-specific types).

use crate::{AppCore, BlockFlowBudget, BudgetBreakdown, BUDGET_SIGNAL};
use aura_core::{effects::reactive::ReactiveEffects, AuraError};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Get the current budget for the active block
///
/// **What it does**: Reads budget state from BUDGET_SIGNAL
/// **Returns**: Domain type (BlockFlowBudget)
/// **Signal pattern**: Read-only operation (no emission)
///
/// This is the primary method for getting budget data across all frontends.
/// Falls back to default budget if signal is not available.
pub async fn get_current_budget(app_core: &Arc<RwLock<AppCore>>) -> BlockFlowBudget {
    let core = app_core.read().await;

    // Try to read from BUDGET_SIGNAL
    match core.read(&*BUDGET_SIGNAL).await {
        Ok(budget) => budget,
        Err(_) => {
            // Fall back to default budget if signal not available
            BlockFlowBudget::default()
        }
    }
}

/// Get a budget breakdown with computed allocation values
///
/// **What it does**: Computes allocation breakdown from current budget
/// **Returns**: Domain type (BudgetBreakdown)
/// **Signal pattern**: Read-only operation (no emission)
///
/// Use this for detailed budget inspection across all frontends.
pub async fn get_budget_breakdown(app_core: &Arc<RwLock<AppCore>>) -> BudgetBreakdown {
    let budget = get_current_budget(app_core).await;
    budget.breakdown()
}

/// Check if a new resident can be added to the block
///
/// **What it does**: Validates budget capacity for new resident
/// **Returns**: Boolean (true if capacity available)
/// **Signal pattern**: Read-only operation (no emission)
///
/// Frontends should call this before attempting to add a resident.
pub async fn can_add_resident(app_core: &Arc<RwLock<AppCore>>) -> bool {
    let budget = get_current_budget(app_core).await;
    budget.can_add_resident()
}

/// Check if current block can join a neighborhood
///
/// **What it does**: Validates budget capacity for neighborhood membership
/// **Returns**: Boolean (true if capacity available)
/// **Signal pattern**: Read-only operation (no emission)
///
/// Neighborhoods require additional budget allocation.
pub async fn can_join_neighborhood(app_core: &Arc<RwLock<AppCore>>) -> bool {
    let budget = get_current_budget(app_core).await;
    budget.can_join_neighborhood()
}

/// Check if content can be pinned to the block
///
/// **What it does**: Validates budget capacity for pinning content
/// **Returns**: Result with available capacity or error
/// **Signal pattern**: Read-only operation (no emission)
///
/// Returns the number of bytes available for pinning.
pub async fn can_pin_content(
    app_core: &Arc<RwLock<AppCore>>,
    content_size_bytes: u64,
) -> Result<u64, AuraError> {
    let budget = get_current_budget(app_core).await;
    let available = budget.pinned_storage_remaining();

    if content_size_bytes > available {
        Err(AuraError::budget_exceeded(format!(
            "Insufficient budget: need {} bytes, have {} available",
            content_size_bytes, available
        )))
    } else {
        Ok(available)
    }
}

/// Update budget state and emit signal
///
/// **What it does**: Updates BUDGET_SIGNAL with new budget state
/// **Returns**: Result indicating success/failure
/// **Signal pattern**: Write operation (emits BUDGET_SIGNAL)
///
/// This is called internally when budget state changes (e.g., after
/// adding a resident, pinning content, or receiving budget updates).
pub async fn update_budget(
    app_core: &Arc<RwLock<AppCore>>,
    budget: BlockFlowBudget,
) -> Result<(), AuraError> {
    let core = app_core.read().await;
    core.emit(&*BUDGET_SIGNAL, budget)
        .await
        .map_err(|e| AuraError::internal(format!("Failed to emit budget signal: {}", e)))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppConfig;
    use aura_core::identifiers::BlockId;

    #[tokio::test]
    async fn test_get_current_budget() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        // Should return default budget when signal not initialized
        let budget = get_current_budget(&app_core).await;
        assert_eq!(budget.resident_count, 0);
    }

    #[tokio::test]
    async fn test_budget_validation() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        // Should allow adding residents when budget is empty
        assert!(can_add_resident(&app_core).await);

        // Should allow joining neighborhoods when budget is empty
        assert!(can_join_neighborhood(&app_core).await);
    }

    #[tokio::test]
    async fn test_can_pin_content() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        // Should allow pinning small content
        let result = can_pin_content(&app_core, 1024).await;
        assert!(result.is_ok());

        // Should reject pinning content larger than available budget
        let result = can_pin_content(&app_core, 100_000_000).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_update_budget() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        // Register signal
        {
            let core = app_core.read().await;
            core.register_signal(&*BUDGET_SIGNAL, BlockFlowBudget::default())
                .await
                .unwrap();
        }

        // Update budget
        let new_budget = BlockFlowBudget {
            block_id: BlockId::from_bytes(&[1; 32]),
            resident_count: 2,
            content_bytes: 1024,
            metadata_bytes: 512,
        };

        update_budget(&app_core, new_budget.clone()).await.unwrap();

        // Verify budget was updated
        let budget = get_current_budget(&app_core).await;
        assert_eq!(budget.resident_count, 2);
        assert_eq!(budget.content_bytes, 1024);
    }
}
