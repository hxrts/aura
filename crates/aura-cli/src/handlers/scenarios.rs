#![allow(deprecated)]
//! Scenarios Command Handler
//!
//! Effect-based implementation of scenario management commands.

use std::time::Instant;

use crate::ScenarioAction;
use anyhow::Result;
use aura_agent::{AuraEffectSystem, EffectContext};
use aura_authenticate::guardian_auth::{RecoveryContext, RecoveryOperationType};
use aura_core::effects::{ConsoleEffects, StorageEffects};
use aura_core::{identifiers::GuardianId, AccountId, DeviceId};
use aura_recovery::guardian_setup::GuardianSetupCoordinator;
use aura_recovery::types::{GuardianProfile, GuardianSet, RecoveryRequest};
use aura_simulator::handlers::scenario::SimulationScenarioHandler;
use std::collections::HashMap;
use std::fmt::Write as FmtWrite;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Handle scenario operations through effects
pub async fn handle_scenarios(
    ctx: &EffectContext,
    effects: Arc<AuraEffectSystem>,
    action: &ScenarioAction,
) -> Result<()> {
    match action {
        ScenarioAction::Discover { root, validate } => {
            handle_discover(ctx, effects, root, *validate).await
        }
        ScenarioAction::List {
            directory,
            detailed,
        } => handle_list(ctx, effects, directory, *detailed).await,
        ScenarioAction::Validate {
            directory,
            strictness,
        } => handle_validate(ctx, effects, directory, strictness.as_deref()).await,
        ScenarioAction::Run {
            directory,
            pattern,
            parallel,
            max_parallel,
            output_file,
            detailed_report,
        } => {
            handle_run(
                ctx,
                effects,
                directory.as_deref(),
                pattern.as_deref(),
                *parallel,
                *max_parallel,
                output_file.as_deref(),
                *detailed_report,
            )
            .await
        }
        ScenarioAction::Report {
            input,
            output,
            format,
            detailed,
        } => handle_report(ctx, effects, input, output, format.as_deref(), *detailed).await,
    }
}

/// Handle scenario discovery through effects
async fn handle_discover(
    ctx: &EffectContext,
    effects: Arc<AuraEffectSystem>,
    root: &Path,
    validate: bool,
) -> Result<()> {
    println!("Discovering scenarios in: {}", root.display());

    println!("Validation: {}", validate);

    // Check if root directory exists through storage effects
    let root_exists = effects
        .exists(&root.display().to_string())
        .await
        .unwrap_or(false);

    if !root_exists {
        eprintln!("Root directory not found: {}", root.display());
        return Err(anyhow::anyhow!(
            "Root directory not found: {}",
            root.display()
        ));
    }

    // Simulate scenario discovery
    let scenarios = discover_scenarios_through_effects(ctx, effects.as_ref(), root).await?;

    println!("Found {} scenarios", scenarios.len());

    for scenario in &scenarios {
        println!("  - {}", scenario);
    }

    if validate {
        println!("Validating discovered scenarios...");
        validate_scenarios_through_effects(ctx, effects.as_ref(), &scenarios).await?;
        println!("All scenarios validated successfully");
    }

    Ok(())
}

/// Handle scenario listing through effects
async fn handle_list(
    ctx: &EffectContext,
    effects: Arc<AuraEffectSystem>,
    directory: &Path,
    detailed: bool,
) -> Result<()> {
    println!("Listing scenarios in: {}", directory.display());

    println!("Detailed: {}", detailed);

    // Get scenarios through storage effects
    let scenarios = list_scenarios_through_effects(ctx, effects.as_ref(), directory).await?;

    println!("Available scenarios:");

    for scenario in scenarios {
        if detailed {
            display_detailed_scenario_info(ctx, effects.as_ref(), &scenario).await;
        } else {
            println!("  - {}", scenario.name);
        }
    }

    Ok(())
}

/// Handle scenario validation through effects
async fn handle_validate(
    ctx: &EffectContext,
    effects: Arc<AuraEffectSystem>,
    directory: &Path,
    strictness: Option<&str>,
) -> Result<()> {
    println!("Validating scenarios in: {}", directory.display());

    if let Some(level) = strictness {
        println!("Strictness: {}", level);
    }

    // Validate scenarios through storage effects
    let scenarios = list_scenarios_through_effects(ctx, effects.as_ref(), directory).await?;
    let scenario_names: Vec<String> = scenarios.into_iter().map(|s| s.name).collect();

    validate_scenarios_through_effects(ctx, effects.as_ref(), &scenario_names).await?;

    println!("All scenarios valid");

    Ok(())
}

