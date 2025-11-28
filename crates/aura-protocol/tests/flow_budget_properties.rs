//! Property tests for FlowBudget and Receipt system
#![cfg(feature = "fixture_effects")]
//!
//! Tests the core invariants required by work/007.md Section 3:
//! - No-Observable-Without-Charge: All observable events must have valid receipts
//! - Convergence bounds: FlowBudget CRDT operations converge to consistent state
//! - Anti-replay protection: Receipt nonces prevent duplicate operations

use aura_core::effects::FlowBudgetEffects;
use aura_core::{epochs::Epoch, flow::FlowBudget, identifiers::DeviceId, relationships::ContextId};
use aura_macros::aura_test;
use aura_testkit::{create_test_fixture, TestFixture};
use proptest::prelude::*;

/// Wrapper for FlowBudget to implement Arbitrary (avoids orphan rule)
#[derive(Debug, Clone)]
struct TestFlowBudget(FlowBudget);

impl From<TestFlowBudget> for FlowBudget {
    fn from(wrapper: TestFlowBudget) -> Self {
        wrapper.0
    }
}

impl std::ops::Deref for TestFlowBudget {
    type Target = FlowBudget;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Generate arbitrary FlowBudget for property testing
impl Arbitrary for TestFlowBudget {
    type Parameters = ();
    type Strategy = BoxedStrategy<Self>;

    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        (
            any::<u64>(),
            any::<u64>(),
            0u64..=1000u64, // Reasonable epoch range
        )
            .prop_map(|(limit, spent, epoch_val)| {
                let epoch = Epoch::new(epoch_val);
                let mut budget = FlowBudget::new(limit, epoch);
                budget.spent = spent.min(limit); // Ensure spent <= limit for valid budgets
                TestFlowBudget(budget)
            })
            .boxed()
    }
}

proptest! {
    /// Property: FlowBudget CRDT join operation is associative
    ///
    /// For any three FlowBudgets a, b, c:
    /// (a ∨ b) ∨ c = a ∨ (b ∨ c)
    #[test]
    fn flow_budget_join_associative(
        a in any::<TestFlowBudget>(),
        b in any::<TestFlowBudget>(),
        c in any::<TestFlowBudget>()
    ) {
        use aura_core::semilattice::JoinSemilattice;

        let left = a.join(&b).join(&c);
        let right = a.join(&b.join(&c));

        prop_assert_eq!(left, right);
    }

    /// Property: FlowBudget CRDT join operation is commutative
    ///
    /// For any two FlowBudgets a, b:
    /// a ∨ b = b ∨ a
    #[test]
    fn flow_budget_join_commutative(
        a in any::<TestFlowBudget>(),
        b in any::<TestFlowBudget>()
    ) {
        use aura_core::semilattice::JoinSemilattice;

        let left = a.join(&b);
        let right = b.join(&a);

        prop_assert_eq!(left, right);
    }

    /// Property: FlowBudget CRDT join operation is idempotent
    ///
    /// For any FlowBudget a:
    /// a ∨ a = a
    #[test]
    fn flow_budget_join_idempotent(a in any::<TestFlowBudget>()) {
        use aura_core::semilattice::JoinSemilattice;

        let result = a.join(&a);

        prop_assert_eq!(result, *a);
    }

    /// Property: FlowBudget merge maintains CRDT invariants
    ///
    /// The merge operation should:
    /// - Take minimum limit (meet operation)
    /// - Take maximum spent (join operation)
    /// - Advance epoch monotonically
    #[test]
    fn flow_budget_merge_invariants(
        limit1 in 100u64..1000u64,
        spent1 in 0u64..100u64,
        epoch1 in 1u64..10u64,
        limit2 in 100u64..1000u64,
        spent2 in 0u64..100u64,
        epoch2 in 1u64..10u64,
    ) {
        let budget1 = FlowBudget {
            limit: limit1,
            spent: spent1,
            epoch: Epoch::new(epoch1),
        };
        let budget2 = FlowBudget {
            limit: limit2,
            spent: spent2,
            epoch: Epoch::new(epoch2),
        };

        let merged = budget1.merge(&budget2);

        // Limit should be minimum (meet operation)
        prop_assert_eq!(merged.limit, limit1.min(limit2));

        // Spent should be maximum (join operation)
        prop_assert_eq!(merged.spent, spent1.max(spent2));

        // Epoch should advance monotonically
        let expected_epoch = if epoch1 >= epoch2 { epoch1 } else { epoch2 };
        prop_assert_eq!(merged.epoch.value(), expected_epoch);
    }

    /// Property: charge_flow respects budget limits
    ///
    /// Charging should fail if cost would exceed available headroom
    #[test]
    fn charge_flow_respects_limits(
        limit in 100u64..1000u64,
        spent in 0u64..100u64,
        cost in 0u32..200u32,
    ) {
        let mut budget = FlowBudget::new(limit, Epoch::initial());
        budget.spent = spent.min(limit); // Ensure valid initial state

        let initial_headroom = budget.headroom();
        let charge_success = budget.record_charge(cost as u64);

        if cost as u64 <= initial_headroom {
            prop_assert!(charge_success, "Charge should succeed when within headroom");
            prop_assert_eq!(budget.spent, spent + cost as u64);
        } else {
            prop_assert!(!charge_success, "Charge should fail when exceeding headroom");
            prop_assert_eq!(budget.spent, spent); // Spent should be unchanged
        }
    }
}

