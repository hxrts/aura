//! Session replay for deterministic test reproduction.
//!
//! Records and replays tool API interactions with exact timing and seed state,
//! enabling reproduction of test failures and regression verification.

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;

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
    /// Exact source and executable identity of the prebuilt replay lane.
    #[serde(default)]
    pub compatibility: Option<ReplayCompatibility>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayCompatibility {
    pub source_revision: String,
    pub harness_sha256: String,
    pub instance_executables: Vec<ReplayExecutable>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayExecutable {
    pub instance_id: String,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub driver_sha256: Option<String>,
}

impl ReplayCompatibility {
    pub fn capture(config: &crate::config::RunConfig) -> Result<Self> {
        let revision = std::process::Command::new("git")
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .args(["rev-parse", "HEAD"])
            .output()
            .context("read replay source revision")?;
        if !revision.status.success() {
            bail!("replay source revision unavailable");
        }
        let source_revision = String::from_utf8(revision.stdout)
            .context("decode replay source revision")?
            .trim()
            .to_owned();
        if source_revision.len() != 40 || !source_revision.bytes().all(|b| b.is_ascii_hexdigit()) {
            bail!("invalid replay source revision");
        }
        let harness_sha256 = sha256_file(&std::env::current_exe()?)?;
        let mut instance_executables = Vec::new();
        for instance in &config.instances {
            let (command, driver) = match instance.mode {
                crate::config::InstanceMode::Local => {
                    let command = instance
                        .command
                        .clone()
                        .or_else(|| {
                            std::env::var("AURA_HARNESS_AURA_BIN")
                                .ok()
                                .filter(|path| std::path::Path::new(path).is_file())
                        })
                        .ok_or_else(|| {
                            anyhow!(
                                "local replay requires an explicit prebuilt executable for {}",
                                instance.id
                            )
                        })?;
                    (command, None)
                }
                crate::config::InstanceMode::Browser => {
                    let driver = instance
                        .args
                        .iter()
                        .find(|arg| arg.ends_with(".mjs"))
                        .map(std::path::PathBuf::from)
                        .unwrap_or_else(|| {
                            std::path::PathBuf::from(
                                "crates/aura-harness/playwright-driver/playwright_driver.mjs",
                            )
                        });
                    (
                        instance.command.clone().unwrap_or_else(|| "node".into()),
                        Some(driver),
                    )
                }
                crate::config::InstanceMode::Ssh => bail!(
                    "remote executable identity is unavailable for {}",
                    instance.id
                ),
            };
            let path = which::which(&command)
                .with_context(|| format!("resolve replay executable for {}", instance.id))?;
            instance_executables.push(ReplayExecutable {
                instance_id: instance.id.clone(),
                sha256: sha256_file(&path)?,
                driver_sha256: driver.as_deref().map(sha256_file).transpose()?,
            });
        }
        Ok(Self {
            source_revision,
            harness_sha256,
            instance_executables,
        })
    }
}

fn sha256_file(path: &std::path::Path) -> Result<String> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("open replay executable {}", path.display()))?;
    let mut digest = Sha256::new();
    let mut chunk = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        digest.update(&chunk[..count]);
    }
    Ok(hex::encode(digest.finalize()))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayDivergence {
    pub action_index: u64,
    pub request_method: String,
    pub expected_response: String,
    pub actual_response: String,
    pub field_path: String,
    pub expected_shape: String,
    pub actual_shape: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayOutcome {
    pub actions_executed: u64,
    pub mismatches: u64,
    pub first_divergence: Option<ReplayDivergence>,
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
        if self.compatibility.is_none() {
            bail!("replay bundle has no prebuilt revision evidence; regenerate with this harness");
        }
        Ok(())
    }
}

pub struct ReplayRunner;

