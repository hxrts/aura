//! Differential Testing Harness for Lean Oracle
//!
//! This module provides infrastructure for differential testing between
//! the Rust implementation and the Lean specification oracle.
//!
//! ## Lean Correspondence
//! - File: verification/lean/Aura/Runner.lean
//! - Commands: evidence-merge, frost-aggregate, guard-evaluate
//!
//! ## Task Correspondence
//! - T7.11: Create Rust differential test harness
//! - T7.12: Integrate differential tests into CI
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────┐    ┌─────────────────┐
//! │  Rust Code      │    │  Lean Oracle    │
//! │  (Production)   │    │  (Spec)         │
//! └────────┬────────┘    └────────┬────────┘
//!          │                      │
//!          ▼                      ▼
//! ┌─────────────────────────────────────────┐
//! │         Differential Harness            │
//! │  - Convert inputs to JSON               │
//! │  - Invoke Lean CLI                      │
//! │  - Parse JSON outputs                   │
//! │  - Compare results                      │
//! │  - Report divergences                   │
//! └─────────────────────────────────────────┘
//! ```

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::process::{Command, Stdio};

/// Result type for differential testing
pub type DifferentialResult<T> = Result<T, DifferentialError>;

/// Errors that can occur during differential testing
#[derive(Debug, Clone)]
pub enum DifferentialError {
    /// Lean oracle binary not found
    OracleNotFound(String),
    /// Failed to invoke Lean oracle
    OracleInvocationFailed(String),
    /// Failed to parse oracle output
    ParseError(String),
    /// Rust and Lean outputs diverge
    Divergence(DivergenceReport),
    /// IO error during testing
    IoError(String),
}

impl std::fmt::Display for DifferentialError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OracleNotFound(path) => write!(f, "Lean oracle not found at: {path}"),
            Self::OracleInvocationFailed(msg) => write!(f, "Oracle invocation failed: {msg}"),
            Self::ParseError(msg) => write!(f, "Failed to parse oracle output: {msg}"),
            Self::Divergence(report) => write!(f, "Divergence detected:\n{report}"),
            Self::IoError(msg) => write!(f, "IO error: {msg}"),
        }
    }
}

impl std::error::Error for DifferentialError {}

/// Report of a divergence between Rust and Lean
#[derive(Debug, Clone)]
pub struct DivergenceReport {
    /// Command that was tested
    pub command: String,
    /// Input provided
    pub input: String,
    /// Rust output
    pub rust_output: String,
    /// Lean output
    pub lean_output: String,
    /// Specific field that diverged (if applicable)
    pub diverged_field: Option<String>,
}

impl std::fmt::Display for DivergenceReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "╔══════════════════════════════════════════════════════════════╗"
        )?;
        writeln!(
            f,
            "║ DIVERGENCE DETECTED                                          ║"
        )?;
        writeln!(
            f,
            "╠══════════════════════════════════════════════════════════════╣"
        )?;
        writeln!(f, "║ Command: {:<52} ║", self.command)?;
        if let Some(field) = &self.diverged_field {
            writeln!(f, "║ Field: {field:<54} ║")?;
        }
        writeln!(
            f,
            "╠──────────────────────────────────────────────────────────────╣"
        )?;
        writeln!(
            f,
            "║ Rust Output:                                                 ║"
        )?;
        for line in self.rust_output.lines().take(5) {
            writeln!(f, "║   {line:<58} ║")?;
        }
        writeln!(
            f,
            "╠──────────────────────────────────────────────────────────────╣"
        )?;
        writeln!(
            f,
            "║ Lean Output:                                                 ║"
        )?;
        for line in self.lean_output.lines().take(5) {
            writeln!(f, "║   {line:<58} ║")?;
        }
        writeln!(
            f,
            "╚══════════════════════════════════════════════════════════════╝"
        )
    }
}

/// Lean Oracle client for differential testing
pub struct LeanOracle {
    /// Path to the Lean verifier binary
    binary_path: String,
}

impl LeanOracle {
    /// Create a new oracle client with default binary path
    pub fn new() -> Self {
        Self {
            binary_path: "verification/lean/build/bin/aura-verifier".to_string(),
        }
    }

    /// Create a new oracle client with custom binary path
    pub fn with_path(path: impl Into<String>) -> Self {
        Self {
            binary_path: path.into(),
        }
    }

