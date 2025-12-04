//! # Flow Budget Integration
//!
//! Block storage budget tracking and enforcement for TUI.
//! See `work/neighbor.md` Section 8 for the storage model.
//!
//! ## Storage Model (v1)
//!
//! - **Block total**: 10 MB fixed allocation
//! - **Resident storage**: 200 KB per resident (max 8 = 1.6 MB)
//! - **Neighborhood donation**: 1 MB per neighborhood (max 4 = 4 MB)
//! - **Public-good space**: Remainder after residents + donations
//!
//! ## Key Principles
//!
//! - `spent` counters are persisted as journal facts
//! - `limits` are derived at runtime from policy + Biscuit capabilities
//! - Charge-before-send invariant enforced by FlowGuard + JournalCoupler

use std::fmt;

/// Storage amounts in bytes
pub const BYTE: u64 = 1;
/// Kilobyte (1024 bytes)
pub const KB: u64 = 1024;
/// Megabyte (1024 KB)
pub const MB: u64 = 1024 * KB;

/// v1 constraints

/// Total block storage allocation (10 MB)
pub const BLOCK_TOTAL_SIZE: u64 = 10 * MB;
/// Storage allocated per resident (200 KB)
pub const RESIDENT_ALLOCATION: u64 = 200 * KB;
/// Maximum number of residents per block
pub const MAX_RESIDENTS: u8 = 8;
/// Storage donated per neighborhood membership (1 MB)
pub const NEIGHBORHOOD_DONATION: u64 = 1 * MB;
/// Maximum number of neighborhoods a block can join
pub const MAX_NEIGHBORHOODS: u8 = 4;

/// Block storage budget
///
/// Tracks current usage and calculates limits based on configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockFlowBudget {
    /// Block ID
    pub block_id: String,
    /// Current number of residents
    pub resident_count: u8,
    /// Storage used by residents (spent counter as fact)
    pub resident_storage_spent: u64,
    /// Number of neighborhoods joined
    pub neighborhood_count: u8,
    /// Total neighborhood donations
    pub neighborhood_donations: u64,
    /// Storage used by pinned content (spent counter as fact)
    pub pinned_storage_spent: u64,
}

impl BlockFlowBudget {
    /// Create a new empty budget for a block
    pub fn new(block_id: impl Into<String>) -> Self {
        Self {
            block_id: block_id.into(),
            resident_count: 0,
            resident_storage_spent: 0,
            neighborhood_count: 0,
            neighborhood_donations: 0,
            pinned_storage_spent: 0,
        }
    }

    /// Total block allocation (fixed at 10 MB in v1)
    pub fn total_allocation(&self) -> u64 {
        BLOCK_TOTAL_SIZE
    }

    /// Maximum resident storage (8 × 200 KB = 1.6 MB)
    pub fn resident_storage_limit(&self) -> u64 {
        MAX_RESIDENTS as u64 * RESIDENT_ALLOCATION
    }

    /// Current resident storage used
    pub fn resident_storage_used(&self) -> u64 {
        self.resident_storage_spent
    }

    /// Remaining resident storage capacity
    pub fn resident_storage_remaining(&self) -> u64 {
        self.resident_storage_limit()
            .saturating_sub(self.resident_storage_spent)
    }

    /// Calculate public-good space limit based on current configuration
    ///
    /// Formula: 10 MB - neighborhood_donations - resident_limit
    pub fn pinned_storage_limit(&self) -> u64 {
        BLOCK_TOTAL_SIZE
            .saturating_sub(self.neighborhood_donations)
            .saturating_sub(self.resident_storage_limit())
    }

    /// Remaining pinned storage capacity
    pub fn pinned_storage_remaining(&self) -> u64 {
        self.pinned_storage_limit()
            .saturating_sub(self.pinned_storage_spent)
    }

    /// Total storage used
    pub fn total_used(&self) -> u64 {
        self.resident_storage_spent + self.neighborhood_donations + self.pinned_storage_spent
    }

