//! Flow budget property tests for CRDT semantics
//!
//! Tests the core invariants required by work/007.md Section 3:
//! - FlowBudget CRDT properties: associative, commutative, idempotent
//! - Budget charging respects limits
//! - Epoch rotation behavior
//! - No-Observable-Without-Charge invariant

use aura_core::{flow::FlowBudget, semilattice::JoinSemilattice, session_epochs::Epoch};

/// Test FlowBudget CRDT join operation is associative
#[test]
fn flow_budget_join_associative() {
    // Test cases with various FlowBudget configurations
    let test_cases = [
        (
            FlowBudget {
                limit: 100,
                spent: 30,
                epoch: Epoch::new(5),
            },
            FlowBudget {
                limit: 80,
                spent: 40,
                epoch: Epoch::new(6),
            },
            FlowBudget {
                limit: 120,
                spent: 20,
                epoch: Epoch::new(4),
            },
        ),
        (
            FlowBudget {
                limit: 500,
                spent: 100,
                epoch: Epoch::new(1),
            },
            FlowBudget {
                limit: 300,
                spent: 200,
                epoch: Epoch::new(2),
            },
            FlowBudget {
                limit: 400,
                spent: 150,
                epoch: Epoch::new(3),
            },
        ),
    ];

    for (a, b, c) in test_cases.iter() {
        // (a ∨ b) ∨ c = a ∨ (b ∨ c)
        let left = a.join(b).join(c);
        let right = a.join(&b.join(c));

        assert_eq!(left, right, "Join operation must be associative");
    }
}

/// Test FlowBudget CRDT join operation is commutative
#[test]
fn flow_budget_join_commutative() {
    let test_cases = [
        (
            FlowBudget {
                limit: 100,
                spent: 30,
                epoch: Epoch::new(5),
            },
            FlowBudget {
                limit: 80,
                spent: 40,
                epoch: Epoch::new(6),
            },
        ),
        (
            FlowBudget {
                limit: 200,
                spent: 0,
                epoch: Epoch::new(1),
            },
            FlowBudget {
                limit: 150,
                spent: 100,
                epoch: Epoch::new(10),
            },
        ),
    ];

    for (a, b) in test_cases.iter() {
        // a ∨ b = b ∨ a
        let left = a.join(b);
        let right = b.join(a);

        assert_eq!(left, right, "Join operation must be commutative");
    }
}

/// Test FlowBudget CRDT join operation is idempotent
#[test]
fn flow_budget_join_idempotent() {
    let test_cases = [
        FlowBudget {
            limit: 100,
            spent: 30,
            epoch: Epoch::new(5),
        },
        FlowBudget {
            limit: 500,
            spent: 0,
            epoch: Epoch::new(1),
        },
        FlowBudget {
            limit: 1000,
            spent: 999,
            epoch: Epoch::new(100),
        },
    ];

    for budget in test_cases.iter() {
        // a ∨ a = a
        let result = budget.join(budget);
        assert_eq!(result, *budget, "Join operation must be idempotent");
    }
}

/// Test FlowBudget merge maintains CRDT invariants
#[test]
fn flow_budget_merge_invariants() {
    let test_cases = [
        (
            FlowBudget {
                limit: 100,
                spent: 30,
                epoch: Epoch::new(5),
            },
            FlowBudget {
                limit: 80,
                spent: 40,
                epoch: Epoch::new(6),
            },
            (80, 40, 6), // Expected: min limit, max spent, max epoch
        ),
        (
            FlowBudget {
                limit: 200,
                spent: 100,
                epoch: Epoch::new(1),
            },
            FlowBudget {
                limit: 150,
                spent: 50,
                epoch: Epoch::new(3),
            },
            (150, 100, 3),
        ),
    ];

    for (budget1, budget2, (expected_limit, expected_spent, expected_epoch)) in test_cases.iter() {
        let merged = budget1.merge(budget2);

        assert_eq!(
            merged.limit, *expected_limit,
            "Limit should be minimum (meet operation)"
        );
        assert_eq!(
            merged.spent, *expected_spent,
            "Spent should be maximum (join operation)"
        );
        assert_eq!(
            merged.epoch.value(),
            *expected_epoch,
            "Epoch should advance monotonically"
        );
    }
}

/// Test charge_flow respects budget limits
#[test]
fn charge_flow_respects_limits() {
    let test_cases = [
        (100, 30, 50, true),    // Within budget: 30 + 50 <= 100
        (100, 30, 70, true),    // At limit: 30 + 70 = 100
        (100, 30, 71, false),   // Exceeds budget: 30 + 71 > 100
        (50, 0, 60, false),     // Exceeds from zero: 0 + 60 > 50
        (1000, 500, 400, true), // Large numbers within budget
    ];

    for (limit, initial_spent, cost, should_succeed) in test_cases.iter() {
        let mut budget = FlowBudget::new(*limit, Epoch::initial());
        budget.spent = *initial_spent;

        let initial_spent_value = budget.spent;
        let charge_success = budget.record_charge(*cost);

        if *should_succeed {
            assert!(charge_success, "Charge should succeed when within headroom");
            assert_eq!(
                budget.spent,
                initial_spent_value + cost,
                "Spent should increase by cost"
            );
        } else {
            assert!(
                !charge_success,
                "Charge should fail when exceeding headroom"
            );
            assert_eq!(
                budget.spent, initial_spent_value,
                "Spent should remain unchanged on failure"
            );
        }
    }
}

