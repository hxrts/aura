//! Unified session management for aura-sync protocols
//!
//! This module provides a centralized session management system that consolidates
//! all session lifecycle, state tracking, and coordination patterns scattered
//! across the aura-sync crate into a choreography-aware abstraction.


use crate::core::{SyncError, SyncResult, SyncConfig, MetricsCollector};
use aura_core::{DeviceId, SessionId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// Unified session state machine following choreographic patterns
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SessionState<T> {
    /// Session initialization phase
    Initializing {
        participants: Vec<DeviceId>,
        timeout_at: u64, // Unix timestamp
        created_at: u64,
    },
    /// Active session with protocol-specific state
    Active {
        protocol_state: T,
        started_at: u64, // Unix timestamp
        participants: Vec<DeviceId>,
        timeout_at: u64,
    },
    /// Session termination phase with results
    Terminating {
        result: SessionResult,
        cleanup_deadline: u64, // Unix timestamp
    },
    /// Session completed and cleaned up
    Completed(SessionResult),
}

impl<T> SessionState<T> {
    /// Check if session has timed out
    pub fn is_timed_out(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        match self {
            SessionState::Initializing { timeout_at, .. } => now >= *timeout_at,
            SessionState::Active { timeout_at, .. } => now >= *timeout_at,
            SessionState::Terminating { cleanup_deadline, .. } => now >= *cleanup_deadline,
            SessionState::Completed(_) => false,
        }
    }

    /// Get session participants
    pub fn participants(&self) -> &[DeviceId] {
        match self {
            SessionState::Initializing { participants, .. } => participants,
            SessionState::Active { participants, .. } => participants,
            SessionState::Terminating { .. } => &[],
            SessionState::Completed(_) => &[],
        }
    }

    /// Get session duration in milliseconds (if active or completed)
    pub fn duration_ms(&self) -> Option<u64> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        match self {
            SessionState::Active { started_at, .. } => Some((now - started_at) * 1000),
            SessionState::Terminating { result, .. } | SessionState::Completed(result) => {
                match result {
                    SessionResult::Success { duration_ms, .. } => Some(*duration_ms),
                    SessionResult::Failure { duration_ms, .. } => Some(*duration_ms),
                    SessionResult::Timeout { duration_ms } => Some(*duration_ms),
                }
            }
            SessionState::Initializing { created_at, .. } => Some((now - created_at) * 1000),
        }
    }

    /// Check if session is in a terminal state
    pub fn is_terminal(&self) -> bool {
        matches!(self, SessionState::Completed(_))
    }

    /// Check if session is active
    pub fn is_active(&self) -> bool {
        matches!(self, SessionState::Active { .. })
    }
}

/// Session results with comprehensive context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionResult {
    Success {
        duration_ms: u64,
        operations_count: usize,
        bytes_transferred: usize,
        participants: Vec<DeviceId>,
        metadata: HashMap<String, String>,
    },
    Failure {
        error: SessionError,
        duration_ms: u64,
        partial_results: Option<PartialResults>,
    },
    Timeout {
        duration_ms: u64,
        last_known_state: String,
    },
}

impl SessionResult {
    /// Check if result represents success
    pub fn is_success(&self) -> bool {
        matches!(self, SessionResult::Success { .. })
    }

    /// Get duration regardless of outcome
    pub fn duration_ms(&self) -> u64 {
        match self {
            SessionResult::Success { duration_ms, .. } => *duration_ms,
            SessionResult::Failure { duration_ms, .. } => *duration_ms,
            SessionResult::Timeout { duration_ms } => *duration_ms,
        }
    }

    /// Get operations count for successful sessions
    pub fn operations_count(&self) -> usize {
        match self {
            SessionResult::Success { operations_count, .. } => *operations_count,
            SessionResult::Failure { partial_results: Some(partial), .. } => {
                partial.operations_completed
            }
            _ => 0,
        }
    }
}

