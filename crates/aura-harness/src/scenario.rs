use std::path::Path;

use anyhow::Result;

use crate::config::{load_scenario_config, require_existing_file, ScenarioConfig};

pub struct ScenarioRunner;

impl ScenarioRunner {
    pub fn load_and_validate(path: &Path) -> Result<ScenarioConfig> {
        require_existing_file(path, "scenario config")?;
        let scenario = load_scenario_config(path)?;
        scenario.validate()?;
        Ok(scenario)
    }
}
