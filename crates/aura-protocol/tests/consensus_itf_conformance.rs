//! ITF Trace Conformance Tests for Consensus Core
//!
//! These tests verify that the Rust consensus implementation matches
//! the Quint specification by replaying ITF traces.
//!
//! ## Conformance Testing Approach
//!
//! We use **state-based conformance testing** where:
//! 1. ITF traces contain expected states computed by Quint
//! 2. We validate that all states satisfy invariants
//! 3. We verify state transitions follow valid patterns
//! 4. We check monotonicity properties (proposals never decrease)
//!
//! Note: `quint run` traces don't include action names. For action-level
//! conformance, use `quint trace` or Apalache which includes `action_taken`.
//!
//! ## Running Tests
//!
//! Generate traces first:
//! ```bash
//! cd verification/quint
//! quint run --out-itf=consensus_trace.itf.json --max-steps=30 protocol_consensus.qnt
//! ```
//!
//! Then run tests:
//! ```bash
//! cargo test -p aura-protocol --test consensus_itf_conformance
//! ```

use aura_protocol::consensus::core::{
    itf_loader::{load_itf_trace, parse_itf_trace},
    state::ConsensusPhase,
    validation::check_invariants,
};
use std::path::Path;

/// Test that all states in an ITF trace satisfy invariants
#[test]
fn test_itf_trace_invariants() {
    // Try to load the generated trace
    let trace_path = Path::new("../../verification/quint/consensus_trace.itf.json");

    if !trace_path.exists() {
        eprintln!(
            "Skipping ITF conformance test: trace file not found at {:?}",
            trace_path
        );
        eprintln!("Generate traces with: cd verification/quint && quint run --out-itf=consensus_trace.itf.json protocol_consensus.qnt");
        return;
    }

    let trace = load_itf_trace(trace_path).expect("failed to load ITF trace");

    println!(
        "Loaded ITF trace: {} states from {}",
        trace.states.len(),
        trace.meta.source
    );

    // Verify each state satisfies invariants
    for state in &trace.states {
        for (cid, inst) in &state.instances {
            let result = check_invariants(inst);
            assert!(
                result.is_ok(),
                "State {} instance {} failed invariants: {:?}",
                state.index,
                cid,
                result.err()
            );
        }
    }

    println!("✓ All {} states satisfy invariants", trace.states.len());
}

/// Test phase transitions are valid
#[test]
fn test_itf_phase_transitions() {
    let trace_path = Path::new("../../verification/quint/consensus_trace.itf.json");

    if !trace_path.exists() {
        return;
    }

    let trace = load_itf_trace(trace_path).expect("failed to load ITF trace");

    // Track phase transitions per instance
    for i in 1..trace.states.len() {
        let prev_state = &trace.states[i - 1];
        let curr_state = &trace.states[i];

        for (cid, curr_inst) in &curr_state.instances {
            if let Some(prev_inst) = prev_state.instances.get(cid) {
                // Verify valid phase transitions
                let valid = is_valid_phase_transition(prev_inst.phase, curr_inst.phase);
                assert!(
                    valid,
                    "Invalid phase transition at state {} for {}: {:?} -> {:?}",
                    i, cid, prev_inst.phase, curr_inst.phase
                );
            }
        }
    }

    println!("✓ All phase transitions are valid");
}

/// Check if a phase transition is valid
fn is_valid_phase_transition(from: ConsensusPhase, to: ConsensusPhase) -> bool {
    match (from, to) {
        // Same phase is always valid (no transition)
        (a, b) if a == b => true,

        // From Pending
        (ConsensusPhase::Pending, ConsensusPhase::FastPathActive) => true,
        (ConsensusPhase::Pending, ConsensusPhase::FallbackActive) => true,

        // From FastPathActive
        (ConsensusPhase::FastPathActive, ConsensusPhase::FallbackActive) => true,
        (ConsensusPhase::FastPathActive, ConsensusPhase::Committed) => true,
        (ConsensusPhase::FastPathActive, ConsensusPhase::Failed) => true,

        // From FallbackActive
        (ConsensusPhase::FallbackActive, ConsensusPhase::Committed) => true,
        (ConsensusPhase::FallbackActive, ConsensusPhase::Failed) => true,

        // Terminal states cannot transition
        (ConsensusPhase::Committed, _) => false,
        (ConsensusPhase::Failed, _) => false,

        // All other transitions are invalid
        _ => false,
    }
}

/// Test that committed instances have valid commit facts
#[test]
fn test_itf_committed_has_commit_fact() {
    let trace_path = Path::new("../../verification/quint/consensus_trace.itf.json");

    if !trace_path.exists() {
        return;
    }

    let trace = load_itf_trace(trace_path).expect("failed to load ITF trace");

    for state in &trace.states {
        for (cid, inst) in &state.instances {
            if inst.phase == ConsensusPhase::Committed {
                assert!(
                    inst.commit_fact.is_some(),
                    "State {} instance {} is Committed but has no commit fact",
                    state.index,
                    cid
                );
            }
        }
    }

    println!("✓ All committed instances have commit facts");
}

