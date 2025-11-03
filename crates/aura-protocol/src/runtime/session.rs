//! Session management for protocol coordination
//!
//! Manages protocol session lifecycle, participant coordination, and session state.

use aura_types::DeviceId;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use uuid::Uuid;

/// Session manager for protocol coordination
pub struct SessionManager {
    /// Active sessions
    sessions: HashMap<Uuid, SessionState>,
    /// Configuration for session management
    config: SessionConfig,
}

/// State of a protocol session
#[derive(Debug, Clone)]
pub struct SessionState {
    /// Session ID
    pub session_id: Uuid,
    /// Protocol type being executed
    pub protocol_type: String,
    /// Session participants
    pub participants: Vec<DeviceId>,
    /// Current session status
    pub status: SessionStatus,
    /// Session creation time
    pub created_at: Instant,
    /// Last activity time
    pub last_activity: Instant,
    /// Session timeout
    pub timeout: Duration,
    /// Connected participants
    pub connected_participants: HashSet<DeviceId>,
    /// Session metadata
    pub metadata: HashMap<String, String>,
}

/// Session status enumeration
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionStatus {
    /// Session is being initialized
    Initializing,
    /// Session is active and running
    Active,
    /// Session is waiting for participants
    WaitingForParticipants,
    /// Session completed successfully
    Completed,
    /// Session failed
    Failed(String),
    /// Session timed out
    TimedOut,
    /// Session was cancelled
    Cancelled,
}

/// Configuration for session management
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Default session timeout
    pub default_timeout: Duration,
    /// Maximum number of concurrent sessions
    pub max_concurrent_sessions: usize,
    /// Cleanup interval for expired sessions
    pub cleanup_interval: Duration,
    /// Grace period before forcing session cleanup
    pub cleanup_grace_period: Duration,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            default_timeout: Duration::from_secs(300), // 5 minutes
            max_concurrent_sessions: 100,
            cleanup_interval: Duration::from_secs(60), // 1 minute
            cleanup_grace_period: Duration::from_secs(30),
        }
    }
}

impl SessionManager {
    /// Create a new session manager
    pub fn new(config: SessionConfig) -> Self {
        Self {
            sessions: HashMap::new(),
            config,
        }
    }

    /// Create a new session
    pub fn create_session(
        &mut self,
        protocol_type: String,
        participants: Vec<DeviceId>,
        timeout: Option<Duration>,
    ) -> Result<Uuid, SessionError> {
        // Check if we're at the session limit
        if self.sessions.len() >= self.config.max_concurrent_sessions {
            return Err(SessionError::TooManySessions);
        }

        let session_id = Uuid::new_v4();
        let now = Instant::now();
        let timeout = timeout.unwrap_or(self.config.default_timeout);

        let session = SessionState {
            session_id,
            protocol_type,
            participants: participants.clone(),
            status: SessionStatus::Initializing,
            created_at: now,
            last_activity: now,
            timeout,
            connected_participants: HashSet::new(),
            metadata: HashMap::new(),
        };

        self.sessions.insert(session_id, session);
        Ok(session_id)
    }

    /// Get session state
    pub fn get_session(&self, session_id: Uuid) -> Option<&SessionState> {
        self.sessions.get(&session_id)
    }

    /// Update session status
    pub fn update_session_status(
        &mut self,
        session_id: Uuid,
        status: SessionStatus,
    ) -> Result<(), SessionError> {
        if let Some(session) = self.sessions.get_mut(&session_id) {
            session.status = status;
            session.last_activity = Instant::now();
            Ok(())
        } else {
            Err(SessionError::SessionNotFound)
        }
    }

    /// Mark participant as connected
    pub fn participant_connected(
        &mut self,
        session_id: Uuid,
        participant: DeviceId,
    ) -> Result<(), SessionError> {
        if let Some(session) = self.sessions.get_mut(&session_id) {
            if session.participants.contains(&participant) {
                session.connected_participants.insert(participant);
                session.last_activity = Instant::now();
                
                // Check if all participants are connected
                if session.connected_participants.len() == session.participants.len() {
                    session.status = SessionStatus::Active;
                }
                
                Ok(())
            } else {
                Err(SessionError::ParticipantNotInSession)
            }
        } else {
            Err(SessionError::SessionNotFound)
        }
    }