/// Partial results for failed sessions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartialResults {
    pub operations_completed: usize,
    pub bytes_transferred: usize,
    pub completed_participants: Vec<DeviceId>,
    pub last_successful_operation: Option<String>,
}

/// Session-specific errors
#[derive(Debug, Clone, thiserror::Error, Serialize, Deserialize)]
pub enum SessionError {
    #[error("Session timeout after {duration_ms}ms")]
    Timeout { duration_ms: u64 },
    
    #[error("Participant {participant} disconnected")]
    ParticipantDisconnected { participant: DeviceId },
    
    #[error("Resource limit exceeded: {limit_type}")]
    ResourceLimitExceeded { limit_type: String },
    
    #[error("Protocol constraint violation: {constraint}")]
    ProtocolViolation { constraint: String },
    
    #[error("Session capacity exceeded: {current}/{max}")]
    CapacityExceeded { current: usize, max: usize },
    
    #[error("Invalid session state transition: {from} -> {to}")]
    InvalidStateTransition { from: String, to: String },
}

/// Session configuration derived from main SyncConfig
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Session timeout duration
    pub timeout: Duration,
    /// Maximum number of participants per session
    pub max_participants: usize,
    /// Cleanup interval for stale sessions
    pub cleanup_interval: Duration,
    /// Maximum concurrent sessions
    pub max_concurrent_sessions: usize,
    /// Session resource limits
    pub resource_limits: SessionResourceLimits,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(300), // 5 minutes
            max_participants: 10,
            cleanup_interval: Duration::from_secs(60), // 1 minute
            max_concurrent_sessions: 20,
            resource_limits: SessionResourceLimits::default(),
        }
    }
}

impl From<&SyncConfig> for SessionConfig {
    fn from(sync_config: &SyncConfig) -> Self {
        Self {
            timeout: sync_config.network.sync_timeout,
            max_participants: 10, // Could be configurable
            cleanup_interval: sync_config.network.cleanup_interval,
            max_concurrent_sessions: sync_config.peer_management.max_concurrent_syncs,
            resource_limits: SessionResourceLimits::from(&sync_config.performance),
        }
    }
}

/// Resource limits for session management
#[derive(Debug, Clone)]
pub struct SessionResourceLimits {
    /// Maximum memory usage per session in bytes
    pub max_memory_per_session: usize,
    /// Maximum duration a session can be active
    pub max_session_duration: Duration,
    /// Maximum number of operations per session
    pub max_operations_per_session: usize,
}

impl Default for SessionResourceLimits {
    fn default() -> Self {
        Self {
            max_memory_per_session: 10 * 1024 * 1024, // 10 MB
            max_session_duration: Duration::from_secs(3600), // 1 hour
            max_operations_per_session: 10000,
        }
    }
}

impl From<&crate::core::PerformanceConfig> for SessionResourceLimits {
    fn from(perf_config: &crate::core::PerformanceConfig) -> Self {
        Self {
            max_memory_per_session: perf_config.memory_limit / 10, // 1/10th of total limit
            max_session_duration: Duration::from_secs(3600), // 1 hour
            max_operations_per_session: 10000,
        }
    }
}

/// Generic session manager for protocol-agnostic session coordination
pub struct SessionManager<T> {
    /// Active sessions indexed by session ID
    sessions: HashMap<SessionId, SessionState<T>>,
    /// Session configuration
    config: SessionConfig,
    /// Metrics collector for session telemetry
    metrics: Option<MetricsCollector>,
    /// Last cleanup timestamp
    last_cleanup: Instant,
}

