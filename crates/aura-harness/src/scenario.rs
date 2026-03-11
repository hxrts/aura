//! Scenario loading, validation, and linting.
//!
//! Loads scenario configurations from TOML, validates step references against
//! instance definitions, and reports configuration warnings and errors.

use std::path::Path;

use anyhow::Result;

use crate::config::{
    load_execution_scenario_config, require_existing_file, RunConfig, ScenarioConfig,
};

pub struct ScenarioRunner;

#[derive(Debug, Clone)]
pub struct ScenarioLintReport {
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

impl ScenarioRunner {
    pub fn load_and_validate(path: &Path) -> Result<ScenarioConfig> {
        require_existing_file(path, "scenario config")?;
        let scenario = load_execution_scenario_config(path)?;
        scenario.validate()?;
        Ok(scenario)
    }

    pub fn lint(run_config: &RunConfig, scenario: &ScenarioConfig) -> ScenarioLintReport {
        let mut warnings = Vec::new();
        let mut errors = Vec::new();
        let instance_ids: std::collections::BTreeSet<_> = run_config
            .instances
            .iter()
            .map(|instance| instance.id.as_str())
            .collect();

        if let Some(steps) = scenario.shared_execution_semantic_steps() {
            for step in steps {
                if let Some(instance) = step.actor.as_ref().map(|actor| actor.0.as_str()) {
                    if !instance_ids.contains(instance) {
                        errors.push(format!(
                            "step {} references unknown instance {}",
                            step.id, instance
                        ));
                    }
                }
            }
            if steps.len() > 100 {
                warnings.push("scenario has more than 100 steps; consider splitting".to_string());
            }
            return ScenarioLintReport { warnings, errors };
        }

        let steps = match scenario.execution_steps() {
            Ok(steps) => steps,
            Err(error) => {
                errors.push(format!("scenario lowering failed: {error}"));
                return ScenarioLintReport { warnings, errors };
            }
        };

        for step in &steps {
            if let Some(instance) = step.instance.as_deref() {
                if !instance_ids.contains(instance) {
                    errors.push(format!(
                        "step {} references unknown instance {}",
                        step.id, instance
                    ));
                }
            }
        }

        let mut previous_request_id: Option<u64> = None;
        for step in &steps {
            let Some(request_id) = step.request_id else {
                continue;
            };
            if previous_request_id.is_some_and(|previous| request_id <= previous) {
                errors.push(format!(
                    "step {} request_id={} must be strictly greater than previous request_id={}",
                    step.id,
                    request_id,
                    previous_request_id.unwrap_or(0)
                ));
            }
            previous_request_id = Some(request_id);
        }

        if steps.len() > 100 {
            warnings.push("scenario has more than 100 steps; consider splitting".to_string());
        }

        ScenarioLintReport { warnings, errors }
    }
}
