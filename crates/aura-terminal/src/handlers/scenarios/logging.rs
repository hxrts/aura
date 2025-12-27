//! Scenario logging and persistence

use std::path::{Path, PathBuf};

use crate::error::{TerminalError, TerminalResult};
use aura_core::effects::{ConsoleEffects, StorageCoreEffects};

use crate::handlers::HandlerContext;

/// In-memory scenario log accumulator
#[derive(Default)]
pub struct ScenarioLog {
    pub lines: Vec<String>,
}

impl ScenarioLog {
    pub fn new(_path: &Path) -> Self {
        Self { lines: Vec::new() }
    }
}

/// Log a line to both the scenario log and console
pub async fn log_line(
    effects: &dyn aura_core::effects::ConsoleEffects,
    log: &mut ScenarioLog,
    line: &str,
) {
    log.lines.push(line.to_string());
    let _ = ConsoleEffects::log_info(effects, line).await;
}

/// Persist scenario log to storage
pub async fn persist_log(
    ctx: &HandlerContext<'_>,
    scenario_path: &Path,
    log: &ScenarioLog,
) -> TerminalResult<String> {
    let output_path = scenario_log_output_path(scenario_path);
    let storage_key = format!("scenario_log:{}", output_path.display());

    ctx.effects()
        .store(&storage_key, log.lines.join("\n").into_bytes())
        .await
    .map_err(|e| {
        TerminalError::Operation(format!(
            "Failed to persist scenario log via storage effects: {}",
            e
        ))
    })?;

    Ok(storage_key)
}

/// Generate output path for scenario log
fn scenario_log_output_path(scenario_path: &Path) -> PathBuf {
    let file_stem = scenario_path
        .file_stem()
        .map(|s| s.to_string_lossy())
        .unwrap_or_else(|| std::borrow::Cow::Borrowed("scenario"));

    Path::new("work")
        .join("scenario_logs")
        .join(format!("{}.log", file_stem))
}