    /// Total storage remaining
    pub fn total_remaining(&self) -> u64 {
        BLOCK_TOTAL_SIZE.saturating_sub(self.total_used())
    }

    /// Usage percentage (0.0 - 1.0)
    pub fn usage_fraction(&self) -> f64 {
        self.total_used() as f64 / BLOCK_TOTAL_SIZE as f64
    }

    /// Check if block can add another resident
    pub fn can_add_resident(&self) -> bool {
        self.resident_count < MAX_RESIDENTS
            && self.resident_storage_remaining() >= RESIDENT_ALLOCATION
    }

    /// Check if block can join another neighborhood
    pub fn can_join_neighborhood(&self) -> bool {
        self.neighborhood_count < MAX_NEIGHBORHOODS
            && self.pinned_storage_remaining() >= NEIGHBORHOOD_DONATION
    }

    /// Check if block can pin content of given size
    pub fn can_pin(&self, size: u64) -> bool {
        self.pinned_storage_remaining() >= size
    }

    /// Add a resident (charge storage)
    ///
    /// Returns error if capacity exceeded.
    pub fn add_resident(&mut self) -> Result<(), BudgetError> {
        if !self.can_add_resident() {
            return Err(BudgetError::ResidentCapacityExceeded {
                current: self.resident_count,
                max: MAX_RESIDENTS,
            });
        }
        self.resident_count += 1;
        self.resident_storage_spent += RESIDENT_ALLOCATION;
        Ok(())
    }

    /// Remove a resident (free storage)
    pub fn remove_resident(&mut self) -> Result<(), BudgetError> {
        if self.resident_count == 0 {
            return Err(BudgetError::NoResidentsToRemove);
        }
        self.resident_count -= 1;
        self.resident_storage_spent = self
            .resident_storage_spent
            .saturating_sub(RESIDENT_ALLOCATION);
        Ok(())
    }

    /// Join a neighborhood (donate storage)
    ///
    /// Returns error if capacity exceeded.
    pub fn join_neighborhood(&mut self) -> Result<(), BudgetError> {
        if !self.can_join_neighborhood() {
            return Err(BudgetError::NeighborhoodCapacityExceeded {
                current: self.neighborhood_count,
                max: MAX_NEIGHBORHOODS,
            });
        }
        self.neighborhood_count += 1;
        self.neighborhood_donations += NEIGHBORHOOD_DONATION;
        Ok(())
    }

    /// Leave a neighborhood (reclaim donated storage)
    pub fn leave_neighborhood(&mut self) -> Result<(), BudgetError> {
        if self.neighborhood_count == 0 {
            return Err(BudgetError::NoNeighborhoodsToLeave);
        }
        self.neighborhood_count -= 1;
        self.neighborhood_donations = self
            .neighborhood_donations
            .saturating_sub(NEIGHBORHOOD_DONATION);
        Ok(())
    }

    /// Pin content (charge storage)
    ///
    /// Returns error if capacity exceeded.
    pub fn pin_content(&mut self, size: u64) -> Result<(), BudgetError> {
        if !self.can_pin(size) {
            return Err(BudgetError::PinnedStorageExceeded {
                requested: size,
                available: self.pinned_storage_remaining(),
            });
        }
        self.pinned_storage_spent += size;
        Ok(())
    }

    /// Unpin content (free storage)
    pub fn unpin_content(&mut self, size: u64) {
        self.pinned_storage_spent = self.pinned_storage_spent.saturating_sub(size);
    }

    /// Get a breakdown summary for display
    pub fn breakdown(&self) -> BudgetBreakdown {
        BudgetBreakdown {
            total: BLOCK_TOTAL_SIZE,
            resident_limit: self.resident_storage_limit(),
            resident_used: self.resident_storage_spent,
            neighborhood_donations: self.neighborhood_donations,
            pinned_limit: self.pinned_storage_limit(),
            pinned_used: self.pinned_storage_spent,
            remaining: self.total_remaining(),
        }
    }
}

