// Configuration management for CLI

use anyhow::Context;
use aura_types::{AccountId, DeviceId};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;

/// CLI configuration structure containing account and device information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Unique identifier for this device
    pub device_id: DeviceId,

    /// Account identifier this device belongs to
    pub account_id: AccountId,

    /// Directory for storing account data
    pub data_dir: PathBuf,
}

impl Config {
    /// Load configuration from a TOML file
    ///
    /// # Arguments
    /// * `path` - Path to the configuration file
    pub async fn load<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let content = fs::read_to_string(path)
            .await
            .context("Failed to read config file")?;

        let config: Config = toml::from_str(&content).context("Failed to parse config file")?;

        Ok(config)
    }

    /// Save configuration to a TOML file
    ///
    /// # Arguments
    /// * `path` - Path where the configuration file should be written
    #[allow(dead_code)]
    pub async fn save<P: AsRef<Path>>(&self, path: P) -> anyhow::Result<()> {
        let content = toml::to_string_pretty(self).context("Failed to serialize config")?;

        if let Some(parent) = path.as_ref().parent() {
            fs::create_dir_all(parent)
                .await
                .context("Failed to create config directory")?;
        }

        fs::write(path, content)
            .await
            .context("Failed to write config file")?;

        Ok(())
    }
}
