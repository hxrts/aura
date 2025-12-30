//! # TUI Budget View Helpers
//!
//! TUI-specific budget display logic and utilities.
//!
//! **Core budget logic is in `aura-app`**. This module provides:
//! - `FlowBudgetView`: UI-specific hints (warning/critical status, display strings)
//! - `example_budget_table()`: Demo data for documentation/testing
//!
//! For shared CLI + TUI logic, see `crate::handlers::budget`.

use aura_app::{BudgetBreakdown, HomeFlowBudget, HOME_TOTAL_SIZE};

/// Flow budget view state for TUI rendering
///
/// Adds UI-specific hints on top of the domain-level HomeFlowBudget.
/// The core budget logic lives in aura-app; this adds display metadata.
#[derive(Debug, Clone)]
pub struct FlowBudgetView {
    /// Home budget
    pub budget: HomeFlowBudget,
    /// Whether budget is in warning state (>80% used)
    pub is_warning: bool,
    /// Whether budget is critical (>95% used)
    pub is_critical: bool,
    /// Human-readable status message
    pub status: String,
}

impl FlowBudgetView {
    /// Create a view from a budget
    ///
    /// Computes UI-specific metadata (warning/critical flags, status message).
    pub fn from_budget(budget: HomeFlowBudget) -> Self {
        let usage = budget.usage_fraction();
        let is_warning = usage > 0.8;
        let is_critical = usage > 0.95;

        let status = if is_critical {
            "Storage critical! Cannot pin new content.".to_string()
        } else if is_warning {
            format!(
                "Storage warning: {} remaining",
                BudgetBreakdown::format_size(budget.total_remaining())
            )
        } else {
            format!(
                "{} of {} used",
                BudgetBreakdown::format_size(budget.total_used()),
                BudgetBreakdown::format_size(HOME_TOTAL_SIZE)
            )
        };

        Self {
            budget,
            is_warning,
            is_critical,
            status,
        }
    }

    /// Usage percentage (0-100)
    pub fn usage_percent(&self) -> u8 {
        (self.budget.usage_fraction() * 100.0).min(100.0) as u8
    }
}

/// Create an example budget with neighborhoods for table (Section 8.1 of work/neighbor.md)
///
/// Returns a vec of (neighborhood_count, budget) pairs for documentation/testing.
pub fn example_budget_table() -> Vec<(u8, HomeFlowBudget)> {
    use aura_app::MAX_NEIGHBORHOODS;

    let mut table = Vec::new();

    for n in 1..=MAX_NEIGHBORHOODS {
        let mut budget = HomeFlowBudget::new(format!("example_{n}_neighborhoods"));
        for _ in 0..n {
            budget.join_neighborhood().unwrap();
        }
        table.push((n, budget));
    }

    table
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flow_budget_view() {
        let budget = HomeFlowBudget::new("test");

        // Normal state
        let view = FlowBudgetView::from_budget(budget.clone());
        assert!(!view.is_warning);
        assert!(!view.is_critical);
        assert!(view.usage_percent() == 0);
    }

    #[test]
    fn test_example_budget_table() {
        let table = example_budget_table();
        assert_eq!(table.len(), aura_app::MAX_NEIGHBORHOODS as usize);

        // Verify each row has correct neighborhood count
        for (neighborhoods, budget) in table {
            assert_eq!(budget.neighborhood_count, neighborhoods);
        }
    }
}
