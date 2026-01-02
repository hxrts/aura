//! # Budget Handlers - Terminal-Specific Formatting
//!
//! This module provides terminal-specific budget formatting for CLI and TUI.
//! Business logic has been moved to `aura_app::ui::workflows::budget`.
//!
//! ## Architecture
//!
//! - **Business Logic**: `aura_app::ui::workflows::budget` (portable)
//! - **Formatting**: This module (terminal-specific)
//!
//! ## Usage
//!
//! ### CLI Commands
//!
//! ```rust,ignore
//! use aura_app::ui::workflows::budget;
//! use crate::handlers::budget::format_budget_status;
//!
//! pub async fn show_budget_status(app_core: &Arc<RwLock<AppCore>>) {
//!     let budget = budget::get_current_budget(app_core).await;
//!     let formatted = format_budget_status(&budget);
//!     println!("{}", formatted);
//! }
//! ```
//!
//! ### TUI Screens
//!
//! ```rust,ignore
//! use aura_app::ui::workflows::budget;
//!
//! // Get budget data from workflow
//! let budget = budget::get_current_budget(&app_core).await;
//!
//! // TUI adds visual hints (colors, warnings)
//! let view = FlowBudgetView::from_budget(budget);
//! ```

use aura_app::ui::prelude::*;

// Re-export workflow functions for backward compatibility
// Business logic is now in aura_app::ui::workflows::budget
pub use aura_app::ui::workflows::budget::{
    can_add_resident, can_join_neighborhood, can_pin_content, get_budget_breakdown,
    get_current_budget, update_budget,
};

/// Format budget status as a human-readable string
///
/// Returns a multi-line summary suitable for CLI output or TUI display.
/// Includes total usage, per-category breakdowns, and capacity warnings.
#[must_use]
pub fn format_budget_status(budget: &HomeFlowBudget) -> String {
    let breakdown = budget.breakdown();
    let usage_percent = (budget.usage_fraction() * 100.0) as u8;

    let mut output = String::new();
    output.push_str(&format!("Home Storage Budget: {}\n", budget.home_id));
    output.push_str("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");
    output.push_str(&format!(
        "\nTotal: {} / {} ({}% used)\n",
        BudgetBreakdown::format_size(budget.total_used()),
        BudgetBreakdown::format_size(breakdown.total),
        usage_percent
    ));

    output.push_str("\nResident Storage:\n");
    output.push_str(&format!(
        "  {} residents ({} max)\n",
        budget.resident_count,
        aura_app::ui::types::MAX_RESIDENTS
    ));
    output.push_str(&format!(
        "  {} / {} used\n",
        BudgetBreakdown::format_size(breakdown.resident_used),
        BudgetBreakdown::format_size(breakdown.resident_limit)
    ));

    output.push_str("\nNeighborhood Donations:\n");
    output.push_str(&format!(
        "  {} neighborhoods ({} max)\n",
        budget.neighborhood_count,
        aura_app::ui::types::MAX_NEIGHBORHOODS
    ));
    output.push_str(&format!(
        "  {} donated\n",
        BudgetBreakdown::format_size(breakdown.neighborhood_donations)
    ));

    output.push_str("\nPinned Content:\n");
    output.push_str(&format!(
        "  {} / {} used\n",
        BudgetBreakdown::format_size(breakdown.pinned_used),
        BudgetBreakdown::format_size(breakdown.pinned_limit)
    ));

    output.push_str(&format!(
        "\nRemaining: {}\n",
        BudgetBreakdown::format_size(breakdown.remaining)
    ));

    // Add warnings if capacity is high
    if usage_percent > 95 {
        output.push_str("\n⚠️  CRITICAL: Storage is nearly full!\n");
    } else if usage_percent > 80 {
        output.push_str("\n⚠️  WARNING: Storage is running low\n");
    }

    output
}

/// Format budget breakdown as a compact one-line summary
///
/// Returns a short status line suitable for status bars or compact displays.
#[must_use]
pub fn format_budget_compact(budget: &HomeFlowBudget) -> String {
    let usage_percent = (budget.usage_fraction() * 100.0) as u8;
    format!(
        "Storage: {} / {} ({}%)",
        BudgetBreakdown::format_size(budget.total_used()),
        BudgetBreakdown::format_size(budget.total_allocation()),
        usage_percent
    )
}

/// Check if budget can accommodate a new resident
///
/// Returns Ok(()) if capacity is available, Err with message otherwise.
/// Use this before attempting to add residents.
pub fn check_can_add_resident(budget: &HomeFlowBudget) -> Result<(), String> {
    if budget.can_add_resident() {
        Ok(())
    } else {
        Err(format!(
            "Cannot add resident: home at capacity ({}/{})",
            budget.resident_count,
            aura_app::ui::types::MAX_RESIDENTS
        ))
    }
}

/// Check if budget can accommodate joining a neighborhood
///
/// Returns Ok(()) if capacity is available, Err with message otherwise.
/// Use this before attempting to join neighborhoods.
pub fn check_can_join_neighborhood(budget: &HomeFlowBudget) -> Result<(), String> {
    if budget.can_join_neighborhood() {
        Ok(())
    } else {
        Err(format!(
            "Cannot join neighborhood: home at capacity ({}/{})",
            budget.neighborhood_count,
            aura_app::ui::types::MAX_NEIGHBORHOODS
        ))
    }
}

/// Check if budget can accommodate pinning content of given size
///
/// Returns Ok(()) if space is available, Err with message otherwise.
/// Use this before attempting to pin content.
pub fn check_can_pin(budget: &HomeFlowBudget, size_bytes: u64) -> Result<(), String> {
    if budget.can_pin(size_bytes) {
        Ok(())
    } else {
        Err(format!(
            "Cannot pin content: need {}, have {} available",
            BudgetBreakdown::format_size(size_bytes),
            BudgetBreakdown::format_size(budget.pinned_storage_remaining())
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::hash;
    use aura_core::identifiers::HomeId;

    fn test_home_id(label: &str) -> HomeId {
        HomeId::from_bytes(hash::hash(label.as_bytes()))
    }

    #[test]
    fn test_format_budget_status() {
        let home_id = test_home_id("test-home");
        let mut budget = HomeFlowBudget::new(home_id);
        budget.add_resident().unwrap();
        budget.join_neighborhood().unwrap();

        let formatted = format_budget_status(&budget);

        // Check that key information is present
        assert!(formatted.contains(&home_id.to_string()));
        assert!(formatted.contains("1 residents"));
        assert!(formatted.contains("1 neighborhoods"));
        assert!(formatted.contains("Remaining"));
    }

    #[test]
    fn test_format_budget_compact() {
        let budget = HomeFlowBudget::new(test_home_id("test"));
        let compact = format_budget_compact(&budget);

        assert!(compact.contains("Storage:"));
        assert!(compact.contains("%"));
    }

    #[test]
    fn test_capacity_checks() {
        let mut budget = HomeFlowBudget::new(test_home_id("test"));

        // Should be able to add resident initially
        assert!(check_can_add_resident(&budget).is_ok());

        // Fill up residents
        for _ in 0..aura_app::ui::types::MAX_RESIDENTS {
            budget.add_resident().unwrap();
        }

        // Now should fail
        assert!(check_can_add_resident(&budget).is_err());
    }
}
