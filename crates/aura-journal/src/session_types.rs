//! Journal session types and event tracking.
//!
//! This module provides simplified session management for journal operations,
//! focusing on event-based state tracking rather than full protocol lifecycle.

use aura_types::{AccountId, SessionId};
// Import protocol types from aura-types (moved from aura-protocol-types)
use aura_types::{FrostParticipantId, ProtocolSessionStatus, ThresholdConfig};
use serde::{Deserialize, Serialize};

/// Journal session errors
#[derive(Debug, thiserror::Error)]
pub enum JournalSessionError {
    #[error("Session not found: {0}")]
    SessionNotFound(SessionId),

    #[error("Invalid session state: {0}")]
    InvalidState(String),

    #[error("Session expired: {0}")]
    SessionExpired(SessionId),
}

/// Simplified session state tracking for journal operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalSession {
    /// Session identifier
    pub session_id: SessionId,

    /// Current session status
    pub status: ProtocolSessionStatus,

    /// Session participants
    pub participants: Vec<FrostParticipantId>,

    /// Account this session belongs to
    pub account_id: AccountId,

    /// Session creation timestamp
    pub created_at: u64,

    /// Optional session expiration
    pub expires_at: Option<u64>,

    /// Threshold configuration if applicable
    pub threshold_config: Option<ThresholdConfig>,
}

impl JournalSession {
    /// Create a new journal session
    pub fn new(
        session_id: SessionId,
        account_id: AccountId,
        participants: Vec<FrostParticipantId>,
        threshold_config: Option<ThresholdConfig>,
    ) -> Self {
        Self {
            session_id,
            status: ProtocolSessionStatus::Initializing,
            participants,
            account_id,
            created_at: 0, // Should be set by effects
            expires_at: None,
            threshold_config,
        }
    }

    /// Update session status
    pub fn update_status(&mut self, status: ProtocolSessionStatus) {
        self.status = status;
    }

    /// Check if session is expired
    pub fn is_expired(&self, current_time: u64) -> bool {
        self.expires_at.is_some_and(|exp| current_time > exp)
    }

    /// Check if session is terminal
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            ProtocolSessionStatus::Completed
                | ProtocolSessionStatus::Failed(_)
                | ProtocolSessionStatus::Terminated
        )
    }
}

/// Simple session manager for journal operations
#[derive(Debug, Clone)]
pub struct JournalSessionManager {
    sessions: std::collections::HashMap<SessionId, JournalSession>,
}

impl JournalSessionManager {
    /// Create new session manager
    pub fn new() -> Self {
        Self {
            sessions: std::collections::HashMap::new(),
        }
    }

    /// Add a new session
    pub fn add_session(&mut self, session: JournalSession) {
        self.sessions.insert(session.session_id, session);
    }

    /// Get session by ID
    pub fn get_session(&self, session_id: &SessionId) -> Option<&JournalSession> {
        self.sessions.get(session_id)
    }

    /// Update session status
    pub fn update_session_status(
        &mut self,
        session_id: &SessionId,
        status: ProtocolSessionStatus,
    ) -> Result<(), JournalSessionError> {
        let session = self
            .sessions
            .get_mut(session_id)
            .ok_or(JournalSessionError::SessionNotFound(*session_id))?;
        session.update_status(status);
        Ok(())
    }

    /// Remove expired sessions
    pub fn cleanup_expired(&mut self, current_time: u64) -> Vec<SessionId> {
        let expired: Vec<SessionId> = self
            .sessions
            .iter()
            .filter(|(_, session)| session.is_expired(current_time))
            .map(|(id, _)| *id)
            .collect();

        for id in &expired {
            self.sessions.remove(id);
        }

        expired
    }
}

/// Default implementation for session manager
impl Default for JournalSessionManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper functions for session management
impl JournalSessionManager {
    /// Get all active sessions
    pub fn active_sessions(&self) -> Vec<&JournalSession> {
        self.sessions
            .values()
            .filter(|session| !session.is_terminal())
            .collect()
    }

    /// Get sessions by status
    pub fn sessions_with_status(&self, status: ProtocolSessionStatus) -> Vec<&JournalSession> {
        self.sessions
            .values()
            .filter(|session| session.status == status)
            .collect()
    }

    /// Count sessions by account
    pub fn session_count_for_account(&self, account_id: AccountId) -> usize {
        self.sessions
            .values()
            .filter(|session| session.account_id == account_id)
            .count()
    }
}
