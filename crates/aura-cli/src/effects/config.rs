//! TODO fix - Simplified CLI configuration using DAG-CBOR
//!
//! **CLEANUP**: Replaced complex config loading/validation with simple DAG-CBOR
//! serialization. Eliminates 92 lines of TOML parsing and validation boilerplate.

pub use aura_core::{AuraError, AuraResult};

/// CLI result type
pub type Result<T> = AuraResult<T>;

/// Minimal CLI configuration
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CliConfig {
    /// Enable verbose logging
    pub verbose: bool,
    /// Output format preference
    pub format: OutputFormat,
}

/// Output format options for CLI commands
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum OutputFormat {
    /// Human-readable output
    Human,
    /// JSON output for scripting
    Json,
}

impl Default for CliConfig {
    fn default() -> Self {
        Self {
            verbose: false,
            format: OutputFormat::Human,
        }
    }
}
