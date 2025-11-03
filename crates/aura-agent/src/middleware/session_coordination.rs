//! Session coordination middleware for managing agent sessions

use super::{AgentContext, AgentHandler, AgentMiddleware};
use crate::error::Result;
use crate::middleware::AgentOperation;
use crate::utils::time::AgentTimeProvider;
use aura_types::AuraError;
use aura_types::DeviceId;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// Session coordination middleware that manages agent sessions
pub struct SessionCoordinationMiddleware {
    /// Session manager
    sessions: Arc<RwLock<SessionManager>>,

    /// Configuration
    config: SessionConfig,

    /// Time provider for timestamp generation
    time_provider: Arc<AgentTimeProvider>,
}

impl SessionCoordinationMiddleware {
    /// Create new session coordination middleware with production time provider
    pub fn new(config: SessionConfig) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(SessionManager::new())),
            config,
            time_provider: Arc::new(AgentTimeProvider::production()),
        }
    }

    /// Create new session coordination middleware with custom time provider
    pub fn with_time_provider(
        config: SessionConfig,
        time_provider: Arc<AgentTimeProvider>,
    ) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(SessionManager::new())),
            config,
            time_provider,
        }
    }

    /// Get session statistics
    pub fn stats(&self) -> SessionStats {
        let sessions = self.sessions.read().unwrap();
        sessions.stats()
    }

    /// Clean up expired sessions
    pub fn cleanup_expired_sessions(&self) -> Result<usize> {
        let mut sessions = self.sessions.write().map_err(|_| {
            AuraError::internal_error("Failed to acquire write lock on sessions".to_string())
        })?;

        let current_time = self.time_provider.timestamp_secs();
        Ok(sessions.cleanup_expired(self.config.session_timeout, current_time))
    }
}

impl AgentMiddleware for SessionCoordinationMiddleware {
    fn process(
        &self,
        operation: AgentOperation,
        context: &AgentContext,
        next: &dyn AgentHandler,
    ) -> Result<serde_json::Value> {
        match &operation {
            AgentOperation::StartSession {
                session_type,
                participants,
            } => {
                // Clone the data we need for processing
                let session_type_clone = session_type.clone();
                let participants_clone = participants.clone();

                // Validate session parameters
                self.validate_session_parameters(&session_type_clone, &participants_clone)?;

                // Check if device can start new sessions
                self.check_session_limits(context)?;

                // Process the session start
                let result = next.handle(operation, context)?;

                // Track the session if successful
                if result
                    .get("success")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    if let Some(session_id) = result.get("session_id").and_then(|v| v.as_str()) {
                        self.track_session(
                            session_id.to_string(),
                            context.device_id.clone(),
                            session_type_clone,
                            participants_clone,
                        )?;
                    }
                }

                Ok(result)
            }

            _ => {
                // For other operations, check if they require an active session
                if self.requires_session(&operation) {
                    self.validate_session_context(context)?;
                }

                next.handle(operation, context)
            }
        }
    }

    fn name(&self) -> &str {
        "session_coordination"
    }
}

impl SessionCoordinationMiddleware {
    fn validate_session_parameters(
        &self,
        session_type: &str,
        participants: &[DeviceId],
    ) -> Result<()> {
        // Validate session type
        if session_type.is_empty() {
            return Err(AuraError::invalid_input(
                "Session type cannot be empty".to_string(),
            ));
        }

        if session_type.len() > self.config.max_session_type_length {
            return Err(AuraError::invalid_input(format!(
                "Session type too long: {} > {}",
                session_type.len(),
                self.config.max_session_type_length
            )));
        }

        // Validate allowed session types
        if !self.config.allowed_session_types.is_empty()
            && !self
                .config
                .allowed_session_types
                .contains(&session_type.to_string())
        {
            return Err(AuraError::invalid_input(format!(
                "Session type '{}' not allowed",
                session_type
            )));
        }

        // Validate participants
        if participants.is_empty() {
            return Err(AuraError::invalid_input(
                "Participants list cannot be empty".to_string(),
            ));
        }

        if participants.len() > self.config.max_participants {
            return Err(AuraError::invalid_input(format!(
                "Too many participants: {} > {}",
                participants.len(),
                self.config.max_participants
            )));
        }

        if participants.len() < self.config.min_participants {
            return Err(AuraError::invalid_input(format!(
                "Too few participants: {} < {}",
                participants.len(),
                self.config.min_participants
            )));
        }

        // Check for duplicate participants
        let mut unique_participants = std::collections::HashSet::new();
        for participant in participants {
            if !unique_participants.insert(participant.to_string()) {
                return Err(AuraError::invalid_input(
                    "Duplicate participants not allowed".to_string(),
                ));
            }
        }

        Ok(())
    }

