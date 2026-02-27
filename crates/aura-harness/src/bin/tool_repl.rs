#![allow(missing_docs)]

use std::io::{self, BufRead, Write};
use std::path::PathBuf;

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
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    require_existing_file(&cli.config, "run config")?;
    let config = load_and_validate_run_config(&cli.config)?;

    let coordinator = HarnessCoordinator::from_run_config(&config)?;
    let mut tool_api = ToolApi::new(coordinator);
    tool_api.start_all()?;

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = line.with_context(|| "failed to read stdin line")?;
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
