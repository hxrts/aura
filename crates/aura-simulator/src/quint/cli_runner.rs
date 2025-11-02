//! Real Quint CLI Integration
//!
//! Replaces placeholder implementations with actual Quint CLI execution.
//! Provides parsing, verification, and property evaluation using the real Quint tool.

use crate::quint::types::{QuintInvariant, QuintSpec};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;
use tokio::process::Command as AsyncCommand;

/// Errors from Quint CLI operations
#[derive(Error, Debug)]
pub enum QuintCliError {
    #[error("Quint CLI not found: {0}")]
    CliNotFound(String),

    #[error("Quint CLI execution failed: {0}")]
    ExecutionFailed(String),

    #[error("Quint output parsing failed: {0}")]
    ParseFailed(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON parsing error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Result type for Quint CLI operations
pub type QuintCliResult<T> = Result<T, QuintCliError>;

/// Quint CLI parse output structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuintParseOutput {
    /// Parsed module definitions from the specification
    pub modules: Vec<QuintModule>,
    /// Non-fatal warnings encountered during parsing
    pub warnings: Vec<String>,
    /// Fatal errors that prevented successful parsing
    pub errors: Vec<String>,
}

/// Quint module definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuintModule {
    /// Module name as defined in the Quint specification
    pub name: String,
    /// Function, operator, and property definitions within the module
    pub definitions: Vec<QuintDefinition>,
}

/// Quint definition (function, operator, invariant, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum QuintDefinition {
    /// Function or operator definition
    #[serde(rename = "def")]
    Definition {
        /// Definition name
        name: String,
        /// Type signature
        #[serde(rename = "type")]
        def_type: String,
        /// Optional function body expression
        body: Option<String>,
    },
    /// Value binding
    #[serde(rename = "val")]
    Value {
        /// Value name
        name: String,
        /// Value type
        #[serde(rename = "type")]
        val_type: String,
        /// Bound expression
        expr: String,
    },
    /// Assumption or constraint
    #[serde(rename = "assume")]
    Assumption {
        /// Optional assumption name
        name: Option<String>,
        /// Assumption expression
        expr: String,
    },
    /// Module import statement
    #[serde(rename = "import")]
    Import {
        /// Imported module name
        name: String,
        /// Source module path
        from: String,
    },
}

/// Quint verification result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuintVerificationResult {
    /// Verification outcome: "ok", "error", or "violation"
    pub outcome: String,
    /// Property violations discovered during verification
    pub violations: Vec<QuintViolation>,
    /// Verification statistics and metrics
    pub statistics: QuintStatistics,
}

/// Property violation details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuintViolation {
    /// Name of the violated property
    pub property: String,
    /// Human-readable description of the violation
    pub description: String,
    /// Optional counterexample trace leading to the violation
    pub trace: Option<Vec<serde_json::Value>>,
}

/// Verification statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuintStatistics {
    /// Number of unique states explored during verification
    pub states_explored: u64,
    /// Number of state transitions explored during verification
    pub transitions_explored: u64,
    /// Total verification time in milliseconds
    pub time_ms: u64,
}

/// Real Quint CLI runner that executes actual `quint` commands
pub struct QuintCliRunner {
    /// Path to the quint executable
    quint_path: PathBuf,
    /// Working directory for Quint operations
    working_dir: PathBuf,
    /// Timeout for CLI operations in milliseconds
    _timeout_ms: u64,
}

impl QuintCliRunner {
    /// Create a new Quint CLI runner
    pub fn new(quint_path: Option<PathBuf>, working_dir: PathBuf) -> QuintCliResult<Self> {
        let quint_path = quint_path.unwrap_or_else(|| PathBuf::from("quint"));

        // Verify quint is available
        let output = Command::new(&quint_path)
            .arg("--version")
            .output()
            .map_err(|e| QuintCliError::CliNotFound(format!("Cannot execute quint: {}", e)))?;

        if !output.status.success() {
            return Err(QuintCliError::CliNotFound(
                "Quint CLI executable not found or failed version check".to_string(),
            ));
        }

        Ok(Self {
            quint_path,
            working_dir,
            _timeout_ms: 30000, // 30 second default timeout
        })
    }