    /// Mark participant as disconnected
    pub fn participant_disconnected(
        &mut self,
        session_id: Uuid,
        participant: DeviceId,
    ) -> Result<(), SessionError> {
        if let Some(session) = self.sessions.get_mut(&session_id) {
            session.connected_participants.remove(&participant);
            session.last_activity = Instant::now();
            
            // Update status if not enough participants
            if session.connected_participants.len() < session.participants.len() {
                session.status = SessionStatus::WaitingForParticipants;
            }
            
            Ok(())
        } else {
            Err(SessionError::SessionNotFound)
        }
    }

    /// Complete a session
    pub fn complete_session(&mut self, session_id: Uuid) -> Result<(), SessionError> {
        self.update_session_status(session_id, SessionStatus::Completed)
    }

    /// Fail a session
    pub fn fail_session(&mut self, session_id: Uuid, reason: String) -> Result<(), SessionError> {
        self.update_session_status(session_id, SessionStatus::Failed(reason))
    }

    /// Cancel a session
    pub fn cancel_session(&mut self, session_id: Uuid) -> Result<(), SessionError> {
        self.update_session_status(session_id, SessionStatus::Cancelled)
    }

    /// Remove a session
    pub fn remove_session(&mut self, session_id: Uuid) -> Option<SessionState> {
        self.sessions.remove(&session_id)
    }

    /// Get all active sessions
    pub fn active_sessions(&self) -> impl Iterator<Item = &SessionState> {
        self.sessions.values().filter(|s| {
            matches!(s.status, SessionStatus::Active | SessionStatus::WaitingForParticipants)
        })
    }

    /// Cleanup expired sessions
    pub fn cleanup_expired_sessions(&mut self) -> Vec<Uuid> {
        let now = Instant::now();
        let mut expired_sessions = Vec::new();

        // Find expired sessions
        for (session_id, session) in &mut self.sessions {
            let time_since_activity = now.duration_since(session.last_activity);
            
            if time_since_activity > session.timeout {
                session.status = SessionStatus::TimedOut;
                expired_sessions.push(*session_id);
            }
        }

        // Remove expired sessions after grace period
        let mut to_remove = Vec::new();
        for session_id in &expired_sessions {
            if let Some(session) = self.sessions.get(session_id) {
                let time_since_timeout = now.duration_since(session.last_activity);
                if time_since_timeout > session.timeout + self.config.cleanup_grace_period {
                    to_remove.push(*session_id);
                }
            }
        }

        for session_id in &to_remove {
            self.sessions.remove(session_id);
        }

        expired_sessions
    }

    /// Get session statistics
    pub fn session_stats(&self) -> SessionStats {
        let total_sessions = self.sessions.len();
        let mut active_sessions = 0;
        let mut completed_sessions = 0;
        let mut failed_sessions = 0;

        for session in self.sessions.values() {
            match session.status {
                SessionStatus::Active | SessionStatus::WaitingForParticipants => {
                    active_sessions += 1;
                }
                SessionStatus::Completed => completed_sessions += 1,
                SessionStatus::Failed(_) | SessionStatus::TimedOut => failed_sessions += 1,
                _ => {}
            }
        }

        SessionStats {
            total_sessions,
            active_sessions,
            completed_sessions,
            failed_sessions,
        }
    }
}

/// Session management errors
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("Session not found")]
    SessionNotFound,
    
    #[error("Too many concurrent sessions")]
    TooManySessions,
    
    #[error("Participant not in session")]
    ParticipantNotInSession,
    
    #[error("Session already completed")]
    SessionAlreadyCompleted,
    
    #[error("Invalid session state transition")]
    InvalidStateTransition,
}

/// Session statistics
#[derive(Debug, Clone)]
pub struct SessionStats {
    pub total_sessions: usize,
    pub active_sessions: usize,
    pub completed_sessions: usize,
    pub failed_sessions: usize,
}