//! Scenario loading, validation, and linting.
//!
//! Loads scenario configurations from TOML, validates step references against
//! instance definitions, and reports configuration warnings and errors.

use std::path::Path;

use anyhow::Result;

use crate::config::{load_scenario_config, require_existing_file, RunConfig, ScenarioConfig};

pub struct ScenarioRunner;

#[derive(Debug, Clone)]
pub struct ScenarioLintReport {
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

impl ScenarioRunner {
    pub fn load_and_validate(path: &Path) -> Result<ScenarioConfig> {
        require_existing_file(path, "scenario config")?;
        let scenario = load_scenario_config(path)?;
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

        if let Some(semantic_steps) = scenario.semantic_steps() {
            for step in semantic_steps {
                if let Some(instance) = step.actor.as_ref().map(|actor| actor.0.as_str()) {
                    if !instance_ids.contains(instance) {
                        errors.push(format!(
                            "step {} references unknown instance {}",
                            step.id, instance
                        ));
                    }
                }
            }
            if semantic_steps.len() > 100 {
                warnings.push("scenario has more than 100 steps; consider splitting".to_string());
            }
            return ScenarioLintReport { warnings, errors };
        }

        let Some(compatibility_steps) = scenario.compatibility_steps() else {
            errors.push("non-semantic scenarios must expose compatibility steps".to_string());
            return ScenarioLintReport { warnings, errors };
        };

        for step in compatibility_steps {
            if let Some(instance) = step.instance.as_deref() {
                if !instance_ids.contains(instance) {
                    errors.push(format!(
                        "step {} references unknown instance {}",
                        step.id, instance
                    ));
                }
            }
        }

        if compatibility_steps.len() > 100 {
            warnings.push("scenario has more than 100 steps; consider splitting".to_string());
        }

        ScenarioLintReport { warnings, errors }
    }
}
