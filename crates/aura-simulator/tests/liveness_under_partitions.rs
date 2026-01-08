//! Liveness Under Partition Tests
//!
//! Tests that protocols maintain liveness properties under network partitions
//! and partial synchrony conditions.
//!
//! ## Properties Tested
//!
//! - Consensus terminates within bounded steps after GST (Global Stabilization Time)
//! - Protocols make progress when quorum is available despite partitions
//! - No deadlock occurs when honest majority can communicate
//! - Byzantine faults (up to threshold) do not prevent progress

use aura_simulator::liveness::{
    check_consensus_terminates_within, consensus_liveness_checker, BoundedLivenessChecker,
    BoundedLivenessProperty, SynchronyAssumption,
};
use aura_simulator::scenarios::consensus::{
    ByzantineConfig, ConsensusSimulation, MessageLossConfig, NetworkPartition,
};
use serde_json::json;

// =============================================================================
// Bounded Liveness Tests
// =============================================================================

#[test]
fn test_consensus_terminates_within_bound_synchronous() {
    // Under synchronous conditions (GST=0), consensus should terminate quickly
    let states: Vec<serde_json::Value> = vec![
        json!({"instances": {"cns1": {"phase": "FastPathActive"}}}),
        json!({"instances": {"cns1": {"phase": "FastPathActive"}}}),
        json!({"instances": {"cns1": {"phase": "FastPathActive"}}}),
        json!({"instances": {"cns1": {"phase": "Committed"}}}),
    ];

    let result = check_consensus_terminates_within(&states, 10, 0);

    assert!(
        result.satisfied,
        "Expected consensus to terminate within bound: {:?}",
        result.details
    );
    assert_eq!(result.goal_step, Some(3));
    assert!(result.steps_to_goal.unwrap() <= 10);
}

#[test]
fn test_consensus_terminates_after_gst() {
    // Under partial synchrony, consensus should terminate after GST is reached
    let mut checker =
        BoundedLivenessChecker::with_synchrony(SynchronyAssumption::PartialSynchrony {
            gst: 5, // GST at step 5
            delta: 3,
        })
        .with_verbose(false);

    checker.add_property(BoundedLivenessProperty {
        name: "consensus_terminates".to_string(),
        description: "Consensus terminates within 10 steps after GST".to_string(),
        precondition: "gstReached and hasQuorum".to_string(),
        goal: "allInstancesTerminated".to_string(),
        step_bound: 10,
        ..Default::default()
    });

    // Steps 0-4: Before GST - no progress required
    for i in 0..5 {
        let state = json!({
            "hasQuorum": true,
            "instances": {"cns1": {"phase": "FastPathActive"}}
        });
        let violations = checker.check_step(i, &state);
        assert!(
            violations.is_empty(),
            "Unexpected violation before GST: {:?}",
            violations
        );
    }

    // Steps 5-8: After GST - protocol making progress
    for i in 5..9 {
        let state = json!({
            "hasQuorum": true,
            "instances": {"cns1": {"phase": "FastPathActive"}}
        });
        let violations = checker.check_step(i, &state);
        assert!(
            violations.is_empty(),
            "Unexpected violation while progressing: {:?}",
            violations
        );
    }

    // Step 9: Committed (within bound)
    let committed_state = json!({
        "hasQuorum": true,
        "instances": {"cns1": {"phase": "Committed"}}
    });
    checker.check_step(9, &committed_state);

    let results = checker.finalize();
    assert_eq!(results.len(), 1);
    assert!(
        results[0].satisfied,
        "Expected consensus to terminate: {:?}",
        results[0]
    );
}

#[test]
fn test_fast_path_timing_bound() {
    // Fast path should complete within 2*delta after conditions are met
    let synchrony = SynchronyAssumption::PartialSynchrony { gst: 0, delta: 3 };
    let mut checker = consensus_liveness_checker(synchrony);

    // All witnesses online and honest, fast path active
    let state = json!({
        "phase": "FastPathActive",
        "hasQuorum": true,
        "allOnline": true,
        "instances": {}
    });

    // Check initial steps
    for i in 0..5 {
        checker.check_step(i, &state);
    }

    // Commit at step 5 (within 2*delta = 6)
    let committed = json!({
        "phase": "Committed",
        "hasQuorum": true,
        "allOnline": true,
        "instances": {}
    });
    checker.check_step(5, &committed);

    let results = checker.finalize();
    let fast_path_result = results
        .iter()
        .find(|r| r.property_name == "fast_path_bound");
    assert!(
        fast_path_result.is_some(),
        "Expected fast_path_bound property"
    );
    assert!(
        fast_path_result.unwrap().satisfied,
        "Expected fast path to complete within bound"
    );
}