impl<T> SessionManager<T>
where
    T: Clone + Send + Sync + Serialize + for<'de> Deserialize<'de>,
{
    /// Create a new session manager
    ///
    /// Note: Callers should obtain `now` via `TimeEffects::now_instant()` and pass it to this method
    pub fn new(config: SessionConfig, now: Instant) -> Self {
        Self {
            sessions: HashMap::new(),
            config,
            metrics: None,
            last_cleanup: now,
        }
    }

    /// Create session manager with metrics collection
    ///
    /// Note: Callers should obtain `now` via `TimeEffects::now_instant()` and pass it to this method
    pub fn with_metrics(config: SessionConfig, metrics: MetricsCollector, now: Instant) -> Self {
        Self {
            sessions: HashMap::new(),
            config,
            metrics: Some(metrics),
            last_cleanup: now,
        }
    }

    /// Create a new session with participants
    ///
    /// Note: Callers should obtain `now` via `TimeEffects::now_instant()` and pass it to this method
    pub fn create_session(&mut self, participants: Vec<DeviceId>, now: Instant) -> SyncResult<SessionId> {
        // Validate participant count
        if participants.len() > self.config.max_participants {
            return Err(SyncError::validation(&format!(
                "Too many participants: {} > {}",
                participants.len(),
                self.config.max_participants
            )));
        }

        // Check concurrent session limit
        let active_count = self.count_active_sessions();
        if active_count >= self.config.max_concurrent_sessions {
            return Err(SyncError::resource_exhausted_with_limit(
                "concurrent_sessions",
                "Maximum concurrent sessions exceeded",
                self.config.max_concurrent_sessions as u64,
            ));
        }

        let session_id = SessionId::new();
        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let session_state = SessionState::Initializing {
            participants,
            timeout_at: now_secs + self.config.timeout.as_secs(),
            created_at: now_secs,
        };

        self.sessions.insert(session_id, session_state);

        // Record metrics
        if let Some(ref metrics) = self.metrics {
            metrics.record_sync_start(&session_id.to_string(), now);
        }

        Ok(session_id)
    }

    /// Activate a session with initial protocol state
    pub fn activate_session(&mut self, session_id: SessionId, protocol_state: T) -> SyncResult<()> {
        let session = self.sessions.get_mut(&session_id)
            .ok_or_else(|| SyncError::session(&format!("Session {} not found", session_id)))?;

        match session {
            SessionState::Initializing { participants, .. } => {
                if session.is_timed_out() {
                    return Err(SyncError::timeout("session_activation", self.config.timeout));
                }

                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();

                let participants = participants.clone();
                *session = SessionState::Active {
                    protocol_state,
                    started_at: now,
                    participants,
                    timeout_at: now + self.config.resource_limits.max_session_duration.as_secs(),
                };

                Ok(())
            }
            _ => Err(SyncError::session(&format!(
                "Session {} is not in initializing state",
                session_id
            ))),
        }
    }

    /// Update session protocol state
    pub fn update_session(&mut self, session_id: SessionId, new_state: T) -> SyncResult<()> {
        let session = self.sessions.get_mut(&session_id)
            .ok_or_else(|| SyncError::session(&format!("Session {} not found", session_id)))?;

        match session {
            SessionState::Active { protocol_state, .. } => {
                if session.is_timed_out() {
                    self.timeout_session(session_id)?;
                    return Err(SyncError::timeout("session_update", self.config.timeout));
                }

                *protocol_state = new_state;
                Ok(())
            }
            _ => Err(SyncError::session(&format!(
                "Session {} is not active",
                session_id
            ))),
        }
    }

    /// Complete a session successfully
    pub fn complete_session(
        &mut self,
        session_id: SessionId,
        operations_count: usize,
        bytes_transferred: usize,
        metadata: HashMap<String, String>,
    ) -> SyncResult<()> {
        let session = self.sessions.get_mut(&session_id)
            .ok_or_else(|| SyncError::session(&format!("Session {} not found", session_id)))?;

        let duration_ms = session.duration_ms().unwrap_or(0);
        let participants = session.participants().to_vec();

        let result = SessionResult::Success {
            duration_ms,
            operations_count,
            bytes_transferred,
            participants,
            metadata,
        };

        *session = SessionState::Completed(result.clone());

        // Record metrics
        if let Some(ref metrics) = self.metrics {
            metrics.record_sync_completion(&session_id.to_string(), operations_count, bytes_transferred);
        }

        Ok(())
    }

    /// Fail a session with error context
    pub fn fail_session(
        &mut self,
        session_id: SessionId,
        error: SessionError,
        partial_results: Option<PartialResults>,
    ) -> SyncResult<()> {
        let session = self.sessions.get_mut(&session_id)
            .ok_or_else(|| SyncError::session(&format!("Session {} not found", session_id)))?;

        let duration_ms = session.duration_ms().unwrap_or(0);

        let result = SessionResult::Failure {
            error: error.clone(),
            duration_ms,
            partial_results,
        };

        *session = SessionState::Completed(result);

        // Record metrics
        if let Some(ref metrics) = self.metrics {
            let category = match error {
                SessionError::Timeout { .. } => crate::core::ErrorCategory::Timeout,
                SessionError::ParticipantDisconnected { .. } => crate::core::ErrorCategory::Network,
                SessionError::ResourceLimitExceeded { .. } => crate::core::ErrorCategory::Resource,
                SessionError::ProtocolViolation { .. } => crate::core::ErrorCategory::Protocol,
                SessionError::CapacityExceeded { .. } => crate::core::ErrorCategory::Resource,
                SessionError::InvalidStateTransition { .. } => crate::core::ErrorCategory::Protocol,
            };
            metrics.record_sync_failure(&session_id.to_string(), category, &error.to_string());
        }

        Ok(())
    }

    /// Timeout a session
    pub fn timeout_session(&mut self, session_id: SessionId) -> SyncResult<()> {
        let session = self.sessions.get_mut(&session_id)
            .ok_or_else(|| SyncError::session(&format!("Session {} not found", session_id)))?;

        let duration_ms = session.duration_ms().unwrap_or(0);
        let last_known_state = format!("{:?}", session);

        let result = SessionResult::Timeout {
            duration_ms,
            last_known_state,
        };

        *session = SessionState::Completed(result);

        // Record metrics
        if let Some(ref metrics) = self.metrics {
            metrics.record_sync_failure(
                &session_id.to_string(),
                crate::core::ErrorCategory::Timeout,
                "Session timeout",
            );
        }

        Ok(())
    }

    /// Get session state
    pub fn get_session(&self, session_id: &SessionId) -> Option<&SessionState<T>> {
        self.sessions.get(session_id)
    }

    /// Get protocol state for active session
    pub fn get_protocol_state(&self, session_id: &SessionId) -> Option<&T> {
        match self.sessions.get(session_id)? {
            SessionState::Active { protocol_state, .. } => Some(protocol_state),
            _ => None,
        }
    }

    /// List all active sessions
    pub fn active_sessions(&self) -> Vec<(SessionId, &SessionState<T>)> {
        self.sessions
            .iter()
            .filter(|(_, state)| state.is_active())
            .map(|(id, state)| (*id, state))
            .collect()
    }

    /// Count active sessions
    pub fn count_active_sessions(&self) -> usize {
        self.sessions.values().filter(|state| state.is_active()).count()
    }

    /// Count completed sessions
    pub fn count_completed_sessions(&self) -> usize {
        self.sessions.values().filter(|state| state.is_terminal()).count()
    }

    /// Cleanup stale and completed sessions
    ///
    /// Note: Callers should obtain `now` via `TimeEffects::now_instant()` and pass it to this method
    pub fn cleanup_stale_sessions(&mut self, now: Instant) -> SyncResult<usize> {
        if now.duration_since(self.last_cleanup) < self.config.cleanup_interval {
            return Ok(0);
        }

        let mut removed = 0;
        let mut to_timeout = Vec::new();
        let mut to_remove = Vec::new();

        // Identify sessions to timeout or remove
        for (session_id, session) in &self.sessions {
            if session.is_timed_out() && !session.is_terminal() {
                to_timeout.push(*session_id);
            } else if session.is_terminal() {
                // Remove completed sessions after some time
                if let Some(duration_ms) = session.duration_ms() {
                    if duration_ms > self.config.cleanup_interval.as_millis() as u64 {
                        to_remove.push(*session_id);
                    }
                }
            }
        }

        // Timeout sessions
        for session_id in to_timeout {
            self.timeout_session(session_id)?;
            removed += 1;
        }

        // Remove completed sessions
        for session_id in to_remove {
            self.sessions.remove(&session_id);
            removed += 1;
        }

        self.last_cleanup = now;
        Ok(removed)
    }

    /// Get session statistics
    pub fn get_statistics(&self) -> SessionManagerStatistics {
        let mut active_count = 0;
        let mut completed_count = 0;
        let mut failed_count = 0;
        let mut timeout_count = 0;
        let mut total_duration_ms = 0u64;
        let mut total_operations = 0usize;

        for session in self.sessions.values() {
            match session {
                SessionState::Active { .. } => active_count += 1,
                SessionState::Completed(result) => {
                    match result {
                        SessionResult::Success { duration_ms, operations_count, .. } => {
                            completed_count += 1;
                            total_duration_ms += duration_ms;
                            total_operations += operations_count;
                        }
                        SessionResult::Failure { duration_ms, .. } => {
                            failed_count += 1;
                            total_duration_ms += duration_ms;
                        }
                        SessionResult::Timeout { duration_ms } => {
                            timeout_count += 1;
                            total_duration_ms += duration_ms;
                        }
                    }
                }
                _ => {} // Ignore initializing/terminating for stats
            }
        }

        let total_sessions = completed_count + failed_count + timeout_count;
        let success_rate = if total_sessions > 0 {
            (completed_count as f64 / total_sessions as f64) * 100.0
        } else {
            100.0
        };

        let average_duration_ms = if total_sessions > 0 {
            total_duration_ms / total_sessions as u64
        } else {
            0
        };

        SessionManagerStatistics {
            active_sessions: active_count,
            completed_sessions: completed_count,
            failed_sessions: failed_count,
            timeout_sessions: timeout_count,
            total_sessions,
            success_rate_percent: success_rate,
            average_duration_ms,
            total_operations,
        }
    }
}