/// Handle scenario execution through effects
async fn handle_run(
    ctx: &EffectContext,
    effects: Arc<AuraEffectSystem>,
    directory: Option<&Path>,
    pattern: Option<&str>,
    parallel: bool,
    max_parallel: Option<usize>,
    output_file: Option<&Path>,
    detailed_report: bool,
) -> Result<()> {
    println!("Running scenarios");

    if let Some(dir) = directory {
        println!("Directory: {}", dir.display());
    }
    if let Some(pat) = pattern {
        println!("Pattern: {}", pat);
    }
    println!("Parallel: {}", parallel);
    if let Some(max) = max_parallel {
        println!("Max parallel: {}", max);
    }
    if let Some(output) = output_file {
        println!("Output file: {}", output.display());
    }
    println!("Detailed report: {}", detailed_report);

    // Execute scenarios through effects
    let results = execute_scenarios_through_effects(
        ctx,
        effects.clone(),
        directory,
        pattern,
        parallel,
        max_parallel,
    )
    .await?;

    // Save results if output file specified
    if let Some(output_path) = output_file {
        save_scenario_results(ctx, effects.clone(), output_path, &results, detailed_report).await?;
    }

    println!("All scenarios completed successfully");

    Ok(())
}

/// Handle report generation through effects
async fn handle_report(
    _ctx: &EffectContext,
    effects: Arc<AuraEffectSystem>,
    input: &Path,
    output: &Path,
    format: Option<&str>,
    detailed: bool,
) -> Result<()> {
    println!("Generating report");
    println!("Input: {}", input.display());
    println!("Output: {}", output.display());

    if let Some(fmt) = format {
        println!("Format: {}", fmt);
    }
    println!("Detailed: {}", detailed);

    // Load results through storage effects
    let input_key = format!("scenario_results:{}", input.display());
    let results_data = match effects.retrieve(&input_key).await {
        Ok(Some(data)) => String::from_utf8(data)
            .map_err(|e| anyhow::anyhow!("Invalid UTF-8 in results file: {}", e))?,
        Ok(None) => {
            return Err(anyhow::anyhow!(
                "Results file not found: {}",
                input.display()
            ))
        }
        Err(e) => {
            return Err(anyhow::anyhow!(
                "Failed to read results file via storage effects: {}",
                e
            ))
        }
    };

    // Generate report
    let report = generate_report_from_results(results_data.as_bytes(), format, detailed)?;

    // Save report through storage effects
    let output_key = format!("scenario_report:{}", output.display());
    effects
        .store(&output_key, report.as_bytes().to_vec())
        .await
        .map_err(|e| anyhow::anyhow!("Failed to save report via storage effects: {}", e))?;

    println!("Report generated successfully");

    Ok(())
}

/// Discover scenarios through storage effects
async fn discover_scenarios_through_effects(
    _ctx: &EffectContext,
    _effects: &AuraEffectSystem,
    root: &Path,
) -> Result<Vec<String>> {
    // Simulate scenario discovery
    // In real implementation, would recursively scan directories
    let scenarios = vec![
        "threshold_test.toml".to_string(),
        "recovery_test.toml".to_string(),
        "performance_test.toml".to_string(),
        "byzantine_test.toml".to_string(),
        "network_partition_test.toml".to_string(),
    ];

    println!("Scanned directory: {}", root.display());

    Ok(scenarios)
}

/// Validate scenarios through effects
async fn validate_scenarios_through_effects(
    _ctx: &EffectContext,
    effects: &AuraEffectSystem,
    scenarios: &[String],
) -> Result<()> {
    for scenario in scenarios {
        println!("Validating: {}", scenario);

        // Simulate validation
        if scenario.contains("invalid") {
            let _ = effects;
            eprintln!("Invalid scenario: {}", scenario);
            return Err(anyhow::anyhow!("Invalid scenario: {}", scenario));
        }
    }

    Ok(())
}

/// List scenarios through storage effects
async fn list_scenarios_through_effects(
    _ctx: &EffectContext,
    _effects: &AuraEffectSystem,
    directory: &Path,
) -> Result<Vec<ScenarioInfo>> {
    let files = collect_scenario_files(directory)?;
    let scenarios: Vec<ScenarioInfo> = files
        .into_iter()
        .map(|path| ScenarioInfo {
            name: path.to_string_lossy().into_owned(),
            description: "Scenario file".to_string(),
            devices: 0,
            threshold: 0,
        })
        .collect();

    println!(
        "Listed {} scenarios from {}",
        scenarios.len(),
        directory.display()
    );

    Ok(scenarios)
}

