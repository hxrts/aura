//! # Flow Budget Domain Logic & Workflows
//!
//! Block storage budget tracking, enforcement, and portable business logic.
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
//!
//! ## Usage
//!
//! ```rust,ignore
//! use aura_app::workflows::budget::{BlockFlowBudget, BudgetError};
//!
//! let mut budget = BlockFlowBudget::new("block-123");
//!
//! // Check capacity
//! if budget.can_add_resident() {
//!     budget.add_resident()?;
//! }
//!
//! // Join neighborhoods
//! budget.join_neighborhood()?;
//!
//! // Pin content
//! let content_size = 500 * KB;
//! if budget.can_pin(content_size) {
//!     budget.pin_content(content_size)?;
//! }
//! ```

use crate::{AppCore, BUDGET_SIGNAL};
use async_lock::RwLock;
use aura_core::{effects::reactive::ReactiveEffects, AuraError};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;

// =============================================================================
// Constants
// =============================================================================

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

// =============================================================================
// Block Flow Budget
// =============================================================================

/// Block storage budget
///
/// Tracks current usage and calculates limits based on configuration.
/// This is the canonical budget type used across all frontends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
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

// =============================================================================
// Budget Breakdown
// =============================================================================

/// Budget breakdown for display
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
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
    /// Format a size for display (shared utility)
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

// =============================================================================
// Budget Error
// =============================================================================

/// Budget operation error
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Error))]
#[cfg_attr(feature = "uniffi", uniffi(flat_error))]
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

// =============================================================================
// Workflow Functions
// =============================================================================

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

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppConfig;

    // -------------------------------------------------------------------------
    // Domain type tests
    // -------------------------------------------------------------------------

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

        // Verify v1 arithmetic
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
    fn test_format_size() {
        assert_eq!(BudgetBreakdown::format_size(500), "500 B");
        assert_eq!(BudgetBreakdown::format_size(1024), "1.0 KB");
        assert_eq!(BudgetBreakdown::format_size(1536), "1.5 KB");
        assert_eq!(BudgetBreakdown::format_size(1048576), "1.0 MB");
        assert_eq!(BudgetBreakdown::format_size(10485760), "10.0 MB");
    }

    // -------------------------------------------------------------------------
    // Workflow tests
    // -------------------------------------------------------------------------

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
            core.register(&*BUDGET_SIGNAL, BlockFlowBudget::default())
                .await
                .unwrap();
        }

        // Update budget
        let mut new_budget = BlockFlowBudget::new("test-block");
        new_budget.add_resident().unwrap();
        new_budget.add_resident().unwrap();

        update_budget(&app_core, new_budget.clone()).await.unwrap();

        // Verify budget was updated
        let budget = get_current_budget(&app_core).await;
        assert_eq!(budget.resident_count, 2);
        assert_eq!(budget.resident_storage_spent, 2 * RESIDENT_ALLOCATION);
    }
}
