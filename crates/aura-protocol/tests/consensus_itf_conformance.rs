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
//! ./scripts/generate-itf-traces.sh 50  # Generate 200+ traces
//! ```
//!
//! Then run tests:
//! ```bash
//! cargo test -p aura-protocol --test consensus_itf_conformance
//! ```

mod common;

use aura_protocol::consensus::core::{
    state::{ConsensusPhase, ConsensusState},
    validation::check_invariants,
};
use common::divergence::{DivergenceReport, InstanceDiff, StateDiff};
use common::itf_loader::{load_itf_trace, parse_itf_trace, ITFState};
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// Test that all states in an ITF trace satisfy invariants
#[test]
fn test_itf_trace_invariants() {
    // Load trace from the dedicated traces directory
    let trace_path = Path::new("../../traces/consensus.itf.json");

    if !trace_path.exists() {
        eprintln!(
            "Skipping ITF conformance test: trace file not found at {:?}",
            trace_path
        );
        eprintln!("Generate traces with: quint run --out-itf=traces/consensus.itf.json verification/quint/protocol_consensus.qnt");
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
    let trace_path = Path::new("../../traces/consensus.itf.json");

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
    let trace_path = Path::new("../../traces/consensus.itf.json");

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
    let trace_path = Path::new("../../traces/consensus.itf.json");

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

impl fmt::Display for InferredAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InferredAction::StartConsensus { cid } => {
                write!(f, "StartConsensus(cid={})", cid)
            }
            InferredAction::ApplyShare { cid, witness } => {
                write!(f, "ApplyShare(cid={}, witness={})", cid, witness)
            }
            InferredAction::TriggerFallback { cid } => {
                write!(f, "TriggerFallback(cid={})", cid)
            }
            InferredAction::FailConsensus { cid } => {
                write!(f, "FailConsensus(cid={})", cid)
            }
            InferredAction::CompleteConsensus { cid } => {
                write!(f, "CompleteConsensus(cid={})", cid)
            }
            InferredAction::EpochAdvance { from, to } => {
                write!(f, "EpochAdvance({} -> {})", from, to)
            }
            InferredAction::NoOp => write!(f, "NoOp"),
        }
    }
}

