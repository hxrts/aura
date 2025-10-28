//! Native Quint evaluator wrapper
//!
//! This module provides a high-level interface to the Quint Rust evaluator,
//! handling JSON IR input and simulation output processing.

use std::process::Stdio;

use tokio::io::AsyncWriteExt;
use tokio::process::Command;

// Note: We import these types but don't expose them directly to avoid serialization issues

use crate::error::{QuintError, QuintResult};

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

    /// Parse a Quint file via subprocess (simplified interface)
    pub async fn parse_file(&self, file_path: &str) -> QuintResult<String> {
        let quint_cmd = self.quint_path.as_deref().unwrap_or("quint");

        // Use quint parse command to get JSON IR
        let output = Command::new(quint_cmd)
            .args(["parse", "--output=json", file_path])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| QuintError::ParseError(format!("Failed to execute quint: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(QuintError::ParseError(format!(
                "Quint parsing failed: {}",
                stderr
            )));
        }

        let json_output = String::from_utf8(output.stdout)
            .map_err(|e| QuintError::ParseError(format!("Invalid UTF-8 in quint output: {}", e)))?;

        Ok(json_output)
    }

    /// Simulate using the native Rust evaluator via stdin interface
    pub async fn simulate_via_evaluator(&self, json_ir: &str) -> QuintResult<String> {
        // Path to the built quint evaluator binary
        let evaluator_path = "../../ext/quint/evaluator/target/release/quint_evaluator";

        let mut child = Command::new(evaluator_path)
            .args(["simulate-from-stdin"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                QuintError::EvaluationError(format!("Failed to spawn evaluator: {}", e))
            })?;

        // Send JSON IR to stdin
        if let Some(stdin) = child.stdin.take() {
            let mut stdin = stdin;
            stdin.write_all(json_ir.as_bytes()).await.map_err(|e| {
                QuintError::EvaluationError(format!("Failed to write to stdin: {}", e))
            })?;
            stdin.shutdown().await.map_err(|e| {
                QuintError::EvaluationError(format!("Failed to close stdin: {}", e))
            })?;
        }

        // Read output from stdout
        let output = child.wait_with_output().await.map_err(|e| {
            QuintError::EvaluationError(format!("Failed to read evaluator output: {}", e))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(QuintError::EvaluationError(format!(
                "Evaluator failed: {}",
                stderr
            )));
        }

        let result_json = String::from_utf8(output.stdout).map_err(|e| {
            QuintError::EvaluationError(format!("Invalid UTF-8 in evaluator output: {}", e))
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
