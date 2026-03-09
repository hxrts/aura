#![allow(missing_docs)]

pub mod api_version;
pub mod artifact_sync;
pub mod artifacts;
pub mod backend;
pub mod capabilities;
pub mod config;
pub mod coordinator;
pub mod determinism;
pub mod events;
pub mod executor;
pub mod failure_attribution;
pub mod introspection;
pub mod network_lab;
pub mod preflight;
pub mod provisioning;
pub mod replay;
pub mod residue_checks;
pub mod resource_guards;
pub mod routing;
pub mod runtime_substrate;
pub mod scenario;
pub mod scenario_execution;
pub mod screen_normalization;
pub mod tool_api;

use std::path::{Path, PathBuf};

use anyhow::Result;
use config::RunConfig;
use tool_api::StartupSummary;

/// Find the workspace root directory.
///
/// Resolution order:
/// 1. `AURA_WORKSPACE_ROOT` environment variable (if set)
/// 2. Walk up from current directory looking for Cargo.toml with `[workspace]`
/// 3. Fall back to current directory
pub fn workspace_root() -> PathBuf {
    // Check environment variable first
    if let Ok(root) = std::env::var("AURA_WORKSPACE_ROOT") {
        let path = PathBuf::from(root);
        if path.is_dir() {
            return path;
        }
    }

    // Walk up from current directory looking for workspace Cargo.toml
    if let Ok(cwd) = std::env::current_dir() {
        let mut current = cwd.as_path();
        loop {
            let cargo_toml = current.join("Cargo.toml");
            if cargo_toml.exists() {
                if let Ok(contents) = std::fs::read_to_string(&cargo_toml) {
                    if contents.contains("[workspace]") {
                        return current.to_path_buf();
                    }
                }
            }
            match current.parent() {
                Some(parent) => current = parent,
                None => break,
            }
        }
        // Fall back to current directory
        return cwd;
    }

    // Last resort
    PathBuf::from(".")
}

/// Get the default artifacts directory (workspace_root/artifacts).
pub fn default_artifacts_dir() -> PathBuf {
    workspace_root().join("artifacts")
}

/// Load a run configuration and validate all semantic constraints.
pub fn load_and_validate_run_config(path: &Path) -> Result<RunConfig> {
    let config = config::load_run_config(path)?;
    config.validate()?;
    let config = provisioning::materialize_run_config(config, path)?;
    config.validate()?;
    Ok(config)
}

/// Build a structured startup summary for operator and CI diagnostics.
pub fn build_startup_summary(config: &RunConfig) -> StartupSummary {
    StartupSummary::from_run_config(config)
}
