//! Agent Configuration
//!
//! Configuration types for agent runtime behavior.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Agent configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Storage configuration
    pub storage: StorageConfig,

    /// Network configuration
    pub network: NetworkConfig,

    /// Reliability configuration
    pub reliability: ReliabilityConfig,

    /// Choreography configuration
    pub choreography: ChoreographyConfig,
}

/// Storage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Base storage directory
    pub base_path: PathBuf,

    /// Maximum cache size in bytes
    pub cache_size: usize,

    /// Enable storage compression
    pub enable_compression: bool,
}

/// Network configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Bind address
    pub bind_address: String,

    /// Maximum connections
    pub max_connections: usize,

    /// Connection timeout in seconds
    pub connection_timeout: u64,
}

/// Reliability configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReliabilityConfig {
    /// Maximum retry attempts
    pub max_retries: usize,

    /// Base backoff delay in milliseconds
    pub base_backoff_ms: u64,

    /// Maximum backoff delay in milliseconds
    pub max_backoff_ms: u64,
}

/// Choreography configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChoreographyConfig {
    /// Enable choreography debugging
    pub enable_debug: bool,

    /// Choreography timeout in seconds
    pub timeout_secs: u64,

    /// Maximum concurrent choreographies
    pub max_concurrent: usize,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            storage: StorageConfig::default(),
            network: NetworkConfig::default(),
            reliability: ReliabilityConfig::default(),
            choreography: ChoreographyConfig::default(),
        }
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            base_path: PathBuf::from("./aura-data"),
            cache_size: 64 * 1024 * 1024, // 64MB
            enable_compression: true,
        }
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            bind_address: "0.0.0.0:0".to_string(),
            max_connections: 100,
            connection_timeout: 30,
        }
    }
}

impl Default for ReliabilityConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_backoff_ms: 100,
            max_backoff_ms: 5000,
        }
    }
}

impl Default for ChoreographyConfig {
    fn default() -> Self {
        Self {
            enable_debug: false,
            timeout_secs: 60,
            max_concurrent: 10,
        }
    }
}

impl AgentConfig {
    /// Check if this is a testing configuration
    pub fn is_testing(&self) -> bool {
        // For now, consider it testing if the base path contains "test"
        self.storage.base_path.to_string_lossy().contains("test")
    }

    /// Check if this is a simulation configuration
    pub fn is_simulation(&self) -> bool {
        // For now, consider it simulation if the base path contains "sim"
        self.storage.base_path.to_string_lossy().contains("sim")
    }
}