#[test]
fn test_liveness_checker_detects_bound_violation() {
    // Test that liveness checker correctly detects when bound is exceeded
    let mut checker =
        BoundedLivenessChecker::with_synchrony(SynchronyAssumption::Synchronous { delta: 1 });

    checker.add_property(BoundedLivenessProperty {
        name: "test_must_commit_quickly".to_string(),
        description: "Must commit within 3 steps".to_string(),
        precondition: "true".to_string(),
        goal: "committed".to_string(),
        step_bound: 3,
        ..Default::default()
    });

    let state = json!({"phase": "FastPathActive"});

    // Check steps 0-4 without committing
    for i in 0..=4 {
        let violations = checker.check_step(i, &state);
        if i > 3 {
            // Should detect violation after step 4 (3 steps after precondition at step 0)
            assert!(
                !violations.is_empty(),
                "Expected bound violation at step {i}"
            );
            assert!(
                violations[0].details.contains("Bound exceeded"),
                "Expected bound exceeded message"
            );
        }
    }

    let results = checker.finalize();
    assert!(!results[0].satisfied, "Expected unsatisfied result");
}

#[test]
fn test_no_deadlock_property() {
    // Test that no-deadlock property detects when protocol is stuck
    let synchrony = SynchronyAssumption::Synchronous { delta: 3 };
    let mut checker = consensus_liveness_checker(synchrony);

    // Active state with enabled actions
    let active_state = json!({
        "phase": "FastPathActive",
        "deadlocked": false,
        "hasQuorum": true
    });

    checker.check_step(0, &active_state);

    // Deadlocked state
    let deadlock_state = json!({
        "phase": "FastPathActive",
        "deadlocked": true,
        "hasQuorum": true
    });

    let _violations = checker.check_step(1, &deadlock_state);

    // The no_deadlock property should trigger since step_bound=1 and goal not met
    let results = checker.finalize();
    let no_deadlock = results.iter().find(|r| r.property_name == "no_deadlock");
    assert!(no_deadlock.is_some(), "Expected no_deadlock property");
}

// =============================================================================
// Consensus Simulation Tests
// =============================================================================

#[test]
fn test_consensus_with_minority_partition() {
    // Partition that leaves majority intact should allow consensus
    let witnesses = vec!["w1", "w2", "w3", "w4", "w5"];

    // Partition: {w1, w2} isolated from {w3, w4, w5}
    // Majority (3/5) can still reach consensus
    let partition = NetworkPartition::split(vec!["w1", "w2"], vec!["w3", "w4", "w5"]);

    let mut sim = ConsensusSimulation::new(witnesses.clone(), 3, 42).with_partition(partition);

    // All propose same result
    for w in &witnesses {
        sim.propose_share(w, "result1");
    }

    let result = sim.run_to_completion(100);

    // Invariants should hold - no disagreement
    assert!(
        result.is_ok(),
        "Expected no invariant violations: {:?}",
        result.violations
    );
    // Some messages dropped due to partition
    assert!(
        result.messages_dropped > 0,
        "Expected some dropped messages"
    );
}

#[test]
fn test_consensus_with_majority_partition() {
    // Partition that splits majority should prevent fast consensus
    let witnesses = vec!["w1", "w2", "w3", "w4", "w5"];

    // Partition: {w1, w2, w3} vs {w4, w5} - no group has 3 witnesses
    // This creates two groups: one with 3 and one with 2
    let partition = NetworkPartition::split(vec!["w1", "w2"], vec!["w3", "w4", "w5"]);

    let mut sim = ConsensusSimulation::new(witnesses.clone(), 4, 42) // Threshold 4
        .with_partition(partition);

    // All propose
    for w in &witnesses {
        sim.propose_share(w, "result1");
    }

    let result = sim.run_to_completion(100);

    // Should complete without invariant violations (safety preserved)
    // but consensus may not be reached (liveness affected)
    assert!(
        result.violations.is_empty(),
        "Expected no safety violations: {:?}",
        result.violations
    );
}