/// Test headroom calculation is correct
#[test]
fn headroom_calculation_correct() {
    let test_cases = [
        (100, 30, 70), // Normal case
        (100, 0, 100), // Full headroom
        (100, 100, 0), // No headroom
        (100, 150, 0), // Over-spent (saturating_sub)
        (0, 0, 0),     // Zero limit
    ];

    for (limit, spent, expected_headroom) in test_cases.iter() {
        let budget = FlowBudget {
            limit: *limit,
            spent: *spent,
            epoch: Epoch::initial(),
        };

        assert_eq!(
            budget.headroom(),
            *expected_headroom,
            "Headroom calculation incorrect"
        );
    }
}

/// Test can_charge predicate matches record_charge behavior
#[test]
fn can_charge_predicate_matches_record_charge() {
    let test_cases = [
        (100, 30, 50),
        (100, 30, 70),
        (100, 30, 71),
        (50, 0, 60),
        (1000, 500, 400),
    ];

    for (limit, initial_spent, cost) in test_cases.iter() {
        let mut budget = FlowBudget::new(*limit, Epoch::initial());
        budget.spent = *initial_spent;

        let can_charge_result = budget.can_charge(*cost);
        let mut budget_copy = budget;
        let record_charge_result = budget_copy.record_charge(*cost);

        assert_eq!(
            can_charge_result, record_charge_result,
            "can_charge and record_charge must agree"
        );
    }
}

#[cfg(test)]
mod convergence_tests {
    use super::*;

    #[test]
    fn test_distributed_budget_convergence() {
        // Simulate distributed updates converging to consistent state
        let budget_a = FlowBudget {
            limit: 100,
            spent: 30,
            epoch: Epoch::new(5),
        };

        let budget_b = FlowBudget {
            limit: 80,            // More restrictive
            spent: 40,            // Higher spend
            epoch: Epoch::new(6), // Later epoch
        };

        let budget_c = FlowBudget {
            limit: 120,           // Less restrictive
            spent: 20,            // Lower spend
            epoch: Epoch::new(4), // Earlier epoch
        };

        // Test convergence via join operations
        let converged_ab = budget_a.join(&budget_b);
        let converged_abc = converged_ab.join(&budget_c);

        // Alternative order
        let converged_bc = budget_b.join(&budget_c);
        let converged_abc_alt = budget_a.join(&converged_bc);

        // Should converge to same result regardless of order
        assert_eq!(converged_abc, converged_abc_alt);

        // Verify final state has correct CRDT properties
        assert_eq!(converged_abc.limit, 80); // Most restrictive
        assert_eq!(converged_abc.spent, 40); // Highest spend
        assert_eq!(converged_abc.epoch.value(), 6); // Latest epoch
    }

    #[test]
    fn test_epoch_rotation_resets_spent() {
        let mut budget = FlowBudget {
            limit: 100,
            spent: 90,
            epoch: Epoch::new(1),
        };

        // Initially near limit
        assert_eq!(budget.headroom(), 10);
        assert!(!budget.can_charge(50));

        // Rotate to next epoch
        budget.rotate_epoch(Epoch::new(2));

        // Spent should reset, allowing new charges
        assert_eq!(budget.spent, 0);
        assert_eq!(budget.headroom(), 100);
        assert!(budget.can_charge(50));

        // Epoch should advance
        assert_eq!(budget.epoch.value(), 2);
    }

    #[test]
    fn test_epoch_rotation_no_regression() {
        let mut budget = FlowBudget {
            limit: 100,
            spent: 30,
            epoch: Epoch::new(5),
        };

        // Rotating to earlier epoch should not change anything
        budget.rotate_epoch(Epoch::new(3));

        assert_eq!(budget.spent, 30); // Unchanged
        assert_eq!(budget.epoch.value(), 5); // Unchanged

        // Rotating to same epoch should not change anything
        budget.rotate_epoch(Epoch::new(5));

        assert_eq!(budget.spent, 30); // Unchanged
        assert_eq!(budget.epoch.value(), 5); // Unchanged
    }

    #[test]
    fn test_no_observable_without_charge_principle() {
        // This test verifies the principle that all observable actions
        // (like generating receipts) must come from successful budget charges

        let mut budget = FlowBudget::new(50, Epoch::initial());

        // Successful charge within budget
        let success = budget.record_charge(30);
        assert!(success, "Charge within budget should succeed");
        assert_eq!(budget.spent, 30);

        // Failed charge exceeding budget
        let failure = budget.record_charge(30); // 30 + 30 > 50
        assert!(!failure, "Charge exceeding budget should fail");
        assert_eq!(budget.spent, 30); // Spent should remain unchanged

        // Only successful charges should result in observable state changes
        // (In a real system, only successful charges would generate receipts)
    }
}
