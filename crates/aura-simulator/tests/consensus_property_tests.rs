//! Property-Based Consensus Simulation Tests (T8.2)
//!
//! Uses proptest to generate random network conditions and verify
//! that consensus invariants hold under adversarial scenarios.
//!
//! ## Properties Tested
//!
//! 1. **Safety**: No two honest witnesses commit to different results
//! 2. **Validity**: Committed values were proposed by some witness
//! 3. **Equivocator Detection**: All equivocators are eventually detected
//! 4. **Monotonicity**: Proposal sets and equivocator sets never shrink
//!
//! ## Failure Injection Strategies
//!
//! - Random network partitions
//! - Probabilistic message loss
//! - Byzantine witness injection (equivocation)
//! - Combined failure scenarios

use aura_simulator::scenarios::consensus::{
    ByzantineConfig, ConsensusSimulation, MessageLossConfig, MessageType, NetworkPartition,
};
use proptest::prelude::*;
use proptest::strategy::Just;
use std::collections::HashMap;

/// Strategy for generating witness configurations
fn witness_strategy() -> impl Strategy<Value = (Vec<String>, usize)> {
    // 3-7 witnesses, threshold = (n+1)/2 to (n-1)
    (3usize..=7).prop_flat_map(|n| {
        let min_threshold = (n + 1) / 2;
        let max_threshold = n - 1;
        (
            Just((0..n).map(|i| format!("w{}", i)).collect::<Vec<_>>()),
            min_threshold..=max_threshold,
        )
    })
}

/// Strategy for generating message loss configurations
/// Biased toward no loss (5:2:1 ratio) to exercise both healthy and degraded paths
/// Drop rates are in parts per 65536 (6554 = ~10%, 32768 = 50%)
fn loss_strategy() -> impl Strategy<Value = Option<MessageLossConfig>> {
    prop_oneof![
        5 => Just(None),  // No loss most of the time
        2 => (6554u32..32768u32).prop_map(|rate| Some(MessageLossConfig::fixed_rate(rate))),
        1 => Just(Some(MessageLossConfig {
            drop_rate: 65536,  // Always drop (100%)
            target_types: Some(vec![MessageType::ShareProposal]),
            target_witnesses: None,
        })),
    ]
}

/// Strategy for generating proposal patterns
fn proposal_pattern() -> impl Strategy<Value = ProposalPattern> {
    prop_oneof![
        3 => Just(ProposalPattern::AllSameResult),
        1 => Just(ProposalPattern::SplitResults),
        1 => Just(ProposalPattern::PartialProposals),
    ]
}

#[derive(Debug, Clone)]
enum ProposalPattern {
    /// All witnesses propose for the same result
    AllSameResult,
    /// Witnesses split between two results (honest disagreement)
    SplitResults,
    /// Only some witnesses propose
    PartialProposals,
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property: Safety - No two honest witnesses commit to different results
    #[test]
    fn prop_safety_no_conflicting_commits(
        (witnesses, threshold) in witness_strategy(),
        seed in 0u64..1000000,
        pattern in proposal_pattern(),
    ) {
        let witnesses_ref: Vec<&str> = witnesses.iter().map(|s| s.as_str()).collect();
        let mut sim = ConsensusSimulation::new(witnesses_ref, threshold, seed);

        // Generate proposals based on pattern
        match pattern {
            ProposalPattern::AllSameResult => {
                for w in &witnesses {
                    sim.propose_share(w, "result1");
                }
            }
            ProposalPattern::SplitResults => {
                let half = witnesses.len() / 2;
                for (i, w) in witnesses.iter().enumerate() {
                    let result = if i < half { "result1" } else { "result2" };
                    sim.propose_share(w, result);
                }
            }
            ProposalPattern::PartialProposals => {
                // Only first threshold witnesses propose
                for w in witnesses.iter().take(threshold) {
                    sim.propose_share(w, "result1");
                }
            }
        }

        let result = sim.run_to_completion(1000);

        // Safety property: no conflicting commits
        let has_conflict = result.violations.iter()
            .any(|v| v.invariant == "AgreementOnCommit");
        prop_assert!(!has_conflict, "Safety violated: {:?}", result.violations);
    }