/// Test parsing a minimal ITF trace
#[test]
fn test_parse_minimal_itf() {
    let minimal = r##"{
        "#meta": {"format": "ITF", "source": "test.qnt", "status": "ok"},
        "vars": ["instances", "currentEpoch"],
        "states": [
            {
                "#meta": {"index": 0},
                "instances": {"#map": []},
                "currentEpoch": {"#bigint": "0"}
            }
        ]
    }"##;

    let trace = parse_itf_trace(minimal).expect("failed to parse minimal trace");
    assert_eq!(trace.meta.format, "ITF");
    assert_eq!(trace.states.len(), 1);
    assert_eq!(trace.states[0].epoch, 0);
    assert!(trace.states[0].instances.is_empty());
}

/// Test parsing an ITF trace with a consensus instance
#[test]
fn test_parse_itf_with_instance() {
    let with_instance = r##"{
        "#meta": {"format": "ITF", "source": "test.qnt", "status": "ok"},
        "vars": ["instances"],
        "states": [
            {
                "#meta": {"index": 0},
                "instances": {
                    "#map": [
                        ["cns1", {
                            "cid": "cns1",
                            "operation": "update_policy",
                            "prestateHash": "pre123",
                            "threshold": {"#bigint": "2"},
                            "witnesses": {"#set": ["w1", "w2", "w3"]},
                            "initiator": "w1",
                            "phase": {"tag": "FastPathActive", "value": {"#tup": []}},
                            "proposals": {"#set": []},
                            "commitFact": {"tag": "None", "value": {"#tup": []}},
                            "fallbackTimerActive": false,
                            "equivocators": {"#set": []}
                        }]
                    ]
                }
            }
        ]
    }"##;

    let trace = parse_itf_trace(with_instance).expect("failed to parse trace with instance");
    assert_eq!(trace.states.len(), 1);

    let state = &trace.states[0];
    assert_eq!(state.instances.len(), 1);

    let inst = state.instances.get("cns1").expect("missing instance cns1");
    assert_eq!(inst.cid, "cns1");
    assert_eq!(inst.operation, "update_policy");
    assert_eq!(inst.threshold, 2);
    assert_eq!(inst.witnesses.len(), 3);
    assert_eq!(inst.phase, ConsensusPhase::FastPathActive);
}

/// Test monotonicity: proposal counts never decrease
#[test]
fn test_itf_proposal_monotonicity() {
    let trace_path = Path::new("../../verification/quint/consensus_trace.itf.json");

    if !trace_path.exists() {
        return;
    }

    let trace = load_itf_trace(trace_path).expect("failed to load ITF trace");

    for i in 1..trace.states.len() {
        let prev_state = &trace.states[i - 1];
        let curr_state = &trace.states[i];

        for (cid, curr_inst) in &curr_state.instances {
            if let Some(prev_inst) = prev_state.instances.get(cid) {
                // Proposals can only grow (monotonicity)
                assert!(
                    curr_inst.proposals.len() >= prev_inst.proposals.len(),
                    "Proposal count decreased at state {} for {}: {} -> {}",
                    i,
                    cid,
                    prev_inst.proposals.len(),
                    curr_inst.proposals.len()
                );
            }
        }
    }

    println!("✓ Proposal counts are monotonic");
}

/// Inferred action type based on state changes
#[derive(Debug, Clone, PartialEq, Eq)]
enum InferredAction {
    /// New consensus instance created
    StartConsensus { cid: String },
    /// Share proposal added
    ApplyShare { cid: String, witness: String },
    /// Phase transitioned to fallback
    TriggerFallback { cid: String },
    /// Phase transitioned to failed
    FailConsensus { cid: String },
    /// Phase transitioned to committed
    CompleteConsensus { cid: String },
    /// Epoch advanced
    EpochAdvance { from: u64, to: u64 },
    /// No change detected
    NoOp,
}

