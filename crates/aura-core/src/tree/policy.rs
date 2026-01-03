//! Threshold Policy Meet-Semilattice
//!
//! Policies form a meet-semilattice where "more restrictive is smaller".
//! The meet operation selects the stricter of two policies.
//!
//! # Partial Order
//!
//! - `Any ≥ Threshold{m,n}` if m ≥ 1
//! - `Threshold{m2,n} ≤ Threshold{m1,n}` if m2 ≥ m1 (same n)
//! - `All ≤ Threshold{n,n}`
//!
//! # Meet-Semilattice Laws
//!
//! - **Associativity**: `(a ⊓ b) ⊓ c = a ⊓ (b ⊓ c)`
//! - **Commutativity**: `a ⊓ b = b ⊓ a`
//! - **Idempotency**: `a ⊓ a = a`
//!
//! # Reference
//!
//! See [`docs/123_commitment_tree.md`](../../../docs/123_commitment_tree.md) - Policy Lattice section.

use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

/// Threshold policy for tree operations.
///
/// Defines how many participants must approve an operation.
/// Policies can only become stricter (meet operation), never more permissive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u16)]
pub enum Policy {
    /// 1-of-n (any single participant can authorize)
    Any,

    /// m-of-n threshold (m out of n participants must authorize)
    Threshold {
        /// Minimum number of participants required (m)
        m: u16,
        /// Total number of participants (n)
        n: u16,
    },

    /// n-of-n (all participants must authorize)
    All,
}

/// Errors that can occur when validating policies.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PolicyError {
    /// Threshold policy has invalid m/n bounds.
    #[error("Invalid threshold: m must satisfy 1 <= m <= n (m={m}, n={n})")]
    InvalidThreshold { m: u16, n: u16 },
    /// Child count does not match threshold policy's n.
    #[error("Child count mismatch: policy expects n={expected}, got {actual}")]
    ChildCountMismatch { expected: u16, actual: u16 },
    /// Child count is zero.
    #[error("Child count is zero")]
    EmptyChildSet,
    /// Child count exceeds u16::MAX.
    #[error("Child count {child_count} exceeds u16::MAX")]
    ChildCountOverflow { child_count: u64 },
}

impl Policy {
    /// Create a validated m-of-n threshold policy.
    pub fn threshold(m: u16, n: u16) -> Result<Self, PolicyError> {
        if m == 0 || n == 0 || m > n {
            return Err(PolicyError::InvalidThreshold { m, n });
        }
        Ok(Policy::Threshold { m, n })
    }

    /// Compute the meet (⊓) of two policies, selecting the stricter one.
    ///
    /// The meet operation is:
    /// - **Associative**: `(a ⊓ b) ⊓ c = a ⊓ (b ⊓ c)`
    /// - **Commutative**: `a ⊓ b = b ⊓ a`
    /// - **Idempotent**: `a ⊓ a = a`
    ///
    /// # Examples
    ///
    /// ```
    /// use aura_core::tree::Policy;
    ///
    /// let p1 = Policy::Any;
    /// let p2 = Policy::Threshold { m: 2, n: 3 };
    /// assert_eq!(p1.meet(&p2), p2); // Threshold is stricter than Any
    ///
    /// let p3 = Policy::Threshold { m: 2, n: 3 };
    /// let p4 = Policy::Threshold { m: 3, n: 3 };
    /// assert_eq!(p3.meet(&p4), Policy::All); // 3-of-3 threshold normalizes to All
    /// ```
    pub fn meet(&self, other: &Self) -> Self {
        use Policy::*;

        match (self, other) {
            // Idempotency: a ⊓ a = a
            (a, b) if a == b => *a,

            // All is always strictest
            (All, _) | (_, All) => All,

            // Any is always least strict
            (Any, other) | (other, Any) => *other,

            // Threshold ⊓ Threshold
            (Threshold { m: m1, n: n1 }, Threshold { m: m2, n: n2 }) => {
                if n1 == n2 {
                    // Same n: higher m is stricter
                    let max_m = (*m1).max(*m2);
                    if max_m >= *n1 {
                        All // Normalize n-of-n to All
                    } else {
                        Threshold { m: max_m, n: *n1 }
                    }
                } else {
                    // Different n values: cannot meaningfully compare, take stricter approximation
                    // This is a conservative choice - in practice, tree operations should
                    // maintain consistent n values at each level
                    if m1 >= n1 || m2 >= n2 {
                        All // If either is already All-equivalent, result is All
                    } else {
                        // Use integer cross-multiplication to avoid floating point precision issues
                        // Compare m1/n1 vs m2/n2 by comparing m1*n2 vs m2*n1
                        let cross1 = (*m1 as u64) * (*n2 as u64);
                        let cross2 = (*m2 as u64) * (*n1 as u64);

                        let result = match cross1.cmp(&cross2) {
                            std::cmp::Ordering::Greater => Threshold { m: *m1, n: *n1 },
                            std::cmp::Ordering::Less => Threshold { m: *m2, n: *n2 },
                            std::cmp::Ordering::Equal => {
                                // When fractions are equal, choose deterministically to ensure commutativity
                                // Use lexicographic ordering: (m, n) to break ties consistently
                                match (m1, n1).cmp(&(m2, n2)) {
                                    std::cmp::Ordering::Less | std::cmp::Ordering::Equal => {
                                        Threshold { m: *m1, n: *n1 }
                                    }
                                    std::cmp::Ordering::Greater => Threshold { m: *m2, n: *n2 },
                                }
                            }
                        };

                        // Normalize result if it's All-equivalent
                        if let Threshold { m, n } = result {
                            if m >= n {
                                All
                            } else {
                                result
                            }
                        } else {
                            result
                        }
                    }
                }
            }
        }
    }

