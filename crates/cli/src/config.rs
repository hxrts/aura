// Configuration management for CLI

use aura_journal::{AccountId, DeviceId};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub device_id: DeviceId,
    pub account_id: AccountId,
    pub data_dir: PathBuf,
}

impl Config {
    pub async fn load<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let content = fs::read_to_string(path).await
            .map_err(|e| anyhow::anyhow!("Failed to read config file: {}", e))?;
        
        let config: Config = toml::from_str(&content)
            .map_err(|e| anyhow::anyhow!("Failed to parse config file: {}", e))?;
        
        Ok(config)
    }
    
    #[allow(dead_code)]
    pub async fn save<P: AsRef<Path>>(&self, path: P) -> anyhow::Result<()> {
        let content = toml::to_string_pretty(self)
            .map_err(|e| anyhow::anyhow!("Failed to serialize config: {}", e))?;
        
        if let Some(parent) = path.as_ref().parent() {
            fs::create_dir_all(parent).await
                .map_err(|e| anyhow::anyhow!("Failed to create config directory: {}", e))?;
        }
        
        fs::write(path, content).await
            .map_err(|e| anyhow::anyhow!("Failed to write config file: {}", e))?;
        
        Ok(())
    }
}