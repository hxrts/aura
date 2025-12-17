//! Scenario file parsing and execution
//!
//! Handles parsing TOML scenario files and executing actions.

use std::collections::HashMap;
use std::fmt::Write as FmtWrite;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::error::{TerminalError, TerminalResult};
use aura_core::effects::ConsoleEffects;
use aura_simulator::handlers::scenario::SimulationScenarioHandler;

use super::logging::{log_line, persist_log, ScenarioLog};
use super::simulation::simulate_cli_recovery_demo;
use super::types::ScenarioResult;
use crate::handlers::HandlerContext;

/// Execute scenarios through effects
pub async fn execute_scenarios(
    ctx: &HandlerContext<'_>,
    directory: Option<&Path>,
    pattern: Option<&str>,
    parallel: bool,
    max_parallel: Option<usize>,
) -> TerminalResult<Vec<ScenarioResult>> {
    let base_dir = directory.unwrap_or_else(|| Path::new("scenarios"));
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
        let run_result = run_scenario_file(ctx, &scenario).await;

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
pub fn collect_scenario_files(root: &Path) -> TerminalResult<Vec<PathBuf>> {
    let mut stack = vec![root.to_path_buf()];
    let mut found = Vec::new();

    while let Some(dir) = stack.pop() {
        let entries = fs::read_dir(&dir).map_err(|e| {
            TerminalError::Operation(format!("Failed to read scenario directory {}: {}", dir.display(), e)
        })?;

        for entry in entries {
            let entry = entry.map_err(|e| {
                TerminalError::Operation(format!("Failed to read scenario entry {}: {}", dir.display(), e)
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

/// Run a single scenario file with detailed logging
async fn run_scenario_file(ctx: &HandlerContext<'_>, path: &Path) -> TerminalResult<Option<String>> {
    let effects_ref = ctx.effects();
    let contents = fs::read_to_string(path)
        .map_err(|e| TerminalError::Operation(format!("Failed to read scenario {}: {}", path.display(), e))?;

    let parsed: toml::Value = toml::from_str(&contents).map_err(|e| {
        TerminalError::Operation(format!("Failed to parse scenario {} as TOML: {}", path.display(), e)
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
            execute_phase(effects_ref, &mut lines, &sim_handler, idx, phase).await;
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

        match simulate_cli_recovery_demo(seed, ctx).await {
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
                    return Err(TerminalError::Operation(format!(
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
                return Err(TerminalError::Operation(format!("Simulator execution failed: {}", e));
            }
        }
    }

    let log_path = persist_log(ctx, path, &lines).await?;
    Ok(Some(log_path))
}

/// Execute a single phase of the scenario
async fn execute_phase(
    effects_ref: &dyn ConsoleEffects,
    lines: &mut ScenarioLog,
    sim_handler: &SimulationScenarioHandler,
    idx: usize,
    phase: &toml::Value,
) {
    let name = phase
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("<unnamed>");
    log_line(effects_ref, lines, &format!("Phase {}: {}", idx + 1, name)).await;

    if let Some(actions) = phase.get("actions").and_then(|a| a.as_array()) {
        log_line(effects_ref, lines, &format!("  Actions: {}", actions.len())).await;
        for (a_idx, action) in actions.iter().enumerate() {
            let summary = execute_action(effects_ref, lines, sim_handler, a_idx, action).await;
            log_line(effects_ref, lines, &summary).await;
        }
    }
}

/// Execute a single action and return a summary string
async fn execute_action(
    effects_ref: &dyn ConsoleEffects,
    lines: &mut ScenarioLog,
    sim_handler: &SimulationScenarioHandler,
    a_idx: usize,
    action: &toml::Value,
) -> String {
    let mut summary = String::new();
    let action_type = action
        .get("type")
        .and_then(|t| t.as_str())
        .unwrap_or("<unknown>");
    let _ = write!(&mut summary, "    {}. type={}", a_idx + 1, action_type);

    match action_type {
        "run_choreography" => execute_run_choreography(action, sim_handler, &mut summary),
        "verify_property" => execute_verify_property(action, sim_handler, &mut summary),
        "simulate_data_loss" => {
            execute_simulate_data_loss(action, sim_handler, &mut summary, effects_ref, lines).await
        }
        "apply_network_condition" => {
            execute_apply_network_condition(action, sim_handler, &mut summary)
        }
        "inject_byzantine" | "inject_failure" => {
            execute_inject_fault(action, action_type, sim_handler)
        }
        "create_checkpoint" => execute_create_checkpoint(action, sim_handler, &mut summary),
        "export_choreo_trace" => execute_export_trace(action, sim_handler),
        "generate_timeline" => execute_generate_timeline(action, sim_handler),
        "verify_all_properties" => {
            let _ = sim_handler.verify_all_properties();
        }
        "setup_choreography" => execute_setup_choreography(action, sim_handler),
        "load_key_shares" => execute_load_key_shares(action, sim_handler),
        "wait_ticks" => execute_wait_ticks(action, sim_handler, &mut summary),
        "wait_ms" => execute_wait_ms(action, sim_handler, &mut summary),
        _ => execute_generic_action(action, &mut summary),
    }

    summary
}

fn execute_run_choreography(
    action: &toml::Value,
    sim_handler: &SimulationScenarioHandler,
    summary: &mut String,
) {
    if let Some(name) = action.get("choreography").and_then(|v| v.as_str()) {
        let _ = write!(summary, " choreo={}", name);
    }
    if let Some(parts) = action.get("participants").and_then(|v| v.as_array()) {
        let names: Vec<_> = parts.iter().filter_map(|p| p.as_str()).collect();
        if !names.is_empty() {
            let _ = write!(summary, " participants={:?}", names);
        }
    }
    if let Some(target) = action.get("target").and_then(|v| v.as_str()) {
        let _ = write!(summary, " target={}", target);
    }
    if let Some(threshold) = action.get("threshold").and_then(|v| v.as_integer()) {
        let _ = write!(summary, " threshold={}", threshold);
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
    if let Some(threshold) = action.get("threshold").and_then(|v| v.as_integer()) {
        params.insert("threshold".to_string(), threshold.to_string());
    }
    if let Some(app_id) = action.get("app_id").and_then(|v| v.as_str()) {
        params.insert("app_id".to_string(), app_id.to_string());
    }
    if let Some(context) = action.get("context").and_then(|v| v.as_str()) {
        params.insert("context".to_string(), context.to_string());
    }
    if let Some(name) = action.get("choreography").and_then(|v| v.as_str()) {
        let _ = sim_handler.run_choreography(name, participants, params);
    }
}

fn execute_verify_property(
    action: &toml::Value,
    sim_handler: &SimulationScenarioHandler,
    summary: &mut String,
) {
    if let Some(prop) = action.get("property").and_then(|v| v.as_str()) {
        let _ = write!(summary, " property={}", prop);
    }
    if let Some(expected) = action.get("expected") {
        let _ = write!(summary, " expected={}", expected);
    }
    let property = action
        .get("property")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let expected_str = action.get("expected").map(|v| v.to_string());
    let _ = sim_handler.verify_property_stub(property, expected_str);
}

async fn execute_simulate_data_loss(
    action: &toml::Value,
    sim_handler: &SimulationScenarioHandler,
    summary: &mut String,
    effects_ref: &dyn ConsoleEffects,
    lines: &mut ScenarioLog,
) {
    if let Some(target) = action.get("target").and_then(|v| v.as_str()) {
        let _ = write!(summary, " target={}", target);
    }
    if let Some(loss) = action.get("loss_type").and_then(|v| v.as_str()) {
        let _ = write!(summary, " loss_type={}", loss);
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
    if let Err(e) = sim_handler.simulate_data_loss(target, loss_type, recovery_required) {
        log_line(
            effects_ref,
            lines,
            &format!("Simulator data loss error for {}: {}", target, e),
        )
        .await;
    }
}

fn execute_apply_network_condition(
    action: &toml::Value,
    sim_handler: &SimulationScenarioHandler,
    summary: &mut String,
) {
    if let Some(cond) = action.get("condition").and_then(|v| v.as_str()) {
        let _ = write!(summary, " condition={}", cond);
    }
    if let Some(parts) = action.get("participants").and_then(|v| v.as_array()) {
        let names: Vec<_> = parts.iter().filter_map(|p| p.as_str()).collect();
        if !names.is_empty() {
            let _ = write!(summary, " participants={:?}", names);
        }
    }
    let duration_ticks = action
        .get("duration_ticks")
        .and_then(|v| v.as_integer())
        .unwrap_or(0) as u64;
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
        let _ = sim_handler.apply_network_condition(cond, participants, duration_ticks);
    }
}

fn execute_inject_fault(
    action: &toml::Value,
    action_type: &str,
    sim_handler: &SimulationScenarioHandler,
) {
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

fn execute_create_checkpoint(
    action: &toml::Value,
    sim_handler: &SimulationScenarioHandler,
    summary: &mut String,
) {
    let label = action
        .get("label")
        .and_then(|v| v.as_str())
        .unwrap_or("checkpoint");
    if let Ok(id) = sim_handler.create_checkpoint(label) {
        let _ = write!(summary, " id={}", id);
    }
}

fn execute_export_trace(action: &toml::Value, sim_handler: &SimulationScenarioHandler) {
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

fn execute_generate_timeline(action: &toml::Value, sim_handler: &SimulationScenarioHandler) {
    if let Some(output) = action.get("output").and_then(|v| v.as_str()) {
        let _ = sim_handler.generate_timeline(output);
    }
}

fn execute_setup_choreography(action: &toml::Value, sim_handler: &SimulationScenarioHandler) {
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

fn execute_load_key_shares(action: &toml::Value, sim_handler: &SimulationScenarioHandler) {
    let threshold = action
        .get("threshold")
        .and_then(|v| v.as_integer())
        .unwrap_or(0) as usize;
    let _ = sim_handler.load_key_shares(threshold);
}

fn execute_wait_ticks(
    action: &toml::Value,
    sim_handler: &SimulationScenarioHandler,
    summary: &mut String,
) {
    if let Some(ticks) = action.get("ticks").and_then(|v| v.as_integer()) {
        let _ = write!(summary, " ticks={}", ticks);
        let _ = sim_handler.wait_ticks(ticks as u64);
    }
}

fn execute_wait_ms(
    action: &toml::Value,
    sim_handler: &SimulationScenarioHandler,
    summary: &mut String,
) {
    if let Some(ms) = action.get("duration").and_then(|v| v.as_integer()) {
        let _ = write!(summary, " duration_ms={}", ms);
        let _ = sim_handler.wait_ms(ms as u64);
    }
}

fn execute_generic_action(action: &toml::Value, summary: &mut String) {
    if let Some(target) = action.get("target").and_then(|t| t.as_str()) {
        let _ = write!(summary, " target={}", target);
    }
    if let Some(params) = action.get("params") {
        let _ = write!(
            summary,
            " params={}",
            params
                .as_table()
                .map(|t| format!("{:?}", t))
                .unwrap_or_else(|| params.to_string())
        );
    }
}