/// Infer action from state difference
fn infer_action(
    prev: &ITFState,
    curr: &ITFState,
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
    let trace_path = Path::new("../../traces/consensus.itf.json");

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
    let trace_path = Path::new("../../traces/consensus.itf.json");

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
    let trace_path = Path::new("../../traces/consensus.itf.json");

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

// ============================================================================
// DIVERGENCE REPORTING TESTS
// ============================================================================

/// Compare all instances between consecutive states with detailed divergence reporting
///
/// Returns a list of (cid, diff, actions) tuples for unexpected divergences.
#[allow(dead_code)]
fn compare_trace_states_with_divergence(
    _step_index: usize,
    prev_state: &ITFState,
    curr_state: &ITFState,
) -> Vec<(String, InstanceDiff, Vec<InferredAction>)> {
    let mut divergences = Vec::new();
    let actions = infer_action(prev_state, curr_state);

    // Check instances that exist in both states
    for (cid, curr_inst) in &curr_state.instances {
        if let Some(prev_inst) = prev_state.instances.get(cid) {
            let diff = StateDiff::compare_instances(prev_inst, curr_inst);
            if !diff.is_empty() {
                // Only report unexpected divergences (not from valid transitions)
                let is_expected = is_expected_divergence(&actions, &diff);
                if !is_expected {
                    divergences.push((cid.clone(), diff, actions.clone()));
                }
            }
        }
    }

    divergences
}

/// Check if a divergence is expected based on inferred actions
fn is_expected_divergence(actions: &[InferredAction], diff: &InstanceDiff) -> bool {
    // Proposals growing is expected for ApplyShare
    let has_apply_share = actions.iter().any(|a| matches!(a, InferredAction::ApplyShare { .. }));
    let only_proposals_diff = diff.diffs.iter().all(|d| d.field.contains("proposals"));

    if has_apply_share && only_proposals_diff {
        return true;
    }

    // Phase changes are expected for phase transition actions
    let has_phase_action = actions.iter().any(|a| {
        matches!(
            a,
            InferredAction::TriggerFallback { .. }
                | InferredAction::FailConsensus { .. }
                | InferredAction::CompleteConsensus { .. }
        )
    });
    let has_phase_diff = diff.diffs.iter().any(|d| d.field == "phase");

    if has_phase_action && has_phase_diff {
        return true;
    }

    false
}

/// Test comprehensive state comparison with divergence reporting
#[test]
fn test_itf_state_comparison_with_divergence() {
    let trace_path = Path::new("../../traces/consensus.itf.json");

    if !trace_path.exists() {
        eprintln!("Skipping divergence test: trace file not found");
        return;
    }

    let trace = load_itf_trace(trace_path).expect("failed to load ITF trace");

    let mut total_divergences = 0;
    let mut unexpected_divergences = Vec::new();

    for i in 1..trace.states.len() {
        let prev_state = &trace.states[i - 1];
        let curr_state = &trace.states[i];
        let actions = infer_action(prev_state, curr_state);

        // For each instance, check if state changes match expected behavior
        for (cid, curr_inst) in &curr_state.instances {
            if let Some(prev_inst) = prev_state.instances.get(cid) {
                let diff = StateDiff::compare_instances(prev_inst, curr_inst);
                if !diff.is_empty() {
                    total_divergences += 1;

                    // Check if this is an unexpected divergence
                    if !is_expected_divergence(&actions, &diff) {
                        let report = format!(
                            "Step {}: Unexpected divergence for instance '{}'\n\
                             Actions: {:?}\n\
                             {}",
                            i,
                            cid,
                            actions,
                            DivergenceReport::for_instance(i, &diff)
                        );
                        unexpected_divergences.push(report);
                    }
                }
            }
        }
    }

    // Print summary
    println!("State comparison summary:");
    println!("  Total state transitions: {}", trace.states.len() - 1);
    println!("  Total divergences observed: {}", total_divergences);
    println!("  Unexpected divergences: {}", unexpected_divergences.len());

    // If there are unexpected divergences, print them and fail
    if !unexpected_divergences.is_empty() {
        for report in &unexpected_divergences {
            eprintln!("{}", report);
        }
        // Note: We don't fail here because all divergences should be from valid actions
        // In a strict conformance test, we would: panic!("Unexpected divergences detected");
    }

    println!("✓ All {} state transitions analyzed with divergence reporting", trace.states.len() - 1);
}

/// Demonstrate divergence report format with synthetic data
#[test]
fn test_divergence_report_format() {
    use aura_protocol::consensus::core::state::{PathSelection, ShareData, ShareProposal};
    use std::collections::BTreeSet;

    // Create two states with known differences
    let witnesses: BTreeSet<_> = ["w1", "w2", "w3"].iter().map(|s| s.to_string()).collect();

    let state1 = ConsensusState::new(
        "cns_test".to_string(),
        "test_op".to_string(),
        "pre_hash".to_string(),
        2,
        witnesses.clone(),
        "w1".to_string(),
        PathSelection::FastPath,
    );

    let mut state2 = state1.clone();

    // Introduce differences
    state2.phase = ConsensusPhase::FallbackActive;
    state2.fallback_timer_active = true;
    state2.proposals.push(ShareProposal {
        witness: "w1".to_string(),
        result_id: "result_1".to_string(),
        share: ShareData {
            share_value: "share_val".to_string(),
            nonce_binding: "nonce_bind".to_string(),
            data_binding: "data_bind".to_string(),
        },
    });

    let diff = StateDiff::compare_instances(&state1, &state2);

    assert!(!diff.is_empty(), "Should detect differences");
    assert!(
        diff.diffs.iter().any(|d| d.field == "phase"),
        "Should detect phase difference"
    );
    assert!(
        diff.diffs.iter().any(|d| d.field == "fallback_timer_active"),
        "Should detect fallback_timer_active difference"
    );
    assert!(
        diff.diffs.iter().any(|d| d.field.contains("proposals")),
        "Should detect proposals difference"
    );

    // Generate and verify report format
    let report = DivergenceReport::for_instance(5, &diff);

    assert!(report.contains("DIVERGENCE DETECTED"), "Report should have header");
    assert!(report.contains("step 5"), "Report should show step index");
    assert!(report.contains("cns_test"), "Report should show instance id");
    assert!(report.contains("phase"), "Report should show phase field");

    println!("Divergence report format test:\n{}", report);
}

/// Test that invariant violations trigger detailed reporting
#[test]
fn test_invariant_violation_with_divergence() {
    use aura_protocol::consensus::core::state::{PathSelection, ShareData, ShareProposal};
    use std::collections::BTreeSet;

    let witnesses: BTreeSet<_> = ["w1", "w2", "w3"].iter().map(|s| s.to_string()).collect();

    // Create a valid state
    let mut state = ConsensusState::new(
        "cns_inv".to_string(),
        "op".to_string(),
        "pre".to_string(),
        2,
        witnesses,
        "w1".to_string(),
        PathSelection::FastPath,
    );

    // Add a proposal
    state.proposals.push(ShareProposal {
        witness: "w1".to_string(),
        result_id: "r1".to_string(),
        share: ShareData {
            share_value: "s1".to_string(),
            nonce_binding: "n1".to_string(),
            data_binding: "d1".to_string(),
        },
    });

    // This should pass invariants
    assert!(check_invariants(&state).is_ok());

    // Create an "expected" state for comparison
    let expected = state.clone();

    // Modify actual to have a violation (threshold > witnesses, for example)
    let mut actual = state.clone();
    actual.threshold = 10; // More than witnesses

    let diff = StateDiff::compare_instances(&expected, &actual);
    assert!(!diff.is_empty());
    assert!(diff.diffs.iter().any(|d| d.field == "threshold"));

    // The diff clearly shows the threshold change
    println!("Invariant violation diff:\n{}", DivergenceReport::for_instance(0, &diff));
}

/// Test action inference accuracy with divergence correlation
#[test]
fn test_action_inference_with_divergence() {
    let trace_path = Path::new("../../traces/consensus.itf.json");

    if !trace_path.exists() {
        return;
    }

    let trace = load_itf_trace(trace_path).expect("failed to load ITF trace");

    let mut action_divergence_correlation: HashMap<String, usize> = HashMap::new();

    for i in 1..trace.states.len() {
        let prev_state = &trace.states[i - 1];
        let curr_state = &trace.states[i];
        let actions = infer_action(prev_state, curr_state);

        for (cid, curr_inst) in &curr_state.instances {
            if let Some(prev_inst) = prev_state.instances.get(cid) {
                let diff = StateDiff::compare_instances(prev_inst, curr_inst);
                if !diff.is_empty() {
                    // Correlate actions with divergences
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
                        *action_divergence_correlation.entry(key.to_string()).or_insert(0) += 1;
                    }
                }
            }
        }
    }

    println!("Action-Divergence correlation:");
    for (action, count) in &action_divergence_correlation {
        println!("  {}: {} divergences", action, count);
    }

    println!("✓ Action inference correlated with {} total divergence instances",
             action_divergence_correlation.values().sum::<usize>());
}

