#![allow(missing_docs)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use aura_harness::artifacts::ArtifactBundle;
use aura_harness::build_startup_summary;
use aura_harness::config::require_existing_file;
use aura_harness::coordinator::HarnessCoordinator;
use aura_harness::executor::{ExecutionBudgets, ScenarioExecutor};
use aura_harness::load_and_validate_run_config;
use aura_harness::preflight::{run_preflight, PreflightReport};
use aura_harness::replay::{parse_bundle, ReplayBundle, ReplayRunner, REPLAY_SCHEMA_VERSION};
use aura_harness::routing::AddressResolver;
use aura_harness::scenario::ScenarioRunner;
use aura_harness::screen_normalization::normalize_screen;
use aura_harness::tool_api::{ToolApi, ToolRequest};
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
}

#[derive(Debug, Parser)]
struct RunArgs {
    #[arg(long)]
    config: PathBuf,
    #[arg(long)]
    scenario: Option<PathBuf>,
    #[arg(long, default_value = "artifacts")]
    artifacts_dir: PathBuf,
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

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Run(args) => run(args),
        Command::Lint(args) => lint(args),
        Command::Tool(args) => tool(args),
        Command::Replay(args) => replay(args),
    }
}

fn run(args: RunArgs) -> Result<()> {
    require_existing_file(&args.config, "run config")?;
    let config = load_and_validate_run_config(&args.config)?;

    let scenario_config = if let Some(path) = &args.scenario {
        Some(ScenarioRunner::load_and_validate(path)?)
    } else {
        None
    };

    let summary = build_startup_summary(&config);
    let artifact_bundle = ArtifactBundle::create(&args.artifacts_dir, &config.run.name)?;
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
        scenario_config.as_ref(),
    );
    if let Err(error) = run_result {
        let failure_payload = serde_json::json!({ "error": error.to_string() });
        let _ = artifact_bundle.write_json("failure.json", &failure_payload);
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
    scenario_config: Option<&aura_harness::config::ScenarioConfig>,
) -> Result<()> {
    let coordinator = HarnessCoordinator::from_run_config(config)?;
    let mut tool_api = ToolApi::new(coordinator);
    tool_api.start_all()?;

    let mut initial_screens: BTreeMap<String, String> = BTreeMap::new();
    for instance in &config.instances {
        let response = tool_api.handle_request(ToolRequest::Screen {
            instance_id: instance.id.clone(),
        });
        if let aura_harness::tool_api::ToolResponse::Ok { payload } = response {
            let screen = payload
                .get("screen")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string();
            initial_screens.insert(instance.id.clone(), screen);
        }
    }

    let scenario_report = if let Some(scenario) = scenario_config {
        let executor = ScenarioExecutor::from_config(scenario);
        let budgets = ExecutionBudgets {
            global_budget_ms: config.run.global_budget_ms,
            default_step_budget_ms: config.run.step_budget_ms.unwrap_or(2000),
        };
        match executor.execute_with_budgets(scenario, &mut tool_api, budgets) {
            Ok(report) => Some(report),
            Err(error) => {
                let diagnostics =
                    collect_timeout_diagnostics(config, &mut tool_api, &error.to_string());
                artifact_bundle.write_json("timeout_diagnostics.json", &diagnostics)?;
                return Err(error);
            }
        }
    } else {
        None
    };

    let events = tool_api.event_snapshot();
    let action_log = tool_api.action_log();
    tool_api.stop_all()?;

    let routing_metadata: Vec<_> = config
        .instances
        .iter()
        .map(|instance| AddressResolver::resolve(instance, &instance.bind_address))
        .collect();
    let replay_bundle = ReplayBundle {
        schema_version: REPLAY_SCHEMA_VERSION,
        run_config: config.clone(),
        actions: action_log,
        routing_metadata: routing_metadata.clone(),
    };

    artifact_bundle.write_json("startup_summary.json", summary)?;
    artifact_bundle.write_json("preflight_report.json", preflight_report)?;
    artifact_bundle.write_json("events.json", &events)?;
    artifact_bundle.write_json("initial_screens.json", &initial_screens)?;
    artifact_bundle.write_json("routing_metadata.json", &routing_metadata)?;
    artifact_bundle.write_json("replay_bundle.json", &replay_bundle)?;
    if let Some(report) = &scenario_report {
        artifact_bundle.write_json("scenario_report.json", report)?;
    }

    Ok(())
}

fn collect_timeout_diagnostics(
    config: &aura_harness::config::RunConfig,
    tool_api: &mut ToolApi,
    error_message: &str,
) -> serde_json::Value {
    let mut instances = BTreeMap::new();
    for instance in &config.instances {
        let screen_response = tool_api.handle_request(ToolRequest::Screen {
            instance_id: instance.id.clone(),
        });
        let raw_screen = match screen_response {
            aura_harness::tool_api::ToolResponse::Ok { payload } => payload
                .get("screen")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            aura_harness::tool_api::ToolResponse::Error { message } => {
                format!("screen_capture_error: {message}")
            }
        };

        let log_response = tool_api.handle_request(ToolRequest::TailLog {
            instance_id: instance.id.clone(),
            lines: 50,
        });
        let log_tail = match log_response {
            aura_harness::tool_api::ToolResponse::Ok { payload } => payload
                .get("lines")
                .cloned()
                .unwrap_or_else(|| serde_json::json!([])),
            aura_harness::tool_api::ToolResponse::Error { message } => {
                serde_json::json!([format!("tail_log_error: {message}")])
            }
        };

        instances.insert(
            instance.id.clone(),
            serde_json::json!({
                "raw_screen": raw_screen,
                "normalized_screen": normalize_screen(&raw_screen),
                "log_tail": log_tail
            }),
        );
    }

    serde_json::json!({
        "error": error_message,
        "instances": instances,
        "events": tool_api.event_snapshot()
    })
}

fn lint(args: LintArgs) -> Result<()> {
    require_existing_file(&args.config, "run config")?;
    let config = load_and_validate_run_config(&args.config)?;

    if let Some(path) = &args.scenario {
        let scenario = ScenarioRunner::load_and_validate(path)?;
        let lint = ScenarioRunner::lint(&config, &scenario);
        if !lint.errors.is_empty() {
            bail!("scenario lint failed: {}", lint.errors.join(" | "));
        }
        if !lint.warnings.is_empty() {
            println!("lint_warnings={}", lint.warnings.join(" | "));
        }
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
    let payload = std::fs::read_to_string(&args.bundle).with_context(|| {
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