impl Default for BlockFlowBudget {
    fn default() -> Self {
        Self::new("default")
    }
}

/// Budget breakdown for display
#[derive(Debug, Clone)]
pub struct BudgetBreakdown {
    /// Total block allocation
    pub total: u64,
    /// Resident storage limit
    pub resident_limit: u64,
    /// Resident storage used
    pub resident_used: u64,
    /// Storage donated to neighborhoods
    pub neighborhood_donations: u64,
    /// Pinned storage limit
    pub pinned_limit: u64,
    /// Pinned storage used
    pub pinned_used: u64,
    /// Remaining storage
    pub remaining: u64,
}

impl BudgetBreakdown {
    /// Format a size for display
    pub fn format_size(bytes: u64) -> String {
        if bytes >= MB {
            format!("{:.1} MB", bytes as f64 / MB as f64)
        } else if bytes >= KB {
            format!("{:.1} KB", bytes as f64 / KB as f64)
        } else {
            format!("{} B", bytes)
        }
    }
}

/// Budget operation error
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BudgetError {
    /// Cannot add more residents
    ResidentCapacityExceeded {
        /// Current resident count
        current: u8,

        /// Maximum residents
        max: u8,
    },
    /// No residents to remove
    NoResidentsToRemove,
    /// Cannot join more neighborhoods
    NeighborhoodCapacityExceeded {
        /// Current neighborhood count
        current: u8,

        /// Maximum neighborhoods
        max: u8,
    },
    /// No neighborhoods to leave
    NoNeighborhoodsToLeave,
    /// Cannot pin content (insufficient space)
    PinnedStorageExceeded {
        /// Requested bytes
        requested: u64,

        /// Available bytes
        available: u64,
    },
}

impl fmt::Display for BudgetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ResidentCapacityExceeded { current, max } => {
                write!(f, "Block at resident capacity ({}/{})", current, max)
            }
            Self::NoResidentsToRemove => write!(f, "No residents to remove"),
            Self::NeighborhoodCapacityExceeded { current, max } => {
                write!(f, "Block at neighborhood capacity ({}/{})", current, max)
            }
            Self::NoNeighborhoodsToLeave => write!(f, "No neighborhoods to leave"),
            Self::PinnedStorageExceeded {
                requested,
                available,
            } => {
                write!(
                    f,
                    "Insufficient pinned storage: need {}, have {}",
                    BudgetBreakdown::format_size(*requested),
                    BudgetBreakdown::format_size(*available)
                )
            }
        }
    }
}

impl std::error::Error for BudgetError {}

/// Flow budget view state for TUI rendering
#[derive(Debug, Clone)]
pub struct FlowBudgetView {
    /// Block budget
    pub budget: BlockFlowBudget,
    /// Whether budget is in warning state (>80% used)
    pub is_warning: bool,
    /// Whether budget is critical (>95% used)
    pub is_critical: bool,
    /// Human-readable status message
    pub status: String,
}