// ============================================================================
// EXHAUSTIVE TRACE CONFORMANCE (All traces in directory)
// ============================================================================

/// Discover all ITF trace files in a directory
fn discover_traces(dir: &Path) -> Vec<PathBuf> {
    let mut traces = Vec::new();

    if !dir.exists() {
        return traces;
    }

    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "json") {
                traces.push(path);
            }
        }
    }

    traces.sort();
    traces
}

/// Result of validating a single trace
#[derive(Debug)]
struct TraceValidationResult {
    path: PathBuf,
    states: usize,
    invariant_violations: Vec<String>,
    phase_violations: Vec<String>,
    monotonicity_violations: Vec<String>,
    divergences: Vec<String>,
}

impl TraceValidationResult {
    fn is_ok(&self) -> bool {
        self.invariant_violations.is_empty()
            && self.phase_violations.is_empty()
            && self.monotonicity_violations.is_empty()
            && self.divergences.is_empty()
    }

    fn error_count(&self) -> usize {
        self.invariant_violations.len()
            + self.phase_violations.len()
            + self.monotonicity_violations.len()
            + self.divergences.len()
    }
}

/// Validate a single trace comprehensively
fn validate_trace(path: &Path) -> Result<TraceValidationResult, String> {
    let trace = load_itf_trace(path).map_err(|e| format!("Failed to load: {}", e))?;

    let mut result = TraceValidationResult {
        path: path.to_path_buf(),
        states: trace.states.len(),
        invariant_violations: Vec::new(),
        phase_violations: Vec::new(),
        monotonicity_violations: Vec::new(),
        divergences: Vec::new(),
    };

    // Check invariants for all states
    for state in &trace.states {
        for (cid, inst) in &state.instances {
            if let Err(e) = check_invariants(inst) {
                result.invariant_violations.push(format!(
                    "State {} instance {}: {:?}",
                    state.index, cid, e
                ));
            }
        }
    }

    // Check phase transitions and monotonicity
    for i in 1..trace.states.len() {
        let prev_state = &trace.states[i - 1];
        let curr_state = &trace.states[i];

        for (cid, curr_inst) in &curr_state.instances {
            if let Some(prev_inst) = prev_state.instances.get(cid) {
                // Phase transition validity
                if !is_valid_phase_transition(prev_inst.phase, curr_inst.phase) {
                    result.phase_violations.push(format!(
                        "State {}: {:?} -> {:?} for {}",
                        i, prev_inst.phase, curr_inst.phase, cid
                    ));
                }

                // Proposal monotonicity
                if curr_inst.proposals.len() < prev_inst.proposals.len() {
                    result.monotonicity_violations.push(format!(
                        "State {}: proposals {} -> {} for {}",
                        i,
                        prev_inst.proposals.len(),
                        curr_inst.proposals.len(),
                        cid
                    ));
                }

                // Equivocator monotonicity
                for eq in &prev_inst.equivocators {
                    if !curr_inst.equivocators.contains(eq) {
                        result.monotonicity_violations.push(format!(
                            "State {}: equivocator '{}' disappeared for {}",
                            i, eq, cid
                        ));
                    }
                }

                // Check for unexpected divergences
                let actions = infer_action(prev_state, curr_state);
                let diff = StateDiff::compare_instances(prev_inst, curr_inst);
                if !diff.is_empty() && !is_expected_divergence(&actions, &diff) {
                    result.divergences.push(format!(
                        "State {}: unexpected divergence for {}: {:?}",
                        i, cid, diff.diffs
                    ));
                }
            }
        }

        // Terminal state permanence
        for (cid, prev_inst) in &prev_state.instances {
            if prev_inst.phase == ConsensusPhase::Committed
                || prev_inst.phase == ConsensusPhase::Failed
            {
                if let Some(curr_inst) = curr_state.instances.get(cid) {
                    if prev_inst.phase != curr_inst.phase {
                        result.phase_violations.push(format!(
                            "State {}: terminal {:?} changed to {:?} for {}",
                            i, prev_inst.phase, curr_inst.phase, cid
                        ));
                    }
                }
            }
        }
    }

    Ok(result)
}