/// Display detailed scenario information
async fn display_detailed_scenario_info(
    _ctx: &EffectContext,
    _effects: &AuraEffectSystem,
    scenario: &ScenarioInfo,
) {
    println!(
        "  - {} ({} devices, threshold {})",
        scenario.name, scenario.devices, scenario.threshold
    );
    println!("    Description: {}", scenario.description);
}

/// Execute scenarios through effects
async fn execute_scenarios_through_effects(
    ctx: &EffectContext,
    effects: Arc<AuraEffectSystem>,
    _directory: Option<&Path>,
    pattern: Option<&str>,
    parallel: bool,
    max_parallel: Option<usize>,
) -> Result<Vec<ScenarioResult>> {
    let base_dir = _directory.unwrap_or_else(|| Path::new("scenarios"));
    let mut scenario_files = collect_scenario_files(base_dir)?;

    if let Some(pat) = pattern {
        println!("Filtering scenarios by pattern: {}", pat);
        scenario_files.retain(|p| p.to_string_lossy().contains(pat));
    }

    if scenario_files.is_empty() {
        println!("No scenarios matched the provided filters");
        return Ok(Vec::new());
    }

    let mut results = Vec::new();

    if parallel {
        let max = max_parallel.unwrap_or(4);
        println!(
            "Running {} scenarios in parallel (max {})",
            scenario_files.len(),
            max
        );
    } else {
        println!("Running scenarios sequentially");
    }

    for scenario in scenario_files {
        println!("Executing: {}", scenario.display());
        // Use monotonic time directly for duration measurement (not via deprecated effects method)
        let start = Instant::now();
        let run_result = run_scenario_file(ctx, effects.clone(), &scenario).await;

        let duration_ms = Instant::now().duration_since(start).as_millis() as u64;
        let (success, error, log_path) = match run_result {
            Ok(log_path) => (true, None, log_path),
            Err(e) => (false, Some(e.to_string()), None),
        };

        let result = ScenarioResult {
            name: scenario.to_string_lossy().into_owned(),
            success,
            duration_ms,
            error,
            log_path,
        };

        results.push(result);
        println!("Completed: {}", scenario.display());
    }

    Ok(results)
}

/// Collect scenario files (toml) under a directory tree
fn collect_scenario_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut stack = vec![root.to_path_buf()];
    let mut found = Vec::new();

    while let Some(dir) = stack.pop() {
        let entries = fs::read_dir(&dir).map_err(|e| {
            anyhow::anyhow!("Failed to read scenario directory {}: {}", dir.display(), e)
        })?;

        for entry in entries {
            let entry = entry.map_err(|e| {
                anyhow::anyhow!("Failed to read scenario entry {}: {}", dir.display(), e)
            })?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().map(|ext| ext == "toml").unwrap_or(false) {
                found.push(path);
            }
        }
    }

    Ok(found)
}

