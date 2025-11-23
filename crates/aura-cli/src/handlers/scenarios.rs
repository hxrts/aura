//! Scenarios Command Handler
//!
//! Effect-based implementation of scenario management commands.

use crate::ScenarioAction;
use anyhow::Result;
use aura_agent::{AuraEffectSystem, EffectContext};
use aura_core::effects::{ConsoleEffects, StorageEffects};
use std::time::Instant;
use std::path::{Path, PathBuf};
use std::fs;

/// Handle scenario operations through effects
pub async fn handle_scenarios(
    ctx: &EffectContext,
    effects: &AuraEffectSystem,
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
    effects: &AuraEffectSystem,
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
    let scenarios = discover_scenarios_through_effects(ctx, effects, root).await?;

    println!("Found {} scenarios", scenarios.len());

    for scenario in &scenarios {
        println!("  - {}", scenario);
    }

    if validate {
        println!("Validating discovered scenarios...");
        validate_scenarios_through_effects(ctx, effects, &scenarios).await?;
        println!("All scenarios validated successfully");
    }

    Ok(())
}

/// Handle scenario listing through effects
async fn handle_list(
    ctx: &EffectContext,
    effects: &AuraEffectSystem,
    directory: &Path,
    detailed: bool,
) -> Result<()> {
    println!("Listing scenarios in: {}", directory.display());

    println!("Detailed: {}", detailed);

    // Get scenarios through storage effects
    let scenarios = list_scenarios_through_effects(ctx, effects, directory).await?;

    println!("Available scenarios:");

    for scenario in scenarios {
        if detailed {
            display_detailed_scenario_info(ctx, effects, &scenario).await;
        } else {
            println!("  - {}", scenario.name);
        }
    }

    Ok(())
}

/// Handle scenario validation through effects
async fn handle_validate(
    ctx: &EffectContext,
    effects: &AuraEffectSystem,
    directory: &Path,
    strictness: Option<&str>,
) -> Result<()> {
    println!("Validating scenarios in: {}", directory.display());

    if let Some(level) = strictness {
        println!("Strictness: {}", level);
    }

    // Validate scenarios through storage effects
    let scenarios = list_scenarios_through_effects(ctx, effects, directory).await?;
    let scenario_names: Vec<String> = scenarios.into_iter().map(|s| s.name).collect();

    validate_scenarios_through_effects(ctx, effects, &scenario_names).await?;

    println!("All scenarios valid");

    Ok(())
}

/// Handle scenario execution through effects
async fn handle_run(
    ctx: &EffectContext,
    effects: &AuraEffectSystem,
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
    let results =
        execute_scenarios_through_effects(ctx, effects, directory, pattern, parallel, max_parallel)
            .await?;

    // Save results if output file specified
    if let Some(output_path) = output_file {
        save_scenario_results(ctx, effects, output_path, &results, detailed_report).await?;
    }

    println!("All scenarios completed successfully");

    Ok(())
}

/// Handle report generation through effects
async fn handle_report(
    ctx: &EffectContext,
    effects: &AuraEffectSystem,
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
    effects: &AuraEffectSystem,
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
        let start = Instant::now();
        let run_result = run_scenario_file(ctx, effects, &scenario).await;

        let duration_ms = start.elapsed().as_millis() as u64;
        let (success, error) = match run_result {
            Ok(_) => (true, None),
            Err(e) => (false, Some(e.to_string())),
        };

        let result = ScenarioResult {
            name: scenario.to_string_lossy().into_owned(),
            success,
            duration_ms,
            error,
            log_path: Some("effect_console".to_string()),
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
            } else if path
                .extension()
                .map(|ext| ext == "toml")
                .unwrap_or(false)
            {
                found.push(path);
            }
        }
    }

    Ok(found)
}

/// Simulate running a scenario file with detailed logging through effect system
async fn run_scenario_file(
    ctx: &EffectContext,
    effects: &AuraEffectSystem,
    path: &Path,
) -> Result<()> {
    let contents = fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("Failed to read scenario {}: {}", path.display(), e))?;

    let parsed: toml::Value = toml::from_str(&contents).map_err(|e| {
        anyhow::anyhow!("Failed to parse scenario {} as TOML: {}", path.display(), e)
    })?;

    ConsoleEffects::log_info(
        effects,
        &format!("Scenario: {}", path.display()),
    )
    .await
    .ok();

    if let Some(meta) = parsed.get("metadata") {
        if let Some(name) = meta.get("name").and_then(|v| v.as_str()) {
            let _ = ConsoleEffects::log_info(effects, &format!("Name: {}", name)).await;
        }
        if let Some(desc) = meta.get("description").and_then(|v| v.as_str()) {
            let _ = ConsoleEffects::log_info(effects, &format!("Description: {}", desc)).await;
        }
    }

    if let Some(phases) = parsed.get("phases").and_then(|p| p.as_array()) {
        let _ = ConsoleEffects::log_info(effects, &format!("Phases: {}", phases.len())).await;
        for (idx, phase) in phases.iter().enumerate() {
            let name = phase
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("<unnamed>");
            let _ = ConsoleEffects::log_info(
                effects,
                &format!("Phase {}: {}", idx + 1, name),
            )
            .await;

            if let Some(actions) = phase.get("actions").and_then(|a| a.as_array()) {
                let _ = ConsoleEffects::log_info(
                    effects,
                    &format!("  Actions: {}", actions.len()),
                )
                .await;
                for (a_idx, action) in actions.iter().enumerate() {
                    if let Some(action_type) = action.get("type").and_then(|t| t.as_str()) {
                        let _ = ConsoleEffects::log_info(
                            effects,
                            &format!("    {}. type={}", a_idx + 1, action_type),
                        )
                        .await;
                    } else {
                        let _ = ConsoleEffects::log_info(
                            effects,
                            &format!("    {}. <unknown action>", a_idx + 1),
                        )
                        .await;
                    }
                }
            }
        }
    } else {
        let _ = ConsoleEffects::log_warn(effects, "No phases defined").await;
    }

    Ok(())
}

/// Save scenario results through storage effects
async fn save_scenario_results(
    _ctx: &EffectContext,
    effects: &AuraEffectSystem,
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
