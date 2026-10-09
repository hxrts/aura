//! Session replay for deterministic test reproduction.
//!
//! Records and replays tool API interactions with exact timing and seed state,
//! enabling reproduction of test failures and regression verification.

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};

use crate::api_version::TOOL_API_VERSIONS;
use crate::coordinator::{wait_pattern_matches, HarnessCoordinator};
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
            if !action_response_semantics_match(&action.request, &actual, &action.response) {
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

fn action_response_semantics_match(
    request: &crate::tool_api::ToolRequest,
    left: &ToolResponse,
    right: &ToolResponse,
) -> bool {
    if let crate::tool_api::ToolRequest::WaitFor {
        pattern,
        selector: None,
        ..
    } = request
    {
        if let (
            ToolResponse::Ok {
                payload: ToolPayload::DiagnosticScreenCapture(actual),
            },
            ToolResponse::Ok {
                payload: ToolPayload::DiagnosticScreenCapture(expected),
            },
        ) = (left, right)
        {
            // PTY echo and application output can arrive in either capture.
            // Both runs must satisfy the requested observation, not reproduce
            // incidental extra output from the recorded capture.
            return actual.matched == Some(true)
                && expected.matched == Some(true)
                && actual.screen_source == expected.screen_source
                && actual.matched_view == expected.matched_view
                && wait_pattern_matches(&actual.diagnostic_normalized_screen, pattern)
                && wait_pattern_matches(&expected.diagnostic_normalized_screen, pattern);
        }
    }
    left == right
}

pub fn parse_bundle(payload: &str) -> Result<ReplayBundle> {
    let bundle: ReplayBundle = serde_json::from_str(payload)
        .map_err(|error| anyhow!("failed to parse replay bundle JSON: {error}"))?;
    bundle.validate()?;
    Ok(bundle)
}

#[cfg(test)]
mod tests {
    use super::action_response_semantics_match;
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
            !action_response_semantics_match(
                &crate::tool_api::ToolRequest::ReadClipboard {
                    instance_id: "alice".to_string()
                },
                &actual,
                &expected,
            ),
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

        let request = crate::tool_api::ToolRequest::WaitFor {
            instance_id: "alice".to_string(),
            pattern: "phase2-replay".to_string(),
            selector: None,
            timeout_ms: 2000,
            screen_source: ScreenSource::Default,
        };
        assert!(action_response_semantics_match(
            &request, &actual, &expected
        ));
        assert!(action_response_semantics_match(
            &request, &expected, &actual
        ));
        let absent = crate::tool_api::ToolRequest::WaitFor {
            instance_id: "alice".to_string(),
            pattern: "missing-output".to_string(),
            selector: None,
            timeout_ms: 2000,
            screen_source: ScreenSource::Default,
        };
        assert!(!action_response_semantics_match(
            &absent, &actual, &expected
        ));
    }
}
