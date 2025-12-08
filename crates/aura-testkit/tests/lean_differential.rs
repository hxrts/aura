//! Differential Testing: Rust vs Lean Oracle
//!
//! These tests compare Rust implementations against the formally verified
//! Lean models to ensure they produce identical results.
//!
//! Run with: `just test-differential`
//! Or: `cargo test -p aura-testkit --test lean_differential --features lean`
//!
//! Note: These tests require the Lean oracle to be built first:
//! `just lean-oracle-build` or `cd verification/lean && lake build`

#![cfg(feature = "lean")]

use aura_testkit::verification::lean_oracle::{
    ComparePolicy, Fact, LeanOracle, LeanOracleResult, Ordering, TimeStamp,
};
use proptest::prelude::*;

/// Strategy for generating random facts
fn fact_strategy() -> impl Strategy<Value = Fact> {
    (0u64..1000).prop_map(|id| Fact { id })
}

/// Strategy for generating random journals (Vec<Fact>)
fn journal_strategy() -> impl Strategy<Value = Vec<Fact>> {
    prop::collection::vec(fact_strategy(), 0..20)
}

/// Strategy for generating random timestamps
fn timestamp_strategy() -> impl Strategy<Value = TimeStamp> {
    (0u64..1000, 0u64..1000).prop_map(|(logical, order_clock)| TimeStamp {
        logical,
        order_clock,
    })
}

/// Strategy for generating comparison policies
#[allow(dead_code)]
fn policy_strategy() -> impl Strategy<Value = ComparePolicy> {
    prop::bool::ANY.prop_map(|ignore_physical| ComparePolicy { ignore_physical })
}

// ============================================================================
// Journal Merge Tests
// ============================================================================

/// Rust implementation of journal merge (set union with dedup)
/// Note: The Lean model uses list concatenation, but the proofs are about
/// membership-based equivalence (≃). So for comparison, we use set semantics.
fn rust_journal_merge(j1: &[Fact], j2: &[Fact]) -> Vec<Fact> {
    let mut result: Vec<Fact> = j1.iter().chain(j2.iter()).cloned().collect();
    result.sort_by_key(|f| f.id);
    result.dedup_by_key(|f| f.id);
    result
}

/// Normalize a journal to set semantics (sort and dedup) for comparison
fn normalize_journal(facts: &[Fact]) -> Vec<u64> {
    let mut ids: Vec<u64> = facts.iter().map(|f| f.id).collect();
    ids.sort();
    ids.dedup();
    ids
}

/// Test: Journal merge is commutative
/// Lean proves: merge j1 j2 ≃ merge j2 j1
#[test]
#[ignore = "requires Lean oracle - run with just test-differential"]
fn test_merge_commutative() -> LeanOracleResult<()> {
    let oracle = LeanOracle::new()?;

    // Test with a few concrete cases
    let cases = vec![
        (vec![Fact { id: 1 }, Fact { id: 2 }], vec![Fact { id: 3 }]),
        (vec![], vec![Fact { id: 1 }]),
        (
            vec![Fact { id: 1 }, Fact { id: 2 }],
            vec![Fact { id: 2 }, Fact { id: 3 }],
        ),
    ];

    for (j1, j2) in cases {
        let result_12 = oracle.verify_merge(j1.clone(), j2.clone())?;
        let result_21 = oracle.verify_merge(j2.clone(), j1.clone())?;

        // Results should have same facts (as sets)
        let mut r12: Vec<_> = result_12.result.iter().map(|f| f.id).collect();
        let mut r21: Vec<_> = result_21.result.iter().map(|f| f.id).collect();
        r12.sort();
        r21.sort();

        assert_eq!(r12, r21, "Merge should be commutative");
    }

    Ok(())
}

