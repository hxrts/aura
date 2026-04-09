use std::sync::Arc;

use async_lock::RwLock;
use aura_core::AuraError;

use crate::signal_defs::{BUDGET_SIGNAL, BUDGET_SIGNAL_NAME};
use crate::workflows::signals::{emit_signal, read_signal_or_default};
use crate::AppCore;

use super::{BudgetBreakdown, HomeFlowBudget};

/// Get the current budget for the active home.
pub async fn get_current_budget(app_core: &Arc<RwLock<AppCore>>) -> HomeFlowBudget {
    let core = app_core.read().await;
    drop(core);
    read_signal_or_default(app_core, &*BUDGET_SIGNAL).await
}

/// Get a budget breakdown with computed allocation values.
pub async fn get_budget_breakdown(app_core: &Arc<RwLock<AppCore>>) -> BudgetBreakdown {
    let budget = get_current_budget(app_core).await;
    budget.breakdown()
}

/// Check if a new member can be added to the home.
pub async fn can_add_member(app_core: &Arc<RwLock<AppCore>>) -> bool {
    let budget = get_current_budget(app_core).await;
    budget.can_add_member()
}

/// Check if current home can join a neighborhood.
pub async fn can_join_neighborhood(app_core: &Arc<RwLock<AppCore>>) -> bool {
    let budget = get_current_budget(app_core).await;
    budget.can_join_neighborhood()
}

/// Check if content can be pinned to the home.
pub async fn can_pin_content(
    app_core: &Arc<RwLock<AppCore>>,
    content_size_bytes: u64,
) -> Result<u64, AuraError> {
    let budget = get_current_budget(app_core).await;
    let available = budget.pinned_storage_remaining();

    if content_size_bytes > available {
        Err(AuraError::budget_exceeded(format!(
            "Insufficient budget: need {content_size_bytes} bytes, have {available} available"
        )))
    } else {
        Ok(available)
    }
}

/// Update budget state and emit signal.
pub async fn update_budget(
    app_core: &Arc<RwLock<AppCore>>,
    budget: HomeFlowBudget,
) -> Result<(), AuraError> {
    emit_signal(app_core, &*BUDGET_SIGNAL, budget, BUDGET_SIGNAL_NAME).await
}
