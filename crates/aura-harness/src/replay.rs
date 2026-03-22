//! Session replay for deterministic test reproduction.
//!
//! Records and replays tool API interactions with exact timing and seed state,
//! enabling reproduction of test failures and regression verification.

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};

use crate::api_version::TOOL_API_VERSIONS;
use crate::coordinator::HarnessCoordinator;
use crate::determinism::SeedBundle;
use crate::routing::ResolvedDialPath;
use crate::tool_api::{ToolActionRecord, ToolApi, ToolPayload, ToolResponse};

pub const REPLAY_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayBundle {
    pub schema_version: u32,
    pub tool_api_version: String,
    pub run_config: crate::config::RunConfig,
    pub actions: Vec<ToolActionRecord>,
    #[serde(default)]
    pub routing_metadata: Vec<ResolvedDialPath>,
    pub seed_bundle: SeedBundle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayOutcome {
    pub actions_executed: u64,
    pub mismatches: u64,
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
        if !TOOL_API_VERSIONS
            .iter()
            .any(|version| *version == self.tool_api_version)
        {
            bail!(
                "unsupported replay tool_api_version {} supported_versions={:?}",
                self.tool_api_version,
                TOOL_API_VERSIONS
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

        let mut mismatches = 0_u64;
        for action in &bundle.actions {
            let actual = tool_api.handle_request(action.request.clone());
            if !response_semantics_match(&actual, &action.response) {
                mismatches = mismatches.saturating_add(1_u64);
            }
        }

        tool_api.stop_all()?;

        Ok(ReplayOutcome {
            actions_executed: u64::try_from(bundle.actions.len())
                .map_err(|_| anyhow!("replay action count exceeds u64"))?,
            mismatches,
        })
    }
}

fn response_semantics_match(left: &ToolResponse, right: &ToolResponse) -> bool {
    match (left, right) {
        (
            ToolResponse::Ok {
                payload: ToolPayload::DiagnosticScreenCapture(actual),
            },
            ToolResponse::Ok {
                payload: ToolPayload::DiagnosticScreenCapture(expected),
            },
        ) => diagnostic_capture_semantics_match(actual, expected),
        _ => left == right,
    }
}

fn diagnostic_capture_semantics_match(
    actual: &crate::tool_api::DiagnosticScreenCapture,
    expected: &crate::tool_api::DiagnosticScreenCapture,
) -> bool {
    if actual.matched == Some(true) && expected.matched == Some(true) {
        return actual.screen_source == expected.screen_source
            && actual.matched_view == expected.matched_view
            && actual
                .diagnostic_normalized_screen
                .contains(&expected.diagnostic_normalized_screen);
    }

    actual == expected
}

pub fn parse_bundle(payload: &str) -> Result<ReplayBundle> {
    let bundle: ReplayBundle = serde_json::from_str(payload)
        .map_err(|error| anyhow!("failed to parse replay bundle JSON: {error}"))?;
    bundle.validate()?;
    Ok(bundle)
}

#[cfg(test)]
mod tests {
    use super::response_semantics_match;
    use crate::config::ScreenSource;
    use crate::tool_api::{ClipboardPayload, DiagnosticScreenCapture, ToolPayload, ToolResponse};

    #[test]
    fn replay_response_semantics_reject_payload_drift() {
        let expected = ToolResponse::Ok {
            payload: ToolPayload::Clipboard(ClipboardPayload {
                text: "alpha".to_string(),
            }),
        };
        let actual = ToolResponse::Ok {
            payload: ToolPayload::Clipboard(ClipboardPayload {
                text: "beta".to_string(),
            }),
        };

        assert!(
            !response_semantics_match(&actual, &expected),
            "typed replay matching must reject payload drift"
        );
    }

    #[test]
    fn replay_response_semantics_accept_repeated_matched_wait_output() {
        let expected = ToolResponse::Ok {
            payload: ToolPayload::DiagnosticScreenCapture(DiagnosticScreenCapture {
                diagnostic_authoritative_screen: "phase2-replay".to_string(),
                diagnostic_raw_screen: "phase2-replay".to_string(),
                diagnostic_normalized_screen: "phase2-replay".to_string(),
                screen_source: format!("{:?}", ScreenSource::Default).to_ascii_lowercase(),
                capture_consistency: None,
                matched: Some(true),
                matched_view: Some("normalized".to_string()),
            }),
        };
        let actual = ToolResponse::Ok {
            payload: ToolPayload::DiagnosticScreenCapture(DiagnosticScreenCapture {
                diagnostic_authoritative_screen: "phase2-replay\nphase2-replay".to_string(),
                diagnostic_raw_screen: "phase2-replay\nphase2-replay".to_string(),
                diagnostic_normalized_screen: "phase2-replay\nphase2-replay".to_string(),
                screen_source: format!("{:?}", ScreenSource::Default).to_ascii_lowercase(),
                capture_consistency: None,
                matched: Some(true),
                matched_view: Some("normalized".to_string()),
            }),
        };

        assert!(
            response_semantics_match(&actual, &expected),
            "successful diagnostic waits should compare by matched meaning"
        );
    }
}