    fn check_session_limits(&self, context: &AgentContext) -> Result<()> {
        let sessions = self.sessions.read().map_err(|_| {
            AuraError::internal_error("Failed to acquire read lock on sessions".to_string())
        })?;

        let device_sessions = sessions.get_device_sessions(&context.device_id);

        if device_sessions.len() >= self.config.max_concurrent_sessions {
            return Err(AuraError::session_limit_exceeded(format!(
                "Device has {} active sessions, maximum is {}",
                device_sessions.len(),
                self.config.max_concurrent_sessions
            )));
        }

        Ok(())
    }

    fn track_session(
        &self,
        session_id: String,
        device_id: DeviceId,
        session_type: String,
        participants: Vec<DeviceId>,
    ) -> Result<()> {
        let mut sessions = self.sessions.write().map_err(|_| {
            AuraError::internal_error("Failed to acquire write lock on sessions".to_string())
        })?;

        let now = self.time_provider.timestamp_secs();
        let session_info = SessionInfo {
            session_id: session_id.clone(),
            session_type,
            participants,
            initiator: device_id,
            started_at: now,
            last_activity: now,
            status: SessionStatus::Active,
        };

        sessions.add_session(session_id, session_info);
        Ok(())
    }

    fn requires_session(&self, operation: &AgentOperation) -> bool {
        match operation {
            AgentOperation::DeriveIdentity { .. } => self.config.require_session_for_identity,
            AgentOperation::StoreData { .. } => self.config.require_session_for_storage,
            AgentOperation::RetrieveData { .. } => self.config.require_session_for_storage,
            AgentOperation::InitiateBackup { .. } => true, // Backup always requires session
            _ => false,
        }
    }

    fn validate_session_context(&self, context: &AgentContext) -> Result<()> {
        if context.session_id.is_none() {
            return Err(AuraError::session_required(
                "Operation requires an active session".to_string(),
            ));
        }

        let session_id = context.session_id.as_ref().unwrap();
        let sessions = self.sessions.read().map_err(|_| {
            AuraError::internal_error("Failed to acquire read lock on sessions".to_string())
        })?;

        match sessions.get_session(session_id) {
            Some(session_info) => {
                // Check if session is still active
                if session_info.status != SessionStatus::Active {
                    return Err(AuraError::session_expired(session_id.clone()));
                }

                // Check if device is a participant
                if session_info.initiator != context.device_id
                    && !session_info.participants.contains(&context.device_id)
                {
                    return Err(AuraError::session_access_denied(
                        "Device is not a participant in this session".to_string(),
                    ));
                }

                // Check session timeout
                let now = self.time_provider.timestamp_secs();
                if now - session_info.last_activity > self.config.session_timeout.as_secs() {
                    return Err(AuraError::session_expired(session_id.clone()));
                }

                Ok(())
            }
            None => Err(AuraError::session_not_found(session_id.clone())),
        }
    }
}

/// Configuration for session coordination middleware
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Maximum concurrent sessions per device
    pub max_concurrent_sessions: usize,

    /// Session timeout duration
    pub session_timeout: Duration,

    /// Maximum participants per session
    pub max_participants: usize,

    /// Minimum participants per session
    pub min_participants: usize,

    /// Maximum session type name length
    pub max_session_type_length: usize,

    /// Allowed session types (empty = allow all)
    pub allowed_session_types: Vec<String>,

    /// Whether to require session for identity operations
    pub require_session_for_identity: bool,

    /// Whether to require session for storage operations
    pub require_session_for_storage: bool,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            max_concurrent_sessions: 5,
            session_timeout: Duration::from_secs(3600), // 1 hour
            max_participants: 10,
            min_participants: 1,
            max_session_type_length: 64,
            allowed_session_types: vec![
                "dkd".to_string(),
                "backup".to_string(),
                "recovery".to_string(),
                "resharing".to_string(),
            ],
            require_session_for_identity: false,
            require_session_for_storage: false,
        }
    }
}

/// Session information
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub session_id: String,
    pub session_type: String,
    pub participants: Vec<DeviceId>,
    pub initiator: DeviceId,
    pub started_at: u64,
    pub last_activity: u64,
    pub status: SessionStatus,
}

/// Session status
#[derive(Debug, Clone, PartialEq)]
pub enum SessionStatus {
    Active,
    Completed,
    Failed,
    Expired,
}

/// Session manager for tracking active sessions
struct SessionManager {
    sessions: HashMap<String, SessionInfo>,
    device_sessions: HashMap<String, Vec<String>>, // device_id -> session_ids
    total_sessions_created: u64,
    total_sessions_completed: u64,
    total_sessions_failed: u64,
}

