//! Configuration-Driven Session Runtime Construction
//!
//! This module provides configuration-driven construction patterns for
//! LocalSessionRuntime, hiding internal setup complexity behind clean
//! configuration interfaces and enabling dependency injection for testing.

use crate::local_runtime::LocalSessionRuntime;
use crate::Transport;
use aura_crypto::Effects;
use aura_journal::AccountLedger;
use aura_types::{AccountId, AccountIdExt, DeviceId, DeviceIdExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};
use uuid::Uuid;

/// Configuration for session runtime construction
///
/// Encapsulates all parameters needed to create a LocalSessionRuntime
/// with proper dependencies and settings. Enables configuration-driven
/// construction and dependency injection patterns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRuntimeConfig {
    /// Device identifier for this runtime
    pub device_id: DeviceId,
    /// Account identifier
    pub account_id: AccountId,
    /// Session management settings
    pub session_config: SessionConfig,
    /// Transport configuration
    pub transport_config: TransportConfig,
    /// Security and timeout settings
    pub security_config: SecurityConfig,
}

/// Session management configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    /// Maximum concurrent sessions
    pub max_concurrent_sessions: usize,
    /// Default session timeout
    pub default_timeout_seconds: u64,
    /// Enable session persistence
    pub enable_persistence: bool,
    /// Session cleanup interval
    pub cleanup_interval_seconds: u64,
}

/// Transport configuration for session runtime
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportConfig {
    /// Connection timeout
    pub connection_timeout_ms: u64,
    /// Message timeout
    pub message_timeout_ms: u64,
    /// Retry attempts
    pub max_retries: u32,
    /// Enable transport compression
    pub enable_compression: bool,
}

/// Security configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// Protocol timeout (prevents hangs)
    pub protocol_timeout_seconds: u64,
    /// Enable protocol verification
    pub enable_verification: bool,
    /// Maximum protocol participants
    pub max_participants: usize,
}

impl Default for SessionRuntimeConfig {
    fn default() -> Self {
        let device_id = DeviceId::new_with_effects(&Effects::production());
        let account_id = AccountId::new_with_effects(&Effects::production());

        Self {
            device_id,
            account_id,
            session_config: SessionConfig::default(),
            transport_config: TransportConfig::default(),
            security_config: SecurityConfig::default(),
        }
    }
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            max_concurrent_sessions: 10,
            default_timeout_seconds: 300, // 5 minutes
            enable_persistence: true,
            cleanup_interval_seconds: 60, // 1 minute
        }
    }
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            connection_timeout_ms: 10000, // 10 seconds
            message_timeout_ms: 5000,     // 5 seconds
            max_retries: 3,
            enable_compression: true,
        }
    }
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            protocol_timeout_seconds: 600, // 10 minutes
            enable_verification: true,
            max_participants: 100,
        }
    }
}

/// Session runtime factory for configuration-driven construction
pub struct SessionRuntimeFactory {
    /// Effects for deterministic operations
    effects: Arc<Effects>,
    /// Optional transport override
    transport: Option<Arc<dyn Transport>>,
    /// Optional shared ledger handle
    ledger: Option<Arc<RwLock<AccountLedger>>>,
}

impl SessionRuntimeFactory {
    /// Create new session runtime factory
    pub fn new(effects: Effects) -> Self {
        Self {
            effects: Arc::new(effects),
            transport: None,
            ledger: None,
        }
    }

    /// Create factory with custom transport for testing
    pub fn with_transport(mut self, transport: Arc<dyn Transport>) -> Self {
        self.transport = Some(transport);
        self
    }

    /// Provide a shared ledger environment for created runtimes
    pub fn with_ledger(mut self, ledger: Arc<RwLock<AccountLedger>>) -> Self {
        self.ledger = Some(ledger);
        self
    }