    /// Check if this policy is stricter than or equal to another.
    ///
    /// In meet-semilattice terms: `self.is_stricter_than(other)` means `self ≤ other`.
    ///
    /// # Examples
    ///
    /// ```
    /// use aura_core::tree::Policy;
    ///
    /// let any = Policy::Any;
    /// let threshold = Policy::Threshold { m: 2, n: 3 };
    /// let all = Policy::All;
    ///
    /// assert!(all.is_stricter_than(&threshold)); // All ≤ Threshold
    /// assert!(threshold.is_stricter_than(&any));   // Threshold ≤ Any
    /// assert!(all.is_stricter_than(&any));         // All ≤ Any
    /// ```
    pub fn is_stricter_than(&self, other: &Self) -> bool {
        self.meet(other) == *self
    }

    /// Returns the minimum number of signers required for this policy.
    ///
    /// This derives the concrete threshold from the policy given the current
    /// number of children at the node. The threshold is used for signature
    /// verification to ensure enough signers participated.
    ///
    /// # Arguments
    ///
    /// * `child_count` - The number of children (leaves or branches) under this node
    ///
    /// # Returns
    ///
    /// The minimum number of signers required for operations at this node.
    ///
    /// # Examples
    ///
    /// ```
    /// use aura_core::tree::Policy;
    ///
    /// assert_eq!(Policy::Any.required_signers(3).unwrap(), 1);
    /// assert_eq!(Policy::All.required_signers(3).unwrap(), 3);
    /// assert_eq!(
    ///     Policy::Threshold { m: 2, n: 3 }.required_signers(3).unwrap(),
    ///     2
    /// );
    /// ```
    pub fn required_signers(&self, child_count: usize) -> Result<u16, PolicyError> {
        if child_count == 0 {
            return Err(PolicyError::EmptyChildSet);
        }
        let child_count_u16 =
            u16::try_from(child_count).map_err(|_| PolicyError::ChildCountOverflow {
                child_count: child_count as u64,
            })?;

        match self {
            Policy::Any => Ok(1),
            Policy::All => Ok(child_count_u16),
            Policy::Threshold { m, n } => {
                if *m == 0 || *n == 0 || *m > *n {
                    return Err(PolicyError::InvalidThreshold { m: *m, n: *n });
                }
                if child_count_u16 != *n {
                    return Err(PolicyError::ChildCountMismatch {
                        expected: *n,
                        actual: child_count_u16,
                    });
                }
                Ok(*m)
            }
        }
    }
}

impl PartialOrd for Policy {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        use std::cmp::Ordering;
        use Policy::*;

        match (self, other) {
            // Equal policies
            (a, b) if a == b => Some(Ordering::Equal),

            // All is strictest (smallest in partial order)
            (All, _) => Some(Ordering::Less),
            (_, All) => Some(Ordering::Greater),

            // Any is least strict (largest in partial order)
            (Any, _) => Some(Ordering::Greater),
            (_, Any) => Some(Ordering::Less),

            // Threshold comparison (same n)
            (Threshold { m: m1, n: n1 }, Threshold { m: m2, n: n2 }) if n1 == n2 => {
                Some(m1.cmp(m2).reverse()) // Higher m is stricter (smaller in order)
            }

            // Threshold comparison (different n) - not fully comparable
            (Threshold { .. }, Threshold { .. }) => None,
        }
    }
}