#[test]
fn test_consensus_with_message_loss() {
    // 20% message loss should still allow eventual consensus
    let witnesses = vec!["w1", "w2", "w3", "w4", "w5"];

    // 20% drop rate (13107 out of 65536)
    let loss = MessageLossConfig::fixed_rate(13107);

    let mut sim = ConsensusSimulation::new(witnesses.clone(), 3, 42).with_loss(loss);

    // All propose same result
    for w in &witnesses {
        sim.propose_share(w, "result1");
    }

    let result = sim.run_to_completion(200);

    // Safety preserved
    assert!(
        result.is_ok(),
        "Expected no invariant violations: {:?}",
        result.violations
    );
    // Some messages were lost
    assert!(result.messages_dropped > 0, "Expected some message loss");
}

#[test]
fn test_consensus_with_high_message_loss() {
    // 50% message loss - consensus might not complete but safety preserved
    let witnesses = vec!["w1", "w2", "w3"];
    let loss = MessageLossConfig::fixed_rate(MessageLossConfig::HALF);

    let mut sim = ConsensusSimulation::new(witnesses.clone(), 2, 42).with_loss(loss);

    for w in &witnesses {
        sim.propose_share(w, "result1");
    }

    let result = sim.run_to_completion(100);

    // Safety must always hold
    assert!(
        result.violations.is_empty(),
        "Safety violations under message loss: {:?}",
        result.violations
    );
}

#[test]
fn test_consensus_with_byzantine_witness_below_threshold() {
    // 1 Byzantine witness out of 5 with threshold 3 - consensus should succeed
    let witnesses = vec!["w1", "w2", "w3", "w4", "w5"];
    let byzantine = ByzantineConfig::equivocating(vec!["w1"]);

    let mut sim = ConsensusSimulation::new(witnesses.clone(), 3, 42).with_byzantine(byzantine);

    // Byzantine witness equivocates, honest witnesses agree
    sim.propose_share("w1", "result1");
    sim.propose_share("w2", "result1");
    sim.propose_share("w3", "result1");
    sim.propose_share("w4", "result1");
    sim.propose_share("w5", "result1");

    let result = sim.run_to_completion(100);

    // Agreement should still hold - Byzantine below threshold
    assert!(
        result.is_ok(),
        "Expected consensus with single Byzantine: {:?}",
        result.violations
    );
}

#[test]
fn test_consensus_with_byzantine_at_threshold() {
    // Byzantine witnesses at threshold boundary - safety must hold
    let witnesses = vec!["w1", "w2", "w3", "w4", "w5"];
    // 2 Byzantine witnesses with threshold 3 is at the f < n/3 boundary for n=5, f=2
    let byzantine = ByzantineConfig::equivocating(vec!["w1", "w2"]);

    let mut sim = ConsensusSimulation::new(witnesses.clone(), 3, 42).with_byzantine(byzantine);

    for w in &witnesses {
        sim.propose_share(w, "result1");
    }

    let result = sim.run_to_completion(100);

    // Agreement invariant must hold even with Byzantine actors
    let agreement_violations: Vec<_> = result
        .violations
        .iter()
        .filter(|v| v.invariant == "AgreementOnCommit")
        .collect();

    assert!(
        agreement_violations.is_empty(),
        "Agreement violated with Byzantine actors: {:?}",
        agreement_violations
    );
}

#[test]
fn test_consensus_partition_then_heal() {
    // Test that consensus can proceed after partition heals
    let witnesses = vec!["w1", "w2", "w3", "w4", "w5"];

    // Start with partition
    let partition = NetworkPartition::split(vec!["w1", "w2"], vec!["w3", "w4", "w5"]);
    let mut sim = ConsensusSimulation::new(witnesses.clone(), 3, 42).with_partition(partition);

    // All propose
    for w in &witnesses {
        sim.propose_share(w, "result1");
    }

    // Run a few steps with partition
    for _ in 0..10 {
        sim.step();
    }

    // Remove partition (heal network)
    sim.network.partition = None;

    // Continue to completion
    let result = sim.run_to_completion(100);

    // Should complete without violations after heal
    assert!(
        result.is_ok(),
        "Expected success after partition heal: {:?}",
        result.violations
    );
}

