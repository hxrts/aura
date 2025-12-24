//! Agent context for runtime operations
//!
//! Immutable context for agent operations, including platform information,
//! authentication state, configuration, and session management.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use aura_core::identifiers::DeviceId;
use aura_core::SessionId;

/// Immutable context for agent operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentContext {
    /// Platform information
    pub platform: PlatformInfo,
    /// Authentication state
    pub auth_state: AuthenticationState,
    /// Configuration settings (immutable)
    pub config: Arc<HashMap<String, String>>,
    /// Active sessions (immutable)
    pub sessions: Arc<HashMap<SessionId, SessionMetadata>>,
}

/// Platform information for agent context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformInfo {
    /// Operating system
    pub os: String,
    /// Hardware capabilities
    pub has_secure_enclave: bool,
    /// Available storage backends
    pub storage_backends: Arc<Vec<String>>,
}

impl Default for PlatformInfo {
    fn default() -> Self {
        Self {
            os: std::env::consts::OS.to_string(),
            has_secure_enclave: false,
            storage_backends: Arc::new(vec!["filesystem".to_string()]),
        }
    }
}

/// Authentication state for agent context
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuthenticationState {
    /// Whether the device is authenticated
    pub authenticated: bool,
    /// Biometric authentication available
    pub biometric_available: bool,
    /// Last authentication time (epoch millis)
    pub last_auth_time: Option<u64>,
}

/// Metadata for active sessions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    /// When the session was created (epoch millis)
    pub created_at: u64,
    /// Session type identifier
    pub session_type: String,
    /// Session-specific data (immutable)
    pub data: Arc<HashMap<String, Vec<u8>>>,
}

impl AgentContext {
    /// Create a new agent context
    pub fn new(_device_id: DeviceId) -> Self {
        Self {
            platform: PlatformInfo::default(),
            auth_state: AuthenticationState::default(),
            config: Arc::new(HashMap::new()),
            sessions: Arc::new(HashMap::new()),
        }
    }

    /// Create context with configuration value
    pub fn with_config(&self, key: &str, value: &str) -> Self {
        let mut new_config = (*self.config).clone();
        new_config.insert(key.to_string(), value.to_string());

        Self {
            platform: self.platform.clone(),
            auth_state: self.auth_state.clone(),
            config: Arc::new(new_config),
            sessions: self.sessions.clone(),
        }
    }

    /// Get a configuration value
    pub fn get_config(&self, key: &str) -> Option<&str> {
        self.config.get(key).map(|s| s.as_str())
    }

    /// Create context with new session
    pub fn with_session(&self, session_id: SessionId, session_type: &str, created_at: u64) -> Self {
        let mut new_sessions = (*self.sessions).clone();
        let metadata = SessionMetadata {
            created_at,
            session_type: session_type.to_string(),
            data: Arc::new(HashMap::new()),
        };
        new_sessions.insert(session_id, metadata);

        Self {
            platform: self.platform.clone(),
            auth_state: self.auth_state.clone(),
            config: self.config.clone(),
            sessions: Arc::new(new_sessions),
        }
    }

    /// Get session metadata
    pub fn get_session(&self, session_id: &SessionId) -> Option<&SessionMetadata> {
        self.sessions.get(session_id)
    }

    /// Create context without session
    pub fn without_session(&self, session_id: &SessionId) -> Self {
        let mut new_sessions = (*self.sessions).clone();
        new_sessions.remove(session_id);

        Self {
            platform: self.platform.clone(),
            auth_state: self.auth_state.clone(),
            config: self.config.clone(),
            sessions: Arc::new(new_sessions),
        }
    }
}
