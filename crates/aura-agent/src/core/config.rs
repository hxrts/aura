//! Agent Configuration
//!
//! Configuration types for agent runtime behavior, including guardian consensus policy.

use super::guardian::GuardianConsensusPolicy;
use crate::runtime::services::bootstrap_broker::BootstrapBrokerConfig;
use crate::runtime::services::rendezvous_manager::RendezvousManagerConfig;
use aura_core::hash;
use aura_core::DeviceId;
use aura_rendezvous::LanDiscoveryConfig;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Resolve the default storage path for Aura agents.
///
/// This is the SINGLE SOURCE OF TRUTH for agent storage path resolution.
///
/// Priority:
/// 1. `$AURA_PATH/.aura` if AURA_PATH is set
/// 2. `~/.aura` (home directory)
/// 3. `./.aura` (current directory fallback)
pub fn aura_storage_root() -> PathBuf {
    std::env::var("AURA_PATH")
        .ok()
        .map(PathBuf::from)
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn default_storage_path() -> PathBuf {
    aura_storage_root().join(".aura")
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

    /// Bootstrap broker configuration for mixed native/browser startup discovery.
    #[serde(default)]
    pub bootstrap_broker: BootstrapBrokerConfig,
}

/// Storage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Base storage directory
    pub base_path: PathBuf,

    /// Secure storage backend for key material.
    ///
    /// Defaults to the platform credential store. Tests and simulations may
    /// explicitly use the filesystem fallback, but production rejects that
    /// fallback because it is not a secure key boundary.
    #[serde(default)]
    pub secure_storage_backend: SecureStorageBackend,

    /// Encrypted-at-rest storage policy.
    #[serde(default)]
    pub encryption_policy: StorageEncryptionPolicy,

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
            secure_storage_backend: SecureStorageBackend::default(),
            encryption_policy: StorageEncryptionPolicy::Required,
            opaque_names: false,
            cache_size: 50 * 1024 * 1024,
            enable_compression: true,
        }
    }
}

/// Explicit encrypted-at-rest policy for agent storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum StorageEncryptionPolicy {
    /// Production storage must be encrypted at rest.
    #[default]
    Required,
    /// Test-only plaintext storage for focused runtime tests.
    PlaintextForTests,
}

/// Secure storage backend selection for local key material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SecureStorageBackend {
    /// Use the explicit filesystem-backed secure-storage fallback.
    ///
    /// This backend is non-interactive and is allowed only for tests,
    /// simulations, harness runs, and other explicit non-production modes.
    FilesystemFallback,
    /// Use the platform credential store (Keychain, Keystore, libsecret, etc.).
    ///
    /// This may trigger interactive OS prompts depending on the platform and
    /// deployment environment.
    #[default]
    PlatformCredentialStore,
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

    /// Relay this peer registers with (`host:port` of a relay-capable node's
    /// transport listener). While registered, the peer advertises a relay
    /// transport hint so peers that cannot reach it directly dial the relay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay_address: Option<String>,

    /// Serve as a relay: accept registrations on the transport listener and
    /// forward frames addressed to registered peers.
    #[serde(default)]
    pub serve_relay: bool,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            bind_address: "0.0.0.0:0".to_string(),
            max_connections: 64,
            connection_timeout: 10,
            relay_address: None,
            serve_relay: false,
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
            bootstrap_broker: BootstrapBrokerConfig::default(),
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
        let bytes = id.to_bytes().map_err(serde::ser::Error::custom)?;
        serializer.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<DeviceId, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = hex::decode(s).map_err(serde::de::Error::custom)?;
        if bytes.len() != 32 {
            return Err(serde::de::Error::custom("device_id must be 32 bytes"));
        }
        if bytes[16..].iter().any(|byte| *byte != 0) {
            return Err(serde::de::Error::custom(
                "device_id has noncanonical trailing bytes",
            ));
        }
        let mut canonical = [0u8; 32];
        canonical.copy_from_slice(&bytes);
        Ok(DeviceId::from_bytes(canonical))
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
        RendezvousManagerConfig::default()
            .with_lan_discovery(self.lan_discovery.clone())
            .with_bootstrap_broker(self.bootstrap_broker.clone())
    }

    /// Enable LAN discovery
    pub fn with_lan_discovery_enabled(mut self, enabled: bool) -> Self {
        self.lan_discovery.enabled = enabled;
        self
    }

    /// Configure bootstrap broker usage for browser-involved startup discovery.
    pub fn with_bootstrap_broker(mut self, config: BootstrapBrokerConfig) -> Self {
        self.bootstrap_broker = config;
        self
    }
}

#[cfg(test)]
mod config_serde_tests {
    use super::*;

    #[test]
    fn device_id_round_trips_with_canonical_wire_width() {
        let config = AgentConfig::default();
        let encoded = serde_json::to_value(&config).expect("serialize native config");
        let wire = encoded["device_id"].as_str().expect("device id string");
        assert_eq!(wire.len(), 64);
        assert_eq!(wire, hex::encode(config.device_id.to_bytes().unwrap()));
        let decoded: AgentConfig =
            serde_json::from_value(encoded.clone()).expect("deserialize native config");
        assert_eq!(decoded.device_id, config.device_id);
    }

    #[test]
    fn device_id_rejects_short_and_nonhex_wire() {
        let mut invalid = serde_json::to_value(AgentConfig::default()).unwrap();
        invalid["device_id"] = serde_json::Value::String(hex::encode(
            AgentConfig::default().device_id.uuid().as_bytes(),
        ));
        let error = serde_json::from_value::<AgentConfig>(invalid.clone()).unwrap_err();
        assert!(error.to_string().contains("device_id must be 32 bytes"));
        invalid["device_id"] = serde_json::Value::String("00".into());
        let error = serde_json::from_value::<AgentConfig>(invalid.clone()).unwrap_err();
        assert!(error.to_string().contains("device_id must be 32 bytes"));
        invalid["device_id"] = serde_json::Value::String("gg".repeat(32));
        let error = serde_json::from_value::<AgentConfig>(invalid).unwrap_err();
        assert!(error.to_string().contains("Invalid character"));
    }

    #[test]
    fn device_id_rejects_noncanonical_trailing_bytes() {
        let mut invalid = serde_json::to_value(AgentConfig::default()).unwrap();
        let mut bytes = AgentConfig::default().device_id.to_bytes().unwrap();
        bytes[31] = 1;
        invalid["device_id"] = serde_json::Value::String(hex::encode(bytes));
        let error = serde_json::from_value::<AgentConfig>(invalid).unwrap_err();
        assert!(error.to_string().contains("noncanonical trailing bytes"));
    }
}
