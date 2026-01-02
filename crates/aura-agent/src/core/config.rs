//! Agent Configuration
//!
//! Configuration types for agent runtime behavior, including guardian consensus policy.

use super::guardian::GuardianConsensusPolicy;
use crate::runtime::services::rendezvous_manager::RendezvousManagerConfig;
use aura_core::hash;
use aura_core::DeviceId;
use aura_rendezvous::LanDiscoveryConfig;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

fn default_true() -> bool {
    true
}

/// Resolve the default storage path for Aura agents.
///
/// This is the SINGLE SOURCE OF TRUTH for agent storage path resolution.
///
/// Priority:
/// 1. `$AURA_PATH/.aura` if AURA_PATH is set
/// 2. `~/.aura` (home directory)
/// 3. `./.aura` (current directory fallback)
pub fn default_storage_path() -> PathBuf {
    std::env::var("AURA_PATH")
        .ok()
        .map(PathBuf::from)
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".aura")
}

/// Agent configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Device ID for this agent
    #[serde(with = "device_id_serde")]
    pub device_id: DeviceId,

    /// Storage configuration
    pub storage: StorageConfig,

    /// Network configuration
    pub network: NetworkConfig,

    /// Reliability configuration
    pub reliability: ReliabilityConfig,

    /// Choreography configuration
    pub choreography: ChoreographyConfig,

    /// Guardian consensus policy
    #[serde(default)]
    pub guardian: GuardianConsensusPolicy,

    /// LAN discovery configuration
    #[serde(default, skip)]
    pub lan_discovery: LanDiscoveryConfig,
}

/// Storage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Base storage directory
    pub base_path: PathBuf,

    /// Enable encrypted-at-rest storage.
    ///
    /// This should remain `true` in production. Disabling is intended for tests/bring-up only.
    #[serde(default = "default_true")]
    pub encryption_enabled: bool,

    /// Enable opaque storage key names (metadata minimization).
    ///
    /// Note: When enabled, prefix-based key listing is not meaningful without an index.
    #[serde(default)]
    pub opaque_names: bool,

    /// Maximum cache size in bytes
    pub cache_size: u64,

    /// Enable storage compression
    pub enable_compression: bool,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            base_path: default_storage_path(),
            encryption_enabled: true,
            opaque_names: false,
            cache_size: 50 * 1024 * 1024,
            enable_compression: true,
        }
    }
}

/// Network configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Bind address
    pub bind_address: String,

    /// Maximum connections
    pub max_connections: u32,

    /// Connection timeout in seconds
    pub connection_timeout: u64,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            bind_address: "127.0.0.1:0".to_string(),
            max_connections: 64,
            connection_timeout: 10,
        }
    }
}

/// Reliability configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReliabilityConfig {
    /// Maximum retry attempts
    pub max_retries: u32,

    /// Base backoff delay in milliseconds
    pub base_backoff_ms: u64,

    /// Maximum backoff delay in milliseconds
    pub max_backoff_ms: u64,
}

impl Default for ReliabilityConfig {
    fn default() -> Self {
        Self {
            max_retries: 5,
            base_backoff_ms: 200,
            max_backoff_ms: 10_000,
        }
    }
}

/// Choreography configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChoreographyConfig {
    /// Enable choreography debugging
    pub enable_debug: bool,

    /// Choreography timeout in seconds
    pub timeout_secs: u64,

    /// Maximum concurrent choreographies
    pub max_concurrent: u32,
}

impl Default for ChoreographyConfig {
    fn default() -> Self {
        Self {
            enable_debug: false,
            timeout_secs: 30,
            max_concurrent: 16,
        }
    }
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            device_id: derive_device_id("default"),
            storage: StorageConfig::default(),
            network: NetworkConfig::default(),
            reliability: ReliabilityConfig::default(),
            choreography: ChoreographyConfig::default(),
            guardian: GuardianConsensusPolicy::default(),
            lan_discovery: LanDiscoveryConfig::default(),
        }
    }
}

// Custom serde for DeviceId to keep DeviceId opaque in config
mod device_id_serde {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(id: &DeviceId, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(id.0))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<DeviceId, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = hex::decode(s).map_err(serde::de::Error::custom)?;
        let mut arr = [0u8; 32];
        if bytes.len() != 32 {
            return Err(serde::de::Error::custom("device_id must be 32 bytes"));
        }
        arr.copy_from_slice(&bytes);
        Ok(DeviceId::new_from_entropy(arr))
    }
}

/// Derive a deterministic DeviceId from an arbitrary label (helper for tests/config)
pub fn derive_device_id(label: &str) -> DeviceId {
    let digest = hash::hash(label.as_bytes());
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&digest);
    DeviceId::new_from_entropy(arr)
}

impl AgentConfig {
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    pub fn is_simulation(&self) -> bool {
        false
    }

    /// Get a rendezvous manager config with LAN discovery settings from this agent config
    pub fn rendezvous_config(&self) -> RendezvousManagerConfig {
        RendezvousManagerConfig::default().with_lan_discovery(self.lan_discovery.clone())
    }

    /// Enable LAN discovery
    pub fn with_lan_discovery_enabled(mut self, enabled: bool) -> Self {
        self.lan_discovery.enabled = enabled;
        self
    }
}
