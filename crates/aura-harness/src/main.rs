//! Aura test harness CLI entry point.
//!
//! Provides commands for running integration test scenarios, replaying recorded
//! sessions, and validating harness configurations across local and remote instances.

#![allow(missing_docs)]

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use aura_harness::artifact_sync::sync_remote_artifacts;
use aura_harness::build_startup_summary;
use aura_harness::config::{require_existing_file, ScreenSource};
use aura_harness::coordinator::HarnessCoordinator;
use aura_harness::determinism::build_seed_bundle;
use aura_harness::failure_attribution::attribute_failure;
use aura_harness::governance::{self, GovernanceCheck};
use aura_harness::load_and_validate_run_config;
use aura_harness::network_lab::{resolve_backend_mode, NetworkBackendMode};
use aura_harness::preflight::{run_preflight, PreflightReport};
use aura_harness::replay::{parse_bundle, ReplayBundle, ReplayRunner, REPLAY_SCHEMA_VERSION};
use aura_harness::residue_checks::check_run_residue;
use aura_harness::resource_guards::ResourceGuard;
use aura_harness::routing::AddressResolver;
use aura_harness::scenario::ScenarioRunner;
use aura_harness::scenario_execution::{execute_with_run_budgets, lint_for_run};
use aura_harness::tool_api::{DiagnosticScreenCapture, ToolApi, ToolPayload, ToolRequest};
use aura_harness::{artifacts::ArtifactBundle, default_artifacts_dir};
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "aura-harness")]
#[command(about = "Aura runtime harness coordinator")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Load and validate a run config, then emit startup diagnostics and artifacts.
    Run(RunArgs),
    /// Lint run and scenario files for schema and semantic validity.
    Lint(LintArgs),
    /// Execute a single tool API request for smoke testing.
    Tool(ToolArgs),
    /// Replay a previously recorded run bundle.
    Replay(ReplayArgs),
    /// Run typed repository governance checks for harness UX policy.
    Governance(GovernanceArgs),
}

#[derive(Debug, Parser)]
struct RunArgs {
    #[arg(long)]
    config: PathBuf,
    #[arg(long)]
    scenario: Option<PathBuf>,
    /// Directory for test artifacts. Defaults to <workspace_root>/artifacts.
    #[arg(long)]
    artifacts_dir: Option<PathBuf>,
    #[arg(long, default_value = "mock")]
    network_backend: String,
}

impl RunArgs {
    fn artifacts_dir(&self) -> PathBuf {
        self.artifacts_dir
            .clone()
            .unwrap_or_else(default_artifacts_dir)
    }
}

#[derive(Debug, Parser)]
struct LintArgs {
    #[arg(long)]
    config: PathBuf,
    #[arg(long)]
    scenario: Option<PathBuf>,
}

#[derive(Debug, Parser)]
struct ToolArgs {
    #[arg(long)]
    config: PathBuf,
    #[arg(long)]
    request_json: String,
}

#[derive(Debug, Parser)]
struct ReplayArgs {
    #[arg(long)]
    bundle: PathBuf,
}

#[derive(Debug, Parser)]
struct GovernanceArgs {
    #[arg(value_enum)]
    check: GovernanceCheckArg,
}

#[derive(Debug, Clone, clap::ValueEnum)]
enum GovernanceCheckArg {
    SharedScenarioContract,
    ScenarioLegality,
    CoreScenarioMechanics,
    UserFlowCoverage,
    UiParityContract,
    SettingsSurfaceContract,
    ScenarioShapeContract,
    GovernanceWrappers,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Run(args) => run(args),
        Command::Lint(args) => lint(args),
        Command::Tool(args) => tool(args),
        Command::Replay(args) => replay(args),
        Command::Governance(args) => governance_check(args),
    }
}

fn governance_check(args: GovernanceArgs) -> Result<()> {
    let check = match args.check {
        GovernanceCheckArg::SharedScenarioContract => GovernanceCheck::SharedScenarioContract,
        GovernanceCheckArg::ScenarioLegality => GovernanceCheck::ScenarioLegality,
        GovernanceCheckArg::CoreScenarioMechanics => GovernanceCheck::CoreScenarioMechanics,
        GovernanceCheckArg::UserFlowCoverage => GovernanceCheck::UserFlowCoverage,
        GovernanceCheckArg::UiParityContract => GovernanceCheck::UiParityContract,
        GovernanceCheckArg::SettingsSurfaceContract => GovernanceCheck::SettingsSurfaceContract,
        GovernanceCheckArg::ScenarioShapeContract => GovernanceCheck::ScenarioShapeContract,
        GovernanceCheckArg::GovernanceWrappers => GovernanceCheck::GovernanceWrappers,
    };
    governance::run(check)
}

