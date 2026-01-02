//! Unified session management for aura-sync protocols
//!
//! This module provides a centralized session management system for
//! all session lifecycle, state tracking, and coordination patterns.
//!
//! **Time System**: Uses `PhysicalTime` for timestamps per the unified time architecture.

use crate::core::metrics::ErrorCategory;
use crate::core::{
    sync_resource_with_limit, sync_session_error, sync_timeout_error, sync_validation_error,
    MetricsCollector, SyncConfig, SyncResult,
};
use aura_core::time::PhysicalTime;
use aura_core::{DeviceId, SessionId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Unified session state machine following choreographic patterns
///
/// **Time System**: Uses `PhysicalTime` for timestamps per the unified time architecture.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SessionState<T> {
    /// Session initialization phase
    Initializing {
        participants: Vec<DeviceId>,
        /// Timeout timestamp (unified time system)
        timeout_at: PhysicalTime,
        /// Creation timestamp (unified time system)
        created_at: PhysicalTime,
    },
    /// Active session with protocol-specific state
    Active {
        protocol_state: T,
        /// Start timestamp (unified time system)
        started_at: PhysicalTime,
        participants: Vec<DeviceId>,
        /// Timeout timestamp (unified time system)
        timeout_at: PhysicalTime,
    },
    /// Session termination phase with results
    Terminating {
        result: SessionResult,
        /// Cleanup deadline timestamp (unified time system)
        cleanup_deadline: PhysicalTime,
    },
    /// Session completed and cleaned up
    Completed(SessionResult),
}

impl<T> SessionState<T> {
    /// Check if session has timed out
    ///
    /// **Time System**: Accepts `PhysicalTime` for comparison.
    pub fn is_timed_out(&self, now: &PhysicalTime) -> bool {
        match self {
            SessionState::Initializing { timeout_at, .. } => now.ts_ms >= timeout_at.ts_ms,
            SessionState::Active { timeout_at, .. } => now.ts_ms >= timeout_at.ts_ms,
            SessionState::Terminating {
                cleanup_deadline, ..
            } => now.ts_ms >= cleanup_deadline.ts_ms,
            SessionState::Completed(_) => false,
        }
    }

    /// Check if session has timed out (from milliseconds)
    ///
    /// Convenience method for backward compatibility.
    pub fn is_timed_out_ms(&self, now_ms: u64) -> bool {
        self.is_timed_out(&PhysicalTime {
            ts_ms: now_ms,
            uncertainty: None,
        })
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
    ///
    /// **Time System**: Accepts `PhysicalTime` for comparison.
    pub fn duration_ms(&self, current_time: &PhysicalTime) -> Option<u64> {
        match self {
            SessionState::Active { started_at, .. } => {
                Some(current_time.ts_ms.saturating_sub(started_at.ts_ms))
            }
            SessionState::Terminating { result, .. } | SessionState::Completed(result) => {
                match result {
                    SessionResult::Success { duration_ms, .. } => Some(*duration_ms),
                    SessionResult::Failure { duration_ms, .. } => Some(*duration_ms),
                    SessionResult::Timeout { duration_ms, .. } => Some(*duration_ms),
                }
            }
            SessionState::Initializing { created_at, .. } => {
                Some(current_time.ts_ms.saturating_sub(created_at.ts_ms))
            }
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SessionResult {
    Success {
        duration_ms: u64,
        operations_count: u64,
        bytes_transferred: u64,
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
            SessionResult::Timeout { duration_ms, .. } => *duration_ms,
        }
    }

    /// Get operations count for successful sessions
    pub fn operations_count(&self) -> u64 {
        match self {
            SessionResult::Success {
                operations_count, ..
            } => *operations_count,
            SessionResult::Failure {
                partial_results: Some(partial),
                ..
            } => partial.operations_completed,
            _ => 0,
        }
    }
}

/// Partial results for failed sessions
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PartialResults {
    pub operations_completed: u64,
    pub bytes_transferred: u64,
    pub completed_participants: Vec<DeviceId>,
    pub last_successful_operation: Option<String>,
}

/// Session-specific errors
#[derive(Debug, Clone, PartialEq, thiserror::Error, Serialize, Deserialize)]
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
    CapacityExceeded { current: u64, max: u64 },

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
            max_concurrent_sessions: sync_config.peer_management.max_concurrent_syncs as usize,
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
            max_memory_per_session: 10 * 1024 * 1024,        // 10 MB
            max_session_duration: Duration::from_secs(3600), // 1 hour
            max_operations_per_session: 10000,
        }
    }
}

