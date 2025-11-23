//! Session management handlers for agent operations with choreographic patterns

use async_trait::async_trait;
use aura_macros::choreography;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::effects::{
    SessionHandle, SessionInfo, SessionManagementEffects, SessionMessage, SessionRole,
    SessionStatus, SessionType,
};
use aura_core::{
    identifiers::{DeviceId, SessionId},
    AuraResult as Result,
};

// Session lifecycle choreography
//
// This choreography implements session lifecycle management:
// 1. Manager creates session and distributes invitations
// 2. Participants join or decline the session
// 3. Manager tracks session state and coordinates activities
// 4. Sessions can be ended gracefully or forcefully
choreography! {
    #[namespace = "session_lifecycle"]
    protocol SessionLifecycleChoreography {
        roles: Manager, Participants[*];

        // Phase 1: Session Creation and Invitation
        Manager[guard_capability = "create_session",
               flow_cost = 100,
               journal_facts = "session_created"]
        -> Participants[*]: SessionInvitation(SessionInvitation);

        // Phase 2: Participant Response
        choice Participants[*] {
            join: {
                Participants[*][guard_capability = "join_session",
                              flow_cost = 75,
                              journal_facts = "session_joined"]
                -> Manager: SessionJoined(SessionJoined);
            }
            decline: {
                Participants[*][guard_capability = "decline_session",
                              flow_cost = 50,
                              journal_facts = "session_declined"]
                -> Manager: SessionDeclined(SessionDeclined);
            }
        }

        // Phase 3: Session Management (ongoing)
        loop {
            choice Manager {
                message: {
                    Manager[guard_capability = "broadcast_message",
                           flow_cost = 25,
                           journal_facts = "message_sent"]
                    -> Participants[*]: SessionMessage(SessionMessage);
                }
                status_update: {
                    Manager[guard_capability = "update_session_status",
                           flow_cost = 50,
                           journal_facts = "session_status_updated"]
                    -> Participants[*]: SessionStatusUpdate(SessionStatusUpdate);
                }
                end_session: {
                    Manager[guard_capability = "end_session",
                           flow_cost = 100,
                           journal_facts = "session_ended"]
                    -> Participants[*]: SessionEnded(SessionEnded);
                    break;
                }
            }
        }
    }
}

// Message types for session lifecycle choreography

/// Session invitation message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInvitation {
    pub session_id: SessionId,
    pub session_type: SessionType,
    pub manager_id: DeviceId,
    pub invited_participants: Vec<DeviceId>,
    pub created_at: u64,
}

/// Session joined confirmation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionJoined {
    pub session_id: SessionId,
    pub participant_id: DeviceId,
    pub joined_at: u64,
}

/// Session declined response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDeclined {
    pub session_id: SessionId,
    pub participant_id: DeviceId,
    pub reason: Option<String>,
    pub declined_at: u64,
}

/// Session status update message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStatusUpdate {
    pub session_id: SessionId,
    pub new_status: SessionStatus,
    pub updated_at: u64,
    pub message: Option<String>,
}

/// Session ended notification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEnded {
    pub session_id: SessionId,
    pub ended_at: u64,
    pub reason: String,
}

/// Choreography-aware session handler for testing and production
#[derive(Debug, Clone)]
pub struct MemorySessionHandler<T: aura_core::TimeEffects> {
    device_id: DeviceId,
    sessions: Arc<RwLock<HashMap<SessionId, SessionData>>>,
    session_messages: Arc<RwLock<HashMap<SessionId, Vec<SessionMessage>>>>,
    choreography_state: Arc<RwLock<HashMap<SessionId, ChoreographySessionState>>>,
    time_effects: Arc<T>,
}

/// Extended session data with choreography support
#[derive(Debug, Clone)]
struct ChoreographySessionState {
    manager_id: DeviceId,
    invited_participants: Vec<DeviceId>,
    active_participants: Vec<DeviceId>,
    declined_participants: Vec<DeviceId>,
    choreography_phase: SessionChoreographyPhase,
}