    /// Check if the oracle binary exists and is runnable
    pub fn check_availability(&self) -> DifferentialResult<OracleInfo> {
        let output = Command::new(&self.binary_path)
            .arg("version")
            .output()
            .map_err(|e| {
                DifferentialError::OracleNotFound(format!("{}: {}", self.binary_path, e))
            })?;

        if !output.status.success() {
            return Err(DifferentialError::OracleInvocationFailed(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }

        let info: OracleInfo = serde_json::from_slice(&output.stdout)
            .map_err(|e| DifferentialError::ParseError(e.to_string()))?;

        Ok(info)
    }

    /// Invoke a command with JSON input and return parsed output
    pub fn invoke<I: Serialize, O: for<'de> Deserialize<'de>>(
        &self,
        command: &str,
        input: &I,
    ) -> DifferentialResult<O> {
        let input_json = serde_json::to_string(input)
            .map_err(|e| DifferentialError::ParseError(format!("Input serialization: {e}")))?;

        let mut child = Command::new(&self.binary_path)
            .arg(command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| DifferentialError::OracleInvocationFailed(e.to_string()))?;

        // Write input to stdin
        if let Some(stdin) = child.stdin.as_mut() {
            stdin
                .write_all(input_json.as_bytes())
                .map_err(|e| DifferentialError::IoError(e.to_string()))?;
        }

        let output = child
            .wait_with_output()
            .map_err(|e| DifferentialError::IoError(e.to_string()))?;

        if !output.status.success() {
            return Err(DifferentialError::OracleInvocationFailed(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }

        serde_json::from_slice(&output.stdout)
            .map_err(|e| DifferentialError::ParseError(format!("Output parsing: {e}")))
    }
}

impl Default for LeanOracle {
    fn default() -> Self {
        Self::new()
    }
}

/// Information about the oracle
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleInfo {
    pub version: String,
    pub modules: Vec<String>,
}

// ============================================================================
// EVIDENCE MERGE DIFFERENTIAL TESTING
// ============================================================================

/// Input for evidence merge command
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceMergeInput {
    pub evidence1: EvidenceJson,
    pub evidence2: EvidenceJson,
}

/// Evidence structure in JSON format
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceJson {
    pub consensus_id: String,
    pub votes: Vec<VoteJson>,
    pub equivocators: Vec<String>,
}

/// Vote structure in JSON format
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoteJson {
    pub witness: String,
    pub consensus_id: String,
    pub result_id: String,
    pub prestate_hash: String,
    pub share: ShareJson,
}

/// Share structure in JSON format
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareJson {
    pub share_value: String,
    pub nonce_binding: String,
    pub data_binding: String,
}

/// Output of evidence merge command
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceMergeOutput {
    pub result: EvidenceJson,
    pub votes_count: usize,
    pub equivocators_count: usize,
}

// ============================================================================
// FROST AGGREGATE DIFFERENTIAL TESTING
// ============================================================================

/// Input for FROST aggregate command
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrostAggregateInput {
    pub shares: Vec<FrostShareJson>,
    pub threshold: usize,
}

/// FROST share in JSON format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrostShareJson {
    pub witness: String,
    pub share_value: String,
}

/// Output of FROST aggregate command
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrostAggregateOutput {
    pub success: bool,
    pub signer_count: usize,
    pub threshold: usize,
    pub signers: Vec<String>,
}

// ============================================================================
// GUARD EVALUATE DIFFERENTIAL TESTING
// ============================================================================

/// Input for guard evaluate command
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardEvaluateInput {
    pub steps: Vec<GuardStepJson>,
}

/// Guard step in JSON format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardStepJson {
    pub flow_cost: usize,
    pub cap_req: Option<String>,
}

/// Output of guard evaluate command
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardEvaluateOutput {
    pub total_cost: usize,
    pub step_count: usize,
    pub valid: bool,
}

// ============================================================================
// DIFFERENTIAL TEST ASSERTIONS
// ============================================================================

/// Assert that Rust and Lean evidence merge outputs match
pub fn assert_evidence_merge_matches(
    rust_output: &EvidenceMergeOutput,
    lean_output: &EvidenceMergeOutput,
    input: &EvidenceMergeInput,
) -> DifferentialResult<()> {
    if rust_output.votes_count != lean_output.votes_count {
        return Err(DifferentialError::Divergence(DivergenceReport {
            command: "evidence-merge".to_string(),
            input: serde_json::to_string_pretty(input).unwrap_or_default(),
            rust_output: format!("votes_count: {}", rust_output.votes_count),
            lean_output: format!("votes_count: {}", lean_output.votes_count),
            diverged_field: Some("votes_count".to_string()),
        }));
    }

    if rust_output.equivocators_count != lean_output.equivocators_count {
        return Err(DifferentialError::Divergence(DivergenceReport {
            command: "evidence-merge".to_string(),
            input: serde_json::to_string_pretty(input).unwrap_or_default(),
            rust_output: format!("equivocators_count: {}", rust_output.equivocators_count),
            lean_output: format!("equivocators_count: {}", lean_output.equivocators_count),
            diverged_field: Some("equivocators_count".to_string()),
        }));
    }

    Ok(())
}

/// Assert that Rust and Lean FROST aggregate outputs match
pub fn assert_frost_aggregate_matches(
    rust_output: &FrostAggregateOutput,
    lean_output: &FrostAggregateOutput,
    input: &FrostAggregateInput,
) -> DifferentialResult<()> {
    if rust_output.success != lean_output.success {
        return Err(DifferentialError::Divergence(DivergenceReport {
            command: "frost-aggregate".to_string(),
            input: serde_json::to_string_pretty(input).unwrap_or_default(),
            rust_output: format!("success: {}", rust_output.success),
            lean_output: format!("success: {}", lean_output.success),
            diverged_field: Some("success".to_string()),
        }));
    }

    if rust_output.signer_count != lean_output.signer_count {
        return Err(DifferentialError::Divergence(DivergenceReport {
            command: "frost-aggregate".to_string(),
            input: serde_json::to_string_pretty(input).unwrap_or_default(),
            rust_output: format!("signer_count: {}", rust_output.signer_count),
            lean_output: format!("signer_count: {}", lean_output.signer_count),
            diverged_field: Some("signer_count".to_string()),
        }));
    }

    Ok(())
}

/// Assert that Rust and Lean guard evaluate outputs match
pub fn assert_guard_evaluate_matches(
    rust_output: &GuardEvaluateOutput,
    lean_output: &GuardEvaluateOutput,
    input: &GuardEvaluateInput,
) -> DifferentialResult<()> {
    if rust_output.total_cost != lean_output.total_cost {
        return Err(DifferentialError::Divergence(DivergenceReport {
            command: "guard-evaluate".to_string(),
            input: serde_json::to_string_pretty(input).unwrap_or_default(),
            rust_output: format!("total_cost: {}", rust_output.total_cost),
            lean_output: format!("total_cost: {}", lean_output.total_cost),
            diverged_field: Some("total_cost".to_string()),
        }));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_divergence_report_display() {
        let report = DivergenceReport {
            command: "evidence-merge".to_string(),
            input: r#"{"evidence1": {}, "evidence2": {}}"#.to_string(),
            rust_output: "votes_count: 2".to_string(),
            lean_output: "votes_count: 3".to_string(),
            diverged_field: Some("votes_count".to_string()),
        };

        let display = format!("{}", report);
        assert!(display.contains("DIVERGENCE"));
        assert!(display.contains("evidence-merge"));
        assert!(display.contains("votes_count"));
    }

    #[test]
    fn test_evidence_json_serialization() {
        let evidence = EvidenceJson {
            consensus_id: "cns1".to_string(),
            votes: vec![],
            equivocators: vec!["eq1".to_string()],
        };

        let json = serde_json::to_string(&evidence).unwrap();
        assert!(json.contains("cns1"));
        assert!(json.contains("eq1"));

        let parsed: EvidenceJson = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.consensus_id, "cns1");
    }

    #[test]
    fn test_frost_aggregate_json_serialization() {
        let input = FrostAggregateInput {
            shares: vec![
                FrostShareJson {
                    witness: "w1".to_string(),
                    share_value: "s1".to_string(),
                },
                FrostShareJson {
                    witness: "w2".to_string(),
                    share_value: "s2".to_string(),
                },
            ],
            threshold: 2,
        };

        let json = serde_json::to_string(&input).unwrap();
        let parsed: FrostAggregateInput = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.shares.len(), 2);
        assert_eq!(parsed.threshold, 2);
    }
}
