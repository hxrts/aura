#![allow(missing_docs)]

use std::path::PathBuf;

use anyhow::Result;
use aura_harness::artifacts::ArtifactBundle;
use aura_harness::build_startup_summary;
use aura_harness::config::require_existing_file;
use aura_harness::load_and_validate_run_config;
use aura_harness::scenario::ScenarioRunner;
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
    /// Load and validate a run config and print startup summary.
    Run(RunArgs),
    /// Lint run and scenario files for schema and semantic validity.
    Lint(LintArgs),
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
struct ReplayArgs {
    #[arg(long)]
    bundle: PathBuf,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Run(args) => run(args),
        Command::Lint(args) => lint(args),
        Command::Replay(args) => replay(args),
    }
}

fn run(args: RunArgs) -> Result<()> {
    require_existing_file(&args.config, "run config")?;
    let config = load_and_validate_run_config(&args.config)?;

    if let Some(path) = &args.scenario {
        ScenarioRunner::load_and_validate(path)?;
    }

    let summary = build_startup_summary(&config);
    let artifact_bundle = ArtifactBundle::create(&args.artifacts_dir, &config.run.name)?;
    artifact_bundle.write_json("startup_summary.json", &summary)?;

    println!("{}", serde_json::to_string_pretty(&summary)?);
    println!("artifact_root={}", artifact_bundle.root.display());
    println!("artifact_run_dir={}", artifact_bundle.run_dir.display());

    Ok(())
}

fn lint(args: LintArgs) -> Result<()> {
    require_existing_file(&args.config, "run config")?;
    let config = load_and_validate_run_config(&args.config)?;

    if let Some(path) = &args.scenario {
        ScenarioRunner::load_and_validate(path)?;
    }

    println!(
        "lint_ok run={} instances={} schema_version={}",
        config.run.name,
        config.instances.len(),
        config.schema_version
    );
    Ok(())
}

fn replay(args: ReplayArgs) -> Result<()> {
    require_existing_file(&args.bundle, "replay bundle")?;
    println!("replay bundle accepted: {}", args.bundle.display());
    println!("replay execution will be implemented in Phase 2");
    Ok(())
}
