//! FlowBudget primitives
//!
//! Canonical types for enforcing FlowBudget and receipt semantics as described
//! in the theoretical model (`docs/001_theoretical_foundations.md`) and info
//! flow specifications (`docs/103_info_flow_budget.md`).

/// Maximum size for receipt signatures in bytes.
/// Supports Ed25519 (64 bytes) with room for future signature schemes.
pub const MAX_SIGNATURE_BYTES: usize = 256;

use crate::{
    domain::content::Hash32,
    semilattice::{Bottom, CvState, JoinSemilattice},
    types::epochs::Epoch,
    types::identifiers::{AuthorityId, ContextId},
    AuraError,
};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Errors that can occur during budget operations.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum BudgetError {
    /// Budget exhausted - cannot charge the requested cost
    #[error("Budget exhausted: requested {cost}, remaining {remaining}")]
    Exhausted {
        /// The cost that was requested
        cost: u64,
        /// The remaining budget
        remaining: u64,
    },
    /// Budget arithmetic overflow
    #[error("Budget arithmetic overflow: spent {spent} + cost {cost}")]
    Overflow {
        /// The current spent value
        spent: u64,
        /// The cost that was requested
        cost: u64,
    },
}

/// Cost in flow budget units (bounded to u32 for transport compatibility).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FlowCost(u32);

impl FlowCost {
    /// Create a new flow cost.
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// Return the raw cost value.
    #[must_use]
    pub fn value(self) -> u32 {
        self.0
    }

    /// Return the cost as u64 for budget arithmetic.
    #[must_use]
    pub fn as_u64(self) -> u64 {
        u64::from(self.0)
    }
}

impl fmt::Display for FlowCost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u32> for FlowCost {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

impl From<FlowCost> for u64 {
    fn from(cost: FlowCost) -> Self {
        cost.as_u64()
    }
}

impl TryFrom<u64> for FlowCost {
    type Error = AuraError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        if value <= u64::from(u32::MAX) {
            Ok(Self(value as u32))
        } else {
            Err(AuraError::invalid(format!(
                "FlowCost overflow: {value} exceeds u32::MAX"
            )))
        }
    }
}

impl TryFrom<i64> for FlowCost {
    type Error = AuraError;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        if value < 0 {
            return Err(AuraError::invalid(format!(
                "FlowCost cannot be negative: {value}"
            )));
        }
        FlowCost::try_from(value as u64)
    }
}

/// Monotonic nonce for flow receipts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FlowNonce(u64);

impl FlowNonce {
    /// Create a new flow nonce.
    #[must_use]
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    /// Return the raw nonce value.
    #[must_use]
    pub fn value(self) -> u64 {
        self.0
    }

    /// Return the next nonce, or error on overflow.
    pub fn checked_next(self) -> Result<Self, AuraError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or_else(|| AuraError::invalid("FlowNonce overflow"))
    }
}

impl fmt::Display for FlowNonce {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u64> for FlowNonce {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<FlowNonce> for u64 {
    fn from(value: FlowNonce) -> Self {
        value.0
    }
}

/// Receipt signature bytes (validated length).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiptSig(Vec<u8>);

impl ReceiptSig {
    /// Create a new receipt signature, enforcing max size.
    pub fn new(bytes: Vec<u8>) -> Result<Self, AuraError> {
        if bytes.len() > MAX_SIGNATURE_BYTES {
            return Err(AuraError::invalid(format!(
                "Receipt signature too large: {} bytes (max {})",
                bytes.len(),
                MAX_SIGNATURE_BYTES
            )));
        }
        Ok(Self(bytes))
    }

    /// View the signature bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Consume the signature bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}

impl TryFrom<Vec<u8>> for ReceiptSig {
    type Error = AuraError;

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

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
    #[must_use]
    pub fn new(limit: u64, epoch: Epoch) -> Self {
        Self {
            limit,
            spent: 0,
            epoch,
        }
    }

    /// Remaining headroom before the guard should block.
    #[must_use]
    pub fn headroom(&self) -> u64 {
        self.limit.saturating_sub(self.spent)
    }

    /// Alias for headroom() - returns remaining budget
    #[must_use]
    pub fn remaining(&self) -> u64 {
        self.headroom()
    }

    /// Returns true if charging `cost` would still be within the budget.
    pub fn can_charge(&self, cost: FlowCost) -> Result<bool, BudgetError> {
        let cost_value = u64::from(cost);
        let new_spent = self
            .spent
            .checked_add(cost_value)
            .ok_or(BudgetError::Overflow {
                spent: self.spent,
                cost: cost_value,
            })?;
        Ok(new_spent <= self.limit)
    }

    /// Record a spend if possible.
    ///
    /// Returns `Ok(())` if the charge succeeded, or `Err(BudgetError::Exhausted)`
    /// if the budget would be exceeded.
    pub fn record_charge(&mut self, cost: FlowCost) -> std::result::Result<(), BudgetError> {
        let cost_value = u64::from(cost);
        let new_spent = self
            .spent
            .checked_add(cost_value)
            .ok_or(BudgetError::Overflow {
                spent: self.spent,
                cost: cost_value,
            })?;
        if new_spent <= self.limit {
            self.spent = new_spent;
            Ok(())
        } else {
            Err(BudgetError::Exhausted {
                cost: cost_value,
                remaining: self.headroom(),
            })
        }
    }

    /// Merge two replicas of the same `(context, peer)` budget.
    ///
    /// - `limit` takes the meet (minimum)
    /// - `spent` takes the join (maximum)
    /// - `epoch` advances monotonically (maximum)
    #[must_use]
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
    #[must_use]
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
    pub cost: FlowCost,
    /// Monotonic nonce per `(ctx, src, epoch)` used to prevent replay.
    pub nonce: FlowNonce,
    /// Previous receipt hash (forms a hash chain for auditing).
    pub prev: Hash32,
    /// Transport-level signature or MAC over the receipt fields.
    pub sig: ReceiptSig,
}

impl Receipt {
    /// Create a new receipt.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ctx: ContextId,
        src: AuthorityId,
        dst: AuthorityId,
        epoch: Epoch,
        cost: FlowCost,
        nonce: FlowNonce,
        prev: Hash32,
        sig: ReceiptSig,
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
#[allow(clippy::expect_used)]
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
        assert!(budget.record_charge(FlowCost::new(4)).is_ok());
        assert_eq!(budget.spent, 4);
        assert!(budget.record_charge(FlowCost::new(6)).is_ok());
        assert_eq!(budget.spent, 10);
        assert!(budget.record_charge(FlowCost::new(1)).is_err());
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

    #[test]
    fn record_charge_detects_overflow() {
        let mut budget = FlowBudget {
            limit: u64::MAX,
            spent: u64::MAX,
            epoch: Epoch::initial(),
        };
        let err = budget
            .record_charge(FlowCost::new(1))
            .expect_err("overflow should error");
        assert!(matches!(err, BudgetError::Overflow { .. }));
    }

    #[test]
    fn receipt_sig_enforces_max_size() {
        let oversized = vec![0u8; MAX_SIGNATURE_BYTES + 1];
        assert!(ReceiptSig::new(oversized).is_err());
    }

    #[test]
    fn flow_cost_rejects_overflow() {
        let err = FlowCost::try_from(u64::from(u32::MAX) + 1);
        assert!(err.is_err());
    }
}