/// Session manager statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionManagerStatistics {
    pub active_sessions: usize,
    pub completed_sessions: usize,
    pub failed_sessions: usize,
    pub timeout_sessions: usize,
    pub total_sessions: usize,
    pub success_rate_percent: f64,
    pub average_duration_ms: u64,
    pub total_operations: usize,
}

impl Default for SessionManagerStatistics {
    fn default() -> Self {
        Self {
            active_sessions: 0,
            completed_sessions: 0,
            failed_sessions: 0,
            timeout_sessions: 0,
            total_sessions: 0,
            success_rate_percent: 100.0,
            average_duration_ms: 0,
            total_operations: 0,
        }
    }
}

/// Session manager builder for easy configuration
pub struct SessionManagerBuilder<T> {
    config: SessionConfig,
    metrics: Option<MetricsCollector>,
    _phantom: std::marker::PhantomData<T>,
}

impl<T> SessionManagerBuilder<T>
where
    T: Clone + Send + Sync + Serialize + for<'de> Deserialize<'de>,
{
    /// Create new builder with default configuration
    pub fn new() -> Self {
        Self {
            config: SessionConfig::default(),
            metrics: None,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Set custom configuration
    pub fn config(mut self, config: SessionConfig) -> Self {
        self.config = config;
        self
    }

    /// Enable metrics collection
    pub fn with_metrics(mut self, metrics: MetricsCollector) -> Self {
        self.metrics = Some(metrics);
        self
    }

    /// Build the session manager
    pub fn build(self) -> SessionManager<T> {
        if let Some(metrics) = self.metrics {
            SessionManager::with_metrics(self.config, metrics)
        } else {
            SessionManager::new(self.config)
        }
    }
}

impl<T> Default for SessionManagerBuilder<T>
where
    T: Clone + Send + Sync + Serialize + for<'de> Deserialize<'de>,
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::test_utils::test_device_id;
    use std::thread;
    use std::time::Duration as StdDuration;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct TestProtocolState {
        phase: String,
        data: Vec<u8>,
    }

    #[test]
    fn test_session_creation_and_activation() {
        #[allow(clippy::disallowed_methods)]
        let now = Instant::now();
        let mut manager = SessionManager::<TestProtocolState>::new(SessionConfig::default(), now);
        let participants = vec![test_device_id(1), test_device_id(2)];

        // Create session
        let session_id = manager.create_session(participants.clone(), now).unwrap();
        assert_eq!(manager.count_active_sessions(), 0); // Not active yet

        // Activate session
        let initial_state = TestProtocolState {
            phase: "initialization".to_string(),
            data: vec![1, 2, 3],
        };
        manager.activate_session(session_id, initial_state.clone()).unwrap();
        assert_eq!(manager.count_active_sessions(), 1);

        // Verify session state
        let session = manager.get_session(&session_id).unwrap();
        match session {
            SessionState::Active { protocol_state, participants: session_participants, .. } => {
                assert_eq!(protocol_state, &initial_state);
                assert_eq!(session_participants, &participants);
            }
            _ => panic!("Session should be active"),
        }
    }

    #[test]
    fn test_session_completion() {
        #[allow(clippy::disallowed_methods)]
        let now = Instant::now();
        let mut manager = SessionManager::<TestProtocolState>::new(SessionConfig::default(), now);
        let session_id = manager.create_session(vec![test_device_id(1)], now).unwrap();

        let initial_state = TestProtocolState {
            phase: "test".to_string(),
            data: vec![],
        };
        manager.activate_session(session_id, initial_state).unwrap();

        // Complete session
        let mut metadata = HashMap::new();
        metadata.insert("test_key".to_string(), "test_value".to_string());
        
        manager.complete_session(session_id, 100, 1024, metadata).unwrap();
        assert_eq!(manager.count_active_sessions(), 0);
        assert_eq!(manager.count_completed_sessions(), 1);

        // Verify result
        let session = manager.get_session(&session_id).unwrap();
        match session {
            SessionState::Completed(SessionResult::Success { operations_count, bytes_transferred, .. }) => {
                assert_eq!(*operations_count, 100);
                assert_eq!(*bytes_transferred, 1024);
            }
            _ => panic!("Session should be completed successfully"),
        }
    }

    #[test]
    fn test_session_failure() {
        #[allow(clippy::disallowed_methods)]
        let now = Instant::now();
        let mut manager = SessionManager::<TestProtocolState>::new(SessionConfig::default(), now);
        let session_id = manager.create_session(vec![test_device_id(1)], now).unwrap();

        let initial_state = TestProtocolState {
            phase: "test".to_string(),
            data: vec![],
        };
        manager.activate_session(session_id, initial_state).unwrap();

        // Fail session
        let error = SessionError::ProtocolViolation {
            constraint: "test constraint".to_string(),
        };
        manager.fail_session(session_id, error.clone(), None).unwrap();

        // Verify failure
        let session = manager.get_session(&session_id).unwrap();
        match session {
            SessionState::Completed(SessionResult::Failure { error: session_error, .. }) => {
                match session_error {
                    SessionError::ProtocolViolation { constraint } => {
                        assert_eq!(constraint, "test constraint");
                    }
                    _ => panic!("Wrong error type"),
                }
            }
            _ => panic!("Session should be completed with failure"),
        }
    }

    #[test]
    fn test_concurrent_session_limit() {
        let config = SessionConfig {
            max_concurrent_sessions: 2,
            ..SessionConfig::default()
        };
        #[allow(clippy::disallowed_methods)]
        let now = Instant::now();
        let mut manager = SessionManager::<TestProtocolState>::new(config, now);

        // Create and activate maximum sessions
        let session1 = manager.create_session(vec![test_device_id(1)], now).unwrap();
        let session2 = manager.create_session(vec![test_device_id(1)], now).unwrap();

        let state = TestProtocolState {
            phase: "test".to_string(),
            data: vec![],
        };
        manager.activate_session(session1, state.clone()).unwrap();
        manager.activate_session(session2, state).unwrap();

        // Try to exceed limit
        let result = manager.create_session(vec![test_device_id(1)], now);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SyncError::ResourceExhausted { .. }));
    }

    #[test]
    fn test_session_timeout() {
        let config = SessionConfig {
            timeout: Duration::from_millis(100),
            ..SessionConfig::default()
        };
        #[allow(clippy::disallowed_methods)]
        let now = Instant::now();
        let mut manager = SessionManager::<TestProtocolState>::new(config, now);

        let session_id = manager.create_session(vec![test_device_id(1)], now).unwrap();
        
        // Wait for timeout
        thread::sleep(StdDuration::from_millis(150));
        
        // Try to activate - should fail due to timeout
        let state = TestProtocolState {
            phase: "test".to_string(),
            data: vec![],
        };
        let result = manager.activate_session(session_id, state);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SyncError::Timeout { .. }));
    }

    #[test]
    fn test_cleanup_stale_sessions() {
        let config = SessionConfig {
            cleanup_interval: Duration::from_millis(50),
            ..SessionConfig::default()
        };
        #[allow(clippy::disallowed_methods)]
        let now = Instant::now();
        let mut manager = SessionManager::<TestProtocolState>::new(config, now);

        // Create and complete a session
        let session_id = manager.create_session(vec![test_device_id(1)], now).unwrap();
        let state = TestProtocolState {
            phase: "test".to_string(),
            data: vec![],
        };
        manager.activate_session(session_id, state).unwrap();
        manager.complete_session(session_id, 0, 0, HashMap::new()).unwrap();

        assert_eq!(manager.sessions.len(), 1);

        // Wait for cleanup interval
        thread::sleep(StdDuration::from_millis(100));

        // Cleanup should remove completed sessions
        #[allow(clippy::disallowed_methods)]
        let now_cleanup = Instant::now();
        let removed = manager.cleanup_stale_sessions(now_cleanup).unwrap();
        assert!(removed > 0);
    }

    #[test]
    fn test_session_statistics() {
        #[allow(clippy::disallowed_methods)]
        let now = Instant::now();
        let mut manager = SessionManager::<TestProtocolState>::new(SessionConfig::default(), now);

        // Create and complete some sessions
        for i in 0..3 {
            let session_id = manager.create_session(vec![test_device_id(1)], now).unwrap();
            let state = TestProtocolState {
                phase: "test".to_string(),
                data: vec![],
            };
            manager.activate_session(session_id, state).unwrap();
            
            if i < 2 {
                manager.complete_session(session_id, 10 * (i + 1), 100 * (i + 1), HashMap::new()).unwrap();
            } else {
                let error = SessionError::ProtocolViolation {
                    constraint: "test".to_string(),
                };
                manager.fail_session(session_id, error, None).unwrap();
            }
        }

        let stats = manager.get_statistics();
        assert_eq!(stats.total_sessions, 3);
        assert_eq!(stats.completed_sessions, 2);
        assert_eq!(stats.failed_sessions, 1);
        assert_eq!(stats.timeout_sessions, 0);
        assert!((stats.success_rate_percent - 66.67).abs() < 0.1); // 2/3 * 100
    }
}