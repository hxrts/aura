//! Native Quint evaluator wrapper
//!
//! This module provides a high-level interface to the Quint Rust evaluator,
//! handling JSON IR input and simulation output processing.

use std::process::Stdio;

use async_process::Command;
use futures::io::AsyncWriteExt;

// Note: We import these types but don't expose them directly to avoid serialization issues

use crate::{AuraError, AuraResult};

/// Result of verifying an invariant property
#[derive(Debug, Clone)]
pub struct InvariantVerificationResult {
    /// Name of the invariant that was checked
    pub invariant_name: String,
    /// Whether the invariant holds (true) or was violated (false)
    pub holds: bool,
    /// Counterexample trace if the invariant was violated
    pub counterexample: Option<String>,
    /// Raw stdout from the quint verify command
    pub output: String,
    /// Raw stderr from the quint verify command (if any)
    pub error_output: Option<String>,
}

/// Result of verifying a temporal property
#[derive(Debug, Clone)]
pub struct TemporalVerificationResult {
    /// Name of the temporal property that was checked
    pub property_name: String,
    /// Whether the property holds (true) or was violated (false)
    pub holds: bool,
    /// Whether we fell back to invariant-style checking
    /// (occurs when --temporal flag is not supported)
    pub used_invariant_fallback: bool,
    /// Counterexample trace if the property was violated
    pub counterexample: Option<String>,
    /// Raw stdout from the quint verify command
    pub output: String,
    /// Raw stderr from the quint verify command (if any)
    pub error_output: Option<String>,
}

/// Native Quint evaluator that uses the Rust evaluation engine directly
pub struct QuintEvaluator {
    quint_path: Option<String>,
}

impl QuintEvaluator {
    /// Create a new QuintEvaluator
    ///
    /// If quint_path is None, assumes 'quint' is available in PATH for parsing
    pub fn new(quint_path: Option<String>) -> Self {
        Self { quint_path }
    }