/// Current phase in the session choreography
#[derive(Debug, Clone, PartialEq)]
enum SessionChoreographyPhase {
    Invitation,
    ParticipantResponse,
    Active,
    Ending,
    Ended,
}

/// Internal session data structure
#[derive(Debug, Clone)]
struct SessionData {
    session_id: SessionId,
    session_type: SessionType,
    role: SessionRole,
    participants: Vec<DeviceId>,
    status: SessionStatus,
    created_at: u64,
    updated_at: u64,
}

impl<T: aura_core::TimeEffects> MemorySessionHandler<T> {
    /// Create a new choreography-aware memory session handler
    pub fn new(device_id: DeviceId, time_effects: Arc<T>) -> Self {
        Self {
            device_id,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            session_messages: Arc::new(RwLock::new(HashMap::new())),
            choreography_state: Arc::new(RwLock::new(HashMap::new())),
            time_effects,
        }
    }

    /// Get the device ID this handler is configured for
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Clear all sessions and choreography state (useful for testing)
    pub fn clear(&self) {
        if let Ok(mut sessions) = self.sessions.write() {
            sessions.clear();
        }
        if let Ok(mut messages) = self.session_messages.write() {
            messages.clear();
        }
        if let Ok(mut choreo_state) = self.choreography_state.write() {
            choreo_state.clear();
        }
    }

    /// Get session count
    pub fn session_count(&self) -> usize {
        self.sessions
            .read()
            .map(|sessions| sessions.len())
            .unwrap_or(0)
    }

    /// Get current timestamp using TimeEffects
    async fn current_timestamp(&self) -> u64 {
        self.time_effects.current_timestamp().await
    }

    /// Create session with choreographic participants
    pub async fn create_choreographic_session(
        &self,
        session_type: SessionType,
        participants: Vec<DeviceId>,
    ) -> Result<SessionId> {
        let session_id = self.create_session(session_type).await?;
        
        // Update choreography state with participants
        if let Ok(mut choreo_states) = self.choreography_state.write() {
            if let Some(state) = choreo_states.get_mut(&session_id) {
                state.invited_participants = participants;
                state.choreography_phase = SessionChoreographyPhase::ParticipantResponse;
            }
        }
        
        Ok(session_id)
    }

    /// Simulate participant joining choreographically
    pub async fn choreographic_join(
        &self,
        session_id: SessionId,
        participant_id: DeviceId,
    ) -> Result<()> {
        let timestamp = self.current_timestamp().await;
        
        // Update choreography state
        if let Ok(mut choreo_states) = self.choreography_state.write() {
            if let Some(state) = choreo_states.get_mut(&session_id) {
                if state.invited_participants.contains(&participant_id) {
                    state.active_participants.push(participant_id);
                    state.invited_participants.retain(|&id| id != participant_id);
                    
                    // If all participants responded, move to active phase
                    if state.invited_participants.is_empty() {
                        state.choreography_phase = SessionChoreographyPhase::Active;
                    }
                }
            }
        }
        
        // Update session data
        if let Ok(mut sessions) = self.sessions.write() {
            if let Some(session_data) = sessions.get_mut(&session_id) {
                session_data.participants.push(participant_id);
                session_data.updated_at = timestamp;
                session_data.status = SessionStatus::Active;
            }
        }
        
        Ok(())
    }

    /// Get choreography state for a session
    pub fn get_choreography_phase(&self, session_id: &SessionId) -> Option<SessionChoreographyPhase> {
        self.choreography_state
            .read()
            .ok()
            .and_then(|states| states.get(session_id).map(|s| s.choreography_phase.clone()))
    }
}