impl ReplayRunner {
    pub fn execute(bundle: &ReplayBundle) -> Result<ReplayOutcome> {
        bundle.validate()?;
        let expected = bundle
            .compatibility
            .as_ref()
            .expect("validated compatibility");
        let actual = ReplayCompatibility::capture(&bundle.run_config)?;
        if &actual != expected {
            bail!(
                "replay prebuilt revision or executable differs; rebuild and record a new bundle"
            );
        }

        let coordinator = HarnessCoordinator::from_run_config(&bundle.run_config)?;
        let mut tool_api = ToolApi::new(coordinator);
        tool_api.start_all()?;

        let mut mismatches = 0_u64;
        let mut first_divergence = None;
        let replay_result = (|| -> Result<()> {
            for (index, action) in bundle.actions.iter().enumerate() {
                let actual = tool_api.handle_request(action.request.clone());
                observe_response(
                    index,
                    action,
                    &actual,
                    &mut mismatches,
                    &mut first_divergence,
                )?;
            }
            Ok(())
        })();
        let stop_result = tool_api.stop_all();
        replay_result?;
        stop_result?;

        Ok(ReplayOutcome {
            actions_executed: u64::try_from(bundle.actions.len())
                .map_err(|_| anyhow!("replay action count exceeds u64"))?,
            mismatches,
            first_divergence,
        })
    }
}

fn observe_response(
    index: usize,
    action: &ToolActionRecord,
    actual: &ToolResponse,
    mismatches: &mut u64,
    first_divergence: &mut Option<ReplayDivergence>,
) -> Result<()> {
    if action_response_semantics_match(&action.request, actual, &action.response) {
        return Ok(());
    }
    *mismatches = mismatches.saturating_add(1);
    if first_divergence.is_none() {
        let expected = serde_json::to_value(&action.response)?;
        let observed = serde_json::to_value(actual)?;
        let (field_path, expected_shape, actual_shape) =
            first_json_difference(&expected, &observed, "$".into())
                .unwrap_or_else(|| ("$".into(), "unknown".into(), "unknown".into()));
        *first_divergence = Some(ReplayDivergence {
            action_index: u64::try_from(index)
                .map_err(|_| anyhow!("replay action index exceeds u64"))?,
            request_method: serde_json::to_value(&action.request)?["method"]
                .as_str()
                .unwrap_or("unknown")
                .to_owned(),
            expected_response: response_kind(&action.response),
            actual_response: response_kind(actual),
            field_path,
            expected_shape,
            actual_shape,
        });
    }
    Ok(())
}

fn first_json_difference(
    expected: &serde_json::Value,
    actual: &serde_json::Value,
    path: String,
) -> Option<(String, String, String)> {
    use serde_json::Value;
    if expected == actual {
        return None;
    }
    match (expected, actual) {
        (Value::Object(left), Value::Object(right)) => {
            for key in left.keys().chain(right.keys()) {
                let child = if matches!(
                    key.as_str(),
                    "status"
                        | "payload"
                        | "message"
                        | "text"
                        | "matched"
                        | "screen_source"
                        | "matched_view"
                        | "result"
                        | "code"
                        | "domain"
                        | "state"
                        | "phase"
                        | "instance_id"
                        | "operation_id"
                ) {
                    format!("{path}.{key}")
                } else {
                    format!("{path}.<field>")
                };
                match (left.get(key), right.get(key)) {
                    (Some(a), Some(b)) => {
                        if let Some(diff) = first_json_difference(a, b, child) {
                            return Some(diff);
                        }
                    }
                    (a, b) => return Some((child, value_shape(a), value_shape(b))),
                }
            }
            None
        }
        (Value::Array(left), Value::Array(right)) => {
            for index in 0..left.len().max(right.len()) {
                let child = format!("{path}[{index}]");
                match (left.get(index), right.get(index)) {
                    (Some(a), Some(b)) => {
                        if let Some(diff) = first_json_difference(a, b, child) {
                            return Some(diff);
                        }
                    }
                    (a, b) => return Some((child, value_shape(a), value_shape(b))),
                }
            }
            None
        }
        _ => Some((path, value_shape(Some(expected)), value_shape(Some(actual)))),
    }
}

fn value_shape(value: Option<&serde_json::Value>) -> String {
    match value {
        None => "absent",
        Some(serde_json::Value::Null) => "null",
        Some(serde_json::Value::Bool(_)) => "boolean",
        Some(serde_json::Value::Number(_)) => "number",
        Some(serde_json::Value::String(_)) => "string",
        Some(serde_json::Value::Array(_)) => "array",
        Some(serde_json::Value::Object(_)) => "object",
    }
    .into()
}