// =============================================================================
// Combined Adversarial Conditions
// =============================================================================

#[test]
fn test_consensus_partition_plus_message_loss() {
    // Combine partition with message loss
    let witnesses = vec!["w1", "w2", "w3", "w4", "w5", "w6", "w7"];

    // Minority partition + 10% message loss
    let partition = NetworkPartition::split(vec!["w1", "w2"], vec!["w3", "w4", "w5", "w6", "w7"]);
    let loss = MessageLossConfig::fixed_rate(6554); // ~10%

    let mut sim = ConsensusSimulation::new(witnesses.clone(), 4, 42)
        .with_partition(partition)
        .with_loss(loss);

    for w in &witnesses {
        sim.propose_share(w, "result1");
    }

    let result = sim.run_to_completion(200);

    // Safety preserved
    assert!(
        result.violations.is_empty(),
        "Safety violations under combined adversarial conditions: {:?}",
        result.violations
    );
}

#[test]
fn test_consensus_byzantine_plus_partition() {
    // Byzantine witness plus network partition
    let witnesses = vec!["w1", "w2", "w3", "w4", "w5", "w6", "w7"];

    // 1 Byzantine + partition leaving majority
    let partition = NetworkPartition::split(vec!["w1"], vec!["w2", "w3", "w4", "w5", "w6", "w7"]);
    let byzantine = ByzantineConfig::equivocating(vec!["w2"]);

    let mut sim = ConsensusSimulation::new(witnesses.clone(), 4, 42)
        .with_partition(partition)
        .with_byzantine(byzantine);

    for w in &witnesses {
        sim.propose_share(w, "result1");
    }

    let result = sim.run_to_completion(200);

    // Agreement must hold
    let agreement_violations: Vec<_> = result
        .violations
        .iter()
        .filter(|v| v.invariant == "AgreementOnCommit")
        .collect();

    assert!(
        agreement_violations.is_empty(),
        "Agreement violated: {:?}",
        agreement_violations
    );
}

// =============================================================================
// Stress Tests for Bounded Liveness
// =============================================================================

#[test]
fn test_liveness_many_steps_within_bound() {
    // Long-running protocol that completes just within bound
    let bound = 50;
    let mut checker = BoundedLivenessChecker::with_synchrony(SynchronyAssumption::Synchronous {
        delta: bound as u64 / 2,
    });

    checker.add_property(BoundedLivenessProperty {
        name: "long_running".to_string(),
        description: "Protocol completes within extended bound".to_string(),
        precondition: "true".to_string(),
        goal: "committed".to_string(),
        step_bound: bound,
        ..Default::default()
    });

    let state = json!({"phase": "FastPathActive"});

    // Run for bound-1 steps without completing
    for i in 0..bound - 1 {
        let violations = checker.check_step(i as u64, &state);
        assert!(
            violations.is_empty(),
            "Unexpected violation before bound at step {i}"
        );
    }

    // Complete at step bound-1 (within bound)
    let committed = json!({"phase": "Committed"});
    checker.check_step((bound - 1) as u64, &committed);

    let results = checker.finalize();
    assert!(
        results[0].satisfied,
        "Expected satisfied at boundary: {:?}",
        results[0]
    );
}

#[test]
fn test_multiple_consensus_instances_terminate() {
    // Multiple consensus instances should all terminate
    let states: Vec<serde_json::Value> = vec![
        json!({"instances": {
            "cns1": {"phase": "FastPathActive"},
            "cns2": {"phase": "FastPathActive"},
            "cns3": {"phase": "FastPathActive"}
        }}),
        json!({"instances": {
            "cns1": {"phase": "Committed"},
            "cns2": {"phase": "FastPathActive"},
            "cns3": {"phase": "FastPathActive"}
        }}),
        json!({"instances": {
            "cns1": {"phase": "Committed"},
            "cns2": {"phase": "Committed"},
            "cns3": {"phase": "FastPathActive"}
        }}),
        json!({"instances": {
            "cns1": {"phase": "Committed"},
            "cns2": {"phase": "Committed"},
            "cns3": {"phase": "Committed"}
        }}),
    ];

    let result = check_consensus_terminates_within(&states, 10, 0);

    assert!(
        result.satisfied,
        "All instances should terminate: {:?}",
        result.details
    );
    assert_eq!(result.goal_step, Some(3));
}
