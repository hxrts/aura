//! Agent context for runtime operations
//!
//! Immutable context for agent operations, including platform information,
//! authentication state, configuration, and session management.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

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
    pub fn new() -> Self {
        Self {
            platform: PlatformInfo::default(),
            auth_state: AuthenticationState::default(),
            config: Arc::new(HashMap::new()),
            sessions: Arc::new(HashMap::new()),
        }
    }

    /// Create context with configuration value
    pub fn with_config(&self, key: &str, value: &str) -> Self {
        self.updated(|next| {
            next.config = self.map_config(|config| {
                config.insert(key.to_string(), value.to_string());
            });
        })
    }

    /// Get a configuration value
    pub fn get_config(&self, key: &str) -> Option<&str> {
        self.config.get(key).map(|s| s.as_str())
    }

    /// Create context with new session
    pub fn with_session(&self, session_id: SessionId, session_type: &str, created_at: u64) -> Self {
        let session_type = session_type.to_string();
        self.updated(|next| {
            next.sessions = self.map_sessions(|sessions| {
                sessions.insert(
                    session_id,
                    SessionMetadata {
                        created_at,
                        session_type,
                        data: Arc::new(HashMap::new()),
                    },
                );
            });
        })
    }

    /// Get session metadata
    pub fn get_session(&self, session_id: &SessionId) -> Option<&SessionMetadata> {
        self.sessions.get(session_id)
    }

    /// Create context without session
    pub fn without_session(&self, session_id: &SessionId) -> Self {
        self.updated(|next| {
            next.sessions = self.map_sessions(|sessions| {
                sessions.remove(session_id);
            });
        })
    }

    fn updated(&self, update: impl FnOnce(&mut Self)) -> Self {
        let mut next = self.clone();
        update(&mut next);
        next
    }

    fn map_config(
        &self,
        update: impl FnOnce(&mut HashMap<String, String>),
    ) -> Arc<HashMap<String, String>> {
        let mut config = (*self.config).clone();
        update(&mut config);
        Arc::new(config)
    }

    fn map_sessions(
        &self,
        update: impl FnOnce(&mut HashMap<SessionId, SessionMetadata>),
    ) -> Arc<HashMap<SessionId, SessionMetadata>> {
        let mut sessions = (*self.sessions).clone();
        update(&mut sessions);
        Arc::new(sessions)
    }
}

impl Default for AgentContext {
    fn default() -> Self {
        Self::new()
    }
}
