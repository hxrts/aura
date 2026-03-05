//! Lean Oracle for Differential Testing
//!
//! This module provides a Rust interface to the Lean verification models,
//! allowing differential testing between Rust implementations and formally
//! verified Lean models.
//!
//! The oracle invokes the `aura_verifier` Lean executable with JSON input
//! and parses the JSON output to compare against Rust implementations.
//!
//! ## Version History
//!
//! - 0.4.0: Full structured types (OrderTime, TimeStamp, FactContent, Journal with namespace)
//! - 0.3.0: Previous version with simplified types
//!
//! ## Usage
//!
//! ```ignore
//! use aura_testkit::verification::lean_oracle::LeanOracle;
//! use aura_testkit::verification::lean_types::*;
//!
//! let oracle = LeanOracle::new()?;
//! let j1 = LeanJournal::empty(LeanNamespace::Authority { id: ByteArray32::zero() });
//! let j2 = LeanJournal::empty(LeanNamespace::Authority { id: ByteArray32::zero() });
//! let result = oracle.verify_journal_merge(&j1, &j2)?;
//! ```

use super::lean_types::{
    LeanFlowChargeInput, LeanFlowChargeResult, LeanJournal, LeanJournalMergeResult,
    LeanJournalReduceResult, LeanNamespace, LeanTimestampCompareInput, LeanTimestampCompareResult,
    LeanTimestampOrdering,
};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Result type for Lean Oracle operations
pub type LeanOracleResult<T> = Result<T, LeanOracleError>;

/// Error types for Lean Oracle operations
#[derive(Debug, thiserror::Error)]
pub enum LeanOracleError {
    #[error("Lean binary not found at {path}")]
    BinaryNotFound { path: PathBuf },

