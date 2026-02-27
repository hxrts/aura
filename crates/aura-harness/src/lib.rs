#![allow(missing_docs)]

pub mod artifacts;
pub mod backend;
pub mod capabilities;
pub mod config;
pub mod coordinator;
pub mod events;
pub mod executor;
pub mod preflight;
pub mod replay;
pub mod routing;
pub mod scenario;
pub mod tool_api;

use std::path::Path;

use anyhow::Result;
use config::RunConfig;
use tool_api::StartupSummary;

/// Load a run configuration and validate all semantic constraints.
pub fn load_and_validate_run_config(path: &Path) -> Result<RunConfig> {
    let config = config::load_run_config(path)?;
    config.validate()?;
    Ok(config)
}

/// Build a structured startup summary for operator and CI diagnostics.
pub fn build_startup_summary(config: &RunConfig) -> StartupSummary {
    StartupSummary::from_run_config(config)
}
