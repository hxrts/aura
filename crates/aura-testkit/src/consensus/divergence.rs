//! Divergence Reporting for Conformance Testing
//!
//! Provides detailed state diff reporting when ITF trace conformance tests fail.
//! This module helps identify exactly which fields diverged and why.
//!
//! ## Usage
//!
//! ```ignore
//! let diff = StateDiff::compare(&expected, &actual);
//! if !diff.is_empty() {
//!     println!("{}", DivergenceReport::new(step_index, &diff, action));
//! }
//! ```
//!
//! ## Divergence Format
//!
//! ```text
//! ╔══════════════════════════════════════════════════════════════════════╗
//! ║ DIVERGENCE DETECTED at step 5                                        ║
//! ║ Inferred action: ApplyShare { cid: "cns1", witness: "w2" }           ║
//! ╠══════════════════════════════════════════════════════════════════════╣
//! ║ Instance: cns1                                                       ║
//! ╠──────────────────────────────────────────────────────────────────────╣
//! ║ Field          │ Expected              │ Actual                      ║
//! ╠────────────────┼───────────────────────┼─────────────────────────────╣
//! ║ phase          │ FastPathActive        │ FallbackActive              ║
//! ║ proposals.len  │ 2                     │ 1                           ║
//! ╚══════════════════════════════════════════════════════════════════════╝
//! ```

use aura_consensus::core::state::PureCommitFact;
use aura_consensus::core::{ConsensusState, ShareProposal};
use std::collections::BTreeSet;
use std::fmt;

/// A single field difference between expected and actual state
#[derive(Debug, Clone)]
pub struct FieldDiff {
    /// Name of the field (dot-notation for nested fields)
    pub field: String,
    /// Expected value as string
    pub expected: String,
    /// Actual value as string
    pub actual: String,
}

impl FieldDiff {
    pub fn new(
        field: impl Into<String>,
        expected: impl fmt::Debug,
        actual: impl fmt::Debug,
    ) -> Self {
        Self {
            field: field.into(),
            expected: format!("{:?}", expected),
            actual: format!("{:?}", actual),
        }
    }

    pub fn from_strings(
        field: impl Into<String>,
        expected: impl Into<String>,
        actual: impl Into<String>,
    ) -> Self {
        Self {
            field: field.into(),
            expected: expected.into(),
            actual: actual.into(),
        }
    }
}

/// Collection of field differences for a single instance
#[derive(Debug, Clone)]
pub struct InstanceDiff {
    /// Consensus instance ID
    pub cid: String,
    /// List of field differences
    pub diffs: Vec<FieldDiff>,
}

impl InstanceDiff {
    pub fn new(cid: impl Into<String>) -> Self {
        Self {
            cid: cid.into(),
            diffs: Vec::new(),
        }
    }

    pub fn add(&mut self, diff: FieldDiff) {
        self.diffs.push(diff);
    }

    pub fn is_empty(&self) -> bool {
        self.diffs.is_empty()
    }
}

/// Complete state diff between expected and actual states
#[derive(Debug, Clone, Default)]
pub struct StateDiff {
    /// Differences in global fields (epoch, etc.)
    pub global_diffs: Vec<FieldDiff>,
    /// Differences per instance
    pub instance_diffs: Vec<InstanceDiff>,
    /// Instances present in expected but not actual
    pub missing_instances: Vec<String>,
    /// Instances present in actual but not expected
    pub extra_instances: Vec<String>,
}

