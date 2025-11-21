//! FlowBudget primitives
//!
//! Canonical types for enforcing FlowBudget and receipt semantics as described
//! in the theoretical model (`docs/001_theoretical_foundations.md`) and info
//! flow specifications (`docs/103_info_flow_budget.md`).

use crate::{
    content::Hash32,
    identifiers::{AuthorityId, ContextId},
    semilattice::{Bottom, CvState, JoinSemilattice},
    session_epochs::Epoch,
};
use serde::{Deserialize, Serialize};

/// Effect API-backed flow budget for a `(context, peer)` pair.
///
/// `limit` behaves like a meet-semilattice (shrinks via min), while `spent`
/// behaves like a join-semilattice (grows via max). `epoch` gates replenishment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowBudget {
    /// Maximum observable cost permitted for the epoch.
    pub limit: u64,
    /// Cost already consumed for the epoch.
    pub spent: u64,
    /// Epoch binding the budget and receipts.
    pub epoch: Epoch,
}

impl FlowBudget {
    /// Create a new budget with zero spend.
    pub fn new(limit: u64, epoch: Epoch) -> Self {
        Self {
            limit,
            spent: 0,
            epoch,
        }
    }

    /// Remaining headroom before the guard should block.
    pub fn headroom(&self) -> u64 {
        self.limit.saturating_sub(self.spent)
    }

    /// Alias for headroom() - returns remaining budget
    pub fn remaining(&self) -> u64 {
        self.headroom()
    }

    /// Returns true if charging `cost` would still be within the budget.
    pub fn can_charge(&self, cost: u64) -> bool {
        self.spent.saturating_add(cost) <= self.limit
    }

    /// Record a spend if possible, returning whether the charge succeeded.
    pub fn record_charge(&mut self, cost: u64) -> bool {
        if self.can_charge(cost) {
            self.spent = self.spent.saturating_add(cost);
            true
        } else {
            false
        }
    }

    /// Merge two replicas of the same `(context, peer)` budget.
    ///
    /// - `limit` takes the meet (minimum)
    /// - `spent` takes the join (maximum)
    /// - `epoch` advances monotonically (maximum)
    pub fn merge(&self, other: &Self) -> Self {
        Self {
            limit: self.limit.min(other.limit),
            spent: self.spent.max(other.spent),
            epoch: if self.epoch.value() >= other.epoch.value() {
                self.epoch
            } else {
                other.epoch
            },
        }
    }

    /// Advance to a new epoch, resetting spent if the epoch increases.
    pub fn rotate_epoch(&mut self, next_epoch: Epoch) {
        if next_epoch.value() > self.epoch.value() {
            self.epoch = next_epoch;
            self.spent = 0;
        }
    }
}

impl Default for FlowBudget {
    fn default() -> Self {
        Self::new(0, Epoch::initial())
    }
}

// CRDT implementations for FlowBudget
// FlowBudget uses a hybrid semilattice approach:
// - limit uses meet-semilattice (min) for restrictive merging
// - spent uses join-semilattice (max) for accumulative merging
// - epoch advances monotonically (max)

impl JoinSemilattice for FlowBudget {
    fn join(&self, other: &Self) -> Self {
        Self {
            limit: self.limit.min(other.limit), // Meet for limit (more restrictive)
            spent: self.spent.max(other.spent), // Join for spent (more spent)
            epoch: if self.epoch.value() >= other.epoch.value() {
                self.epoch
            } else {
                other.epoch
            },
        }
    }
}

impl Bottom for FlowBudget {
    fn bottom() -> Self {
        Self::new(0, Epoch::initial())
    }
}

impl CvState for FlowBudget {}

/// FlowBudget key for journal storage: (context, peer) pair
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FlowBudgetKey {
    /// Context identifier for the operation
    pub context: ContextId,
    /// Peer authority identifier
    pub peer: AuthorityId,
}

impl FlowBudgetKey {
    /// Create a new FlowBudgetKey from context and peer
    pub fn new(context: ContextId, peer: AuthorityId) -> Self {
        Self { context, peer }
    }
}

/// Receipt emitted after a successful FlowBudget charge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    /// Context the observable event belongs to.
    pub ctx: ContextId,
    /// Sender authority that spent the budget.
    pub src: AuthorityId,
    /// Receiver authority that can verify the receipt.
    pub dst: AuthorityId,
    /// Epoch binding the receipt to a FlowBudget row.
    pub epoch: Epoch,
    /// Cost that was charged.
    pub cost: u32,
    /// Monotonic nonce per `(ctx, src, epoch)` used to prevent replay.
    pub nonce: u64,
    /// Previous receipt hash (forms a hash chain for auditing).
    pub prev: Hash32,
    /// Transport-level signature or MAC over the receipt fields.
    pub sig: Vec<u8>,
}

impl Receipt {
    /// Create a new receipt.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ctx: ContextId,
        src: AuthorityId,
        dst: AuthorityId,
        epoch: Epoch,
        cost: u32,
        nonce: u64,
        prev: Hash32,
        sig: Vec<u8>,
    ) -> Self {
        Self {
            ctx,
            src,
            dst,
            epoch,
            cost,
            nonce,
            prev,
            sig,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "proptest")]
    use proptest::prelude::*;

    #[cfg(feature = "proptest")]
    impl Arbitrary for FlowBudget {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            (
                100u64..1000u64, // limit
                0u64..100u64,    // spent
                1u64..100u64,    // epoch
            )
                .prop_map(|(limit, spent, epoch_val)| {
                    let epoch = Epoch::new(epoch_val);
                    FlowBudget {
                        limit,
                        spent: spent.min(limit), // Ensure spent <= limit for valid budgets
                        epoch,
                    }
                })
                .boxed()
        }
    }

    #[test]
    fn merge_prefers_more_restrictive_limit_and_higher_spent() {
        let epoch = Epoch::new(3);
        let other_epoch = Epoch::new(5);
        let a = FlowBudget {
            limit: 100,
            spent: 60,
            epoch,
        };
        let b = FlowBudget {
            limit: 80,
            spent: 50,
            epoch: other_epoch,
        };

        let merged = a.merge(&b);
        assert_eq!(merged.limit, 80);
        assert_eq!(merged.spent, 60);
        assert_eq!(merged.epoch, other_epoch);
    }

    #[test]
    fn record_charge_enforces_limit() {
        let mut budget = FlowBudget::new(10, Epoch::initial());
        assert!(budget.record_charge(4));
        assert_eq!(budget.spent, 4);
        assert!(budget.record_charge(6));
        assert_eq!(budget.spent, 10);
        assert!(!budget.record_charge(1));
        assert_eq!(budget.spent, 10);
    }

    #[test]
    fn rotate_epoch_resets_spent() {
        let mut budget = FlowBudget {
            limit: 100,
            spent: 90,
            epoch: Epoch::new(1),
        };
        budget.rotate_epoch(Epoch::new(2));
        assert_eq!(budget.spent, 0);
        assert_eq!(budget.epoch, Epoch::new(2));
    }
}