    /// Create LocalSessionRuntime from configuration
    ///
    /// BEFORE: Complex constructor with many parameters
    /// ```rust,ignore
    /// let runtime = LocalSessionRuntime::new(device_id, account_id, effects);
    /// // Requires knowledge of all internal parameters
    /// ```
    ///
    /// AFTER: Clean configuration-driven construction
    /// ```rust,ignore
    /// let config = SessionRuntimeConfig::default();
    /// let runtime = factory.create_runtime(&config)?;
    /// // All complexity hidden behind configuration
    /// ```
    pub fn create_runtime(
        &self,
        config: &SessionRuntimeConfig,
    ) -> Result<LocalSessionRuntime, ConfigurationError> {
        info!(
            "Creating session runtime with configuration for device {}",
            config.device_id
        );

        // Validate configuration
        self.validate_config(config)?;

        // Create runtime with enhanced constructor (if it existed)
        // For now, demonstrate the pattern with the existing constructor
        let mut runtime =
            LocalSessionRuntime::new(config.device_id, config.account_id, (*self.effects).clone());

        if let (Some(ledger), Some(transport)) = (self.ledger.clone(), self.transport.clone()) {
            // Note: This is currently a sync context, but set_environment is async.
            // In a production setup, this would need to be handled asynchronously.
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    runtime.set_environment(ledger, transport).await;
                });
            });
        }

        debug!(
            "Session runtime created with config: max_sessions={}, timeout={}s",
            config.session_config.max_concurrent_sessions,
            config.session_config.default_timeout_seconds
        );

        Ok(runtime)
    }

    /// Create runtime with testing configuration
    pub fn create_test_runtime(
        &self,
        device_id: DeviceId,
        account_id: AccountId,
    ) -> Result<LocalSessionRuntime, ConfigurationError> {
        let mut config = SessionRuntimeConfig {
            device_id,
            account_id,
            ..SessionRuntimeConfig::default()
        };

        // Override with test-friendly settings
        config.session_config.max_concurrent_sessions = 5;
        config.session_config.default_timeout_seconds = 30; // Shorter for tests
        config.transport_config.connection_timeout_ms = 1000; // Faster for tests
        config.security_config.protocol_timeout_seconds = 60; // Shorter for tests

        self.create_runtime(&config)
    }

    /// Validate runtime configuration
    fn validate_config(&self, config: &SessionRuntimeConfig) -> Result<(), ConfigurationError> {
        if config.session_config.max_concurrent_sessions == 0 {
            return Err(ConfigurationError::InvalidParameter(
                "max_concurrent_sessions must be > 0".to_string(),
            ));
        }

        if config.security_config.max_participants == 0 {
            return Err(ConfigurationError::InvalidParameter(
                "max_participants must be > 0".to_string(),
            ));
        }

        if config.transport_config.connection_timeout_ms == 0 {
            return Err(ConfigurationError::InvalidParameter(
                "connection_timeout_ms must be > 0".to_string(),
            ));
        }

        Ok(())
    }
}

/// Enhanced LocalSessionRuntime with configuration support (demonstration)
///
/// This shows how the LocalSessionRuntime could be enhanced to support
/// configuration-driven construction while maintaining backwards compatibility.
impl LocalSessionRuntime {
    /// AFTER: Configuration-driven constructor
    ///
    /// This demonstrates how the existing LocalSessionRuntime could be
    /// enhanced to support configuration-driven construction.
    #[allow(dead_code)]
    pub fn new_with_config(
        config: &SessionRuntimeConfig,
        effects: Effects,
    ) -> Result<Self, ConfigurationError> {
        info!("Creating session runtime with configuration");

        // Validate configuration
        if config.session_config.max_concurrent_sessions == 0 {
            return Err(ConfigurationError::InvalidParameter(
                "max_concurrent_sessions must be > 0".to_string(),
            ));
        }

        let runtime =
            LocalSessionRuntime::new(config.device_id, config.account_id, effects);

        debug!(
            "Session runtime configured with {} max sessions, {}s timeout",
            config.session_config.max_concurrent_sessions,
            config.session_config.default_timeout_seconds
        );

        Ok(runtime)
    }

    /// Create runtime with minimal configuration for simple cases
    pub fn new_simple(device_id: DeviceId, account_id: AccountId) -> Self {
        Self::new(device_id, account_id, Effects::production())
    }
}

/// Configuration errors
#[derive(Debug, thiserror::Error)]
pub enum ConfigurationError {
    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),
    #[error("Transport configuration error: {0}")]
    TransportError(String),
    #[error("Security configuration error: {0}")]
    SecurityError(String),
    #[error("Effects error: {0}")]
    EffectsError(String),
}

/// Configuration builder for fluent construction
pub struct SessionRuntimeConfigBuilder {
    config: SessionRuntimeConfig,
}

