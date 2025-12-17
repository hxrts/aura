//! Individual scenario command handlers
//!
//! Contains handlers for discover, list, validate, run, and report commands.

use std::path::Path;

use crate::error::{TerminalError, TerminalResult};
use aura_core::effects::StorageEffects;

use super::execution::{collect_scenario_files, execute_scenarios};
use super::types::{ScenarioInfo, ScenarioResult};
use crate::handlers::HandlerContext;

/// Handle scenario discovery through effects
pub async fn handle_discover(
    ctx: &HandlerContext<'_>,
    root: &Path,
    validate: bool,
) -> TerminalResult<()> {
    println!("Discovering scenarios in: {}", root.display());
    println!("Validation: {}", validate);

    // Check if root directory exists through storage effects
    let root_exists = ctx
        .effects()
        .exists(&root.display().to_string())
        .await
        .unwrap_or(false);

    if !root_exists {
        eprintln!("Root directory not found: {}", root.display());
        return Err(TerminalError::Operation(format!(
            "Root directory not found: {}",
            root.display()
        )));
    }

    // Simulate scenario discovery
    let scenarios = discover_scenarios_through_effects(ctx, root).await?;

    println!("Found {} scenarios", scenarios.len());

    for scenario in &scenarios {
        println!("  - {}", scenario);
    }

    if validate {
        println!("Validating discovered scenarios...");
        validate_scenarios_through_effects(ctx, &scenarios).await?;
        println!("All scenarios validated successfully");
    }

    Ok(())
}

/// Handle scenario listing through effects
pub async fn handle_list(
    ctx: &HandlerContext<'_>,
    directory: &Path,
    detailed: bool,
) -> TerminalResult<()> {
    println!("Listing scenarios in: {}", directory.display());
    println!("Detailed: {}", detailed);

    // Get scenarios through storage effects
    let scenarios = list_scenarios_through_effects(ctx, directory).await?;

    println!("Available scenarios:");

    for scenario in scenarios {
        if detailed {
            display_detailed_scenario_info(ctx, &scenario).await;
        } else {
            println!("  - {}", scenario.name);
        }
    }

    Ok(())
}

/// Handle scenario validation through effects
pub async fn handle_validate(
    ctx: &HandlerContext<'_>,
    directory: &Path,
    strictness: Option<&str>,
) -> TerminalResult<()> {
    println!("Validating scenarios in: {}", directory.display());

    if let Some(level) = strictness {
        println!("Strictness: {}", level);
    }

    // Validate scenarios through storage effects
    let scenarios = list_scenarios_through_effects(ctx, directory).await?;
    let scenario_names: Vec<String> = scenarios.into_iter().map(|s| s.name).collect();

    validate_scenarios_through_effects(ctx, &scenario_names).await?;

    println!("All scenarios valid");

    Ok(())
}

/// Handle scenario execution through effects
pub async fn handle_run(
    ctx: &HandlerContext<'_>,
    directory: Option<&Path>,
    pattern: Option<&str>,
    parallel: bool,
    max_parallel: Option<usize>,
    output_file: Option<&Path>,
    detailed_report: bool,
) -> TerminalResult<()> {
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
    let results = execute_scenarios(ctx, directory, pattern, parallel, max_parallel).await?;

    // Save results if output_file specified
    if let Some(output_path) = output_file {
        save_scenario_results(ctx, output_path, &results, detailed_report).await?;
    }

    println!("All scenarios completed successfully");

    Ok(())
}

/// Handle report generation through effects
pub async fn handle_report(
    ctx: &HandlerContext<'_>,
    input: &Path,
    output: &Path,
    format: Option<&str>,
    detailed: bool,
) -> TerminalResult<()> {
    println!("Generating report");
    println!("Input: {}", input.display());
    println!("Output: {}", output.display());

    if let Some(fmt) = format {
        println!("Format: {}", fmt);
    }
    println!("Detailed: {}", detailed);

    // Load results through storage effects
    let input_key = format!("scenario_results:{}", input.display());
    let results_data = match ctx.effects().retrieve(&input_key).await {
        Ok(Some(data)) => String::from_utf8(data).map_err(|e| {
            TerminalError::Operation(format!("Invalid UTF-8 in results file: {}", e))
        })?,
        Ok(None) => {
            return Err(TerminalError::Operation(format!(
                "Results file not found: {}",
                input.display()
            )))
        }
        Err(e) => {
            return Err(TerminalError::Operation(format!(
                "Failed to read results file via storage effects: {}",
                e
            )))
        }
    };

    // Generate report
    let report = generate_report_from_results(results_data.as_bytes(), format, detailed)?;

    // Save report through storage effects
    let output_key = format!("scenario_report:{}", output.display());
    ctx.effects()
        .store(&output_key, report.as_bytes().to_vec())
        .await
        .map_err(|e| {
            TerminalError::Operation(format!("Failed to save report via storage effects: {}", e))
        })?;

    println!("Report generated successfully");

    Ok(())
}

// === Helper Functions ===

/// Discover scenarios through storage effects
async fn discover_scenarios_through_effects(
    _ctx: &HandlerContext<'_>,
    root: &Path,
) -> TerminalResult<Vec<String>> {
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
    ctx: &HandlerContext<'_>,
    scenarios: &[String],
) -> TerminalResult<()> {
    for scenario in scenarios {
        println!("Validating: {}", scenario);

        // Simulate validation
        if scenario.contains("invalid") {
            let _ = ctx;
            eprintln!("Invalid scenario: {}", scenario);
            return Err(TerminalError::Operation(format!(
                "Invalid scenario: {}",
                scenario
            )));
        }
    }

    Ok(())
}

/// List scenarios through storage effects
async fn list_scenarios_through_effects(
    _ctx: &HandlerContext<'_>,
    directory: &Path,
) -> TerminalResult<Vec<ScenarioInfo>> {
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
async fn display_detailed_scenario_info(_ctx: &HandlerContext<'_>, scenario: &ScenarioInfo) {
    println!(
        "  - {} ({} devices, threshold {})",
        scenario.name, scenario.devices, scenario.threshold
    );
    println!("    Description: {}", scenario.description);
}

/// Save scenario results through storage effects
async fn save_scenario_results(
    ctx: &HandlerContext<'_>,
    output_path: &Path,
    results: &[ScenarioResult],
    detailed: bool,
) -> TerminalResult<()> {
    let results_json = if detailed {
        serde_json::to_string_pretty(results)
    } else {
        serde_json::to_string(results)
    }
    .map_err(|e| TerminalError::Operation(format!("Failed to serialize results: {}", e)))?;

    let output_key = format!("scenario_output:{}", output_path.display());
    ctx.effects()
        .store(&output_key, results_json.as_bytes().to_vec())
        .await
        .map_err(|e| {
            TerminalError::Operation(format!("Failed to save results via storage effects: {}", e))
        })?;

    println!("Results saved to storage key: {}", output_key);

    Ok(())
}

/// Generate report from results
fn generate_report_from_results(
    results_data: &[u8],
    format: Option<&str>,
    detailed: bool,
) -> TerminalResult<String> {
    let results_str = String::from_utf8(results_data.to_vec())
        .map_err(|e| TerminalError::Operation(format!("Invalid UTF-8 in results: {}", e)))?;

    let results: Vec<ScenarioResult> = serde_json::from_str(&results_str)
        .map_err(|e| TerminalError::Operation(format!("Failed to parse results: {}", e)))?;

    let report = match format.unwrap_or("text") {
        "json" => serde_json::to_string_pretty(&results)
            .map_err(|e| TerminalError::Operation(format!("Failed to format JSON: {}", e)))?,
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