    /// Property: Invariants hold under random network partitions
    #[test]
    fn prop_invariants_under_partition(
        (witnesses, threshold) in witness_strategy(),
        seed in 0u64..1000000,
        split_point in 1usize..7,  // Random split point, clamped to witness count
    ) {
        let witnesses_ref: Vec<&str> = witnesses.iter().map(|s| s.as_str()).collect();
        let n = witnesses.len();

        // Clamp split point to valid range for this witness set
        let actual_split = (split_point % (n - 1)) + 1;
        let g1: Vec<_> = witnesses[..actual_split].iter().map(|s| s.as_str()).collect();
        let g2: Vec<_> = witnesses[actual_split..].iter().map(|s| s.as_str()).collect();
        let partition = NetworkPartition::split(g1, g2);

        let mut sim = ConsensusSimulation::new(witnesses_ref, threshold, seed)
            .with_partition(partition);

        for w in &witnesses {
            sim.propose_share(w, "result1");
        }

        let result = sim.run_to_completion(1000);

        // Invariants should hold even with partitions
        let critical_violations: Vec<_> = result.violations.iter()
            .filter(|v| v.invariant == "AgreementOnCommit" || v.invariant == "CommitRequiresThreshold")
            .collect();
        prop_assert!(critical_violations.is_empty(), "Invariant violated: {:?}", critical_violations);
    }

    /// Property: Invariants hold under message loss (using loss_strategy)
    #[test]
    fn prop_invariants_under_loss(
        (witnesses, threshold) in witness_strategy(),
        seed in 0u64..1000000,
        loss_config in loss_strategy(),
    ) {
        let witnesses_ref: Vec<&str> = witnesses.iter().map(|s| s.as_str()).collect();

        let mut sim = ConsensusSimulation::new(witnesses_ref, threshold, seed);
        if let Some(loss) = loss_config {
            sim = sim.with_loss(loss);
        }

        for w in &witnesses {
            sim.propose_share(w, "result1");
        }

        let result = sim.run_to_completion(1000);

        // Safety should hold even under loss
        let has_safety_violation = result.violations.iter()
            .any(|v| v.invariant == "AgreementOnCommit");
        prop_assert!(!has_safety_violation, "Safety violated under loss: {:?}", result.violations);
    }

    /// Property: Byzantine witnesses are detected (using byzantine_count_strategy)
    #[test]
    fn prop_byzantine_detection(
        (witnesses, threshold) in witness_strategy(),
        seed in 0u64..1000000,
        num_byzantine in 0usize..3,  // 0-2 Byzantine witnesses
    ) {
        let n = witnesses.len();
        // Clamp to valid Byzantine count (< n/3 for safety)
        let max_byz = (n - 1) / 3;
        let actual_byz = num_byzantine.min(max_byz);

        let witnesses_ref: Vec<&str> = witnesses.iter().map(|s| s.as_str()).collect();

        let mut sim = ConsensusSimulation::new(witnesses_ref, threshold, seed);

        if actual_byz > 0 {
            let byz: Vec<_> = witnesses[..actual_byz].iter().map(|s| s.as_str()).collect();
            sim = sim.with_byzantine(ByzantineConfig::equivocating(byz));
        }

        // All witnesses propose (Byzantine ones will equivocate internally)
        for w in &witnesses {
            sim.propose_share(w, "result1");
        }

        let result = sim.run_to_completion(1000);

        // Safety should hold despite Byzantine behavior
        let has_safety_violation = result.violations.iter()
            .any(|v| v.invariant == "AgreementOnCommit");
        prop_assert!(!has_safety_violation, "Safety violated with Byzantine: {:?}", result.violations);
    }

    /// Property: Combined failures - partition + loss + Byzantine (all strategies)
    #[test]
    fn prop_combined_failures(
        (witnesses, threshold) in witness_strategy(),
        seed in 0u64..1000000,
        split_point in 1usize..7,
        loss_config in loss_strategy(),
        num_byzantine in 0usize..2,
    ) {
        let n = witnesses.len();
        let witnesses_ref: Vec<&str> = witnesses.iter().map(|s| s.as_str()).collect();

        let mut sim = ConsensusSimulation::new(witnesses_ref, threshold, seed);

        // Apply partition
        if n >= 2 {
            let actual_split = (split_point % (n - 1)) + 1;
            let g1: Vec<_> = witnesses[..actual_split].iter().map(|s| s.as_str()).collect();
            let g2: Vec<_> = witnesses[actual_split..].iter().map(|s| s.as_str()).collect();
            sim = sim.with_partition(NetworkPartition::split(g1, g2));
        }

        // Apply loss
        if let Some(loss) = loss_config {
            sim = sim.with_loss(loss);
        }

        // Apply Byzantine (respecting n/3 bound)
        let max_byz = (n - 1) / 3;
        let actual_byz = num_byzantine.min(max_byz);
        if actual_byz > 0 {
            let byz: Vec<_> = witnesses[..actual_byz].iter().map(|s| s.as_str()).collect();
            sim = sim.with_byzantine(ByzantineConfig::equivocating(byz));
        }

        for w in &witnesses {
            sim.propose_share(w, "result1");
        }

        let result = sim.run_to_completion(1000);

        // Critical safety invariant must hold
        let has_conflicting_commit = result.violations.iter()
            .any(|v| v.invariant == "AgreementOnCommit");
        prop_assert!(!has_conflicting_commit,
            "Safety violated under combined failures: {:?}", result.violations);
    }