    /// Parse a Quint specification file
    pub async fn parse_spec(&self, spec_file: &Path) -> QuintCliResult<QuintParseOutput> {
        let output = AsyncCommand::new(&self.quint_path)
            .arg("parse")
            .arg("--out=json")
            .arg(spec_file)
            .current_dir(&self.working_dir)
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(QuintCliError::ExecutionFailed(format!(
                "Quint parse failed: {}",
                stderr
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let parse_result: QuintParseOutput = serde_json::from_str(&stdout).map_err(|e| {
            QuintCliError::ParseFailed(format!("Failed to parse Quint JSON output: {}", e))
        })?;

        Ok(parse_result)
    }

    /// Verify properties in a Quint specification
    pub async fn verify_spec(
        &self,
        spec_file: &Path,
        max_steps: Option<u32>,
    ) -> QuintCliResult<QuintVerificationResult> {
        let mut cmd = AsyncCommand::new(&self.quint_path);
        cmd.arg("verify").arg("--out=json").arg(spec_file);

        if let Some(steps) = max_steps {
            cmd.arg(format!("--max-steps={}", steps));
        }

        let output = cmd.current_dir(&self.working_dir).output().await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(QuintCliError::ExecutionFailed(format!(
                "Quint verify failed: {}",
                stderr
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let verify_result: QuintVerificationResult =
            serde_json::from_str(&stdout).map_err(|e| {
                QuintCliError::ParseFailed(format!(
                    "Failed to parse Quint verification output: {}",
                    e
                ))
            })?;

        Ok(verify_result)
    }

    /// Run a specific property check
    pub async fn check_property(
        &self,
        spec_file: &Path,
        property_name: &str,
    ) -> QuintCliResult<bool> {
        let output = AsyncCommand::new(&self.quint_path)
            .arg("run")
            .arg("--out=json")
            .arg("--init=true")
            .arg(format!("--invariant={}", property_name))
            .arg(spec_file)
            .current_dir(&self.working_dir)
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(QuintCliError::ExecutionFailed(format!(
                "Quint property check failed: {}",
                stderr
            )));
        }

        // Parse the result to determine if property holds
        let stdout = String::from_utf8_lossy(&output.stdout);
        let result: serde_json::Value = serde_json::from_str(&stdout)?;

        // Check if the property evaluation indicates success
        let success = result
            .get("outcome")
            .and_then(|v| v.as_str())
            .map(|s| s == "ok")
            .unwrap_or(false);

        Ok(success)
    }

    /// Generate random traces using Quint
    pub async fn generate_traces(
        &self,
        spec_file: &Path,
        count: u32,
        max_steps: u32,
    ) -> QuintCliResult<Vec<serde_json::Value>> {
        let output = AsyncCommand::new(&self.quint_path)
            .arg("run")
            .arg("--out=json")
            .arg(format!("--max-samples={}", count))
            .arg(format!("--max-steps={}", max_steps))
            .arg("--trace")
            .arg(spec_file)
            .current_dir(&self.working_dir)
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(QuintCliError::ExecutionFailed(format!(
                "Quint trace generation failed: {}",
                stderr
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let traces: Vec<serde_json::Value> = serde_json::from_str(&stdout)?;

        Ok(traces)
    }

    /// Convert Quint parse output to our internal QuintSpec format
    pub fn parse_output_to_spec(
        &self,
        parse_output: QuintParseOutput,
        file_path: &Path,
    ) -> QuintCliResult<QuintSpec> {
        let spec_name = file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        let mut invariants = Vec::new();
        let temporal_properties = Vec::new();

        // Extract properties from parsed modules
        for module in parse_output.modules {
            for definition in module.definitions {
                match definition {
                    QuintDefinition::Value { name, expr, .. } => {
                        // Treat named values as potential invariants
                        if name.starts_with("inv_") || name.contains("invariant") {
                            invariants.push(QuintInvariant {
                                name: name.clone(),
                                description: format!("Invariant: {}", name),
                                expression: expr,
                                source_location: "cli_runner".to_string(),
                                enabled: true,
                                tags: vec!["auto-extracted".to_string()],
                            });
                        }
                    }
                    QuintDefinition::Assumption { name, expr } => {
                        let assumption_name =
                            name.unwrap_or_else(|| "unnamed_assumption".to_string());
                        invariants.push(QuintInvariant {
                            name: assumption_name.clone(),
                            description: format!("Assumption: {}", assumption_name),
                            expression: expr,
                            source_location: "cli_runner".to_string(),
                            enabled: true,
                            tags: vec!["assumption".to_string()],
                        });
                    }
                    _ => {
                        // Other definition types might be handled in the future
                    }
                }
            }
        }

        Ok(QuintSpec {
            name: spec_name.clone(),
            file_path: file_path.to_path_buf(),
            module_name: spec_name,
            version: "1.0".to_string(),
            description: format!("Parsed from {}", file_path.display()),
            modules: vec![], // Could be populated from parse_output.modules
            metadata: HashMap::new(),
            invariants,
            temporal_properties,
            safety_properties: vec![],
            state_variables: Vec::new(),
            actions: Vec::new(),
        })
    }

    /// Set timeout for CLI operations
    pub fn set_timeout(&mut self, timeout_ms: u64) {
        self._timeout_ms = timeout_ms;
    }

    /// Get the Quint version
    pub async fn get_version(&self) -> QuintCliResult<String> {
        let output = AsyncCommand::new(&self.quint_path)
            .arg("--version")
            .output()
            .await?;

        if !output.status.success() {
            return Err(QuintCliError::ExecutionFailed(
                "Failed to get Quint version".to_string(),
            ));
        }

        let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(version)
    }
}