impl fmt::Display for Policy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Policy::Any => write!(f, "Any"),
            Policy::Threshold { m, n } => write!(f, "{m}-of-{n}"),
            Policy::All => write!(f, "All"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_policy_display() {
        assert_eq!(format!("{}", Policy::Any), "Any");
        assert_eq!(format!("{}", Policy::Threshold { m: 2, n: 3 }), "2-of-3");
        assert_eq!(format!("{}", Policy::All), "All");
    }

    #[test]
    fn test_meet_idempotency() {
        // a ⊓ a = a
        let policies = vec![Policy::Any, Policy::Threshold { m: 2, n: 3 }, Policy::All];

        for p in policies {
            assert_eq!(p.meet(&p), p, "Idempotency failed for {p:?}");
        }
    }

    #[test]
    fn test_meet_commutativity() {
        // a ⊓ b = b ⊓ a
        let test_cases = vec![
            (Policy::Any, Policy::Threshold { m: 2, n: 3 }),
            (Policy::Any, Policy::All),
            (Policy::Threshold { m: 2, n: 3 }, Policy::All),
            (
                Policy::Threshold { m: 2, n: 3 },
                Policy::Threshold { m: 3, n: 3 },
            ),
        ];

        for (a, b) in test_cases {
            assert_eq!(
                a.meet(&b),
                b.meet(&a),
                "Commutativity failed for {a:?} and {b:?}",
            );
        }
    }

    #[test]
    fn test_meet_associativity() {
        // (a ⊓ b) ⊓ c = a ⊓ (b ⊓ c)
        let test_cases = vec![
            (Policy::Any, Policy::Threshold { m: 2, n: 3 }, Policy::All),
            (
                Policy::Threshold { m: 1, n: 3 },
                Policy::Threshold { m: 2, n: 3 },
                Policy::Threshold { m: 3, n: 3 },
            ),
        ];

        for (a, b, c) in test_cases {
            let left = a.meet(&b).meet(&c);
            let right = a.meet(&b.meet(&c));
            assert_eq!(left, right, "Associativity failed for {a:?}, {b:?}, {c:?}",);
        }
    }

    #[test]
    fn test_meet_selects_stricter() {
        assert_eq!(
            Policy::Any.meet(&Policy::Threshold { m: 2, n: 3 }),
            Policy::Threshold { m: 2, n: 3 }
        );

        assert_eq!(Policy::Any.meet(&Policy::All), Policy::All);

        assert_eq!(
            Policy::Threshold { m: 2, n: 3 }.meet(&Policy::All),
            Policy::All
        );
    }

    #[test]
    fn test_threshold_meet_same_n() {
        let p1 = Policy::Threshold { m: 2, n: 3 };
        let p2 = Policy::Threshold { m: 3, n: 3 };
        assert_eq!(p1.meet(&p2), Policy::All); // 3-of-3 threshold normalizes to All
    }

    #[test]
    fn test_is_stricter_than() {
        assert!(Policy::All.is_stricter_than(&Policy::Any));
        assert!(Policy::All.is_stricter_than(&Policy::Threshold { m: 2, n: 3 }));
        assert!(Policy::Threshold { m: 2, n: 3 }.is_stricter_than(&Policy::Any));

        assert!(!Policy::Any.is_stricter_than(&Policy::All));
        assert!(!Policy::Any.is_stricter_than(&Policy::Threshold { m: 2, n: 3 }));
    }

    #[test]
    fn test_partial_ord() {
        assert!(Policy::All < Policy::Any);
        assert!(Policy::All < Policy::Threshold { m: 2, n: 3 });
        assert!(Policy::Threshold { m: 3, n: 3 } < Policy::Threshold { m: 2, n: 3 });

        // Not comparable: different n values
        assert_eq!(
            Policy::Threshold { m: 2, n: 3 }.partial_cmp(&Policy::Threshold { m: 2, n: 5 }),
            None
        );
    }

    #[test]
    fn test_threshold_validation() {
        assert!(Policy::threshold(0, 3).is_err());
        assert!(Policy::threshold(1, 0).is_err());
        assert!(Policy::threshold(3, 2).is_err());
        assert!(Policy::threshold(2, 3).is_ok());
    }

    #[test]
    fn test_required_signers_child_count_mismatch() {
        let policy = Policy::Threshold { m: 2, n: 3 };
        let err = policy.required_signers(2).unwrap_err();
        assert!(matches!(
            err,
            PolicyError::ChildCountMismatch {
                expected: 3,
                actual: 2
            }
        ));
    }

    #[test]
    fn test_required_signers_invalid_threshold() {
        let policy = Policy::Threshold { m: 0, n: 3 };
        let err = policy.required_signers(3).unwrap_err();
        assert!(matches!(err, PolicyError::InvalidThreshold { m: 0, n: 3 }));
    }
}