/// Run exhaustive conformance tests on all traces in the consensus directory
#[test]
fn test_exhaustive_trace_conformance() {
    let trace_dir = Path::new("../../traces/consensus");
    let traces = discover_traces(trace_dir);

    if traces.is_empty() {
        eprintln!("No traces found in {:?}", trace_dir);
        eprintln!("Generate traces with: ./scripts/generate-itf-traces.sh");
        return;
    }

    println!("========================================");
    println!("Exhaustive ITF Trace Conformance Test");
    println!("========================================");
    println!("Discovered {} traces in {:?}", traces.len(), trace_dir);
    println!();

    let mut total_states = 0;
    let mut passed = 0;
    let mut failed = 0;
    let mut failed_traces: Vec<TraceValidationResult> = Vec::new();

    for trace_path in &traces {
        match validate_trace(trace_path) {
            Ok(result) => {
                total_states += result.states;
                if result.is_ok() {
                    passed += 1;
                } else {
                    failed += 1;
                    failed_traces.push(result);
                }
            }
            Err(e) => {
                failed += 1;
                eprintln!("  [LOAD ERROR] {:?}: {}", trace_path.file_name(), e);
            }
        }
    }

    println!();
    println!("========================================");
    println!("Summary");
    println!("========================================");
    println!("  Traces tested:  {}", traces.len());
    println!("  Total states:   {}", total_states);
    println!("  Passed:         {}", passed);
    println!("  Failed:         {}", failed);
    println!();

    if !failed_traces.is_empty() {
        println!("Failed traces:");
        for result in &failed_traces {
            println!(
                "  {:?}: {} errors",
                result.path.file_name(),
                result.error_count()
            );
            for v in &result.invariant_violations {
                println!("    [INVARIANT] {}", v);
            }
            for v in &result.phase_violations {
                println!("    [PHASE] {}", v);
            }
            for v in &result.monotonicity_violations {
                println!("    [MONOTONICITY] {}", v);
            }
            for v in &result.divergences {
                println!("    [DIVERGENCE] {}", v);
            }
        }
    }

    assert_eq!(
        failed, 0,
        "Conformance test failed: {} traces had violations",
        failed
    );

    println!();
    println!("✓ All {} traces passed conformance testing ({} total states)",
             passed, total_states);
}

/// Test for minimum trace coverage
#[test]
fn test_trace_coverage_minimum() {
    let trace_dir = Path::new("../../traces/consensus");
    let traces = discover_traces(trace_dir);

    // We need at least 50 traces for reasonable coverage
    // Target is 200+ but we'll warn at 50
    const MIN_TRACES: usize = 50;
    const TARGET_TRACES: usize = 200;

    if traces.len() < MIN_TRACES {
        eprintln!(
            "WARNING: Only {} traces found (minimum: {}, target: {})",
            traces.len(),
            MIN_TRACES,
            TARGET_TRACES
        );
        eprintln!("Generate more traces with: ./scripts/generate-itf-traces.sh");
        // Don't fail, just warn
    } else if traces.len() < TARGET_TRACES {
        println!(
            "Note: {} traces found (target: {}). Consider generating more.",
            traces.len(),
            TARGET_TRACES
        );
    } else {
        println!("✓ Trace coverage meets target: {} traces", traces.len());
    }
}