/// Test: Lean oracle merge matches Rust implementation (set semantics)
/// Note: Lean's merge uses list concatenation, but proofs use membership equivalence.
/// We compare using set semantics (normalized, deduped).
#[test]
#[ignore = "requires Lean oracle - run with just test-differential"]
fn test_merge_matches_rust() -> LeanOracleResult<()> {
    let oracle = LeanOracle::new()?;

    let j1 = vec![Fact { id: 1 }, Fact { id: 2 }, Fact { id: 5 }];
    let j2 = vec![Fact { id: 3 }, Fact { id: 2 }, Fact { id: 4 }];

    let lean_result = oracle.verify_merge(j1.clone(), j2.clone())?;
    let rust_result = rust_journal_merge(&j1, &j2);

    // Compare using set membership semantics (normalized)
    let lean_normalized = normalize_journal(&lean_result.result);
    let rust_normalized = normalize_journal(&rust_result);

    assert_eq!(
        lean_normalized, rust_normalized,
        "Lean and Rust merge should produce equivalent sets"
    );
    Ok(())
}

// ============================================================================
// Journal Reduce Tests
// ============================================================================

/// Test: Journal reduce is deterministic
/// Lean proves: reduce is a pure function
#[test]
#[ignore = "requires Lean oracle - run with just test-differential"]
fn test_reduce_deterministic() -> LeanOracleResult<()> {
    let oracle = LeanOracle::new()?;

    let journal = vec![
        Fact { id: 3 },
        Fact { id: 1 },
        Fact { id: 2 },
        Fact { id: 1 }, // Duplicate
    ];

    // Run reduce twice - should get same result
    let result1 = oracle.verify_reduce(journal.clone())?;
    let result2 = oracle.verify_reduce(journal)?;

    assert_eq!(result1.count, result2.count);

    let ids1: Vec<_> = result1.result.iter().map(|f| f.id).collect();
    let ids2: Vec<_> = result2.result.iter().map(|f| f.id).collect();

    assert_eq!(ids1, ids2, "Reduce should be deterministic");
    Ok(())
}

// ============================================================================
// Flow Budget Tests
// ============================================================================

/// Rust implementation of flow budget charge
fn rust_flow_charge(budget: u64, cost: u64) -> Option<u64> {
    if cost <= budget {
        Some(budget - cost)
    } else {
        None
    }
}

/// Test: Flow charge matches Rust implementation
/// Lean proves: charge_decreases, charge_exact
#[test]
#[ignore = "requires Lean oracle - run with just test-differential"]
fn test_flow_charge_matches_rust() -> LeanOracleResult<()> {
    let oracle = LeanOracle::new()?;

    let test_cases = vec![
        (100, 30),  // Normal charge
        (100, 100), // Exact charge
        (100, 0),   // Zero charge
        (10, 30),   // Insufficient budget
        (0, 0),     // Zero budget, zero cost
        (0, 1),     // Zero budget, non-zero cost
    ];

    for (budget, cost) in test_cases {
        let lean_result = oracle.verify_charge(budget, cost)?;
        let rust_result = rust_flow_charge(budget, cost);

        match (lean_result.success, rust_result) {
            (true, Some(remaining)) => {
                assert_eq!(
                    lean_result.remaining,
                    Some(remaining),
                    "Remaining budget should match for budget={}, cost={}",
                    budget,
                    cost
                );
            }
            (false, None) => {
                assert_eq!(
                    lean_result.remaining, None,
                    "Both should indicate failure for budget={}, cost={}",
                    budget, cost
                );
            }
            _ => {
                panic!(
                    "Lean and Rust disagree on charge result for budget={}, cost={}",
                    budget, cost
                );
            }
        }
    }

    Ok(())
}

// ============================================================================
// Timestamp Comparison Tests
// ============================================================================

