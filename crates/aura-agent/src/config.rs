//! Configuration types for the Aura agent
//!
//! This module defines configuration types that are pure data structures.
//! All configuration I/O is performed through effects, not direct file access.

use aura_types::{AccountId, DeviceId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Complete agent configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentConfig {
    /// Device-specific settings
    pub device: DeviceSettings,
    /// Authentication settings
    pub auth: AuthSettings,
    /// Journal/ledger settings
    pub journal: JournalSettings,
    /// Additional agent settings
    pub agent: AgentSettings,
}

/// Device-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceSettings {
    /// This device's unique identifier
    pub device_id: DeviceId,
    /// Account this device belongs to
    pub account_id: Option<AccountId>,
    /// Device display name
    pub device_name: String,
    /// Hardware security level required
    pub security_level: SecurityLevel,
    /// Storage encryption enabled
    pub encryption_enabled: bool,
    /// Maximum storage size in bytes
    pub max_storage_size: u64,
}

/// Authentication configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthSettings {
    /// Enable biometric authentication
    pub biometric_enabled: bool,
    /// Session timeout in seconds
    pub session_timeout: u64,
    /// Maximum failed auth attempts
    pub max_auth_attempts: u32,
    /// Auth lockout duration in seconds
    pub lockout_duration: u64,
    /// Require device attestation
    pub require_attestation: bool,
}

/// Journal/ledger configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JournalSettings {
    /// Sync interval in seconds
    pub sync_interval: u64,
    /// Maximum pending operations
    pub max_pending_ops: u32,
    /// Operation timeout in seconds
    pub operation_timeout: u64,
    /// Enable background sync
    pub background_sync: bool,
    /// Compression enabled for journal data
    pub compression_enabled: bool,
}

/// General agent settings
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentSettings {
    /// Log level (error, warn, info, debug, trace)
    pub log_level: LogLevel,
    /// Enable metrics collection
    pub metrics_enabled: bool,
    /// Enable distributed tracing
    pub tracing_enabled: bool,
    /// Network timeouts
    pub network_timeout: u64,
    /// Retry configuration
    pub retry_config: RetryConfig,
    /// Cache settings
    pub cache_settings: CacheSettings,
}

/// Security level requirements
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SecurityLevel {
    /// Basic software-based security
    Basic,
    /// Hardware-backed security (TEE)
    Hardware,
    /// High-security hardware (HSM)
    HighSecurity,
}

/// Logging level configuration
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum LogLevel {
    /// Only errors
    Error,
    /// Errors and warnings
    Warn,
    /// Errors, warnings, and info
    Info,
    /// Errors, warnings, info, and debug
    Debug,
    /// All log levels including trace
    Trace,
}

/// Retry configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RetryConfig {
    /// Maximum retry attempts
    pub max_attempts: u32,
    /// Initial retry delay in milliseconds
    pub initial_delay_ms: u64,
    /// Maximum retry delay in milliseconds
    pub max_delay_ms: u64,
    /// Exponential backoff multiplier
    pub backoff_multiplier: f32,
}

/// Cache configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CacheSettings {
    /// Maximum cache size in bytes
    pub max_size_bytes: u64,
    /// Cache TTL in seconds
    pub ttl_seconds: u64,
    /// Enable cache compression
    pub compression_enabled: bool,
}

/// Builder for agent configuration
#[derive(Debug, Default)]
pub struct AgentConfigBuilder {
    device: Option<DeviceSettings>,
    auth: Option<AuthSettings>,
    journal: Option<JournalSettings>,
    agent: Option<AgentSettings>,
    overrides: HashMap<String, serde_json::Value>,
}

impl AgentConfigBuilder {
    /// Create a new config builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Set device settings
    pub fn device(mut self, device: DeviceSettings) -> Self {
        self.device = Some(device);
        self
    }

    /// Set device ID
    pub fn device_id(mut self, device_id: DeviceId) -> Self {
        let device = self.device.get_or_insert_with(DeviceSettings::default);
        device.device_id = device_id;
        self
    }

    /// Set device name
    pub fn device_name(mut self, name: &str) -> Self {
        let device = self.device.get_or_insert_with(DeviceSettings::default);
        device.device_name = name.to_string();
        self
    }

    /// Set account ID
    pub fn account_id(mut self, account_id: AccountId) -> Self {
        let device = self.device.get_or_insert_with(DeviceSettings::default);
        device.account_id = Some(account_id);
        self
    }

    /// Set security level
    pub fn security_level(mut self, level: SecurityLevel) -> Self {
        let device = self.device.get_or_insert_with(DeviceSettings::default);
        device.security_level = level;
        self
    }

    /// Set authentication settings
    pub fn auth(mut self, auth: AuthSettings) -> Self {
        self.auth = Some(auth);
        self
    }

    /// Set journal settings
    pub fn journal(mut self, journal: JournalSettings) -> Self {
        self.journal = Some(journal);
        self
    }