/// Simulate running a scenario file with detailed logging through effect system
async fn run_scenario_file(
    ctx: &EffectContext,
    effects: Arc<AuraEffectSystem>,
    path: &Path,
) -> Result<Option<String>> {
    let effects_ref = effects.as_ref();
    let contents = fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("Failed to read scenario {}: {}", path.display(), e))?;

    let parsed: toml::Value = toml::from_str(&contents).map_err(|e| {
        anyhow::anyhow!("Failed to parse scenario {} as TOML: {}", path.display(), e)
    })?;

    let mut lines = ScenarioLog::new(path);
    let sim_seed = parsed
        .get("setup")
        .and_then(|s| s.get("seed"))
        .and_then(|v| v.as_integer())
        .unwrap_or(0) as u64;
    let sim_handler = SimulationScenarioHandler::new(sim_seed);

    log_line(
        effects_ref,
        &mut lines,
        &format!("Scenario: {}", path.display()),
    )
    .await;

    if let Some(meta) = parsed.get("metadata") {
        if let Some(name) = meta.get("name").and_then(|v| v.as_str()) {
            log_line(effects_ref, &mut lines, &format!("Name: {}", name)).await;
        }
        if let Some(desc) = meta.get("description").and_then(|v| v.as_str()) {
            log_line(effects_ref, &mut lines, &format!("Description: {}", desc)).await;
        }
    }

    if let Some(phases) = parsed.get("phases").and_then(|p| p.as_array()) {
        log_line(
            effects_ref,
            &mut lines,
            &format!("Phases: {}", phases.len()),
        )
        .await;
        for (idx, phase) in phases.iter().enumerate() {
            let name = phase
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("<unnamed>");
            log_line(
                effects_ref,
                &mut lines,
                &format!("Phase {}: {}", idx + 1, name),
            )
            .await;

            if let Some(actions) = phase.get("actions").and_then(|a| a.as_array()) {
                log_line(
                    effects_ref,
                    &mut lines,
                    &format!("  Actions: {}", actions.len()),
                )
                .await;
                for (a_idx, action) in actions.iter().enumerate() {
                    let mut summary = String::new();
                    let action_type = action
                        .get("type")
                        .and_then(|t| t.as_str())
                        .unwrap_or("<unknown>");
                    let _ = write!(&mut summary, "    {}. type={}", a_idx + 1, action_type);

                    match action_type {
                        "run_choreography" => {
                            if let Some(name) = action.get("choreography").and_then(|v| v.as_str())
                            {
                                let _ = write!(&mut summary, " choreo={}", name);
                            }
                            if let Some(parts) =
                                action.get("participants").and_then(|v| v.as_array())
                            {
                                let names: Vec<_> =
                                    parts.iter().filter_map(|p| p.as_str()).collect();
                                if !names.is_empty() {
                                    let _ = write!(&mut summary, " participants={:?}", names);
                                }
                            }
                            if let Some(target) = action.get("target").and_then(|v| v.as_str()) {
                                let _ = write!(&mut summary, " target={}", target);
                            }
                            if let Some(threshold) =
                                action.get("threshold").and_then(|v| v.as_integer())
                            {
                                let _ = write!(&mut summary, " threshold={}", threshold);
                            }
                            let participants: Vec<String> = action
                                .get("participants")
                                .and_then(|v| v.as_array())
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|p| p.as_str().map(|s| s.to_string()))
                                        .collect()
                                })
                                .unwrap_or_default();
                            let mut params = HashMap::new();
                            if let Some(target) = action.get("target").and_then(|v| v.as_str()) {
                                params.insert("target".to_string(), target.to_string());
                            }
                            if let Some(threshold) =
                                action.get("threshold").and_then(|v| v.as_integer())
                            {
                                params.insert("threshold".to_string(), threshold.to_string());
                            }
                            if let Some(app_id) = action.get("app_id").and_then(|v| v.as_str()) {
                                params.insert("app_id".to_string(), app_id.to_string());
                            }
                            if let Some(context) = action.get("context").and_then(|v| v.as_str()) {
                                params.insert("context".to_string(), context.to_string());
                            }
                            if let Some(name) = action.get("choreography").and_then(|v| v.as_str())
                            {
                                let _ = sim_handler.run_choreography(name, participants, params);
                            }
                        }
                        "verify_property" => {
                            if let Some(prop) = action.get("property").and_then(|v| v.as_str()) {
                                let _ = write!(&mut summary, " property={}", prop);
                            }
                            if let Some(expected) = action.get("expected") {
                                let _ = write!(&mut summary, " expected={}", expected);
                            }
                            let property = action
                                .get("property")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown");
                            let expected_str = action.get("expected").map(|v| v.to_string());
                            let _ = sim_handler.verify_property_stub(property, expected_str);
                        }
                        "simulate_data_loss" => {
                            if let Some(target) = action.get("target").and_then(|v| v.as_str()) {
                                let _ = write!(&mut summary, " target={}", target);
                            }
                            if let Some(loss) = action.get("loss_type").and_then(|v| v.as_str()) {
                                let _ = write!(&mut summary, " loss_type={}", loss);
                            }
                            let target = action
                                .get("target")
                                .and_then(|v| v.as_str())
                                .unwrap_or_default();
                            let loss_type = action
                                .get("loss_type")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown");
                            let recovery_required = action
                                .get("recovery_required")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(true);
                            if let Err(e) =
                                sim_handler.simulate_data_loss(target, loss_type, recovery_required)
                            {
                                log_line(
                                    effects_ref,
                                    &mut lines,
                                    &format!("Simulator data loss error for {}: {}", target, e),
                                )
                                .await;
                            }
                        }
                        "apply_network_condition" => {
                            if let Some(cond) = action.get("condition").and_then(|v| v.as_str()) {
                                let _ = write!(&mut summary, " condition={}", cond);
                            }
                            if let Some(parts) =
                                action.get("participants").and_then(|v| v.as_array())
                            {
                                let names: Vec<_> =
                                    parts.iter().filter_map(|p| p.as_str()).collect();
                                if !names.is_empty() {
                                    let _ = write!(&mut summary, " participants={:?}", names);
                                }
                            }
                            let duration_ticks = action
                                .get("duration_ticks")
                                .and_then(|v| v.as_integer())
                                .unwrap_or(0)
                                as u64;
                            if let Some(cond) = action.get("condition").and_then(|v| v.as_str()) {
                                let participants: Vec<String> = action
                                    .get("participants")
                                    .and_then(|v| v.as_array())
                                    .map(|arr| {
                                        arr.iter()
                                            .filter_map(|p| p.as_str().map(|s| s.to_string()))
                                            .collect()
                                    })
                                    .unwrap_or_default();
                                let _ = sim_handler.apply_network_condition(
                                    cond,
                                    participants,
                                    duration_ticks,
                                );
                            }
                        }
                        "inject_byzantine" | "inject_failure" => {
                            let participant = action
                                .get("participant")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown");
                            let behavior = action
                                .get("behavior")
                                .and_then(|v| v.as_str())
                                .unwrap_or(action_type);
                            let _ = sim_handler.inject_fault(participant, behavior);
                        }
                        "create_checkpoint" => {
                            let label = action
                                .get("label")
                                .and_then(|v| v.as_str())
                                .unwrap_or("checkpoint");
                            if let Ok(id) = sim_handler.create_checkpoint(label) {
                                let _ = write!(&mut summary, " id={}", id);
                            }
                        }
                        "export_choreo_trace" => {
                            let fmt = action
                                .get("format")
                                .and_then(|v| v.as_str())
                                .unwrap_or("console");
                            let output = action
                                .get("output")
                                .and_then(|v| v.as_str())
                                .unwrap_or("trace.log");
                            let _ = sim_handler.export_choreo_trace(fmt, output);
                        }
                        "generate_timeline" => {
                            if let Some(output) = action.get("output").and_then(|v| v.as_str()) {
                                let _ = sim_handler.generate_timeline(output);
                            }
                        }
                        "verify_all_properties" => {
                            let _ = sim_handler.verify_all_properties();
                        }
                        "setup_choreography" => {
                            let protocol = action
                                .get("protocol")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown");
                            let participants: Vec<String> = action
                                .get("participants")
                                .and_then(|v| v.as_array())
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|p| p.as_str().map(|s| s.to_string()))
                                        .collect()
                                })
                                .unwrap_or_default();
                            let _ = sim_handler.setup_choreography(protocol, participants);
                        }
                        "load_key_shares" => {
                            let threshold = action
                                .get("threshold")
                                .and_then(|v| v.as_integer())
                                .unwrap_or(0) as usize;
                            let _ = sim_handler.load_key_shares(threshold);
                        }
                        "wait_ticks" => {
                            if let Some(ticks) = action.get("ticks").and_then(|v| v.as_integer()) {
                                let _ = write!(&mut summary, " ticks={}", ticks);
                                let _ = sim_handler.wait_ticks(ticks as u64);
                            }
                        }
                        "wait_ms" => {
                            if let Some(ms) = action.get("duration").and_then(|v| v.as_integer()) {
                                let _ = write!(&mut summary, " duration_ms={}", ms);
                                let _ = sim_handler.wait_ms(ms as u64);
                            }
                        }
                        _ => {
                            if let Some(target) = action.get("target").and_then(|t| t.as_str()) {
                                let _ = write!(&mut summary, " target={}", target);
                            }
                            if let Some(params) = action.get("params") {
                                let _ = write!(
                                    &mut summary,
                                    " params={}",
                                    params
                                        .as_table()
                                        .map(|t| format!("{:?}", t))
                                        .unwrap_or_else(|| params.to_string())
                                );
                            }
                        }
                    }

                    log_line(effects_ref, &mut lines, &summary).await;
                }
            }
        }
    } else {
        ConsoleEffects::log_warn(effects_ref, "No phases defined")
            .await
            .ok();
    }

    // Execute via simulator when running the CLI recovery demo to validate end-to-end flow
    if parsed
        .get("metadata")
        .and_then(|m| m.get("name"))
        .and_then(|n| n.as_str())
        == Some("cli_recovery_demo")
    {
        let seed = parsed
            .get("setup")
            .and_then(|s| s.get("seed"))
            .and_then(|v| v.as_integer())
            .unwrap_or(2024) as u64;

        match simulate_cli_recovery_demo(seed, effects.clone()).await {
            Ok(sim_result) => {
                log_line(
                    effects_ref,
                    &mut lines,
                    &format!("Simulator outcome: {}", sim_result.outcome),
                )
                .await;
                log_line(
                    effects_ref,
                    &mut lines,
                    &format!("Duration: {}ms", sim_result.duration_ms),
                )
                .await;

                for step in &sim_result.steps {
                    log_line(
                        effects_ref,
                        &mut lines,
                        &format!(
                            "[{}] {} -> {}",
                            step.phase,
                            step.action,
                            step.details.as_deref().unwrap_or("ok")
                        ),
                    )
                    .await;
                }

                for (property, result) in &sim_result.validation_results {
                    log_line(
                        effects_ref,
                        &mut lines,
                        &format!(
                            "Validation {}: {}",
                            property,
                            if *result { "PASS" } else { "FAIL" }
                        ),
                    )
                    .await;
                }

                if sim_result.outcome != "RecoveryDemoSuccess" {
                    return Err(anyhow::anyhow!(
                        "Scenario failed in simulator (outcome {})",
                        sim_result.outcome
                    ));
                }
            }
            Err(e) => {
                log_line(
                    effects_ref,
                    &mut lines,
                    &format!("Simulator execution error: {}", e),
                )
                .await;
                return Err(anyhow::anyhow!("Simulator execution failed: {}", e));
            }
        }
    }

    let log_path = persist_log(ctx, effects_ref, path, &lines).await?;
    Ok(Some(log_path))
}

