use super::{BudgetBreakdown, HomeFlowBudget, MAX_MEMBERS, MAX_NEIGHBORHOODS};

/// Typed validation failures for budget capacity checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BudgetValidationError {
    /// Member capacity is exhausted for the current home.
    MemberCapacityExceeded {
        /// Current member count.
        current: u8,
        /// Maximum member count.
        max: u8,
    },
    /// Neighborhood capacity is exhausted for the current home.
    NeighborhoodCapacityExceeded {
        /// Current joined-neighborhood count.
        current: u8,
        /// Maximum joined-neighborhood count.
        max: u8,
    },
    /// Pinned storage would exceed the remaining budget.
    PinCapacityExceeded {
        /// Requested content size in bytes.
        requested_bytes: u64,
        /// Remaining budget in bytes.
        available_bytes: u64,
    },
}

impl std::fmt::Display for BudgetValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MemberCapacityExceeded { current, max } => {
                write!(f, "Cannot add member: home at capacity ({current}/{max})")
            }
            Self::NeighborhoodCapacityExceeded { current, max } => {
                write!(
                    f,
                    "Cannot join neighborhood: home at capacity ({current}/{max})"
                )
            }
            Self::PinCapacityExceeded {
                requested_bytes,
                available_bytes,
            } => {
                write!(
                    f,
                    "Cannot pin content: need {}, have {} available",
                    BudgetBreakdown::format_size(*requested_bytes),
                    BudgetBreakdown::format_size(*available_bytes)
                )
            }
        }
    }
}

impl std::error::Error for BudgetValidationError {}

/// Check if budget can accommodate a new member, with error message.
pub fn check_can_add_member(budget: &HomeFlowBudget) -> Result<(), BudgetValidationError> {
    if budget.can_add_member() {
        Ok(())
    } else {
        Err(BudgetValidationError::MemberCapacityExceeded {
            current: budget.member_count,
            max: MAX_MEMBERS,
        })
    }
}

/// Check if budget can accommodate joining a neighborhood, with error message.
pub fn check_can_join_neighborhood(budget: &HomeFlowBudget) -> Result<(), BudgetValidationError> {
    if budget.can_join_neighborhood() {
        Ok(())
    } else {
        Err(BudgetValidationError::NeighborhoodCapacityExceeded {
            current: budget.neighborhood_count,
            max: MAX_NEIGHBORHOODS,
        })
    }
}

/// Check if budget can accommodate pinning content of given size, with error message.
pub fn check_can_pin(
    budget: &HomeFlowBudget,
    size_bytes: u64,
) -> Result<(), BudgetValidationError> {
    if budget.can_pin(size_bytes) {
        Ok(())
    } else {
        Err(BudgetValidationError::PinCapacityExceeded {
            requested_bytes: size_bytes,
            available_bytes: budget.pinned_storage_remaining(),
        })
    }
}