    /// Parse a Quint file via subprocess using the configured quint binary
    pub async fn parse_file(&self, file_path: &str) -> AuraResult<String> {
        let quint_cmd = self.quint_path.as_deref().unwrap_or("quint");

        // Use quint parse command to get JSON IR
        let output = Command::new(quint_cmd)
            .args(["parse", "--output=json", file_path])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| AuraError::invalid(format!("Failed to execute quint: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AuraError::invalid(format!(
                "Quint parsing failed: {}",
                stderr
            )));
        }

        let json_output = String::from_utf8(output.stdout)
            .map_err(|e| AuraError::invalid(format!("Invalid UTF-8 in quint output: {}", e)))?;

        Ok(json_output)
    }

    /// Verify an invariant property using Quint model checking
    ///
    /// Runs `quint verify --invariant={invariant_name} {spec_path}` and returns
    /// a verification result indicating whether the invariant holds.
    pub async fn verify_invariant(
        &self,
        spec_path: &str,
        invariant_name: &str,
    ) -> AuraResult<InvariantVerificationResult> {
        let quint_cmd = self.quint_path.as_deref().unwrap_or("quint");

        let output = Command::new(quint_cmd)
            .args([
                "verify",
                &format!("--invariant={}", invariant_name),
                spec_path,
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| {
                AuraError::invalid(format!(
                    "Failed to execute quint verify for invariant '{}': {}",
                    invariant_name, e
                ))
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        // Quint verify returns exit code 0 if property holds, non-zero if violated
        let holds = output.status.success();

        // Parse counterexample from output if verification failed
        let counterexample = if !holds {
            // Try to extract counterexample trace from output
            Self::extract_counterexample(&stdout, &stderr)
        } else {
            None
        };

        Ok(InvariantVerificationResult {
            invariant_name: invariant_name.to_string(),
            holds,
            counterexample,
            output: stdout.to_string(),
            error_output: if stderr.is_empty() {
                None
            } else {
                Some(stderr.to_string())
            },
        })
    }

    /// Verify a temporal property using Quint model checking
    ///
    /// Runs `quint verify --temporal={property_name} {spec_path}` and returns
    /// a verification result indicating whether the temporal property holds.
    ///
    /// Note: Temporal property verification may require Apalache backend and
    /// additional configuration. If the quint CLI doesn't support --temporal
    /// directly, this will fall back to treating it as an invariant check.
    pub async fn verify_temporal(
        &self,
        spec_path: &str,
        property_name: &str,
    ) -> AuraResult<TemporalVerificationResult> {
        let quint_cmd = self.quint_path.as_deref().unwrap_or("quint");

        // Try temporal flag first; if that fails, fall back to invariant check
        // since some temporal properties can be expressed as safety invariants
        let output = Command::new(quint_cmd)
            .args([
                "verify",
                &format!("--temporal={}", property_name),
                spec_path,
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await;

        let (output, used_fallback) = match output {
            Ok(out) => {
                // Check if the error indicates unsupported flag
                let stderr = String::from_utf8_lossy(&out.stderr);
                if stderr.contains("Unknown option") || stderr.contains("unknown option") {
                    // Fall back to invariant-style check
                    let fallback_output = Command::new(quint_cmd)
                        .args([
                            "verify",
                            &format!("--invariant={}", property_name),
                            spec_path,
                        ])
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped())
                        .output()
                        .await
                        .map_err(|e| {
                            AuraError::invalid(format!(
                                "Failed to execute quint verify for temporal property '{}': {}",
                                property_name, e
                            ))
                        })?;
                    (fallback_output, true)
                } else {
                    (out, false)
                }
            }
            Err(e) => {
                return Err(AuraError::invalid(format!(
                    "Failed to execute quint verify for temporal property '{}': {}",
                    property_name, e
                )));
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let holds = output.status.success();

        let counterexample = if !holds {
            Self::extract_counterexample(&stdout, &stderr)
        } else {
            None
        };

        Ok(TemporalVerificationResult {
            property_name: property_name.to_string(),
            holds,
            used_invariant_fallback: used_fallback,
            counterexample,
            output: stdout.to_string(),
            error_output: if stderr.is_empty() {
                None
            } else {
                Some(stderr.to_string())
            },
        })
    }

    /// Extract counterexample trace from Quint verification output
    fn extract_counterexample(stdout: &str, stderr: &str) -> Option<String> {
        // Quint outputs counterexamples in ITF format or as structured trace
        // Look for common patterns indicating a counterexample
        let combined = format!("{}\n{}", stdout, stderr);

        // Check for ITF trace markers
        if combined.contains("\"#meta\"") && combined.contains("\"states\"") {
            // Looks like an ITF trace - return the relevant portion
            if let Some(start) = combined.find('{') {
                if let Some(end) = combined.rfind('}') {
                    return Some(combined[start..=end].to_string());
                }
            }
        }

        // Check for "counterexample" or "violation" keywords
        if combined.contains("counterexample") || combined.contains("violation") {
            return Some(combined);
        }

        // If verification failed but no structured counterexample, return the output as context
        if !stdout.is_empty() || !stderr.is_empty() {
            Some(combined)
        } else {
            None
        }
    }

    /// Simulate using the native Rust evaluator via stdin interface
    pub async fn simulate_via_evaluator(&self, json_ir: &str) -> AuraResult<String> {
        // Path to the built quint evaluator binary (provided by nix environment)
        let evaluator_path = std::env::var("QUINT_EVALUATOR_PATH")
            .ok()
            .unwrap_or_else(|| "quint_evaluator".to_string());

        let mut child = Command::new(evaluator_path)
            .args(["simulate-from-stdin"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| AuraError::internal(format!("Failed to spawn evaluator: {}", e)))?;

        // Send JSON IR to stdin
        if let Some(stdin) = child.stdin.take() {
            let mut stdin = stdin;
            stdin
                .write_all(json_ir.as_bytes())
                .await
                .map_err(|e| AuraError::internal(format!("Failed to write to stdin: {}", e)))?;
            stdin
                .close()
                .await
                .map_err(|e| AuraError::internal(format!("Failed to close stdin: {}", e)))?;
        }

        // Read output from stdout
        let output = child
            .output()
            .await
            .map_err(|e| AuraError::internal(format!("Failed to read evaluator output: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AuraError::internal(format!("Evaluator failed: {}", stderr)));
        }

        let result_json = String::from_utf8(output.stdout).map_err(|e| {
            AuraError::internal(format!("Invalid UTF-8 in evaluator output: {}", e))
        })?;

        Ok(result_json)
    }
}

impl Default for QuintEvaluator {
    fn default() -> Self {
        Self::new(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evaluator_creation() {
        let evaluator = QuintEvaluator::new(None);
        assert!(evaluator.quint_path.is_none());

        let evaluator = QuintEvaluator::new(Some("/path/to/quint".to_string()));
        assert_eq!(evaluator.quint_path, Some("/path/to/quint".to_string()));
    }
}