impl StateDiff {
    /// Compare two ConsensusState instances and return differences
    pub fn compare_instances(expected: &ConsensusState, actual: &ConsensusState) -> InstanceDiff {
        let mut diff = InstanceDiff::new(&expected.cid);

        // Compare scalar fields
        if expected.cid != actual.cid {
            diff.add(FieldDiff::new("cid", &expected.cid, &actual.cid));
        }

        if expected.operation != actual.operation {
            diff.add(FieldDiff::new(
                "operation",
                &expected.operation,
                &actual.operation,
            ));
        }

        if expected.prestate_hash != actual.prestate_hash {
            diff.add(FieldDiff::new(
                "prestate_hash",
                &expected.prestate_hash,
                &actual.prestate_hash,
            ));
        }

        if expected.threshold != actual.threshold {
            diff.add(FieldDiff::new(
                "threshold",
                &expected.threshold,
                &actual.threshold,
            ));
        }

        if expected.initiator != actual.initiator {
            diff.add(FieldDiff::new(
                "initiator",
                &expected.initiator,
                &actual.initiator,
            ));
        }

        if expected.phase != actual.phase {
            diff.add(FieldDiff::new("phase", &expected.phase, &actual.phase));
        }

        if expected.fallback_timer_active != actual.fallback_timer_active {
            diff.add(FieldDiff::new(
                "fallback_timer_active",
                &expected.fallback_timer_active,
                &actual.fallback_timer_active,
            ));
        }

        // Compare witnesses set
        let witnesses_diff = Self::compare_sets(&expected.witnesses, &actual.witnesses);
        if !witnesses_diff.is_empty() {
            diff.add(FieldDiff::from_strings(
                "witnesses",
                Self::format_set(&expected.witnesses),
                Self::format_set(&actual.witnesses),
            ));
            for wd in witnesses_diff {
                diff.add(wd);
            }
        }

        // Compare equivocators set
        let equivocators_diff = Self::compare_sets(&expected.equivocators, &actual.equivocators);
        if !equivocators_diff.is_empty() {
            diff.add(FieldDiff::from_strings(
                "equivocators",
                Self::format_set(&expected.equivocators),
                Self::format_set(&actual.equivocators),
            ));
            for ed in equivocators_diff {
                diff.add(ed);
            }
        }

        // Compare proposals
        let proposals_diff = Self::compare_proposals(&expected.proposals, &actual.proposals);
        if !proposals_diff.is_empty() {
            diff.add(FieldDiff::from_strings(
                "proposals.len",
                expected.proposals.len().to_string(),
                actual.proposals.len().to_string(),
            ));
            for pd in proposals_diff {
                diff.add(pd);
            }
        }

        // Compare commit facts
        match (&expected.commit_fact, &actual.commit_fact) {
            (Some(e), Some(a)) => {
                let fact_diffs = Self::compare_commit_facts(e, a);
                for fd in fact_diffs {
                    diff.add(fd);
                }
            }
            (Some(e), None) => {
                diff.add(FieldDiff::from_strings(
                    "commit_fact",
                    format!("Some({:?})", e),
                    "None".to_string(),
                ));
            }
            (None, Some(a)) => {
                diff.add(FieldDiff::from_strings(
                    "commit_fact",
                    "None".to_string(),
                    format!("Some({:?})", a),
                ));
            }
            (None, None) => {}
        }

        diff
    }

    /// Compare two sets and return differences
    fn compare_sets(expected: &BTreeSet<String>, actual: &BTreeSet<String>) -> Vec<FieldDiff> {
        let mut diffs = Vec::new();

        let missing: Vec<_> = expected.difference(actual).collect();
        if !missing.is_empty() {
            diffs.push(FieldDiff::from_strings(
                "  missing",
                format!("{:?}", missing),
                "<not present>",
            ));
        }

        let extra: Vec<_> = actual.difference(expected).collect();
        if !extra.is_empty() {
            diffs.push(FieldDiff::from_strings(
                "  extra",
                "<not expected>",
                format!("{:?}", extra),
            ));
        }

        diffs
    }

    /// Format a set for display
    fn format_set(set: &BTreeSet<String>) -> String {
        let mut items: Vec<_> = set.iter().collect();
        items.sort();
        format!(
            "{{{}}}",
            items
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }

    /// Compare two proposal lists
    fn compare_proposals(expected: &[ShareProposal], actual: &[ShareProposal]) -> Vec<FieldDiff> {
        let mut diffs = Vec::new();

        // Build witness maps for comparison
        let expected_by_witness: std::collections::HashMap<_, _> =
            expected.iter().map(|p| (&p.witness, p)).collect();
        let actual_by_witness: std::collections::HashMap<_, _> =
            actual.iter().map(|p| (&p.witness, p)).collect();

        // Find missing proposals
        for (witness, prop) in &expected_by_witness {
            if !actual_by_witness.contains_key(witness) {
                diffs.push(FieldDiff::from_strings(
                    format!("proposals[{}]", witness),
                    format!("result_id={}", prop.result_id),
                    "<missing>",
                ));
            }
        }

        // Find extra proposals
        for (witness, prop) in &actual_by_witness {
            if !expected_by_witness.contains_key(witness) {
                diffs.push(FieldDiff::from_strings(
                    format!("proposals[{}]", witness),
                    "<not expected>",
                    format!("result_id={}", prop.result_id),
                ));
            }
        }

        // Compare matching proposals
        for (witness, expected_prop) in &expected_by_witness {
            if let Some(actual_prop) = actual_by_witness.get(witness) {
                if expected_prop.result_id != actual_prop.result_id {
                    diffs.push(FieldDiff::from_strings(
                        format!("proposals[{}].result_id", witness),
                        &expected_prop.result_id,
                        &actual_prop.result_id,
                    ));
                }

                if expected_prop.share != actual_prop.share {
                    diffs.push(FieldDiff::from_strings(
                        format!("proposals[{}].share", witness),
                        format!("{:?}", expected_prop.share),
                        format!("{:?}", actual_prop.share),
                    ));
                }
            }
        }

        diffs
    }

    /// Compare two commit facts
    fn compare_commit_facts(expected: &PureCommitFact, actual: &PureCommitFact) -> Vec<FieldDiff> {
        let mut diffs = Vec::new();

        if expected.cid != actual.cid {
            diffs.push(FieldDiff::new(
                "commit_fact.cid",
                &expected.cid,
                &actual.cid,
            ));
        }

        if expected.result_id != actual.result_id {
            diffs.push(FieldDiff::new(
                "commit_fact.result_id",
                &expected.result_id,
                &actual.result_id,
            ));
        }

        if expected.signature != actual.signature {
            diffs.push(FieldDiff::new(
                "commit_fact.signature",
                &expected.signature,
                &actual.signature,
            ));
        }

        if expected.prestate_hash != actual.prestate_hash {
            diffs.push(FieldDiff::new(
                "commit_fact.prestate_hash",
                &expected.prestate_hash,
                &actual.prestate_hash,
            ));
        }

        diffs
    }

    /// Check if there are any differences
    pub fn is_empty(&self) -> bool {
        self.global_diffs.is_empty()
            && self.instance_diffs.iter().all(|d| d.is_empty())
            && self.missing_instances.is_empty()
            && self.extra_instances.is_empty()
    }
}

/// Formatted divergence report for display
#[derive(Debug)]
pub struct DivergenceReport<'a> {
    /// Step index where divergence occurred
    pub step_index: usize,
    /// The state diff
    pub diff: &'a StateDiff,
    /// Description of action that caused divergence
    pub action_description: Option<String>,
}