impl SessionRuntimeConfigBuilder {
    /// Create new builder with default configuration
    pub fn new() -> Self {
        Self {
            config: SessionRuntimeConfig::default(),
        }
    }

    /// Set device and account IDs
    pub fn with_device(mut self, device_id: DeviceId, account_id: AccountId) -> Self {
        self.config.device_id = device_id;
        self.config.account_id = account_id;
        self
    }

    /// Configure session limits
    pub fn with_session_limits(mut self, max_sessions: usize, timeout_seconds: u64) -> Self {
        self.config.session_config.max_concurrent_sessions = max_sessions;
        self.config.session_config.default_timeout_seconds = timeout_seconds;
        self
    }

    /// Configure transport settings
    pub fn with_transport_timeouts(mut self, connection_ms: u64, message_ms: u64) -> Self {
        self.config.transport_config.connection_timeout_ms = connection_ms;
        self.config.transport_config.message_timeout_ms = message_ms;
        self
    }

    /// Configure security settings
    pub fn with_security(mut self, protocol_timeout: u64, max_participants: usize) -> Self {
        self.config.security_config.protocol_timeout_seconds = protocol_timeout;
        self.config.security_config.max_participants = max_participants;
        self
    }

    /// Build the configuration
    pub fn build(self) -> SessionRuntimeConfig {
        self.config
    }
}

impl Default for SessionRuntimeConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_default_configuration() {
        let config = SessionRuntimeConfig::default();
        assert_eq!(config.session_config.max_concurrent_sessions, 10);
        assert_eq!(config.session_config.default_timeout_seconds, 300);
        assert!(config.session_config.enable_persistence);
    }

    #[test]
    fn test_configuration_builder() {
        let device_id = DeviceId(Uuid::new_v4());
        let account_id = AccountId(Uuid::new_v4());

        let config = SessionRuntimeConfigBuilder::new()
            .with_device(device_id, account_id)
            .with_session_limits(5, 60)
            .with_transport_timeouts(2000, 1000)
            .with_security(120, 50)
            .build();

        assert_eq!(config.device_id, device_id);
        assert_eq!(config.account_id, account_id);
        assert_eq!(config.session_config.max_concurrent_sessions, 5);
        assert_eq!(config.session_config.default_timeout_seconds, 60);
        assert_eq!(config.transport_config.connection_timeout_ms, 2000);
        assert_eq!(config.security_config.max_participants, 50);
    }

    #[test]
    fn test_runtime_factory() {
        let effects = Effects::production();
        let factory = SessionRuntimeFactory::new(effects);

        let device_id = DeviceId(Uuid::new_v4());
        let account_id = AccountId(Uuid::new_v4());

        let result = factory.create_test_runtime(device_id, account_id);
        assert!(result.is_ok(), "Test runtime creation should succeed");
    }

    #[test]
    fn test_configuration_validation() {
        let effects = Effects::production();
        let factory = SessionRuntimeFactory::new(effects);

        let mut config = SessionRuntimeConfig::default();
        config.session_config.max_concurrent_sessions = 0; // Invalid

        let result = factory.create_runtime(&config);
        assert!(result.is_err(), "Invalid configuration should be rejected");
    }
}

/// Example usage showing configuration-driven construction
#[allow(dead_code)]
pub async fn example_usage() -> Result<(), ConfigurationError> {
    // Simple case: Use defaults
    let runtime1 =
        LocalSessionRuntime::new_simple(DeviceId(Uuid::new_v4()), AccountId(Uuid::new_v4()));

    // Advanced case: Configuration-driven
    let config = SessionRuntimeConfigBuilder::new()
        .with_device(DeviceId(Uuid::new_v4()), AccountId(Uuid::new_v4()))
        .with_session_limits(20, 600) // 20 sessions, 10 min timeout
        .with_transport_timeouts(5000, 3000) // 5s connection, 3s message
        .with_security(900, 200) // 15 min protocol timeout, 200 max participants
        .build();

    let factory = SessionRuntimeFactory::new(Effects::production());
    let runtime2 = factory.create_runtime(&config)?;

    println!("Runtime created with configuration-driven construction");
    let _ = (runtime1, runtime2); // Use variables to avoid warnings

    Ok(())
}