    #[error("Failed to execute Lean verifier: {0}")]
    ExecutionFailed(#[from] std::io::Error),

    #[error("Failed to parse JSON output: {0}")]
    JsonParseError(#[from] serde_json::Error),

    #[error("Lean verifier returned error: {message}")]
    VerifierError { message: String },

    #[error("Version mismatch: expected {expected}, got {actual}")]
    VersionMismatch { expected: String, actual: String },
}

/// Version information returned by the Lean oracle
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OracleVersion {
    pub version: String,
    pub modules: Vec<String>,
}

// ============================================================================
// Full-Fidelity Journal Types (v0.4.0+)
// ============================================================================

/// Full journal merge input with structured types.
#[derive(Debug, Clone, Serialize)]
pub struct FullJournalMergeInput {
    pub journal1: LeanJournal,
    pub journal2: LeanJournal,
}

/// Full journal reduce input with structured types.
#[derive(Debug, Clone, Serialize)]
pub struct FullJournalReduceInput {
    pub journal: LeanJournal,
}

/// Lean Oracle for differential testing
///
/// This struct wraps the Lean verifier executable and provides methods
/// for invoking the formally verified models.
#[derive(Debug, Clone)]
pub struct LeanOracle {
    /// Path to the Lean verifier executable
    lean_binary: PathBuf,
    /// Expected version string
    expected_version: String,
}

impl LeanOracle {
    /// Create a new Lean Oracle with the default binary path
    ///
    /// Looks for the binary at `verification/lean/.lake/build/bin/aura_verifier`
    /// relative to the workspace root.
    pub fn new() -> LeanOracleResult<Self> {
        let workspace_root = Self::find_workspace_root()?;
        let binary_path = workspace_root
            .join("verification")
            .join("lean")
            .join(".lake")
            .join("build")
            .join("bin")
            .join("aura_verifier");

        Self::with_binary_path(binary_path)
    }

    /// Create a new Lean Oracle with a specific binary path
    pub fn with_binary_path(path: PathBuf) -> LeanOracleResult<Self> {
        if !path.exists() {
            return Err(LeanOracleError::BinaryNotFound { path });
        }

        Ok(Self {
            lean_binary: path,
            expected_version: "0.4.0".to_string(),
        })
    }

    /// Find the workspace root by looking for Cargo.toml
    fn find_workspace_root() -> LeanOracleResult<PathBuf> {
        let mut current = std::env::current_dir()?;
        loop {
            let cargo_toml = current.join("Cargo.toml");
            if cargo_toml.exists() {
                // Check if this is the workspace root (has [workspace] section)
                let content = std::fs::read_to_string(&cargo_toml)?;
                if content.contains("[workspace]") {
                    return Ok(current);
                }
            }
            if !current.pop() {
                return Err(LeanOracleError::BinaryNotFound {
                    path: PathBuf::from("workspace root"),
                });
            }
        }
    }

    /// Get version information from the oracle
    pub fn version(&self) -> LeanOracleResult<OracleVersion> {
        let output = self.run_command("version", "")?;
        let version: OracleVersion = serde_json::from_str(&output)?;
        Ok(version)
    }

    /// Verify that the oracle version matches expected
    pub fn verify_version(&self) -> LeanOracleResult<()> {
        let version = self.version()?;
        if version.version != self.expected_version {
            return Err(LeanOracleError::VersionMismatch {
                expected: self.expected_version.clone(),
                actual: version.version,
            });
        }
        Ok(())
    }

    /// Verify flow budget charge operation using canonical typed payloads.
    pub fn verify_flow_charge(
        &self,
        input: &LeanFlowChargeInput,
    ) -> LeanOracleResult<LeanFlowChargeResult> {
        let input_json = serde_json::to_string(input)?;
        let output = self.run_command("flow-charge", &input_json)?;
        self.parse_output(&output)
    }

    /// Verify timestamp comparison using canonical typed payloads.
    pub fn verify_timestamp_compare(
        &self,
        input: &LeanTimestampCompareInput,
    ) -> LeanOracleResult<LeanTimestampOrdering> {
        let input_json = serde_json::to_string(input)?;
        let output = self.run_command("timestamp-compare", &input_json)?;
        let result: LeanTimestampCompareResult = self.parse_output(&output)?;
        Ok(result.ordering)
    }

    // ========================================================================
    // Full-Fidelity Journal Operations (v0.4.0+)
    // ========================================================================

    /// Verify full journal merge with namespace checking.
    ///
    /// This uses the structured Fact type with OrderTime, TimeStamp, and FactContent.
    /// Journals must have the same namespace to merge successfully.
    ///
    /// Returns `Err(VerifierError)` with "namespace mismatch" if namespaces differ.
    pub fn verify_journal_merge(
        &self,
        journal1: &LeanJournal,
        journal2: &LeanJournal,
    ) -> LeanOracleResult<LeanJournalMergeResult> {
        let input = FullJournalMergeInput {
            journal1: journal1.clone(),
            journal2: journal2.clone(),
        };
        let input_json = serde_json::to_string(&input)?;
        let output = self.run_command("journal-merge", &input_json)?;

        // Check for namespace mismatch error
        if let Ok(error_obj) = serde_json::from_str::<serde_json::Value>(&output) {
            if error_obj.get("error").map(|e| e.as_str()) == Some(Some("namespace mismatch")) {
                return Err(LeanOracleError::VerifierError {
                    message: "namespace mismatch".to_string(),
                });
            }
        }

        self.parse_output(&output)
    }

    /// Verify full journal reduce with structured types.
    ///
    /// This uses the structured Fact type with OrderTime, TimeStamp, and FactContent.
    pub fn verify_journal_reduce(
        &self,
        journal: &LeanJournal,
    ) -> LeanOracleResult<LeanJournalReduceResult> {
        let input = FullJournalReduceInput {
            journal: journal.clone(),
        };
        let input_json = serde_json::to_string(&input)?;
        let output = self.run_command("journal-reduce", &input_json)?;
        self.parse_output(&output)
    }

    /// Check if two namespaces are equal.
    ///
    /// Helper method for determining if journals can be merged.
    pub fn namespaces_equal(ns1: &LeanNamespace, ns2: &LeanNamespace) -> bool {
        ns1 == ns2
    }

    /// Run a command against the Lean verifier
    fn run_command(&self, command: &str, input: &str) -> LeanOracleResult<String> {
        let mut child = Command::new(&self.lean_binary)
            .arg(command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        // Write input to stdin
        if !input.is_empty() {
            if let Some(ref mut stdin) = child.stdin {
                stdin.write_all(input.as_bytes())?;
            }
        }

        let output = child.wait_with_output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(LeanOracleError::VerifierError {
                message: stderr.to_string(),
            });
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Parse JSON output, checking for error field
    fn parse_output<T: for<'de> Deserialize<'de>>(&self, output: &str) -> LeanOracleResult<T> {
        // First check if output contains an error
        if let Ok(error_obj) = serde_json::from_str::<serde_json::Value>(output) {
            if let Some(error) = error_obj.get("error") {
                return Err(LeanOracleError::VerifierError {
                    message: error.as_str().unwrap_or("Unknown error").to_string(),
                });
            }
        }
        Ok(serde_json::from_str(output)?)
    }
}

impl Default for LeanOracle {
    fn default() -> Self {
        Self::new().expect("Failed to create default LeanOracle")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verification::lean_types::ByteArray32;
    use crate::verification::lean_types::{LeanComparePolicy, LeanCompareTimeStamp};

    // These tests require the Lean verifier to be built
    // Run `just lean-oracle-build` first

    #[test]
    #[ignore = "requires Lean verifier to be built"]
    fn test_oracle_version() {
        let oracle = LeanOracle::new().expect("Failed to create oracle");
        let version = oracle.version().expect("Failed to get version");
        assert_eq!(version.version, "0.4.0");
        assert!(version.modules.contains(&"Journal".to_string()));
    }

    #[test]
    #[ignore = "requires Lean verifier to be built"]
    fn test_journal_merge() {
        let oracle = LeanOracle::new().expect("Failed to create oracle");
        let ns = LeanNamespace::Authority {
            id: ByteArray32::zero(),
        };
        let j1 = LeanJournal::empty(ns.clone());
        let j2 = LeanJournal::empty(ns);
        let result = oracle
            .verify_journal_merge(&j1, &j2)
            .expect("Failed to merge");
        assert_eq!(result.count, 0);
    }

    #[test]
    #[ignore = "requires Lean verifier to be built"]
    fn test_flow_charge() {
        let oracle = LeanOracle::new().expect("Failed to create oracle");

        // Successful charge
        let result = oracle
            .verify_flow_charge(&LeanFlowChargeInput {
                budget: 100,
                cost: 30,
            })
            .expect("Failed to charge");
        assert!(result.success);
        assert_eq!(result.remaining, Some(70));

        // Failed charge (insufficient budget)
        let result = oracle
            .verify_flow_charge(&LeanFlowChargeInput {
                budget: 10,
                cost: 30,
            })
            .expect("Failed to charge");
        assert!(!result.success);
        assert_eq!(result.remaining, None);
    }

    #[test]
    #[ignore = "requires Lean verifier to be built"]
    fn test_timestamp_compare() {
        let oracle = LeanOracle::new().expect("Failed to create oracle");

        // With ignorePhysical = true, only logical time matters
        let result = oracle
            .verify_timestamp_compare(&LeanTimestampCompareInput {
                policy: LeanComparePolicy {
                    ignore_physical: true,
                },
                a: LeanCompareTimeStamp {
                    logical: 5,
                    order_clock: 100,
                },
                b: LeanCompareTimeStamp {
                    logical: 10,
                    order_clock: 50,
                },
            })
            .expect("Failed to compare");
        assert_eq!(result, LeanTimestampOrdering::Lt);

        // Equal logical times
        let result = oracle
            .verify_timestamp_compare(&LeanTimestampCompareInput {
                policy: LeanComparePolicy {
                    ignore_physical: true,
                },
                a: LeanCompareTimeStamp {
                    logical: 10,
                    order_clock: 100,
                },
                b: LeanCompareTimeStamp {
                    logical: 10,
                    order_clock: 50,
                },
            })
            .expect("Failed to compare");
        assert_eq!(result, LeanTimestampOrdering::Eq);
    }
}
