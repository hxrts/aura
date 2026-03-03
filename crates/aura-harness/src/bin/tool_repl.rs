#![allow(missing_docs)]

use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use aura_harness::config::require_existing_file;
use aura_harness::coordinator::HarnessCoordinator;
use aura_harness::load_and_validate_run_config;
use aura_harness::scenario::ScenarioRunner;
use aura_harness::scenario_execution::execute_with_run_budgets;
use aura_harness::tool_api::{ToolApi, ToolRequest, ToolResponse};
use clap::Parser;
use serde::{Deserialize, Serialize};
use tokio::time::Instant;

#[derive(Debug, Parser)]
#[command(name = "aura-harness-tool-repl")]
#[command(about = "Persistent ToolApi REPL for manual harness operation")]
struct Cli {
    #[arg(long)]
    config: PathBuf,
    /// Optional scripted scenario TOML to execute before entering interactive mode.
    #[arg(long)]
    prelude: Option<PathBuf>,
    /// Auto-shutdown after this many milliseconds without incoming requests.
    /// Set to 0 to disable idle timeout.
    #[arg(long, default_value_t = 600_000)]
    idle_timeout_ms: u64,
    /// Require every request line to include an `id` field.
    #[arg(long, default_value_t = false)]
    require_request_id: bool,
    /// Enforce strictly increasing numeric request ids.
    #[arg(long, default_value_t = false)]
    strict_request_id_order: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplRequestEnvelope {
    #[serde(default)]
    id: Option<serde_json::Value>,
    #[serde(flatten)]
    request: ToolRequest,
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
struct ReplResponseEnvelope {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<serde_json::Value>,
    #[serde(flatten)]
    response: ToolResponse,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    require_existing_file(&cli.config, "run config")?;
    let config = load_and_validate_run_config(&cli.config)?;
    let prelude = if let Some(path) = &cli.prelude {
        Some(ScenarioRunner::load_and_validate(path)?)
    } else {
        None
    };

    let coordinator = HarnessCoordinator::from_run_config(&config)?;
    let mut tool_api = ToolApi::new(coordinator);
    tool_api.start_all()?;

    if let Some(scenario) = &prelude {
        if let Err(error) = execute_with_run_budgets(&config, scenario, &mut tool_api) {
            let _ = tool_api.stop_all();
            return Err(error);
        }
        eprintln!("prelude_complete scenario_id={}", scenario.id);
    }

    let mut stdout = io::stdout();
    let idle_timeout = if cli.idle_timeout_ms == 0 {
        None
    } else {
        Some(Duration::from_millis(cli.idle_timeout_ms))
    };
    let poll_interval = Duration::from_millis(250);
    let mut last_activity = Instant::now();
    let mut last_request_id: Option<u64> = None;
    let shutdown_requested = Arc::new(AtomicBool::new(false));

    {
        let shutdown_requested = Arc::clone(&shutdown_requested);
        ctrlc::set_handler(move || {
            shutdown_requested.store(true, Ordering::SeqCst);
        })
        .with_context(|| "failed to install signal handler")?;
    }

    let (tx, rx) = mpsc::channel::<Result<String, io::Error>>();
    thread::spawn(move || {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            if tx.send(line).is_err() {
                break;
            }
        }
    });

    loop {
        if shutdown_requested.load(Ordering::SeqCst) {
            eprintln!("shutdown signal received; stopping harness instances");
            break;
        }

        let line = match rx.recv_timeout(poll_interval) {
            Ok(line) => {
                last_activity = Instant::now();
                line.with_context(|| "failed to read stdin line")?
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if let Some(timeout) = idle_timeout {
                    if last_activity.elapsed() >= timeout {
                        eprintln!(
                            "idle timeout reached ({} ms); shutting down harness instances",
                            cli.idle_timeout_ms
                        );
                        break;
                    }
                }
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.eq_ignore_ascii_case("exit") || trimmed.eq_ignore_ascii_case("quit") {
            break;
        }

        let (request_id, response) = match serde_json::from_str::<ReplRequestEnvelope>(trimmed) {
            Ok(envelope) => {
                if cli.require_request_id && envelope.id.is_none() {
                    (
                        None,
                        ToolResponse::Error {
                            message: "request id is required by --require-request-id".to_string(),
                        },
                    )
                } else if cli.strict_request_id_order {
                    if let Some(raw_id) = envelope.id.clone() {
                        match raw_id.as_u64() {
                            Some(value) => {
                                if last_request_id.is_some_and(|previous| value <= previous) {
                                    (
                                        envelope.id,
                                        ToolResponse::Error {
                                            message: format!(
                                                "request id {value} is not strictly greater than previous id {}",
                                                last_request_id.unwrap_or(0)
                                            ),
                                        },
                                    )
                                } else {
                                    last_request_id = Some(value);
                                    let response = tool_api.handle_request(envelope.request);
                                    (envelope.id, response)
                                }
                            }
                            None => (
                                envelope.id,
                                ToolResponse::Error {
                                    message: "strict request id order requires numeric u64 ids"
                                        .to_string(),
                                },
                            ),
                        }
                    } else {
                        (
                            None,
                            ToolResponse::Error {
                                message: "request id is required by --strict-request-id-order"
                                    .to_string(),
                            },
                        )
                    }
                } else {
                    let response = tool_api.handle_request(envelope.request);
                    (envelope.id, response)
                }
            }
            Err(error) => (
                None,
                ToolResponse::Error {
                    message: format!("invalid ToolRequest JSON: {error}"),
                },
            ),
        };

        let response = ReplResponseEnvelope {
            id: request_id,
            response,
        };

        writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
        stdout.flush()?;
    }

    tool_api.stop_all()?;
    Ok(())
}