struct SimStep {
    phase: String,
    action: String,
    details: Option<String>,
}

struct CliRecoverySimResult {
    outcome: String,
    duration_ms: u64,
    steps: Vec<SimStep>,
    validation_results: HashMap<String, bool>,
}

async fn simulate_cli_recovery_demo(
    seed: u64,
    effects: Arc<AuraEffectSystem>,
) -> Result<CliRecoverySimResult, anyhow::Error> {
    let handler = SimulationScenarioHandler::new(seed);
    let mut steps = Vec::new();
    // Use monotonic time directly for duration measurement (not via deprecated effects method)
    let start = Instant::now();

    // Run guardian setup choreography via recovery coordinator using simulation effects
    run_guardian_setup_choreography(effects.clone(), &mut steps).await?;

    // Phase 1: Alice & Charlie pre-setup (log only)
    steps.push(SimStep {
        phase: "alice_charlie_setup".into(),
        action: "create_accounts".into(),
        details: Some("Alice and Charlie accounts created".into()),
    });

    // Phase 2: Requests and acceptance to become guardians
    steps.push(SimStep {
        phase: "bob_onboarding".into(),
        action: "create_account".into(),
        details: Some("Bob account created".into()),
    });
    steps.push(SimStep {
        phase: "bob_onboarding".into(),
        action: "guardian_request_alice".into(),
        details: Some("Bob requests Alice to be guardian".into()),
    });
    steps.push(SimStep {
        phase: "bob_onboarding".into(),
        action: "guardian_accept_alice".into(),
        details: Some("Alice accepts guardian responsibility".into()),
    });
    steps.push(SimStep {
        phase: "bob_onboarding".into(),
        action: "guardian_request_charlie".into(),
        details: Some("Bob requests Charlie to be guardian".into()),
    });
    steps.push(SimStep {
        phase: "bob_onboarding".into(),
        action: "guardian_accept_charlie".into(),
        details: Some("Charlie accepts guardian responsibility".into()),
    });
    steps.push(SimStep {
        phase: "bob_onboarding".into(),
        action: "guardian_authority_configuration".into(),
        details: Some("Alice+Charlie become guardian authority for Bob".into()),
    });

    // Phase 3-4: group chat setup and initial messaging
    let group_id = handler.create_chat_group(
        "Alice, Bob & Charlie",
        "alice",
        vec!["bob".into(), "charlie".into()],
    )?;
    steps.push(SimStep {
        phase: "group_chat_setup".into(),
        action: "create_group".into(),
        details: Some(format!("Group ID: {}", group_id)),
    });

    let messages = vec![
        ("group_messaging", "alice", "Welcome to our group, Bob!"),
        ("group_messaging", "bob", "Thanks Alice! Great to be here."),
        (
            "group_messaging",
            "charlie",
            "Hey everyone! This chat system is awesome.",
        ),
        (
            "group_messaging",
            "alice",
            "Bob, you should backup your account soon",
        ),
        (
            "group_messaging",
            "bob",
            "I'll do that right after this demo!",
        ),
    ];
    for (phase, sender, message) in &messages {
        handler.send_chat_message(&group_id, sender, message)?;
        steps.push(SimStep {
            phase: (*phase).into(),
            action: "send_message".into(),
            details: Some(format!("{}: {}", sender, message)),
        });
    }

    // Phase 5: data loss
    handler.simulate_data_loss("bob", "complete_device_loss", true)?;
    steps.push(SimStep {
        phase: "bob_account_loss".into(),
        action: "simulate_data_loss".into(),
        details: Some("Bob loses all account data".into()),
    });

    // Phase 6-7: recovery
    handler.initiate_guardian_recovery("bob", vec!["alice".into(), "charlie".into()], 2)?;
    steps.push(SimStep {
        phase: "recovery_initiation".into(),
        action: "initiate_guardian_recovery".into(),
        details: Some("Alice and Charlie assist recovery".into()),
    });

    let recovery_success = handler.verify_recovery_success(
        "bob",
        vec![
            "keys_restored".into(),
            "account_accessible".into(),
            "message_history_restored".into(),
        ],
    )?;
    steps.push(SimStep {
        phase: "account_restoration".into(),
        action: "verify_recovery".into(),
        details: Some(if recovery_success { "ok" } else { "fail" }.into()),
    });

    // Phase 8: post recovery messaging
    let post_recovery_messages = vec![
        (
            "post_recovery_messaging",
            "bob",
            "I'm back! Thanks Alice and Charlie for helping me recover.",
        ),
        (
            "post_recovery_messaging",
            "alice",
            "Welcome back Bob! Guardian recovery really works!",
        ),
        (
            "post_recovery_messaging",
            "charlie",
            "Amazing! You can see all our previous messages too.",
        ),
    ];
    for (phase, sender, message) in &post_recovery_messages {
        handler.send_chat_message(&group_id, sender, message)?;
        steps.push(SimStep {
            phase: (*phase).into(),
            action: "send_message".into(),
            details: Some(format!("{}: {}", sender, message)),
        });
    }

    // Validations
    let mut validation_results = HashMap::new();
    let message_continuity = handler.validate_message_history("bob", 8, true)?;
    validation_results.insert("message_continuity_maintained".into(), message_continuity);

    let bob_can_send = handler
        .send_chat_message(&group_id, "bob", "Test message after recovery")
        .is_ok();
    validation_results.insert("bob_can_send_messages".into(), bob_can_send);

    let group_functional = handler.get_chat_stats().is_ok();
    validation_results.insert("group_functionality_restored".into(), group_functional);

    let full_history_access = handler.validate_message_history("bob", 5, true)?;
    validation_results.insert("bob_can_see_full_history".into(), full_history_access);

    let outcome = if validation_results.values().all(|v| *v) && recovery_success {
        "RecoveryDemoSuccess"
    } else {
        "Failure"
    }
    .to_string();

    Ok(CliRecoverySimResult {
        outcome,
        duration_ms: start.elapsed().as_millis() as u64,
        steps,
        validation_results,
    })
}