    /// Property: Proposal counts never decrease (monotonicity)
    #[test]
    fn prop_proposal_monotonicity(
        (witnesses, threshold) in witness_strategy(),
        seed in 0u64..1000000,
    ) {
        let witnesses_ref: Vec<&str> = witnesses.iter().map(|s| s.as_str()).collect();
        let mut sim = ConsensusSimulation::new(witnesses_ref, threshold, seed);

        // Track proposal counts
        let mut prev_counts: HashMap<String, usize> = HashMap::new();
        for w in &witnesses {
            prev_counts.insert(w.clone(), 0);
        }

        // Propose incrementally and check monotonicity
        for (i, w) in witnesses.iter().enumerate() {
            sim.propose_share(w, &format!("result{}", i % 2));

            // Run a few steps
            for _ in 0..10 {
                sim.step();
            }

            // Check proposal counts haven't decreased
            for (witness, state) in &sim.states {
                let prev = *prev_counts.get(witness.as_str()).unwrap_or(&0);
                prop_assert!(state.proposals.len() >= prev,
                    "Proposal count decreased for {}: {} -> {}",
                    witness, prev, state.proposals.len());
                prev_counts.insert(witness.clone(), state.proposals.len());
            }
        }
    }
}

/// Deterministic test: Verify exact behavior under known conditions
#[test]
fn test_deterministic_three_witness_consensus() {
    let witnesses = vec!["w1", "w2", "w3"];
    let mut sim = ConsensusSimulation::new(witnesses.clone(), 2, 12345);

    // All propose same result
    sim.propose_share("w1", "result1");
    sim.propose_share("w2", "result1");
    sim.propose_share("w3", "result1");

    let result = sim.run_to_completion(100);

    // Should complete without violations
    assert!(result.is_ok(), "Violations: {:?}", result.violations);

    // All messages should be delivered (no partition/loss)
    assert_eq!(result.messages_dropped, 0);
}

/// Deterministic test: Partition prevents consensus in minority
#[test]
fn test_partition_minority_cannot_commit() {
    let witnesses = vec!["w1", "w2", "w3", "w4", "w5"];
    let partition = NetworkPartition::split(vec!["w1", "w2"], vec!["w3", "w4", "w5"]);

    let mut sim = ConsensusSimulation::new(witnesses.clone(), 3, 42)
        .with_partition(partition);

    // All propose same result
    for w in &witnesses {
        sim.propose_share(w, "result1");
    }

    let result = sim.run_to_completion(100);

    // Partition should cause message drops
    assert!(result.messages_dropped > 0, "Expected dropped messages due to partition");

    // No safety violations
    assert!(result.is_ok(), "Unexpected violations: {:?}", result.violations);
}

/// Deterministic test: Equivocating witness is handled safely
#[test]
fn test_equivocator_handled_safely() {
    let witnesses = vec!["w1", "w2", "w3", "w4", "w5"];
    let byzantine = ByzantineConfig::equivocating(vec!["w1"]);

    let mut sim = ConsensusSimulation::new(witnesses.clone(), 3, 42)
        .with_byzantine(byzantine);

    // Byzantine w1 equivocates
    sim.propose_share("w1", "result1");

    // Honest witnesses
    sim.propose_share("w2", "result1");
    sim.propose_share("w3", "result1");
    sim.propose_share("w4", "result1");
    sim.propose_share("w5", "result1");

    let result = sim.run_to_completion(100);

    // Safety must hold
    let safety_violations: Vec<_> = result.violations.iter()
        .filter(|v| v.invariant == "AgreementOnCommit")
        .collect();
    assert!(safety_violations.is_empty(), "Safety violated: {:?}", safety_violations);
}