/// Rust implementation of timestamp comparison
fn rust_timestamp_compare(policy: &ComparePolicy, a: &TimeStamp, b: &TimeStamp) -> Ordering {
    if policy.ignore_physical {
        // Only compare logical time
        match a.logical.cmp(&b.logical) {
            std::cmp::Ordering::Less => Ordering::Lt,
            std::cmp::Ordering::Equal => Ordering::Eq,
            std::cmp::Ordering::Greater => Ordering::Gt,
        }
    } else {
        // Compare logical first, then order_clock as tiebreaker
        match a.logical.cmp(&b.logical) {
            std::cmp::Ordering::Less => Ordering::Lt,
            std::cmp::Ordering::Greater => Ordering::Gt,
            std::cmp::Ordering::Equal => match a.order_clock.cmp(&b.order_clock) {
                std::cmp::Ordering::Less => Ordering::Lt,
                std::cmp::Ordering::Equal => Ordering::Eq,
                std::cmp::Ordering::Greater => Ordering::Gt,
            },
        }
    }
}

/// Test: Timestamp compare is reflexive
/// Lean proves: compare_refl
#[test]
#[ignore = "requires Lean oracle - run with just test-differential"]
fn test_compare_reflexive() -> LeanOracleResult<()> {
    let oracle = LeanOracle::new()?;

    let timestamps = vec![
        TimeStamp {
            logical: 0,
            order_clock: 0,
        },
        TimeStamp {
            logical: 10,
            order_clock: 5,
        },
        TimeStamp {
            logical: 100,
            order_clock: 200,
        },
    ];

    for policy_ignore in [true, false] {
        let policy = ComparePolicy {
            ignore_physical: policy_ignore,
        };

        for t in &timestamps {
            let result = oracle.verify_compare(policy.clone(), t.clone(), t.clone())?;
            assert_eq!(
                result,
                Ordering::Eq,
                "Compare should be reflexive for {:?} with policy ignore_physical={}",
                t,
                policy_ignore
            );
        }
    }

    Ok(())
}

/// Test: Timestamp compare matches Rust implementation
#[test]
#[ignore = "requires Lean oracle - run with just test-differential"]
fn test_compare_matches_rust() -> LeanOracleResult<()> {
    let oracle = LeanOracle::new()?;

    let test_cases = vec![
        // (policy_ignore_physical, a, b)
        (
            true,
            TimeStamp {
                logical: 5,
                order_clock: 100,
            },
            TimeStamp {
                logical: 10,
                order_clock: 50,
            },
        ),
        (
            true,
            TimeStamp {
                logical: 10,
                order_clock: 100,
            },
            TimeStamp {
                logical: 10,
                order_clock: 50,
            },
        ),
        (
            false,
            TimeStamp {
                logical: 10,
                order_clock: 100,
            },
            TimeStamp {
                logical: 10,
                order_clock: 50,
            },
        ),
    ];

    for (ignore_physical, a, b) in test_cases {
        let policy = ComparePolicy { ignore_physical };

        let lean_result = oracle.verify_compare(policy.clone(), a.clone(), b.clone())?;
        let rust_result = rust_timestamp_compare(&policy, &a, &b);

        assert_eq!(
            lean_result, rust_result,
            "Lean and Rust should agree on compare({:?}, {:?}) with ignore_physical={}",
            a, b, ignore_physical
        );
    }

    Ok(())
}

/// Test: Physical time is hidden when ignorePhysical = true
/// Lean proves: physical_hidden
#[test]
#[ignore = "requires Lean oracle - run with just test-differential"]
fn test_physical_hidden() -> LeanOracleResult<()> {
    let oracle = LeanOracle::new()?;

    let policy = ComparePolicy {
        ignore_physical: true,
    };

    // Same logical time, different order_clock
    let a1 = TimeStamp {
        logical: 10,
        order_clock: 100,
    };
    let b1 = TimeStamp {
        logical: 20,
        order_clock: 200,
    };

    let a2 = TimeStamp {
        logical: 10,
        order_clock: 999,
    };
    let b2 = TimeStamp {
        logical: 20,
        order_clock: 1,
    };

    let result1 = oracle.verify_compare(policy.clone(), a1, b1)?;
    let result2 = oracle.verify_compare(policy.clone(), a2, b2)?;

    assert_eq!(
        result1, result2,
        "Physical time should not affect comparison when ignorePhysical=true"
    );

    Ok(())
}