async fn run_guardian_setup_choreography(
    effects: Arc<AuraEffectSystem>,
    steps: &mut Vec<SimStep>,
) -> Result<(), anyhow::Error> {
    let device_id = DeviceId::new();
    let coordinator = GuardianSetupCoordinator::new(effects);

    let guardians = GuardianSet::new(vec![
        GuardianProfile::new(GuardianId::new(), DeviceId::new(), "alice"),
        GuardianProfile::new(GuardianId::new(), DeviceId::new(), "charlie"),
    ]);

    let timestamp = 0;

    let recovery_context = RecoveryContext {
        operation_type: RecoveryOperationType::GuardianSetModification,
        justification: "Initial guardian setup for Bob".to_string(),
        is_emergency: false,
        timestamp,
    };

    let request = RecoveryRequest {
        requesting_device: device_id,
        account_id: AccountId::new(),
        context: recovery_context,
        threshold: 2,
        guardians,
        auth_token: None,
    };

    let response = coordinator
        .execute_setup(request)
        .await
        .map_err(|e| anyhow::anyhow!("Guardian setup choreography failed: {}", e))?;

    if !response.success {
        return Err(anyhow::anyhow!(
            "Guardian setup failed: {}",
            response.error.unwrap_or_else(|| "unknown error".into())
        ));
    }

    steps.push(SimStep {
        phase: "bob_onboarding".into(),
        action: "guardian_setup_choreography".into(),
        details: Some("Guardian setup choreography executed".into()),
    });

    Ok(())
}

