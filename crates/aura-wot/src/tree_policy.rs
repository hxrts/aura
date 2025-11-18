//! Tree Policy System
//!
//! This module provides threshold policies for tree-based authorization.

use serde::{Deserialize, Serialize};

/// Policy for tree-based authorization
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Policy {
    /// Allow if any condition is met
    Any,
    /// Allow only if all conditions are met
    All,
    /// Threshold policy - allow if at least m out of n conditions are met
    Threshold { m: u16, n: u16 },
}

impl Policy {
    /// Create a new threshold policy
    ///
    /// # Arguments
    /// * `m` - Minimum number of approvals required (threshold)
    /// * `n` - Total number of participants
    ///
    /// For simple cases, use `Policy::All` or `Policy::Any` directly.
    pub fn new(m: u16, n: u16) -> Self {
        if m == 0 {
            Policy::Any
        } else if m == n {
            Policy::All
        } else {
            Policy::Threshold { m, n }
        }
    }

    /// Check if the policy is satisfied by the given number of approvals out of total
    pub fn is_satisfied(&self, approvals: u16, total: u16) -> bool {
        match self {
            Policy::Any => approvals > 0,
            Policy::All => approvals == total,
            Policy::Threshold { m, n: _ } => approvals >= *m,
        }
    }
}

impl Default for Policy {
    fn default() -> Self {
        Policy::Any
    }
}