#[async_trait]
impl<T: aura_core::TimeEffects> SessionManagementEffects for MemorySessionHandler<T> {
    async fn create_session(&self, session_type: SessionType) -> Result<SessionId> {
        let session_id = SessionId::new();
        let timestamp = self.current_timestamp().await;

        let session_data = SessionData {
            session_id,
            session_type,
            role: SessionRole::Coordinator,
            participants: vec![self.device_id],
            status: SessionStatus::Created,
            created_at: timestamp,
            updated_at: timestamp,
        };

        // Initialize choreography state
        let choreography_state = ChoreographySessionState {
            manager_id: self.device_id,
            invited_participants: Vec::new(),
            active_participants: vec![self.device_id],
            declined_participants: Vec::new(),
            choreography_phase: SessionChoreographyPhase::Invitation,
        };

        let mut sessions = self
            .sessions
            .write()
            .map_err(|_| aura_core::AuraError::internal("Failed to acquire write lock"))?;

        let mut choreo_states = self
            .choreography_state
            .write()
            .map_err(|_| aura_core::AuraError::internal("Failed to acquire choreography write lock"))?;

        sessions.insert(session_id, session_data);
        choreo_states.insert(session_id, choreography_state);

        // Initialize empty message list for this session
        let mut messages = self
            .session_messages
            .write()
            .map_err(|_| aura_core::AuraError::internal("Failed to acquire messages write lock"))?;
        messages.insert(session_id, Vec::new());

        Ok(session_id)
    }

    async fn join_session(&self, session_id: SessionId) -> Result<SessionHandle> {
        let mut sessions = self
            .sessions
            .write()
            .map_err(|_| aura_core::AuraError::internal("Failed to acquire write lock"))?;

        let session = sessions
            .get_mut(&session_id)
            .ok_or_else(|| aura_core::AuraError::not_found("Session not found"))?;

        // Add this device as participant
        if !session.participants.contains(&self.device_id) {
            session.participants.push(self.device_id);
        }

        session.status = SessionStatus::Active;
        session.updated_at = self.current_timestamp().await;

        Ok(SessionHandle {
            session_id,
            role: SessionRole::Participant,
            participants: session.participants.clone(),
            created_at: session.created_at,
        })
    }

    async fn leave_session(&self, session_id: SessionId) -> Result<()> {
        let mut sessions = self
            .sessions
            .write()
            .map_err(|_| aura_core::AuraError::internal("Failed to acquire write lock"))?;

        let session = sessions
            .get_mut(&session_id)
            .ok_or_else(|| aura_core::AuraError::not_found("Session not found"))?;

        // Remove this device from participants
        session.participants.retain(|&id| id != self.device_id);
        session.updated_at = self.current_timestamp().await;

        // If no participants left, mark as completed
        if session.participants.is_empty() {
            session.status = SessionStatus::Completed;
        }

        Ok(())
    }

    async fn end_session(&self, session_id: SessionId) -> Result<()> {
        let mut sessions = self
            .sessions
            .write()
            .map_err(|_| aura_core::AuraError::internal("Failed to acquire write lock"))?;

        let session = sessions
            .get_mut(&session_id)
            .ok_or_else(|| aura_core::AuraError::not_found("Session not found"))?;

        session.status = SessionStatus::Completed;
        session.updated_at = self.current_timestamp().await;

        Ok(())
    }

    async fn list_active_sessions(&self) -> Result<Vec<SessionInfo>> {
        let sessions = self
            .sessions
            .read()
            .map_err(|_| aura_core::AuraError::internal("Failed to acquire read lock"))?;

        let active_sessions = sessions
            .values()
            .filter(|session| {
                matches!(
                    session.status,
                    SessionStatus::Created | SessionStatus::Active
                )
            })
            .map(|session| SessionInfo {
                session_id: session.session_id,
                session_type: session.session_type.clone(),
                role: session.role.clone(),
                participants: session.participants.clone(),
                status: session.status.clone(),
                created_at: session.created_at,
                updated_at: session.updated_at,
                timeout_at: None,
                operation: None,
                metadata: std::collections::HashMap::new(),
            })
            .collect();

        Ok(active_sessions)
    }

    async fn get_session_status(&self, session_id: SessionId) -> Result<SessionStatus> {
        let sessions = self
            .sessions
            .read()
            .map_err(|_| aura_core::AuraError::internal("Failed to acquire read lock"))?;

        let session = sessions
            .get(&session_id)
            .ok_or_else(|| aura_core::AuraError::not_found("Session not found"))?;

        Ok(session.status.clone())
    }