impl<'a> DivergenceReport<'a> {
    pub fn new(step_index: usize, diff: &'a StateDiff, action: Option<impl fmt::Display>) -> Self {
        Self {
            step_index,
            diff,
            action_description: action.map(|a| a.to_string()),
        }
    }

    /// Create a simple report for an instance divergence
    pub fn for_instance(step_index: usize, diff: &'a InstanceDiff) -> String {
        let mut report = String::new();

        report.push_str(
            "\n╔══════════════════════════════════════════════════════════════════════╗\n",
        );
        report.push_str(&format!(
            "║ DIVERGENCE DETECTED at step {:<40} ║\n",
            step_index
        ));
        report.push_str(
            "╠══════════════════════════════════════════════════════════════════════╣\n",
        );
        report.push_str(&format!("║ Instance: {:<60} ║\n", diff.cid));
        report.push_str(
            "╠──────────────────────────────────────────────────────────────────────╣\n",
        );
        report.push_str(&format!(
            "║ {:<14} │ {:<21} │ {:<27} ║\n",
            "Field", "Expected", "Actual"
        ));
        report.push_str(
            "╠────────────────┼───────────────────────┼─────────────────────────────╣\n",
        );

        for fd in &diff.diffs {
            let field = if fd.field.len() > 14 {
                format!("{}…", &fd.field[..13])
            } else {
                fd.field.clone()
            };

            let expected = if fd.expected.len() > 21 {
                format!("{}…", &fd.expected[..20])
            } else {
                fd.expected.clone()
            };

            let actual = if fd.actual.len() > 27 {
                format!("{}…", &fd.actual[..26])
            } else {
                fd.actual.clone()
            };

            report.push_str(&format!(
                "║ {:<14} │ {:<21} │ {:<27} ║\n",
                field, expected, actual
            ));
        }

        report.push_str(
            "╚══════════════════════════════════════════════════════════════════════╝\n",
        );

        report
    }
}