fn run(args: RunArgs) -> Result<()> {
    require_existing_file(&args.config, "run config")?;
    let config = load_and_validate_run_config(&args.config)?;
    let requested_backend: NetworkBackendMode = args
        .network_backend
        .parse()
        .context("invalid --network-backend value")?;
    let backend_preflight = resolve_backend_mode(requested_backend);

    let scenario_config = if let Some(path) = &args.scenario {
        Some(ScenarioRunner::load_and_validate(path)?)
    } else {
        None
    };

    let summary = build_startup_summary(&config);
    let artifact_bundle = ArtifactBundle::create(&args.artifacts_dir(), &config.run.name)?;
    let residue_report = check_run_residue(&config);
    let _ = artifact_bundle.write_json("residue_report.json", &residue_report);
    if !residue_report.clean {
        return Err(anyhow!(
            "run residue detected before startup: {}",
            residue_report
                .issues
                .iter()
                .map(|issue| format!("{}:{}:{}", issue.instance_id, issue.kind, issue.detail))
                .collect::<Vec<_>>()
                .join(" | ")
        ));
    }
    let preflight_report = match run_preflight(&config, scenario_config.as_ref()) {
        Ok(report) => report,
        Err(error) => {
            let payload = serde_json::json!({ "error": error.to_string() });
            let _ = artifact_bundle.write_json("preflight_error.json", &payload);
            return Err(error);
        }
    };

    let run_result = run_with_artifacts(
        &config,
        &artifact_bundle,
        &summary,
        &preflight_report,
        &backend_preflight,
        scenario_config.as_ref(),
    );
    if let Err(error) = run_result {
        let attribution = attribute_failure(&error.to_string());
        let failure_payload = serde_json::json!({ "error": error.to_string() });
        let _ = artifact_bundle.write_json("failure.json", &failure_payload);
        let _ = artifact_bundle.write_json("failure_attribution.json", &attribution);
        return Err(error);
    }

    println!("{}", serde_json::to_string_pretty(&summary)?);
    println!("artifact_root={}", artifact_bundle.root.display());
    println!("artifact_run_dir={}", artifact_bundle.run_dir.display());

    Ok(())
}

