//! Lean Oracle for Differential Testing
//!
//! This module provides a Rust interface to the Lean verification models,
//! allowing differential testing between Rust implementations and formally
//! verified Lean models.
//!
//! The oracle invokes the `aura_verifier` Lean executable with JSON input
//! and parses the JSON output to compare against Rust implementations.

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

/// Journal merge input
#[derive(Debug, Clone, Serialize)]
pub struct JournalMergeInput {
    pub journal1: Vec<Fact>,
    pub journal2: Vec<Fact>,
}

/// Journal merge result
#[derive(Debug, Clone, Deserialize)]
pub struct JournalMergeResult {
    pub result: Vec<Fact>,
    pub count: usize,
}

/// Journal reduce input
#[derive(Debug, Clone, Serialize)]
pub struct JournalReduceInput {
    pub journal: Vec<Fact>,
}

/// Journal reduce result
#[derive(Debug, Clone, Deserialize)]
pub struct JournalReduceResult {
    pub result: Vec<Fact>,
    pub count: usize,
}

/// A fact in the journal (matching Lean model)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Fact {
    pub id: u64,
}

/// Flow charge input
#[derive(Debug, Clone, Serialize)]
pub struct FlowChargeInput {
    pub budget: u64,
    pub cost: u64,
}

/// Flow charge result
#[derive(Debug, Clone, Deserialize)]
pub struct FlowChargeResult {
    pub success: bool,
    pub remaining: Option<u64>,
}

/// Timestamp comparison input
#[derive(Debug, Clone, Serialize)]
pub struct TimestampCompareInput {
    pub policy: ComparePolicy,
    pub a: TimeStamp,
    pub b: TimeStamp,
}

/// Comparison policy
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComparePolicy {
    pub ignore_physical: bool,
}

/// Timestamp structure matching Lean model
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeStamp {
    pub logical: u64,
    pub order_clock: u64,
}

/// Timestamp comparison result
#[derive(Debug, Clone, Deserialize)]
pub struct TimestampCompareResult {
    pub ordering: String,
}

/// Ordering result enum
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ordering {
    Lt,
    Eq,
    Gt,
}

impl From<&str> for Ordering {
    fn from(s: &str) -> Self {
        match s {
            "lt" => Ordering::Lt,
            "eq" => Ordering::Eq,
            "gt" => Ordering::Gt,
            _ => panic!("Unknown ordering: {}", s),
        }
    }
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
            expected_version: "0.2.0".to_string(),
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

    /// Verify journal merge operation
    ///
    /// Runs the Lean model's merge function and returns the result.
    pub fn verify_merge(
        &self,
        journal1: Vec<Fact>,
        journal2: Vec<Fact>,
    ) -> LeanOracleResult<JournalMergeResult> {
        let input = JournalMergeInput { journal1, journal2 };
        let input_json = serde_json::to_string(&input)?;
        let output = self.run_command("journal-merge", &input_json)?;
        self.parse_output(&output)
    }

    /// Verify journal reduce operation
    ///
    /// Runs the Lean model's reduce function and returns the result.
    pub fn verify_reduce(&self, journal: Vec<Fact>) -> LeanOracleResult<JournalReduceResult> {
        let input = JournalReduceInput { journal };
        let input_json = serde_json::to_string(&input)?;
        let output = self.run_command("journal-reduce", &input_json)?;
        self.parse_output(&output)
    }

    /// Verify flow budget charge operation
    ///
    /// Runs the Lean model's charge function and returns the result.
    pub fn verify_charge(&self, budget: u64, cost: u64) -> LeanOracleResult<FlowChargeResult> {
        let input = FlowChargeInput { budget, cost };
        let input_json = serde_json::to_string(&input)?;
        let output = self.run_command("flow-charge", &input_json)?;
        self.parse_output(&output)
    }

    /// Verify timestamp comparison
    ///
    /// Runs the Lean model's compare function and returns the ordering.
    pub fn verify_compare(
        &self,
        policy: ComparePolicy,
        a: TimeStamp,
        b: TimeStamp,
    ) -> LeanOracleResult<Ordering> {
        let input = TimestampCompareInput { policy, a, b };
        let input_json = serde_json::to_string(&input)?;
        let output = self.run_command("timestamp-compare", &input_json)?;
        let result: TimestampCompareResult = self.parse_output(&output)?;
        Ok(Ordering::from(result.ordering.as_str()))
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

    // These tests require the Lean verifier to be built
    // Run `just lean-oracle-build` first

    #[test]
    #[ignore = "requires Lean verifier to be built"]
    fn test_oracle_version() {
        let oracle = LeanOracle::new().expect("Failed to create oracle");
        let version = oracle.version().expect("Failed to get version");
        assert_eq!(version.version, "0.2.0");
        assert!(version.modules.contains(&"Journal".to_string()));
    }

    #[test]
    #[ignore = "requires Lean verifier to be built"]
    fn test_journal_merge() {
        let oracle = LeanOracle::new().expect("Failed to create oracle");
        let result = oracle
            .verify_merge(
                vec![Fact { id: 1 }, Fact { id: 2 }],
                vec![Fact { id: 3 }],
            )
            .expect("Failed to merge");
        assert_eq!(result.count, 3);
    }

    #[test]
    #[ignore = "requires Lean verifier to be built"]
    fn test_flow_charge() {
        let oracle = LeanOracle::new().expect("Failed to create oracle");

        // Successful charge
        let result = oracle.verify_charge(100, 30).expect("Failed to charge");
        assert!(result.success);
        assert_eq!(result.remaining, Some(70));

        // Failed charge (insufficient budget)
        let result = oracle.verify_charge(10, 30).expect("Failed to charge");
        assert!(!result.success);
        assert_eq!(result.remaining, None);
    }

    #[test]
    #[ignore = "requires Lean verifier to be built"]
    fn test_timestamp_compare() {
        let oracle = LeanOracle::new().expect("Failed to create oracle");

        // With ignorePhysical = true, only logical time matters
        let result = oracle
            .verify_compare(
                ComparePolicy {
                    ignore_physical: true,
                },
                TimeStamp {
                    logical: 5,
                    order_clock: 100,
                },
                TimeStamp {
                    logical: 10,
                    order_clock: 50,
                },
            )
            .expect("Failed to compare");
        assert_eq!(result, Ordering::Lt);

        // Equal logical times
        let result = oracle
            .verify_compare(
                ComparePolicy {
                    ignore_physical: true,
                },
                TimeStamp {
                    logical: 10,
                    order_clock: 100,
                },
                TimeStamp {
                    logical: 10,
                    order_clock: 50,
                },
            )
            .expect("Failed to compare");
        assert_eq!(result, Ordering::Eq);
    }
}