    /// Set agent settings
    pub fn agent(mut self, agent: AgentSettings) -> Self {
        self.agent = Some(agent);
        self
    }

    /// Set configuration override
    pub fn override_config(mut self, key: &str, value: serde_json::Value) -> Self {
        self.overrides.insert(key.to_string(), value);
        self
    }

    /// Build the configuration
    pub fn build(self) -> AgentConfig {
        let mut config = AgentConfig {
            device: self.device.unwrap_or_default(),
            auth: self.auth.unwrap_or_default(),
            journal: self.journal.unwrap_or_default(),
            agent: self.agent.unwrap_or_default(),
        };

        // Apply overrides
        for (key, value) in self.overrides {
            Self::apply_override_static(&mut config, &key, value);
        }

        config
    }

    fn apply_override_static(config: &mut AgentConfig, key: &str, value: serde_json::Value) {
        match key {
            "device.encryption_enabled" => {
                if let Ok(val) = serde_json::from_value(value) {
                    config.device.encryption_enabled = val;
                }
            }
            "auth.session_timeout" => {
                if let Ok(val) = serde_json::from_value(value) {
                    config.auth.session_timeout = val;
                }
            }
            "journal.sync_interval" => {
                if let Ok(val) = serde_json::from_value(value) {
                    config.journal.sync_interval = val;
                }
            }
            "agent.log_level" => {
                if let Ok(val) = serde_json::from_value(value) {
                    config.agent.log_level = val;
                }
            }
            _ => {
                // Ignore unknown overrides
            }
        }
    }

    fn apply_override(&self, config: &mut AgentConfig, key: &str, value: serde_json::Value) {
        Self::apply_override_static(config, key, value);
    }
}

// Default implementations for all config types

impl Default for DeviceSettings {
    fn default() -> Self {
        Self {
            device_id: DeviceId::new(),
            account_id: None,
            device_name: "Aura Device".to_string(),
            security_level: SecurityLevel::Basic,
            encryption_enabled: true,
            max_storage_size: 100 * 1024 * 1024, // 100MB
        }
    }
}

impl Default for AuthSettings {
    fn default() -> Self {
        Self {
            biometric_enabled: true,
            session_timeout: 3600, // 1 hour
            max_auth_attempts: 3,
            lockout_duration: 300, // 5 minutes
            require_attestation: false,
        }
    }
}

impl Default for JournalSettings {
    fn default() -> Self {
        Self {
            sync_interval: 30, // 30 seconds
            max_pending_ops: 1000,
            operation_timeout: 60, // 1 minute
            background_sync: true,
            compression_enabled: true,
        }
    }
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            log_level: LogLevel::Info,
            metrics_enabled: false,
            tracing_enabled: false,
            network_timeout: 30, // 30 seconds
            retry_config: RetryConfig::default(),
            cache_settings: CacheSettings::default(),
        }
    }
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_delay_ms: 100,
            max_delay_ms: 5000,
            backoff_multiplier: 2.0,
        }
    }
}

impl Default for CacheSettings {
    fn default() -> Self {
        Self {
            max_size_bytes: 10 * 1024 * 1024, // 10MB
            ttl_seconds: 3600,                // 1 hour
            compression_enabled: true,
        }
    }
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            device: DeviceSettings::default(),
            auth: AuthSettings::default(),
            journal: JournalSettings::default(),
            agent: AgentSettings::default(),
        }
    }
}

impl AgentConfig {
    /// Create a new config builder
    pub fn builder() -> AgentConfigBuilder {
        AgentConfigBuilder::new()
    }

    /// Get timeout as Duration
    pub fn operation_timeout(&self) -> Duration {
        Duration::from_secs(self.journal.operation_timeout)
    }

    /// Get session timeout as Duration
    pub fn session_timeout(&self) -> Duration {
        Duration::from_secs(self.auth.session_timeout)
    }

    /// Get network timeout as Duration
    pub fn network_timeout(&self) -> Duration {
        Duration::from_secs(self.agent.network_timeout)
    }

    /// Check if hardware security is required
    pub fn requires_hardware_security(&self) -> bool {
        matches!(
            self.device.security_level,
            SecurityLevel::Hardware | SecurityLevel::HighSecurity
        )
    }

    /// Check if high security is required
    pub fn requires_high_security(&self) -> bool {
        matches!(self.device.security_level, SecurityLevel::HighSecurity)
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.device.device_name.is_empty() {
            return Err("Device name cannot be empty".to_string());
        }

        if self.device.max_storage_size == 0 {
            return Err("Storage size must be greater than 0".to_string());
        }

        if self.auth.session_timeout == 0 {
            return Err("Session timeout must be greater than 0".to_string());
        }

        if self.journal.operation_timeout == 0 {
            return Err("Operation timeout must be greater than 0".to_string());
        }

        if self.agent.retry_config.max_attempts == 0 {
            return Err("Max retry attempts must be greater than 0".to_string());
        }

        Ok(())
    }
}
