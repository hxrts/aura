#![allow(missing_docs)]

use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use aura_harness::config::require_existing_file;
use aura_harness::coordinator::HarnessCoordinator;
use aura_harness::load_and_validate_run_config;
use aura_harness::tool_api::{ToolApi, ToolRequest};
use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "aura-harness-tool-repl")]
#[command(about = "Persistent ToolApi REPL for manual harness operation")]
struct Cli {
    #[arg(long)]
    config: PathBuf,
    /// Auto-shutdown after this many milliseconds without incoming requests.
    /// Set to 0 to disable idle timeout.
    #[arg(long, default_value_t = 600_000)]
    idle_timeout_ms: u64,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    require_existing_file(&cli.config, "run config")?;
    let config = load_and_validate_run_config(&cli.config)?;

    let coordinator = HarnessCoordinator::from_run_config(&config)?;
    let mut tool_api = ToolApi::new(coordinator);
    tool_api.start_all()?;

    let mut stdout = io::stdout();
    let idle_timeout = if cli.idle_timeout_ms == 0 {
        None
    } else {
        Some(Duration::from_millis(cli.idle_timeout_ms))
    };
    let poll_interval = Duration::from_millis(250);
    let mut last_activity = Instant::now();
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

        let response = match serde_json::from_str::<ToolRequest>(trimmed) {
            Ok(request) => tool_api.handle_request(request),
            Err(error) => aura_harness::tool_api::ToolResponse::Error {
                message: format!("invalid ToolRequest JSON: {error}"),
            },
        };

        writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
        stdout.flush()?;
    }

    tool_api.stop_all()?;
    Ok(())
}