fn response_kind(response: &ToolResponse) -> String {
    match response {
        ToolResponse::Error { .. } => "error".into(),
        ToolResponse::Ok { payload } => format!(
            "ok:{}",
            match payload {
                ToolPayload::Negotiation(_) => "negotiation",
                ToolPayload::DiagnosticScreenCapture(_) => "diagnostic_screen_capture",
                ToolPayload::UiSnapshot(_) => "ui_snapshot",
                ToolPayload::InstanceMetadata(_) => "instance_metadata",
                ToolPayload::SemanticCommand(_) => "semantic_command",
                ToolPayload::UiSnapshotEvent(_) => "ui_snapshot_event",
                ToolPayload::Status(_) => "status",
                ToolPayload::ContactInvitationCreated(_) => "contact_invitation_created",
                ToolPayload::TailLog(_) => "tail_log",
                ToolPayload::Clipboard(_) => "clipboard",
                ToolPayload::DeviceEnrollmentSetup(_) => "device_enrollment_setup",
                ToolPayload::AuthorityId(_) => "authority_id",
                ToolPayload::DiagnosticChannels(_) => "diagnostic_channels",
                ToolPayload::DiagnosticSelection(_) => "diagnostic_selection",
                ToolPayload::DiagnosticContacts(_) => "diagnostic_contacts",
            }
        ),
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
    use super::{
        action_response_semantics_match, observe_response, ReplayCompatibility, ReplayExecutable,
    };
    use crate::config::ScreenSource;
    use crate::tool_api::{
        ClipboardPayload, DiagnosticScreenCapture, ToolActionRecord, ToolPayload, ToolRequest,
        ToolResponse,
    };

    #[test]
    fn replay_first_divergence_is_typed_and_redacted() {
        let request = ToolRequest::SendKeys {
            instance_id: "private-id".into(),
            keys: "secret-value".into(),
        };
        let recorded = ToolActionRecord {
            request,
            response: ToolResponse::Error {
                message: "secret-error".into(),
            },
        };
        let actual = ToolResponse::Ok {
            payload: ToolPayload::Clipboard(ClipboardPayload {
                text: "secret-payload".into(),
            }),
        };
        let mut mismatches = 0;
        let mut first = None;
        observe_response(3, &recorded, &actual, &mut mismatches, &mut first).unwrap();
        observe_response(4, &recorded, &actual, &mut mismatches, &mut first).unwrap();
        assert_eq!(mismatches, 2);
        let first = first.unwrap();
        assert_eq!(first.action_index, 3);
        assert_eq!(first.request_method, "send_keys");
        assert_eq!(first.expected_response, "error");
        assert_eq!(first.actual_response, "ok:clipboard");
        assert_eq!(first.field_path, "$.message");
        let serialized = serde_json::to_string(&first).unwrap();
        assert!(!serialized.contains("secret"));
        assert!(!serialized.contains("private-id"));
    }

    #[test]
    fn replay_same_variant_payload_drift_reports_redacted_field_path() {
        let record = ToolActionRecord {
            request: ToolRequest::ReadClipboard {
                instance_id: "alice".into(),
            },
            response: ToolResponse::Ok {
                payload: ToolPayload::Clipboard(ClipboardPayload {
                    text: "secret-left".into(),
                }),
            },
        };
        let actual = ToolResponse::Ok {
            payload: ToolPayload::Clipboard(ClipboardPayload {
                text: "secret-right".into(),
            }),
        };
        let mut count = 0;
        let mut first = None;
        observe_response(0, &record, &actual, &mut count, &mut first).unwrap();
        let first = first.unwrap();
        assert_eq!(count, 1);
        assert_eq!(first.expected_response, "ok:clipboard");
        assert_eq!(first.actual_response, "ok:clipboard");
        assert_eq!(first.field_path, "$.payload.text");
        assert_eq!(first.expected_shape, "string");
        assert_eq!(first.actual_shape, "string");
        let summary = serde_json::to_string(&first).unwrap();
        assert!(!summary.contains("secret-left"));
        assert!(!summary.contains("secret-right"));
    }

    #[test]
    fn replay_compatibility_rejects_changed_prebuilt_digest() {
        let original = ReplayCompatibility {
            source_revision: "a".repeat(40),
            harness_sha256: "b".repeat(64),
            instance_executables: vec![ReplayExecutable {
                instance_id: "alice".into(),
                sha256: "c".repeat(64),
                driver_sha256: None,
            }],
        };
        let mut changed = original.clone();
        changed.instance_executables[0].sha256 = "d".repeat(64);
        assert_ne!(original, changed);
    }

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
