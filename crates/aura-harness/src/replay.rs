use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};

use crate::coordinator::HarnessCoordinator;
use crate::routing::ResolvedDialPath;
use crate::tool_api::{ToolActionRecord, ToolApi, ToolResponse};

pub const REPLAY_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayBundle {
    pub schema_version: u32,
    pub run_config: crate::config::RunConfig,
    pub actions: Vec<ToolActionRecord>,
    #[serde(default)]
    pub routing_metadata: Vec<ResolvedDialPath>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayOutcome {
    pub actions_executed: usize,
    pub mismatches: usize,
}

impl ReplayBundle {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != REPLAY_SCHEMA_VERSION {
            bail!(
                "unsupported replay schema_version {}. expected {}",
                self.schema_version,
                REPLAY_SCHEMA_VERSION
            );
        }
        self.run_config.validate()?;
        Ok(())
    }
}

pub struct ReplayRunner;

impl ReplayRunner {
    pub fn execute(bundle: &ReplayBundle) -> Result<ReplayOutcome> {
        bundle.validate()?;

        let coordinator = HarnessCoordinator::from_run_config(&bundle.run_config)?;
        let mut tool_api = ToolApi::new(coordinator);
        tool_api.start_all()?;

        let mut mismatches = 0usize;
        for action in &bundle.actions {
            let actual = tool_api.handle_request(action.request.clone());
            if !response_shape_matches(&actual, &action.response) {
                mismatches = mismatches.saturating_add(1);
            }
        }

        tool_api.stop_all()?;

        Ok(ReplayOutcome {
            actions_executed: bundle.actions.len(),
            mismatches,
        })
    }
}

fn response_shape_matches(left: &ToolResponse, right: &ToolResponse) -> bool {
    matches!(
        (left, right),
        (ToolResponse::Ok { .. }, ToolResponse::Ok { .. })
            | (ToolResponse::Error { .. }, ToolResponse::Error { .. })
    )
}

pub fn parse_bundle(payload: &str) -> Result<ReplayBundle> {
    let bundle: ReplayBundle = serde_json::from_str(payload)
        .map_err(|error| anyhow!("failed to parse replay bundle JSON: {error}"))?;
    bundle.validate()?;
    Ok(bundle)
}