fn run_with_artifacts(
    config: &aura_harness::config::RunConfig,
    artifact_bundle: &ArtifactBundle,
    summary: &aura_harness::tool_api::StartupSummary,
    preflight_report: &PreflightReport,
    backend_preflight: &aura_harness::network_lab::BackendPreflightReport,
    scenario_config: Option<&aura_harness::config::ScenarioConfig>,
) -> Result<()> {
    let verbose_steps = std::env::var_os("AURA_HARNESS_VERBOSE_STEPS").is_some();
    let seed_bundle = build_seed_bundle(config);
    let mut resource_guard = ResourceGuard::from_run_config(config);
    resource_guard.sample("run_start");

    let coordinator = HarnessCoordinator::from_run_config(config)?;
    let mut tool_api = ToolApi::new(coordinator);
    if verbose_steps {
        eprintln!("[harness] startup phase=start_all begin");
    }
    tool_api.start_all()?;
    if verbose_steps {
        eprintln!("[harness] startup phase=start_all done");
    }

    let mut initial_screens: BTreeMap<String, String> = BTreeMap::new();
    for instance in &config.instances {
        if verbose_steps {
            eprintln!(
                "[harness] startup phase=initial_screen begin instance={}",
                instance.id
            );
        }
        let response = tool_api.handle_request(ToolRequest::Screen {
            instance_id: instance.id.clone(),
            screen_source: ScreenSource::Default,
        });
        if let aura_harness::tool_api::ToolResponse::Ok {
            payload: ToolPayload::DiagnosticScreenCapture(capture),
        } = response
        {
            initial_screens.insert(instance.id.clone(), capture.diagnostic_authoritative_screen);
        }
        if verbose_steps {
            eprintln!(
                "[harness] startup phase=initial_screen done instance={}",
                instance.id
            );
        }
    }

    let scenario_report = if let Some(scenario) = scenario_config {
        match execute_with_run_budgets(config, scenario, &mut tool_api) {
            Ok(report) => Some(report),
            Err(error) => {
                let diagnostics =
                    collect_failure_diagnostics(config, &mut tool_api, &error.to_string());
                artifact_bundle.write_json("failure_diagnostics.json", &diagnostics)?;
                if let Some(step_id) = diagnostics
                    .get("failing_step")
                    .and_then(serde_json::Value::as_str)
                {
                    let file_name = format!(
                        "failure_diagnostics__{}.json",
                        sanitize_artifact_component(step_id)
                    );
                    artifact_bundle.write_json(&file_name, &diagnostics)?;
                }
                if error.to_string().to_ascii_lowercase().contains("timeout") {
                    artifact_bundle.write_json("timeout_diagnostics.json", &diagnostics)?;
                }
                artifact_bundle.write_json(
                    "failure_attribution.json",
                    &attribute_failure(&error.to_string()),
                )?;
                return Err(error);
            }
        }
    } else {
        None
    };

    let events = tool_api.event_snapshot();
    let action_log = tool_api.action_log();
    tool_api.stop_all()?;
    resource_guard.sample("run_stop");

    let routing_metadata: Vec<_> = config
        .instances
        .iter()
        .map(|instance| AddressResolver::resolve(instance, &instance.bind_address))
        .collect();
    let replay_bundle = ReplayBundle {
        schema_version: REPLAY_SCHEMA_VERSION,
        tool_api_version: tool_api.negotiated_version().to_string(),
        run_config: config.clone(),
        actions: action_log,
        routing_metadata: routing_metadata.clone(),
        seed_bundle: seed_bundle.clone(),
    };
    let remote_sync_report = sync_remote_artifacts(config, artifact_bundle)?;
    let resource_report = resource_guard.report();

    artifact_bundle.write_json("startup_summary.json", summary)?;
    artifact_bundle.write_json("preflight_report.json", preflight_report)?;
    artifact_bundle.write_json("network_backend_preflight.json", backend_preflight)?;
    artifact_bundle.write_json("events.json", &events)?;
    artifact_bundle.write_json("initial_screens.json", &initial_screens)?;
    artifact_bundle.write_json("routing_metadata.json", &routing_metadata)?;
    artifact_bundle.write_json("replay_bundle.json", &replay_bundle)?;
    artifact_bundle.write_json("seed_bundle.json", &seed_bundle)?;
    artifact_bundle.write_json("resource_report.json", &resource_report)?;
    artifact_bundle.write_json("remote_artifact_sync.json", &remote_sync_report)?;
    if let Some(report) = &scenario_report {
        artifact_bundle.write_json("scenario_report.json", report)?;
    }

    Ok(())
}