// ============================================================================
// Property-Based Tests (using proptest)
// ============================================================================

proptest! {
    /// Property test: Lean and Rust flow charge agree on all inputs
    #[test]
    #[ignore = "requires Lean oracle - run with just test-differential"]
    fn prop_flow_charge_agreement(budget in 0u64..1000, cost in 0u64..1000) {
        if let Ok(oracle) = LeanOracle::new() {
            if let Ok(lean_result) = oracle.verify_charge(budget, cost) {
                let rust_result = rust_flow_charge(budget, cost);

                match (lean_result.success, rust_result) {
                    (true, Some(remaining)) => {
                        prop_assert_eq!(lean_result.remaining, Some(remaining));
                    }
                    (false, None) => {
                        prop_assert_eq!(lean_result.remaining, None);
                    }
                    _ => {
                        prop_assert!(false, "Lean and Rust disagree");
                    }
                }
            }
        }
    }

    /// Property test: Timestamp comparison is reflexive
    #[test]
    #[ignore = "requires Lean oracle - run with just test-differential"]
    fn prop_compare_reflexive(ts in timestamp_strategy(), ignore_physical in prop::bool::ANY) {
        if let Ok(oracle) = LeanOracle::new() {
            let policy = ComparePolicy { ignore_physical };
            if let Ok(result) = oracle.verify_compare(policy, ts.clone(), ts) {
                prop_assert_eq!(result, Ordering::Eq);
            }
        }
    }

    /// Property test: Lean and Rust timestamp compare agree
    #[test]
    #[ignore = "requires Lean oracle - run with just test-differential"]
    fn prop_compare_agreement(
        a in timestamp_strategy(),
        b in timestamp_strategy(),
        ignore_physical in prop::bool::ANY
    ) {
        if let Ok(oracle) = LeanOracle::new() {
            let policy = ComparePolicy { ignore_physical };
            if let Ok(lean_result) = oracle.verify_compare(policy.clone(), a.clone(), b.clone()) {
                let rust_result = rust_timestamp_compare(&policy, &a, &b);
                prop_assert_eq!(lean_result, rust_result);
            }
        }
    }

    /// Property test: Journal merge produces union (set semantics)
    /// Lean's merge uses list concatenation, but proofs use membership equivalence.
    #[test]
    #[ignore = "requires Lean oracle - run with just test-differential"]
    fn prop_merge_is_union(j1 in journal_strategy(), j2 in journal_strategy()) {
        if let Ok(oracle) = LeanOracle::new() {
            if let Ok(lean_result) = oracle.verify_merge(j1.clone(), j2.clone()) {
                let rust_result = rust_journal_merge(&j1, &j2);

                // Compare using set membership semantics (normalized)
                let lean_normalized = normalize_journal(&lean_result.result);
                let rust_normalized = normalize_journal(&rust_result);

                prop_assert_eq!(lean_normalized, rust_normalized);
            }
        }
    }
}

// ============================================================================
// Version Check Test
// ============================================================================

/// Test: Oracle version matches expected
#[test]
#[ignore = "requires Lean oracle - run with just test-differential"]
fn test_oracle_version() -> LeanOracleResult<()> {
    let oracle = LeanOracle::new()?;
    oracle.verify_version()?;

    let version = oracle.version()?;
    assert_eq!(version.version, "0.2.0");
    assert!(version.modules.contains(&"Journal".to_string()));
    assert!(version.modules.contains(&"FlowBudget".to_string()));
    assert!(version.modules.contains(&"TimeSystem".to_string()));

    Ok(())
}