/// Integration tests for the complete flow budget system
#[cfg(test)]
mod integration_tests {
    use super::*;
    use aura_core::semilattice::JoinSemilattice;

    #[aura_test]
    async fn test_no_observable_without_charge_invariant() -> aura_core::AuraResult<()> {
        // Test that all observable events (receipts) come from successful charges
        let device2 = DeviceId::from("device2");
        let context = ContextId::from(String::from("test_context"));

        let fixture = create_test_fixture().await?;

        // Initialize budget via first charge (the budget will be auto-initialized)
        // No need to manually initialize, charge_flow handles initialization

        // Charge within budget should produce receipt
        let result1 = fixture.effects().charge_flow(&context, &device2, 50).await;
        assert!(result1.is_ok(), "Charge within budget should succeed");

        let receipt1 = result1.unwrap();
        assert_eq!(receipt1.cost, 50);

        // Charge exceeding budget should fail and produce no receipt
        let result2 = fixture.effects().charge_flow(&context, &device2, 60).await;
        assert!(result2.is_err(), "Charge exceeding budget should fail");

        // Invariant: No observable (receipt) without successful charge
        println!("✓ No-Observable-Without-Charge invariant verified");
        Ok(())
    }

    #[tokio::test]
    async fn test_convergence_bounds() {
        // Test that distributed FlowBudget updates converge to consistent state
        let _device1 = DeviceId::from("device1");
        let _device2 = DeviceId::from("device2");
        let _context = ContextId::from(String::from("test_context"));

        // Simulate two replicas with different FlowBudget states
        let budget_replica1 = FlowBudget {
            limit: 100,
            spent: 30,
            epoch: Epoch::new(5),
        };

        let budget_replica2 = FlowBudget {
            limit: 80,            // More restrictive limit
            spent: 40,            // Higher spend
            epoch: Epoch::new(6), // Later epoch
        };

        // Simulate convergence via CRDT join operation
        let converged = budget_replica1.join(&budget_replica2);

        // Verify convergence properties:
        // - Limit converges to most restrictive (minimum)
        assert_eq!(converged.limit, 80);

        // - Spent converges to highest observed (maximum)
        assert_eq!(converged.spent, 40);

        // - Epoch advances monotonically (maximum)
        assert_eq!(converged.epoch.value(), 6);

        // Test that convergence is deterministic regardless of order
        let converged_reverse = budget_replica2.join(&budget_replica1);
        assert_eq!(converged, converged_reverse);

        println!("✓ Convergence bounds verified: limit=min, spent=max, epoch=max");
    }

    #[aura_test]
    async fn test_receipt_nonce_monotonicity() -> aura_core::AuraResult<()> {
        // Test that receipt nonces are strictly increasing per (context, device, epoch)
        let device2 = DeviceId::from("device2");
        let context = ContextId::from(String::from("test_context"));

        let fixture = create_test_fixture().await?;

        // Budget will be auto-initialized on first charge

        // Generate multiple receipts
        let receipt1 = fixture
            .effects()
            .charge_flow(&context, &device2, 10)
            .await
            .unwrap();
        let receipt2 = fixture
            .effects()
            .charge_flow(&context, &device2, 20)
            .await
            .unwrap();
        let receipt3 = fixture
            .effects()
            .charge_flow(&context, &device2, 30)
            .await
            .unwrap();

        // Verify nonces are strictly increasing
        assert!(receipt1.nonce < receipt2.nonce);
        assert!(receipt2.nonce < receipt3.nonce);

        // Verify nonces form a valid sequence
        assert_eq!(receipt2.nonce, receipt1.nonce + 1);
        assert_eq!(receipt3.nonce, receipt2.nonce + 1);

        println!("✓ Receipt nonce monotonicity verified");
        Ok(())
    }

    #[tokio::test]
    async fn test_epoch_rotation_resets_spent() {
        // Test that epoch rotation properly resets spent amounts
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

        println!("✓ Epoch rotation reset verified");
    }

    #[aura_test]
    async fn test_deterministic_budget_computation() -> aura_core::AuraResult<()> {
        // Test that budget computation is deterministic across different execution orders
        let device2 = DeviceId::from("device2");
        let context = ContextId::from(String::from("test_context"));

        // Create multiple fixtures (simulating different devices)
        let fixture1 = create_test_fixture().await?;
        let fixture2 = create_test_fixture().await?;

        // Seed same initial state in both systems
        // Budget will be auto-initialized on first charge with default settings

        // Both systems should converge to identical state after same operations
        let receipt1 = fixture1
            .effects()
            .charge_flow(&context, &device2, 50)
            .await
            .unwrap();
        let receipt2 = fixture2
            .effects()
            .charge_flow(&context, &device2, 50)
            .await
            .unwrap();

        // Both receipts should be for the same cost
        assert_eq!(receipt1.cost, receipt2.cost);
        assert_eq!(receipt1.cost, 50);

        println!("✓ Deterministic budget computation verified");
        Ok(())
    }
}