impl<'a> fmt::Display for DivergenceReport<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f)?;
        writeln!(
            f,
            "╔══════════════════════════════════════════════════════════════════════╗"
        )?;
        writeln!(f, "║ DIVERGENCE DETECTED at step {:<40} ║", self.step_index)?;

        if let Some(action) = &self.action_description {
            let action_display = if action.len() > 62 {
                format!("{}…", &action[..61])
            } else {
                action.clone()
            };
            writeln!(f, "║ Action: {:<62} ║", action_display)?;
        }

        writeln!(
            f,
            "╠══════════════════════════════════════════════════════════════════════╣"
        )?;

        // Global diffs
        if !self.diff.global_diffs.is_empty() {
            writeln!(
                f,
                "║ Global State Changes:                                                ║"
            )?;
            for gd in &self.diff.global_diffs {
                writeln!(f, "║   {}: {} → {}", gd.field, gd.expected, gd.actual)?;
            }
        }

        // Missing instances
        if !self.diff.missing_instances.is_empty() {
            writeln!(f, "║ Missing instances: {:?}", self.diff.missing_instances)?;
        }

        // Extra instances
        if !self.diff.extra_instances.is_empty() {
            writeln!(f, "║ Unexpected instances: {:?}", self.diff.extra_instances)?;
        }

        // Instance diffs
        for inst_diff in &self.diff.instance_diffs {
            if inst_diff.is_empty() {
                continue;
            }

            writeln!(f, "║ Instance: {:<60} ║", inst_diff.cid)?;
            writeln!(
                f,
                "╠──────────────────────────────────────────────────────────────────────╣"
            )?;
            writeln!(
                f,
                "║ {:<14} │ {:<21} │ {:<27} ║",
                "Field", "Expected", "Actual"
            )?;
            writeln!(
                f,
                "╠────────────────┼───────────────────────┼─────────────────────────────╣"
            )?;

            for fd in &inst_diff.diffs {
                let field = if fd.field.len() > 14 {
                    format!("{}…", &fd.field[..13])
                } else {
                    fd.field.clone()
                };

                let expected = if fd.expected.len() > 21 {
                    format!("{}…", &fd.expected[..20])
                } else {
                    fd.expected.clone()
                };

                let actual = if fd.actual.len() > 27 {
                    format!("{}…", &fd.actual[..26])
                } else {
                    fd.actual.clone()
                };

                writeln!(f, "║ {:<14} │ {:<21} │ {:<27} ║", field, expected, actual)?;
            }
        }

        writeln!(
            f,
            "╚══════════════════════════════════════════════════════════════════════╝"
        )?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_consensus::core::state::{ConsensusPhase, PathSelection, ShareData};

    fn make_test_state() -> ConsensusState {
        let witnesses: BTreeSet<_> = ["w1", "w2", "w3"].iter().map(|s| s.to_string()).collect();
        ConsensusState::new(
            "cns1".to_string(),
            "update_policy".to_string(),
            "pre_abc".to_string(),
            2,
            witnesses,
            "w1".to_string(),
            PathSelection::FastPath,
        )
    }

    #[test]
    fn test_compare_identical_states() {
        let state1 = make_test_state();
        let state2 = make_test_state();

        let diff = StateDiff::compare_instances(&state1, &state2);
        assert!(diff.is_empty(), "Identical states should have no diff");
    }

    #[test]
    fn test_compare_phase_difference() {
        let state1 = make_test_state();
        let mut state2 = make_test_state();
        state2.phase = ConsensusPhase::FallbackActive;

        let diff = StateDiff::compare_instances(&state1, &state2);
        assert!(!diff.is_empty());
        assert!(diff.diffs.iter().any(|d| d.field == "phase"));
    }

    #[test]
    fn test_compare_proposal_difference() {
        let mut state1 = make_test_state();
        let mut state2 = make_test_state();

        state1.proposals.push(ShareProposal {
            witness: "w1".to_string(),
            result_id: "r1".to_string(),
            share: ShareData {
                share_value: "s1".to_string(),
                nonce_binding: "n1".to_string(),
                data_binding: "d1".to_string(),
            },
        });

        state2.proposals.push(ShareProposal {
            witness: "w1".to_string(),
            result_id: "r2".to_string(), // Different result_id
            share: ShareData {
                share_value: "s1".to_string(),
                nonce_binding: "n1".to_string(),
                data_binding: "d1".to_string(),
            },
        });

        let diff = StateDiff::compare_instances(&state1, &state2);
        assert!(!diff.is_empty());
        assert!(diff.diffs.iter().any(|d| d.field.contains("result_id")));
    }

    #[test]
    fn test_compare_equivocator_difference() {
        let mut state1 = make_test_state();
        let state2 = make_test_state();

        state1.equivocators.insert("bad_actor".to_string());

        let diff = StateDiff::compare_instances(&state1, &state2);
        assert!(!diff.is_empty());
        assert!(diff.diffs.iter().any(|d| d.field.contains("equivocators")));
    }

    #[test]
    fn test_divergence_report_format() {
        let state1 = make_test_state();
        let mut state2 = make_test_state();
        state2.phase = ConsensusPhase::FallbackActive;
        state2.threshold = 3;

        let diff = StateDiff::compare_instances(&state1, &state2);
        let report = DivergenceReport::for_instance(5, &diff);

        assert!(report.contains("DIVERGENCE DETECTED"));
        assert!(report.contains("step 5"));
        assert!(report.contains("cns1"));
        assert!(report.contains("phase"));
        assert!(report.contains("threshold"));
    }

    #[test]
    fn test_set_comparison() {
        let set1: BTreeSet<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        let set2: BTreeSet<String> = ["b", "c", "d"].iter().map(|s| s.to_string()).collect();

        let diffs = StateDiff::compare_sets(&set1, &set2);
        assert_eq!(diffs.len(), 2); // missing and extra
    }
}