    async fn send_session_message(&self, session_id: SessionId, message: &[u8]) -> Result<()> {
        // Check if session exists
        {
            let sessions = self
                .sessions
                .read()
                .map_err(|_| aura_core::AuraError::internal("Failed to acquire read lock"))?;

            if !sessions.contains_key(&session_id) {
                return Err(aura_core::AuraError::not_found("Session not found"));
            }
        }

        let session_message = SessionMessage {
            from: self.device_id,
            to: None, // Broadcast to all participants
            timestamp: self.current_timestamp().await,
            message_type: "application/octet-stream".to_string(),
            payload: message.to_vec(),
        };

        let mut messages = self
            .session_messages
            .write()
            .map_err(|_| aura_core::AuraError::internal("Failed to acquire messages write lock"))?;

        messages
            .entry(session_id)
            .or_insert_with(Vec::new)
            .push(session_message);

        Ok(())
    }

    async fn receive_session_messages(&self, session_id: SessionId) -> Result<Vec<SessionMessage>> {
        let messages = self
            .session_messages
            .read()
            .map_err(|_| aura_core::AuraError::internal("Failed to acquire messages read lock"))?;

        Ok(messages.get(&session_id).cloned().unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    // Simple test time effects implementation
    #[derive(Debug, Clone)]
    struct TestTimeEffects;
    
    #[async_trait::async_trait]
    impl aura_core::TimeEffects for TestTimeEffects {
        async fn current_timestamp(&self) -> u64 {
            1234567890 // Fixed timestamp for testing
        }
        
        async fn current_timestamp_millis(&self) -> u64 {
            1234567890000 // Fixed timestamp in millis
        }
        
        async fn sleep(&self, _duration: std::time::Duration) {
            // No-op for tests
        }
    }

    #[tokio::test]
    async fn test_memory_session_operations() {
        let device_id = DeviceId::new();
        let time_effects = Arc::new(TestTimeEffects);
        let handler = MemorySessionHandler::new(device_id, time_effects);

        // Test create session
        let session_id = handler
            .create_session(SessionType::ThresholdOperation)
            .await
            .unwrap();
        assert_eq!(handler.session_count(), 1);

        // Test get session status
        let status = handler.get_session_status(session_id).await.unwrap();
        assert!(matches!(status, SessionStatus::Created));

        // Test list sessions
        let sessions = handler.list_active_sessions().await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, session_id);

        // Test end session
        handler.end_session(session_id).await.unwrap();
        let status = handler.get_session_status(session_id).await.unwrap();
        assert!(matches!(status, SessionStatus::Completed));
    }

    #[tokio::test]
    async fn test_session_join_and_leave() {
        let device_id = DeviceId::new();
        let time_effects = Arc::new(TestTimeEffects);
        let handler = MemorySessionHandler::new(device_id, time_effects);

        let session_id = handler.create_session(SessionType::Recovery).await.unwrap();

        // Test join session
        let handle = handler.join_session(session_id).await.unwrap();
        assert_eq!(handle.session_id, session_id);
        assert!(matches!(handle.role, SessionRole::Participant));
        assert_eq!(handle.participants.len(), 1);

        // Test leave session
        handler.leave_session(session_id).await.unwrap();

        // Session should now be completed since no participants left
        let status = handler.get_session_status(session_id).await.unwrap();
        assert!(matches!(status, SessionStatus::Completed));
    }

    #[tokio::test]
    async fn test_session_messaging() {
        let device_id = DeviceId::new();
        let time_effects = Arc::new(TestTimeEffects);
        let handler = MemorySessionHandler::new(device_id, time_effects);

        let session_id = handler
            .create_session(SessionType::Coordination)
            .await
            .unwrap();

        let message_data = b"Hello, session!";

        // Test send message
        handler
            .send_session_message(session_id, message_data)
            .await
            .unwrap();

        // Test receive messages
        let messages = handler.receive_session_messages(session_id).await.unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].payload, message_data);
        assert_eq!(messages[0].from, device_id);
    }
}