fn collect_failure_diagnostics(
    config: &aura_harness::config::RunConfig,
    tool_api: &mut ToolApi,
    error_message: &str,
) -> serde_json::Value {
    let failing_step = parse_failing_step(error_message);
    let mut instances = BTreeMap::new();
    for instance in &config.instances {
        let screen_response = tool_api.handle_request(ToolRequest::Screen {
            instance_id: instance.id.clone(),
            screen_source: ScreenSource::Default,
        });
        let diagnostic_capture = match screen_response {
            aura_harness::tool_api::ToolResponse::Ok {
                payload: ToolPayload::DiagnosticScreenCapture(capture),
            } => capture,
            aura_harness::tool_api::ToolResponse::Ok { payload } => DiagnosticScreenCapture {
                diagnostic_authoritative_screen: format!(
                    "diagnostic_screen_unexpected_payload: {:?}",
                    payload
                ),
                diagnostic_raw_screen: String::new(),
                diagnostic_normalized_screen: String::new(),
                screen_source: "default".to_string(),
                capture_consistency: None,
                matched: None,
                matched_view: None,
            },
            aura_harness::tool_api::ToolResponse::Error { message } => {
                let error = format!("screen_capture_error: {message}");
                DiagnosticScreenCapture {
                    diagnostic_authoritative_screen: error.clone(),
                    diagnostic_raw_screen: error.clone(),
                    diagnostic_normalized_screen: error,
                    screen_source: "default".to_string(),
                    capture_consistency: None,
                    matched: None,
                    matched_view: None,
                }
            }
        };

        let dom_screen_response = tool_api.handle_request(ToolRequest::Screen {
            instance_id: instance.id.clone(),
            screen_source: ScreenSource::Dom,
        });
        let dom_capture = match dom_screen_response {
            aura_harness::tool_api::ToolResponse::Ok {
                payload: ToolPayload::DiagnosticScreenCapture(capture),
            } => serde_json::to_value(capture).unwrap_or_else(|error| {
                serde_json::json!({ "error": format!("dom_capture_encode_error: {error}") })
            }),
            aura_harness::tool_api::ToolResponse::Ok { payload } => serde_json::json!({
                "error": format!("dom_capture_unexpected_payload: {payload:?}")
            }),
            aura_harness::tool_api::ToolResponse::Error { message } => serde_json::json!({
                "error": format!("dom_screen_capture_error: {message}")
            }),
        };

        let ui_state_response = tool_api.handle_request(ToolRequest::UiState {
            instance_id: instance.id.clone(),
        });
        let (ui_state_snapshot, ui_state, ui_state_error) = match ui_state_response {
            aura_harness::tool_api::ToolResponse::Ok {
                payload: ToolPayload::UiSnapshot(snapshot),
            } => (
                Some(snapshot.clone()),
                Some(serde_json::to_value(snapshot).unwrap_or_else(
                    |error| serde_json::json!({ "encode_error": error.to_string() }),
                )),
                None,
            ),
            aura_harness::tool_api::ToolResponse::Ok { payload } => (
                None,
                Some(serde_json::json!({
                    "unexpected_payload": format!("{payload:?}")
                })),
                Some("ui_state_unexpected_payload".to_string()),
            ),
            aura_harness::tool_api::ToolResponse::Error { message } => (None, None, Some(message)),
        };

        let render_convergence = match &ui_state_snapshot {
            Some(snapshot) => {
                let semantic_screen = serde_json::to_value(&snapshot.screen)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_string))
                    .unwrap_or_else(|| format!("{:?}", snapshot.screen));
                let semantic_modal = snapshot
                    .open_modal
                    .as_ref()
                    .and_then(|modal| serde_json::to_value(modal).ok())
                    .and_then(|value| value.as_str().map(str::to_string));
                let dom_authoritative = dom_capture
                    .get("diagnostic_authoritative_screen")
                    .and_then(serde_json::Value::as_str);
                serde_json::json!({
                    "semantic_screen": semantic_screen,
                    "semantic_modal": semantic_modal,
                    "diagnostic_dom_authoritative_screen": dom_authoritative,
                    "screen_matches_dom": dom_authoritative == Some(semantic_screen.as_str()),
                })
            }
            None => serde_json::json!({
                "error": ui_state_error.clone().unwrap_or_else(|| "ui_state_unavailable".to_string())
            }),
        };

        let runtime_events = ui_state_snapshot
            .as_ref()
            .map(|snapshot| {
                serde_json::to_value(&snapshot.runtime_events).unwrap_or_else(|error| {
                    serde_json::json!([format!("runtime_events_encode_error: {error}")])
                })
            })
            .unwrap_or_else(|| serde_json::json!([]));

        let log_response = tool_api.handle_request(ToolRequest::TailLog {
            instance_id: instance.id.clone(),
            lines: 200,
        });
        let log_tail = match log_response {
            aura_harness::tool_api::ToolResponse::Ok {
                payload: ToolPayload::TailLog(lines),
            } => serde_json::to_value(lines.lines).unwrap_or_else(|error| {
                serde_json::json!([format!("tail_log_encode_error: {error}")])
            }),
            aura_harness::tool_api::ToolResponse::Ok { payload } => {
                serde_json::json!([format!("tail_log_unexpected_payload: {payload:?}")])
            }
            aura_harness::tool_api::ToolResponse::Error { message } => {
                serde_json::json!([format!("tail_log_error: {message}")])
            }
        };
        let browser_artifacts = instance
            .env
            .iter()
            .find_map(|entry| {
                let (key, value) = entry.split_once('=')?;
                (key == "AURA_HARNESS_BROWSER_ARTIFACT_DIR").then_some(value)
            })
            .map(recent_artifact_paths)
            .unwrap_or_default();

        instances.insert(
            instance.id.clone(),
            serde_json::json!({
                "diagnostic_authoritative_screen": diagnostic_capture.diagnostic_authoritative_screen,
                "diagnostic_raw_screen": diagnostic_capture.diagnostic_raw_screen,
                "diagnostic_normalized_screen": diagnostic_capture.diagnostic_normalized_screen,
                "dom_capture": dom_capture,
                "ui_state": ui_state,
                "ui_state_error": ui_state_error,
                "render_convergence": render_convergence,
                "runtime_events": runtime_events,
                "log_tail": log_tail,
                "browser_artifacts": browser_artifacts
            }),
        );
    }

    let action_log = tool_api.action_log();
    let action_log_tail: Vec<_> = action_log
        .into_iter()
        .rev()
        .take(50)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();

    serde_json::json!({
        "error": error_message,
        "failing_step": failing_step,
        "failure_attribution": attribute_failure(error_message),
        "instances": instances,
        "events": tool_api.event_snapshot(),
        "action_log_tail": action_log_tail
    })
}