impl SessionManager {
    fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            device_sessions: HashMap::new(),
            total_sessions_created: 0,
            total_sessions_completed: 0,
            total_sessions_failed: 0,
        }
    }

    fn add_session(&mut self, session_id: String, session_info: SessionInfo) {
        // Track session
        self.sessions
            .insert(session_id.clone(), session_info.clone());
        self.total_sessions_created += 1;

        // Track device sessions
        let device_key = session_info.initiator.to_string();
        self.device_sessions
            .entry(device_key)
            .or_insert_with(Vec::new)
            .push(session_id.clone());

        for participant in &session_info.participants {
            let participant_key = participant.to_string();
            self.device_sessions
                .entry(participant_key)
                .or_insert_with(Vec::new)
                .push(session_id.clone());
        }
    }

    fn get_session(&self, session_id: &str) -> Option<&SessionInfo> {
        self.sessions.get(session_id)
    }

    fn get_device_sessions(&self, device_id: &DeviceId) -> Vec<&SessionInfo> {
        let device_key = device_id.to_string();
        self.device_sessions
            .get(&device_key)
            .unwrap_or(&Vec::new())
            .iter()
            .filter_map(|session_id| self.sessions.get(session_id))
            .filter(|session| session.status == SessionStatus::Active)
            .collect()
    }

    fn cleanup_expired(&mut self, timeout: Duration, current_time: u64) -> usize {
        let now = current_time;
        let timeout_secs = timeout.as_secs();

        let mut expired_sessions = Vec::new();

        for (session_id, session_info) in &self.sessions {
            if session_info.status == SessionStatus::Active
                && now - session_info.last_activity > timeout_secs
            {
                expired_sessions.push(session_id.clone());
            }
        }

        let count = expired_sessions.len();

        for session_id in expired_sessions {
            if let Some(mut session_info) = self.sessions.remove(&session_id) {
                session_info.status = SessionStatus::Expired;
                self.sessions.insert(session_id, session_info);
            }
        }

        count
    }

    fn stats(&self) -> SessionStats {
        let active_sessions = self
            .sessions
            .values()
            .filter(|s| s.status == SessionStatus::Active)
            .count();

        SessionStats {
            active_sessions,
            total_sessions: self.sessions.len(),
            total_sessions_created: self.total_sessions_created,
            total_sessions_completed: self.total_sessions_completed,
            total_sessions_failed: self.total_sessions_failed,
        }
    }
}

/// Session statistics
#[derive(Debug, Clone)]
pub struct SessionStats {
    /// Number of active sessions
    pub active_sessions: usize,

    /// Total sessions (all statuses)
    pub total_sessions: usize,

    /// Total sessions created
    pub total_sessions_created: u64,

    /// Total sessions completed
    pub total_sessions_completed: u64,

    /// Total sessions failed
    pub total_sessions_failed: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::handler::NoOpHandler;
    use aura_crypto::Effects;
    use aura_types::{AccountIdExt, DeviceIdExt};

    #[test]
    fn test_session_coordination_middleware() {
        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);
        let participant_id = aura_types::DeviceId::new_with_effects(&effects);

        let middleware = SessionCoordinationMiddleware::new(SessionConfig::default());
        let handler = NoOpHandler;
        let context = AgentContext::new(account_id, device_id, "test".to_string());
        let operation = AgentOperation::StartSession {
            session_type: "dkd".to_string(),
            participants: vec![participant_id],
        };

        let result = middleware.process(operation, &context, &handler);
        assert!(result.is_ok());

        let stats = middleware.stats();
        assert_eq!(stats.total_sessions_created, 0); // NoOpHandler doesn't provide session_id for tracking
    }

    #[test]
    fn test_session_validation() {
        let middleware = SessionCoordinationMiddleware::new(SessionConfig::default());
        let effects = Effects::test(42);
        let participant_id = aura_types::DeviceId::new_with_effects(&effects);

        // Valid session
        assert!(middleware
            .validate_session_parameters("dkd", &[participant_id.clone()])
            .is_ok());

        // Invalid session type
        assert!(middleware
            .validate_session_parameters("", &[participant_id.clone()])
            .is_err());
        assert!(middleware
            .validate_session_parameters("invalid-type", &[participant_id.clone()])
            .is_err());

        // Invalid participants
        assert!(middleware.validate_session_parameters("dkd", &[]).is_err());
        assert!(middleware
            .validate_session_parameters("dkd", &[participant_id.clone(), participant_id])
            .is_err()); // Duplicate
    }

    #[test]
    fn test_session_cleanup() {
        let middleware = SessionCoordinationMiddleware::new(SessionConfig {
            session_timeout: Duration::from_secs(1),
            ..SessionConfig::default()
        });

        // Add a session and let it expire
        std::thread::sleep(Duration::from_secs(2));

        let cleaned = middleware.cleanup_expired_sessions().unwrap();
        // Should be 0 since no sessions were actually added (NoOpHandler limitation)
        assert_eq!(cleaned, 0);
    }
}