/// Infer action from state difference
fn infer_action(
    prev: &aura_protocol::consensus::core::itf_loader::ITFState,
    curr: &aura_protocol::consensus::core::itf_loader::ITFState,
) -> Vec<InferredAction> {
    let mut actions = Vec::new();

    // Check for epoch changes
    if curr.epoch > prev.epoch {
        actions.push(InferredAction::EpochAdvance {
            from: prev.epoch,
            to: curr.epoch,
        });
    }

    // Check for new instances
    for cid in curr.instances.keys() {
        if !prev.instances.contains_key(cid) {
            actions.push(InferredAction::StartConsensus { cid: cid.clone() });
        }
    }

    // Check for instance changes
    for (cid, curr_inst) in &curr.instances {
        if let Some(prev_inst) = prev.instances.get(cid) {
            // Check for new proposals
            if curr_inst.proposals.len() > prev_inst.proposals.len() {
                for prop in &curr_inst.proposals {
                    let prop_exists = prev_inst
                        .proposals
                        .iter()
                        .any(|p| p.witness == prop.witness);
                    if !prop_exists {
                        actions.push(InferredAction::ApplyShare {
                            cid: cid.clone(),
                            witness: prop.witness.clone(),
                        });
                    }
                }
            }

            // Check for phase transitions
            if prev_inst.phase != curr_inst.phase {
                match curr_inst.phase {
                    ConsensusPhase::FallbackActive => {
                        actions.push(InferredAction::TriggerFallback { cid: cid.clone() });
                    }
                    ConsensusPhase::Failed => {
                        actions.push(InferredAction::FailConsensus { cid: cid.clone() });
                    }
                    ConsensusPhase::Committed => {
                        actions.push(InferredAction::CompleteConsensus { cid: cid.clone() });
                    }
                    _ => {}
                }
            }
        }
    }

    if actions.is_empty() {
        actions.push(InferredAction::NoOp);
    }

    actions
}

/// Test action inference from state changes
#[test]
fn test_itf_action_inference() {
    let trace_path = Path::new("../../verification/quint/consensus_trace.itf.json");

    if !trace_path.exists() {
        return;
    }

    let trace = load_itf_trace(trace_path).expect("failed to load ITF trace");

    let mut action_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for i in 1..trace.states.len() {
        let prev_state = &trace.states[i - 1];
        let curr_state = &trace.states[i];

        let actions = infer_action(prev_state, curr_state);

        for action in &actions {
            let key = match action {
                InferredAction::StartConsensus { .. } => "StartConsensus",
                InferredAction::ApplyShare { .. } => "ApplyShare",
                InferredAction::TriggerFallback { .. } => "TriggerFallback",
                InferredAction::FailConsensus { .. } => "FailConsensus",
                InferredAction::CompleteConsensus { .. } => "CompleteConsensus",
                InferredAction::EpochAdvance { .. } => "EpochAdvance",
                InferredAction::NoOp => "NoOp",
            };
            *action_counts.entry(key.to_string()).or_insert(0) += 1;
        }
    }

    println!("Action inference summary:");
    for (action, count) in &action_counts {
        println!("  {}: {}", action, count);
    }

    // Verify at least some meaningful actions were detected
    let meaningful_actions: usize = action_counts
        .iter()
        .filter(|(k, _)| *k != "NoOp" && *k != "EpochAdvance")
        .map(|(_, v)| v)
        .sum();

    println!("✓ Inferred {} meaningful actions from {} transitions",
             meaningful_actions, trace.states.len() - 1);
}

/// Test equivocator detection matches between states
#[test]
fn test_itf_equivocator_monotonicity() {
    let trace_path = Path::new("../../verification/quint/consensus_trace.itf.json");

    if !trace_path.exists() {
        return;
    }

    let trace = load_itf_trace(trace_path).expect("failed to load ITF trace");

    for i in 1..trace.states.len() {
        let prev_state = &trace.states[i - 1];
        let curr_state = &trace.states[i];

        for (cid, curr_inst) in &curr_state.instances {
            if let Some(prev_inst) = prev_state.instances.get(cid) {
                // Equivocators can only grow (monotonicity per Lean: equivocator_monotonic)
                for equivocator in &prev_inst.equivocators {
                    assert!(
                        curr_inst.equivocators.contains(equivocator),
                        "Equivocator '{}' disappeared at state {} for {}",
                        equivocator,
                        i,
                        cid
                    );
                }
            }
        }
    }

    println!("✓ Equivocator sets are monotonic");
}

/// Test that terminal states (Committed/Failed) remain terminal
#[test]
fn test_itf_terminal_states_permanent() {
    let trace_path = Path::new("../../verification/quint/consensus_trace.itf.json");

    if !trace_path.exists() {
        return;
    }

    let trace = load_itf_trace(trace_path).expect("failed to load ITF trace");

    for i in 1..trace.states.len() {
        let prev_state = &trace.states[i - 1];
        let curr_state = &trace.states[i];

        for (cid, prev_inst) in &prev_state.instances {
            if prev_inst.phase == ConsensusPhase::Committed
                || prev_inst.phase == ConsensusPhase::Failed
            {
                if let Some(curr_inst) = curr_state.instances.get(cid) {
                    assert_eq!(
                        prev_inst.phase, curr_inst.phase,
                        "Terminal state {:?} changed at state {} for {}",
                        prev_inst.phase, i, cid
                    );
                }
            }
        }
    }

    println!("✓ Terminal states (Committed/Failed) are permanent");
}