fn recent_artifact_paths(dir: &str) -> Vec<String> {
    let mut files: Vec<_> = fs::read_dir(dir)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(|entry| entry.ok()))
        .filter_map(|entry| {
            let path = entry.path();
            let metadata = entry.metadata().ok()?;
            metadata
                .is_file()
                .then_some((metadata.modified().ok(), path))
        })
        .collect();
    files.sort_by_key(|(modified, _)| *modified);
    files
        .into_iter()
        .rev()
        .take(8)
        .map(|(_, path)| path.display().to_string())
        .collect()
}

fn sanitize_artifact_component(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    sanitized.trim_matches('_').to_string()
}

fn parse_failing_step(error_message: &str) -> Option<String> {
    let marker = "step ";
    let start = error_message.find(marker)? + marker.len();
    let remainder = &error_message[start..];
    let end = remainder
        .find(" failed")
        .or_else(|| remainder.find(' '))
        .unwrap_or(remainder.len());
    let candidate = remainder[..end].trim();
    (!candidate.is_empty()).then(|| candidate.to_string())
}

#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use super::{parse_failing_step, sanitize_artifact_component};

    #[test]
    fn parse_failing_step_extracts_step_id_from_executor_error() {
        let error =
            "scenario execution failed: step web-join-channel failed (action=send_chat_command actor=web): timeout";
        assert_eq!(
            parse_failing_step(error).as_deref(),
            Some("web-join-channel")
        );
    }

    #[test]
    fn parse_failing_step_returns_none_without_step_marker() {
        assert_eq!(parse_failing_step("plain failure"), None);
    }

    #[test]
    fn sanitize_artifact_component_rewrites_unsafe_characters() {
        assert_eq!(
            sanitize_artifact_component("web/join modal?"),
            "web_join_modal"
        );
    }
}

fn lint(args: LintArgs) -> Result<()> {
    require_existing_file(&args.config, "run config")?;
    let config = load_and_validate_run_config(&args.config)?;

    if let Some(path) = &args.scenario {
        let scenario = ScenarioRunner::load_and_validate(path)?;
        lint_for_run(&config, &scenario)?;
    }

    println!(
        "lint_ok run={} instances={} schema_version={}",
        config.run.name,
        config.instances.len(),
        config.schema_version
    );
    Ok(())
}

fn tool(args: ToolArgs) -> Result<()> {
    require_existing_file(&args.config, "run config")?;
    let config = load_and_validate_run_config(&args.config)?;

    let request: ToolRequest = serde_json::from_str(&args.request_json)
        .with_context(|| "failed to parse --request-json as ToolRequest")?;

    let coordinator = HarnessCoordinator::from_run_config(&config)?;
    let mut tool_api = ToolApi::new(coordinator);
    tool_api.start_all()?;
    let response = tool_api.handle_request(request);
    tool_api.stop_all()?;

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

fn replay(args: ReplayArgs) -> Result<()> {
    require_existing_file(&args.bundle, "replay bundle")?;
    let payload = fs::read_to_string(&args.bundle).with_context(|| {
        format!(
            "failed to read replay bundle from {}",
            args.bundle.display()
        )
    })?;
    let bundle = parse_bundle(&payload)?;
    let outcome = ReplayRunner::execute(&bundle)?;
    println!(
        "replay_ok actions_executed={} mismatches={}",
        outcome.actions_executed, outcome.mismatches
    );
    Ok(())
}
