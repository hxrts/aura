use super::{BudgetBreakdown, HomeFlowBudget, MAX_MEMBERS, MAX_NEIGHBORHOODS};

/// Format budget status as a human-readable string.
///
/// Returns a multi-line summary suitable for CLI output or any frontend.
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

    output.push_str("\nMember Storage:\n");
    output.push_str(&format!(
        "  {} members ({} max)\n",
        budget.member_count, MAX_MEMBERS
    ));
    output.push_str(&format!(
        "  {} / {} used\n",
        BudgetBreakdown::format_size(breakdown.member_used),
        BudgetBreakdown::format_size(breakdown.member_limit)
    ));

    output.push_str("\nNeighborhood Allocations:\n");
    output.push_str(&format!(
        "  {} neighborhoods ({} max)\n",
        budget.neighborhood_count, MAX_NEIGHBORHOODS
    ));
    output.push_str(&format!(
        "  {} contributed\n",
        BudgetBreakdown::format_size(breakdown.neighborhood_allocations)
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

    if usage_percent > 95 {
        output.push_str("\n⚠  CRITICAL: Storage is nearly full!\n");
    } else if usage_percent > 80 {
        output.push_str("\n⚠  WARNING: Storage is running low\n");
    }

    output
}

/// Format budget breakdown as a compact one-line summary.
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