impl FlowBudgetView {
    /// Create a view from a budget
    pub fn from_budget(budget: BlockFlowBudget) -> Self {
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
                BudgetBreakdown::format_size(BLOCK_TOTAL_SIZE)
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

/// Create an example budget with neighborhoods for table (Section 8.1)
pub fn example_budget_table() -> Vec<(u8, BlockFlowBudget)> {
    vec![
        (1, {
            let mut b = BlockFlowBudget::new("example_1_neighborhood");
            b.join_neighborhood().unwrap();
            b
        }),
        (2, {
            let mut b = BlockFlowBudget::new("example_2_neighborhoods");
            b.join_neighborhood().unwrap();
            b.join_neighborhood().unwrap();
            b
        }),
        (3, {
            let mut b = BlockFlowBudget::new("example_3_neighborhoods");
            b.join_neighborhood().unwrap();
            b.join_neighborhood().unwrap();
            b.join_neighborhood().unwrap();
            b
        }),
        (4, {
            let mut b = BlockFlowBudget::new("example_4_neighborhoods");
            b.join_neighborhood().unwrap();
            b.join_neighborhood().unwrap();
            b.join_neighborhood().unwrap();
            b.join_neighborhood().unwrap();
            b
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_budget() {
        let budget = BlockFlowBudget::new("test_block");
        assert_eq!(budget.block_id, "test_block");
        assert_eq!(budget.resident_count, 0);
        assert_eq!(budget.neighborhood_count, 0);
        assert_eq!(budget.total_used(), 0);
    }

    #[test]
    fn test_constants() {
        assert_eq!(BLOCK_TOTAL_SIZE, 10 * MB);
        assert_eq!(RESIDENT_ALLOCATION, 200 * KB);
        assert_eq!(MAX_RESIDENTS, 8);
        assert_eq!(NEIGHBORHOOD_DONATION, MB);
        assert_eq!(MAX_NEIGHBORHOODS, 4);

        // Verify v1 arithmetic from neighbor.md Section 8.3
        let max_resident_storage = MAX_RESIDENTS as u64 * RESIDENT_ALLOCATION;
        assert_eq!(max_resident_storage, 1_638_400); // 1.6 MB
    }

    #[test]
    fn test_add_resident() {
        let mut budget = BlockFlowBudget::new("test");
        assert!(budget.can_add_resident());

        budget.add_resident().unwrap();
        assert_eq!(budget.resident_count, 1);
        assert_eq!(budget.resident_storage_spent, RESIDENT_ALLOCATION);

        // Add max residents
        for _ in 1..MAX_RESIDENTS {
            budget.add_resident().unwrap();
        }
        assert_eq!(budget.resident_count, MAX_RESIDENTS);
        assert!(!budget.can_add_resident());

        // Try to add one more
        let result = budget.add_resident();
        assert!(matches!(
            result,
            Err(BudgetError::ResidentCapacityExceeded { .. })
        ));
    }

    #[test]
    fn test_remove_resident() {
        let mut budget = BlockFlowBudget::new("test");
        budget.add_resident().unwrap();
        budget.add_resident().unwrap();

        budget.remove_resident().unwrap();
        assert_eq!(budget.resident_count, 1);

        budget.remove_resident().unwrap();
        assert_eq!(budget.resident_count, 0);

        let result = budget.remove_resident();
        assert!(matches!(result, Err(BudgetError::NoResidentsToRemove)));
    }

    #[test]
    fn test_join_neighborhood() {
        let mut budget = BlockFlowBudget::new("test");
        assert!(budget.can_join_neighborhood());

        budget.join_neighborhood().unwrap();
        assert_eq!(budget.neighborhood_count, 1);
        assert_eq!(budget.neighborhood_donations, NEIGHBORHOOD_DONATION);

        // Join max neighborhoods
        for _ in 1..MAX_NEIGHBORHOODS {
            budget.join_neighborhood().unwrap();
        }
        assert_eq!(budget.neighborhood_count, MAX_NEIGHBORHOODS);
        assert!(!budget.can_join_neighborhood());

        let result = budget.join_neighborhood();
        assert!(matches!(
            result,
            Err(BudgetError::NeighborhoodCapacityExceeded { .. })
        ));
    }

    #[test]
    fn test_pinned_storage() {
        let mut budget = BlockFlowBudget::new("test");

        // With no neighborhoods, pinned limit = 10 MB - 1.6 MB = 8.4 MB
        let initial_limit = budget.pinned_storage_limit();
        assert_eq!(
            initial_limit,
            BLOCK_TOTAL_SIZE - budget.resident_storage_limit()
        );

        // Join a neighborhood reduces pinned limit by 1 MB
        budget.join_neighborhood().unwrap();
        assert_eq!(
            budget.pinned_storage_limit(),
            initial_limit - NEIGHBORHOOD_DONATION
        );

        // Pin some content
        let content_size = 500 * KB;
        assert!(budget.can_pin(content_size));
        budget.pin_content(content_size).unwrap();
        assert_eq!(budget.pinned_storage_spent, content_size);

        // Unpin
        budget.unpin_content(content_size);
        assert_eq!(budget.pinned_storage_spent, 0);
    }

    #[test]
    fn test_storage_arithmetic_v1() {
        // Verify Section 8.3 table: 4 neighborhoods case
        let mut budget = BlockFlowBudget::new("test");

        // Join 4 neighborhoods
        for _ in 0..4 {
            budget.join_neighborhood().unwrap();
        }

        // Verify arithmetic
        assert_eq!(budget.neighborhood_donations, 4 * MB); // 4 MB
        assert_eq!(budget.resident_storage_limit(), 1_638_400); // 1.6 MB (8 × 200 KB)

        // Public-good space = 10 MB - 4 MB - 1.6 MB = 4.4 MB
        let expected_pinned_limit = BLOCK_TOTAL_SIZE - (4 * MB) - 1_638_400;
        assert_eq!(budget.pinned_storage_limit(), expected_pinned_limit);

        // 4.4 MB = 4,613,734.4 bytes ≈ 4,613,734 (due to integer arithmetic)
        // Actually: 10,485,760 - 4,194,304 - 1,638,400 = 4,653,056
        assert!(budget.pinned_storage_limit() >= 4 * MB);
    }

    #[test]
    fn test_usage_fraction() {
        let mut budget = BlockFlowBudget::new("test");
        assert_eq!(budget.usage_fraction(), 0.0);

        budget.add_resident().unwrap();
        assert!(budget.usage_fraction() > 0.0);

        budget.join_neighborhood().unwrap();
        assert!(budget.usage_fraction() > 0.1);
    }

    #[test]
    fn test_flow_budget_view() {
        let mut budget = BlockFlowBudget::new("test");

        // Normal state
        let view = FlowBudgetView::from_budget(budget.clone());
        assert!(!view.is_warning);
        assert!(!view.is_critical);

        // Fill up most of the budget
        for _ in 0..MAX_RESIDENTS {
            budget.add_resident().unwrap();
        }
        for _ in 0..MAX_NEIGHBORHOODS {
            budget.join_neighborhood().unwrap();
        }
        // Pin most of remaining space
        let pinnable = budget.pinned_storage_remaining();
        budget.pin_content(pinnable - 100 * KB).unwrap();

        let view = FlowBudgetView::from_budget(budget);
        assert!(view.is_warning || view.is_critical);
    }

    #[test]
    fn test_format_size() {
        assert_eq!(BudgetBreakdown::format_size(500), "500 B");
        assert_eq!(BudgetBreakdown::format_size(1024), "1.0 KB");
        assert_eq!(BudgetBreakdown::format_size(1536), "1.5 KB");
        assert_eq!(BudgetBreakdown::format_size(1048576), "1.0 MB");
        assert_eq!(BudgetBreakdown::format_size(10485760), "10.0 MB");
    }

    #[test]
    fn test_example_budget_table() {
        let table = example_budget_table();
        assert_eq!(table.len(), 4);

        // Verify each row matches Section 8.1 table
        for (neighborhoods, budget) in table {
            assert_eq!(budget.neighborhood_count, neighborhoods);
            assert_eq!(budget.neighborhood_donations, neighborhoods as u64 * MB);

            // Remaining public-good space should decrease with more neighborhoods
            let expected_pinned =
                BLOCK_TOTAL_SIZE - (neighborhoods as u64 * MB) - budget.resident_storage_limit();
            assert_eq!(budget.pinned_storage_limit(), expected_pinned);
        }
    }
}