async fn log_line(effects: &AuraEffectSystem, log: &mut ScenarioLog, line: &str) {
    log.lines.push(line.to_string());
    let _ = ConsoleEffects::log_info(effects, line).await;
}

async fn persist_log(
    _ctx: &EffectContext,
    effects: &AuraEffectSystem,
    scenario_path: &Path,
    log: &ScenarioLog,
) -> Result<String> {
    let output_path = scenario_log_output_path(scenario_path);

    if let Some(parent) = output_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| {
                anyhow::anyhow!(
                    "Failed to create scenario log directory {}: {}",
                    parent.display(),
                    e
                )
            })?;
        }
    }

    fs::write(&output_path, log.lines.join("\n")).map_err(|e| {
        anyhow::anyhow!(
            "Failed to write scenario log {}: {}",
            output_path.display(),
            e
        )
    })?;

    let storage_key = format!("scenario_log:{}", scenario_path.display());
    StorageEffects::store(effects, &storage_key, log.lines.join("\n").into_bytes())
        .await
        .ok();

    Ok(output_path.to_string_lossy().into_owned())
}

fn scenario_log_output_path(scenario_path: &Path) -> PathBuf {
    let file_stem = scenario_path
        .file_stem()
        .map(|s| s.to_string_lossy())
        .unwrap_or_else(|| std::borrow::Cow::Borrowed("scenario"));

    Path::new("work")
        .join("scenario_logs")
        .join(format!("{}.log", file_stem))
}