impl From<&crate::core::PerformanceConfig> for SessionResourceLimits {
    fn from(perf_config: &crate::core::PerformanceConfig) -> Self {
        Self {
            max_memory_per_session: (perf_config.memory_limit / 10) as usize, // 1/10th of total limit
            max_session_duration: Duration::from_secs(3600),       // 1 hour
            max_operations_per_session: 10000,
        }
    }
}

/// Generic session manager for protocol-agnostic session coordination
///
/// **Time System**: Uses `PhysicalTime` for timestamps per the unified time architecture.
pub struct SessionManager<T> {
    /// Active sessions indexed by session ID
    sessions: HashMap<SessionId, SessionState<T>>,
    /// Session configuration
    config: SessionConfig,
    /// Metrics collector for session telemetry
    metrics: Option<MetricsCollector>,
    /// Last cleanup timestamp (unified time system)
    last_cleanup: PhysicalTime,
    /// Monotonic counter used to derive deterministic-but-unique session IDs
    session_counter: u64,
}

impl<T> SessionManager<T>
where
    T: Clone + Send + Sync + Serialize + for<'de> Deserialize<'de>,
{
    /// Create a new session manager
    ///
    /// **Time System**: Uses `PhysicalTime` for timestamps.
    pub fn new(config: SessionConfig, now: PhysicalTime) -> Self {
        Self {
            sessions: HashMap::new(),
            config,
            metrics: None,
            last_cleanup: now,
            session_counter: 0,
        }
    }

    /// Create a new session manager from milliseconds timestamp
    ///
    /// Convenience constructor for backward compatibility.
    pub fn new_from_ms(config: SessionConfig, now_ms: u64) -> Self {
        Self::new(
            config,
            PhysicalTime {
                ts_ms: now_ms,
                uncertainty: None,
            },
        )
    }

    /// Create session manager with metrics collection
    ///
    /// **Time System**: Uses `PhysicalTime` for timestamps.
    pub fn with_metrics(
        config: SessionConfig,
        metrics: MetricsCollector,
        now: PhysicalTime,
    ) -> Self {
        Self {
            sessions: HashMap::new(),
            config,
            metrics: Some(metrics),
            last_cleanup: now,
            session_counter: 0,
        }
    }

    /// Deterministically derive a unique session ID using the caller-supplied timestamp
    /// and a local monotonic counter (no ambient randomness).
    fn generate_session_id(&mut self, now: &PhysicalTime) -> SessionId {
        let mut input = Vec::new();
        input.extend_from_slice(b"aura.sync.session.id");
        input.extend_from_slice(&now.ts_ms.to_le_bytes());
        input.extend_from_slice(&self.session_counter.to_le_bytes());
        self.session_counter = self.session_counter.wrapping_add(1);

        let digest = aura_core::hash::hash(&input);
        let mut uuid_bytes = [0u8; 16];
        uuid_bytes.copy_from_slice(&digest[..16]);
        SessionId::from_uuid(uuid::Uuid::from_bytes(uuid_bytes))
    }

    /// Create a new session with participants
    ///
    /// **Time System**: Uses `PhysicalTime` for timestamps.
    /// Note: Callers should obtain `now` via their time provider and pass it to this method
    pub fn create_session(
        &mut self,
        participants: Vec<DeviceId>,
        now: &PhysicalTime,
    ) -> SyncResult<SessionId> {
        // Validate participant count
        if participants.len() > self.config.max_participants {
            return Err(sync_validation_error(format!(
                "Too many participants: {} > {}",
                participants.len(),
                self.config.max_participants
            )));
        }

        // Check concurrent session limit
        let active_count = self.count_active_sessions();
        if active_count >= self.config.max_concurrent_sessions {
            return Err(sync_resource_with_limit(
                "concurrent_sessions",
                "Maximum concurrent sessions exceeded",
                self.config.max_concurrent_sessions as u64,
            ));
        }

        let session_id = self.generate_session_id(now);
        let timeout_ms = now.ts_ms + self.config.timeout.as_millis() as u64;

        let session_state = SessionState::Initializing {
            participants,
            timeout_at: PhysicalTime {
                ts_ms: timeout_ms,
                uncertainty: now.uncertainty,
            },
            created_at: now.clone(),
        };

        self.sessions.insert(session_id, session_state);

        // Record metrics with the provided now parameter
        if let Some(ref metrics) = self.metrics {
            metrics.record_sync_start(&session_id.to_string(), now.ts_ms);
        }

        Ok(session_id)
    }

    /// Create a new session with participants (from milliseconds)
    ///
    /// Convenience method for backward compatibility.
    pub fn create_session_ms(
        &mut self,
        participants: Vec<DeviceId>,
        now_ms: u64,
    ) -> SyncResult<SessionId> {
        self.create_session(
            participants,
            &PhysicalTime {
                ts_ms: now_ms,
                uncertainty: None,
            },
        )
    }

    /// Activate a session with initial protocol state
    ///
    /// **Time System**: Uses `PhysicalTime` for timestamps.
    pub fn activate_session(
        &mut self,
        session_id: SessionId,
        protocol_state: T,
        current_time: &PhysicalTime,
    ) -> SyncResult<()> {
        let session = self
            .sessions
            .get_mut(&session_id)
            .ok_or_else(|| sync_session_error(format!("Session {session_id} not found")))?;

        // Check timeout before pattern matching to avoid borrow conflicts
        if session.is_timed_out(current_time) {
            return Err(sync_timeout_error(
                "session_activation",
                self.config.timeout,
            ));
        }

        match session {
            SessionState::Initializing { participants, .. } => {
                let participants = participants.clone();
                let timeout_ms = current_time.ts_ms
                    + self.config.resource_limits.max_session_duration.as_millis() as u64;
                *session = SessionState::Active {
                    protocol_state,
                    started_at: current_time.clone(),
                    participants,
                    timeout_at: PhysicalTime {
                        ts_ms: timeout_ms,
                        uncertainty: current_time.uncertainty,
                    },
                };

                Ok(())
            }
            _ => Err(sync_session_error(format!(
                "Session {session_id} is not in initializing state"
            ))),
        }
    }

    /// Activate a session with initial protocol state (from milliseconds)
    ///
    /// Convenience method for backward compatibility.
    pub fn activate_session_ms(
        &mut self,
        session_id: SessionId,
        protocol_state: T,
        current_timestamp_ms: u64,
    ) -> SyncResult<()> {
        self.activate_session(
            session_id,
            protocol_state,
            &PhysicalTime {
                ts_ms: current_timestamp_ms,
                uncertainty: None,
            },
        )
    }

    /// Update session protocol state
    ///
    /// **Time System**: Uses `PhysicalTime` for timestamps.
    pub fn update_session(
        &mut self,
        session_id: SessionId,
        new_state: T,
        current_time: &PhysicalTime,
    ) -> SyncResult<()>
    where
        T: std::fmt::Debug,
    {
        let session = self
            .sessions
            .get_mut(&session_id)
            .ok_or_else(|| sync_session_error(format!("Session {session_id} not found")))?;

        // Check timeout before pattern matching to avoid borrow conflicts
        if session.is_timed_out(current_time) {
            self.timeout_session(session_id, current_time)?;
            return Err(sync_timeout_error("session_update", self.config.timeout));
        }

        match session {
            SessionState::Active { protocol_state, .. } => {
                *protocol_state = new_state;
                Ok(())
            }
            _ => Err(sync_session_error(format!(
                "Session {session_id} is not active"
            ))),
        }
    }

    /// Update session protocol state (from milliseconds)
    ///
    /// Convenience method for backward compatibility.
    pub fn update_session_ms(
        &mut self,
        session_id: SessionId,
        new_state: T,
        current_timestamp_ms: u64,
    ) -> SyncResult<()>
    where
        T: std::fmt::Debug,
    {
        self.update_session(
            session_id,
            new_state,
            &PhysicalTime {
                ts_ms: current_timestamp_ms,
                uncertainty: None,
            },
        )
    }

    /// Complete a session successfully
    ///
    /// **Time System**: Uses `PhysicalTime` for timestamps.
    pub fn complete_session(
        &mut self,
        session_id: SessionId,
        operations_count: u64,
        bytes_transferred: u64,
        metadata: HashMap<String, String>,
        current_time: &PhysicalTime,
    ) -> SyncResult<()> {
        let session = self
            .sessions
            .get_mut(&session_id)
            .ok_or_else(|| sync_session_error(format!("Session {session_id} not found")))?;

        let duration_ms = session.duration_ms(current_time).unwrap_or(0);
        let participants = session.participants().to_vec();

        let result = SessionResult::Success {
            duration_ms,
            operations_count,
            bytes_transferred,
            participants,
            metadata,
        };

        *session = SessionState::Completed(result);

        // Record metrics
        if let Some(ref metrics) = self.metrics {
            metrics.record_sync_completion(
                &session_id.to_string(),
                operations_count,
                bytes_transferred,
                current_time.ts_ms,
            );
        }

        Ok(())
    }

    /// Complete a session successfully (from milliseconds)
    ///
    /// Convenience method for backward compatibility.
    pub fn complete_session_ms(
        &mut self,
        session_id: SessionId,
        operations_count: u64,
        bytes_transferred: u64,
        metadata: HashMap<String, String>,
        current_timestamp_ms: u64,
    ) -> SyncResult<()> {
        self.complete_session(
            session_id,
            operations_count,
            bytes_transferred,
            metadata,
            &PhysicalTime {
                ts_ms: current_timestamp_ms,
                uncertainty: None,
            },
        )
    }

    /// Fail a session with error context
    ///
    /// **Time System**: Uses `PhysicalTime` for timestamps.
    pub fn fail_session(
        &mut self,
        session_id: SessionId,
        error: SessionError,
        partial_results: Option<PartialResults>,
        current_time: &PhysicalTime,
    ) -> SyncResult<()> {
        let session = self
            .sessions
            .get_mut(&session_id)
            .ok_or_else(|| sync_session_error(format!("Session {session_id} not found")))?;

        let duration_ms = session.duration_ms(current_time).unwrap_or(0);

        let result = SessionResult::Failure {
            error: error.clone(),
            duration_ms,
            partial_results,
        };

        *session = SessionState::Completed(result);

        // Record metrics
        if let Some(ref metrics) = self.metrics {
            let category = match error {
                SessionError::Timeout { .. } => ErrorCategory::Timeout,
                SessionError::ParticipantDisconnected { .. } => ErrorCategory::Network,
                SessionError::ResourceLimitExceeded { .. } => ErrorCategory::Resource,
                SessionError::ProtocolViolation { .. } => ErrorCategory::Protocol,
                SessionError::CapacityExceeded { .. } => ErrorCategory::Resource,
                SessionError::InvalidStateTransition { .. } => ErrorCategory::Protocol,
            };
            metrics.record_sync_failure(&session_id.to_string(), category, &error.to_string());
        }

        Ok(())
    }

    /// Fail a session with error context (from milliseconds)
    ///
    /// Convenience method for backward compatibility.
    pub fn fail_session_ms(
        &mut self,
        session_id: SessionId,
        error: SessionError,
        partial_results: Option<PartialResults>,
        current_timestamp_ms: u64,
    ) -> SyncResult<()> {
        self.fail_session(
            session_id,
            error,
            partial_results,
            &PhysicalTime {
                ts_ms: current_timestamp_ms,
                uncertainty: None,
            },
        )
    }

    /// Timeout a session
    ///
    /// **Time System**: Uses `PhysicalTime` for timestamps.
    pub fn timeout_session(
        &mut self,
        session_id: SessionId,
        current_time: &PhysicalTime,
    ) -> SyncResult<()>
    where
        T: std::fmt::Debug,
    {
        let session = self
            .sessions
            .get_mut(&session_id)
            .ok_or_else(|| sync_session_error(format!("Session {session_id} not found")))?;

        let duration_ms = session.duration_ms(current_time).unwrap_or(0);
        let last_known_state = format!("{session:?}");

        let result = SessionResult::Timeout {
            duration_ms,
            last_known_state,
        };

        *session = SessionState::Completed(result);

        // Record metrics
        if let Some(ref metrics) = self.metrics {
            metrics.record_sync_failure(
                &session_id.to_string(),
                ErrorCategory::Timeout,
                "Session timeout",
            );
        }

        Ok(())
    }

    /// Timeout a session (from milliseconds)
    ///
    /// Convenience method for backward compatibility.
    pub fn timeout_session_ms(
        &mut self,
        session_id: SessionId,
        current_timestamp_ms: u64,
    ) -> SyncResult<()>
    where
        T: std::fmt::Debug,
    {
        self.timeout_session(
            session_id,
            &PhysicalTime {
                ts_ms: current_timestamp_ms,
                uncertainty: None,
            },
        )
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
        self.sessions
            .values()
            .filter(|state| state.is_active())
            .count()
    }

    /// Count completed sessions
    pub fn count_completed_sessions(&self) -> usize {
        self.sessions
            .values()
            .filter(|state| state.is_terminal())
            .count()
    }

    /// Cleanup stale and completed sessions
    ///
    /// **Time System**: Uses `PhysicalTime` for timestamps.
    /// Note: Callers should obtain `now` via their time provider and pass it to this method
    pub fn cleanup_stale_sessions(&mut self, now: &PhysicalTime) -> SyncResult<usize>
    where
        T: std::fmt::Debug,
    {
        let elapsed_ms = now.ts_ms.saturating_sub(self.last_cleanup.ts_ms);
        if elapsed_ms < self.config.cleanup_interval.as_millis() as u64 {
            return Ok(0);
        }

        let mut removed = 0;
        let mut to_timeout = Vec::new();
        let mut to_remove = Vec::new();

        // Identify sessions to timeout or remove
        for (session_id, session) in &self.sessions {
            if session.is_timed_out(now) && !session.is_terminal() {
                to_timeout.push(*session_id);
            } else if session.is_terminal() {
                // Completed sessions are removed on the next cleanup run
                to_remove.push(*session_id);
            }
        }

        // Timeout sessions
        for session_id in to_timeout {
            self.timeout_session(session_id, now)?;
            removed += 1;
        }

        // Remove completed sessions
        for session_id in to_remove {
            self.sessions.remove(&session_id);
            removed += 1;
        }

        self.last_cleanup = now.clone();
        Ok(removed)
    }

    /// Cleanup stale and completed sessions (from milliseconds)
    ///
    /// Convenience method for backward compatibility.
    pub fn cleanup_stale_sessions_ms(&mut self, now_ms: u64) -> SyncResult<usize>
    where
        T: std::fmt::Debug,
    {
        self.cleanup_stale_sessions(&PhysicalTime {
            ts_ms: now_ms,
            uncertainty: None,
        })
    }

    /// Get session statistics
    pub fn get_statistics(&self) -> SessionManagerStatistics {
        let mut active_count = 0u64;
        let mut completed_count = 0u64;
        let mut failed_count = 0u64;
        let mut timeout_count = 0u64;
        let mut total_duration_ms = 0u64;
        let mut total_operations = 0u64;

        for session in self.sessions.values() {
            match session {
                SessionState::Active { .. } => active_count += 1,
                SessionState::Completed(result) => match result {
                    SessionResult::Success {
                        duration_ms,
                        operations_count,
                        ..
                    } => {
                        completed_count += 1;
                        total_duration_ms += duration_ms;
                        total_operations += operations_count;
                    }
                    SessionResult::Failure { duration_ms, .. } => {
                        failed_count += 1;
                        total_duration_ms += duration_ms;
                    }
                    SessionResult::Timeout { duration_ms, .. } => {
                        timeout_count += 1;
                        total_duration_ms += duration_ms;
                    }
                },
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
            total_duration_ms / total_sessions
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

    /// Close a session for a specific peer
    pub fn close_session(&mut self, peer: DeviceId) -> SyncResult<()> {
        // Find sessions involving this peer and close them
        let session_ids_to_remove: Vec<SessionId> = self
            .sessions
            .iter()
            .filter_map(|(session_id, session_state)| match session_state {
                SessionState::Active { participants, .. } => {
                    if participants.contains(&peer) {
                        Some(*session_id)
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .collect();

        for session_id in session_ids_to_remove {
            self.sessions.remove(&session_id);
        }

        Ok(())
    }

    /// Check if peer has an active session
    pub fn has_active_session(&self, peer: DeviceId) -> bool {
        self.sessions.values().any(|session_state| {
            matches!(session_state, SessionState::Active { participants, .. } if participants.contains(&peer))
        })
    }
}

/// Session manager statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionManagerStatistics {
    pub active_sessions: u64,
    pub completed_sessions: u64,
    pub failed_sessions: u64,
    pub timeout_sessions: u64,
    pub total_sessions: u64,
    pub success_rate_percent: f64,
    pub average_duration_ms: u64,
    pub total_operations: u64,
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
    ///
    /// **Time System**: Uses `PhysicalTime` for timestamps.
    /// Note: Callers should obtain `now` via their time provider and pass it to this method
    pub fn build(self, now: PhysicalTime) -> SessionManager<T> {
        if let Some(metrics) = self.metrics {
            SessionManager::with_metrics(self.config, metrics, now)
        } else {
            SessionManager::new(self.config, now)
        }
    }

    /// Build the session manager (from milliseconds)
    ///
    /// Convenience method for backward compatibility.
    pub fn build_ms(self, now_ms: u64) -> SessionManager<T> {
        self.build(PhysicalTime {
            ts_ms: now_ms,
            uncertainty: None,
        })
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
    use aura_core::AuraError;
    use aura_testkit::builders::test_device_id;

    /// Helper function to create PhysicalTime for tests
    fn test_time(ts_ms: u64) -> PhysicalTime {
        PhysicalTime {
            ts_ms,
            uncertainty: None,
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct TestProtocolState {
        phase: String,
        data: Vec<u8>,
    }

    #[test]
    fn test_session_creation_and_activation() {
        let now = test_time(1000000); // Unix timestamp in ms
        let mut manager =
            SessionManager::<TestProtocolState>::new(SessionConfig::default(), now.clone());
        let participants = vec![test_device_id(1), test_device_id(2)];

        // Create session
        let session_id = manager.create_session(participants.clone(), &now).unwrap();
        assert_eq!(manager.count_active_sessions(), 0); // Not active yet

        // Activate session
        let initial_state = TestProtocolState {
            phase: "initialization".to_string(),
            data: vec![1, 2, 3],
        };
        manager
            .activate_session(session_id, initial_state.clone(), &now)
            .unwrap();
        assert_eq!(manager.count_active_sessions(), 1);

        // Verify session state
        let session = manager.get_session(&session_id).unwrap();
        match session {
            SessionState::Active {
                protocol_state,
                participants: session_participants,
                ..
            } => {
                assert_eq!(protocol_state, &initial_state);
                assert_eq!(session_participants, &participants);
            }
            _ => panic!("Session should be active"),
        }
    }

    #[test]
    fn test_session_completion() {
        let now = test_time(1000000); // Unix timestamp in ms
        let mut manager =
            SessionManager::<TestProtocolState>::new(SessionConfig::default(), now.clone());
        let session_id = manager
            .create_session(vec![test_device_id(1)], &now)
            .unwrap();

        let initial_state = TestProtocolState {
            phase: "test".to_string(),
            data: vec![],
        };
        manager
            .activate_session(session_id, initial_state, &now)
            .unwrap();

        // Complete session
        let mut metadata = HashMap::new();
        metadata.insert("test_key".to_string(), "test_value".to_string());

        manager
            .complete_session(session_id, 100, 1024, metadata, &test_time(1000100))
            .unwrap();
        assert_eq!(manager.count_active_sessions(), 0);
        assert_eq!(manager.count_completed_sessions(), 1);

        // Verify result
        let session = manager.get_session(&session_id).unwrap();
        match session {
            SessionState::Completed(SessionResult::Success {
                operations_count,
                bytes_transferred,
                ..
            }) => {
                assert_eq!(*operations_count, 100);
                assert_eq!(*bytes_transferred, 1024);
            }
            _ => panic!("Session should be completed successfully"),
        }
    }

    #[test]
    fn test_session_failure() {
        let now = test_time(1000000); // Unix timestamp in ms
        let mut manager =
            SessionManager::<TestProtocolState>::new(SessionConfig::default(), now.clone());
        let session_id = manager
            .create_session(vec![test_device_id(1)], &now)
            .unwrap();

        let initial_state = TestProtocolState {
            phase: "test".to_string(),
            data: vec![],
        };
        manager
            .activate_session(session_id, initial_state, &now)
            .unwrap();

        // Fail session
        let error = SessionError::ProtocolViolation {
            constraint: "test constraint".to_string(),
        };
        manager
            .fail_session(session_id, error, None, &test_time(1000010))
            .unwrap();

        // Verify failure
        let session = manager.get_session(&session_id).unwrap();
        match session {
            SessionState::Completed(SessionResult::Failure {
                error: session_error,
                ..
            }) => match session_error {
                SessionError::ProtocolViolation { constraint } => {
                    assert_eq!(constraint, "test constraint");
                }
                _ => panic!("Wrong error type"),
            },
            _ => panic!("Session should be completed with failure"),
        }
    }

    #[test]
    fn test_concurrent_session_limit() {
        let config = SessionConfig {
            max_concurrent_sessions: 2,
            ..SessionConfig::default()
        };
        let now = test_time(1000000); // Unix timestamp in ms
        let mut manager = SessionManager::<TestProtocolState>::new(config, now.clone());

        // Create and activate maximum sessions
        let session1 = manager
            .create_session(vec![test_device_id(1)], &now)
            .unwrap();
        let session2 = manager
            .create_session(vec![test_device_id(1)], &now)
            .unwrap();

        let state = TestProtocolState {
            phase: "test".to_string(),
            data: vec![],
        };
        manager
            .activate_session(session1, state.clone(), &now)
            .unwrap();
        manager.activate_session(session2, state, &now).unwrap();

        // Try to exceed limit
        let result = manager.create_session(vec![test_device_id(1)], &now);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AuraError::Internal { .. }));
    }

    #[test]
    fn test_session_timeout() {
        let config = SessionConfig {
            timeout: Duration::from_millis(100),
            ..SessionConfig::default()
        };
        let now = test_time(1000000); // Unix timestamp in ms
        let mut manager = SessionManager::<TestProtocolState>::new(config, now.clone());

        let session_id = manager
            .create_session(vec![test_device_id(1)], &now)
            .unwrap();

        // Advance time past timeout (100ms timeout, we advance 200ms)
        let future_time = test_time(1000200);

        // Try to activate - should fail due to timeout
        let state = TestProtocolState {
            phase: "test".to_string(),
            data: vec![],
        };
        let result = manager.activate_session(session_id, state, &future_time);
        assert!(result.is_err());
        // Timeout errors now map to Internal
        assert!(matches!(result.unwrap_err(), AuraError::Internal { .. }));
    }

    #[test]
    fn test_cleanup_stale_sessions() {
        let config = SessionConfig {
            cleanup_interval: Duration::from_millis(50),
            ..SessionConfig::default()
        };
        let now = test_time(1000000); // Unix timestamp in ms
        let mut manager = SessionManager::<TestProtocolState>::new(config, now.clone());

        // Create and complete a session
        let session_id = manager
            .create_session(vec![test_device_id(1)], &now)
            .unwrap();
        let state = TestProtocolState {
            phase: "test".to_string(),
            data: vec![],
        };
        manager.activate_session(session_id, state, &now).unwrap();
        manager
            .complete_session(session_id, 0, 0, HashMap::new(), &test_time(1000050))
            .unwrap();

        assert_eq!(manager.sessions.len(), 1);

        // Advance time past cleanup interval (50ms interval, we advance 200ms total)
        let cleanup_time = test_time(1000200);

        // Cleanup should remove completed sessions
        let removed = manager.cleanup_stale_sessions(&cleanup_time).unwrap();
        assert!(removed > 0);
    }

    #[test]
    fn test_session_statistics() {
        let now = test_time(1000000); // Unix timestamp in ms
        let mut manager =
            SessionManager::<TestProtocolState>::new(SessionConfig::default(), now.clone());

        // Create and complete some sessions
        for i in 0..3 {
            let session_id = manager
                .create_session(vec![test_device_id(1)], &now)
                .unwrap();
            let state = TestProtocolState {
                phase: "test".to_string(),
                data: vec![],
            };
            manager.activate_session(session_id, state, &now).unwrap();

            if i < 2 {
                manager
                    .complete_session(
                        session_id,
                        10 * (i + 1),
                        100 * (i + 1),
                        HashMap::new(),
                        &test_time(1000000 + 100 * (i as u64 + 1)),
                    )
                    .unwrap();
            } else {
                let error = SessionError::ProtocolViolation {
                    constraint: "test".to_string(),
                };
                manager
                    .fail_session(session_id, error, None, &test_time(1000050))
                    .unwrap();
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