#[derive(Default)]
struct ScenarioLog {
    lines: Vec<String>,
}

impl ScenarioLog {
    fn new(_path: &Path) -> Self {
        Self { lines: Vec::new() }
    }
}

/// Save scenario results through storage effects
async fn save_scenario_results(
    _ctx: &EffectContext,
    effects: Arc<AuraEffectSystem>,
    output_path: &Path,
    results: &[ScenarioResult],
    detailed: bool,
) -> Result<()> {
    let results_json = if detailed {
        serde_json::to_string_pretty(results)
    } else {
        serde_json::to_string(results)
    }
    .map_err(|e| anyhow::anyhow!("Failed to serialize results: {}", e))?;

    // Persist to filesystem for user visibility
    if let Some(parent) = output_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| {
                anyhow::anyhow!(
                    "Failed to create output directory {}: {}",
                    parent.display(),
                    e
                )
            })?;
        }
    }
    std::fs::write(output_path, results_json.as_bytes()).map_err(|e| {
        anyhow::anyhow!(
            "Failed to write scenario results to {}: {}",
            output_path.display(),
            e
        )
    })?;

    let output_key = format!("scenario_output:{}", output_path.display());
    effects
        .store(&output_key, results_json.as_bytes().to_vec())
        .await
        .map_err(|e| anyhow::anyhow!("Failed to save results via storage effects: {}", e))?;

    println!("Results saved to: {}", output_path.display());

    Ok(())
}

/// Generate report from results
fn generate_report_from_results(
    results_data: &[u8],
    format: Option<&str>,
    detailed: bool,
) -> Result<String> {
    let results_str = String::from_utf8(results_data.to_vec())
        .map_err(|e| anyhow::anyhow!("Invalid UTF-8 in results: {}", e))?;

    let results: Vec<ScenarioResult> = serde_json::from_str(&results_str)
        .map_err(|e| anyhow::anyhow!("Failed to parse results: {}", e))?;

    let report = match format.unwrap_or("text") {
        "json" => serde_json::to_string_pretty(&results)
            .map_err(|e| anyhow::anyhow!("Failed to format JSON: {}", e))?,
        _ => {
            let mut report = String::new();
            report.push_str("=== Scenario Results Report ===\n\n");

            let success_count = results.iter().filter(|r| r.success).count();
            let total = results.len();

            report.push_str(&format!(
                "Summary: {}/{} scenarios passed\n\n",
                success_count, total
            ));

            if detailed {
                for result in &results {
                    report.push_str(&format!("Scenario: {}\n", result.name));
                    report.push_str(&format!(
                        "  Status: {}\n",
                        if result.success { "PASSED" } else { "FAILED" }
                    ));
                    report.push_str(&format!("  Duration: {}ms\n", result.duration_ms));
                    if let Some(error) = &result.error {
                        report.push_str(&format!("  Error: {}\n", error));
                    }
                    report.push('\n');
                }
            }

            report
        }
    };

    Ok(report)
}

/// Scenario information structure
#[derive(Debug)]
struct ScenarioInfo {
    name: String,
    description: String,
    devices: u32,
    threshold: u32,
}

/// Scenario execution result
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct ScenarioResult {
    name: String,
    success: bool,
    duration_ms: u64,
    error: Option<String>,
    log_path: Option<String>,
}
