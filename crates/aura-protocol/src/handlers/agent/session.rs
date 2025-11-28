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
    effects::agent::{ChoreographicMessage, ChoreographicRole, ChoreographyConfig},
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
pub struct MemorySessionHandler<T: aura_core::PhysicalTimeEffects> {
    device_id: DeviceId,
    sessions: Arc<RwLock<HashMap<SessionId, SessionData>>>,
    session_messages: Arc<RwLock<HashMap<SessionId, Vec<SessionMessage>>>>,
    choreographic_messages: Arc<RwLock<HashMap<SessionId, Vec<ChoreographicMessage>>>>,
    choreography_state: Arc<RwLock<HashMap<SessionId, ChoreographySessionState>>>,
    choreography_configs: Arc<RwLock<HashMap<SessionId, ChoreographyConfig>>>,
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

impl<T: aura_core::PhysicalTimeEffects> MemorySessionHandler<T> {
    /// Create a new choreography-aware memory session handler
    pub fn new(device_id: DeviceId, time_effects: Arc<T>) -> Self {
        Self {
            device_id,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            session_messages: Arc::new(RwLock::new(HashMap::new())),
            choreographic_messages: Arc::new(RwLock::new(HashMap::new())),
            choreography_state: Arc::new(RwLock::new(HashMap::new())),
            choreography_configs: Arc::new(RwLock::new(HashMap::new())),
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
        if let Ok(mut choreo_messages) = self.choreographic_messages.write() {
            choreo_messages.clear();
        }
        if let Ok(mut choreo_state) = self.choreography_state.write() {
            choreo_state.clear();
        }
        if let Ok(mut choreo_configs) = self.choreography_configs.write() {
            choreo_configs.clear();
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
        self.time_effects.physical_time().await.map(|t| t.ts_ms).unwrap_or(0)
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
impl<T: aura_core::PhysicalTimeEffects> SessionManagementEffects for MemorySessionHandler<T> {
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

    async fn create_choreographic_session(
        &self,
        session_type: SessionType,
        participants: Vec<DeviceId>,
        choreography_config: ChoreographyConfig,
    ) -> Result<SessionId> {
        let session_id = self.create_session(session_type).await?;

        // Store choreography config
        let mut configs = self
            .choreography_configs
            .write()
            .map_err(|_| aura_core::AuraError::internal("Failed to acquire config write lock"))?;
        configs.insert(session_id, choreography_config);

        // Update choreography state with participants
        let mut choreo_states = self
            .choreography_state
            .write()
            .map_err(|_| aura_core::AuraError::internal("Failed to acquire choreography write lock"))?;

        if let Some(state) = choreo_states.get_mut(&session_id) {
            state.invited_participants = participants;
            state.choreography_phase = SessionChoreographyPhase::ParticipantResponse;
        }

        // Initialize empty choreographic message list
        let mut choreo_messages = self
            .choreographic_messages
            .write()
            .map_err(|_| aura_core::AuraError::internal("Failed to acquire choreographic messages write lock"))?;
        choreo_messages.insert(session_id, Vec::new());

        Ok(session_id)
    }

    async fn join_choreographic_session(
        &self,
        session_id: SessionId,
        role: ChoreographicRole,
    ) -> Result<SessionHandle> {
        let handle = self.join_session(session_id).await?;

        // Update choreography state with the role
        let mut choreo_states = self
            .choreography_state
            .write()
            .map_err(|_| aura_core::AuraError::internal("Failed to acquire choreography write lock"))?;

        if let Some(state) = choreo_states.get_mut(&session_id) {
            let device_id_uuid: uuid::Uuid = self.device_id.into();
            if role.device_id == device_id_uuid {
                if state.invited_participants.contains(&self.device_id) {
                    state.active_participants.push(self.device_id);
                    state.invited_participants.retain(|&id| id != self.device_id);

                    // If all participants responded, move to active phase
                    if state.invited_participants.is_empty() {
                        state.choreography_phase = SessionChoreographyPhase::Active;
                    }
                }
            }
        }

        Ok(handle)
    }

    async fn send_choreographic_message(
        &self,
        session_id: SessionId,
        message_type: &str,
        payload: &[u8],
        target_role: Option<ChoreographicRole>,
    ) -> Result<()> {
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

        // Get choreography config for this session
        let configs = self
            .choreography_configs
            .read()
            .map_err(|_| aura_core::AuraError::internal("Failed to acquire config read lock"))?;

        let config = configs
            .get(&session_id)
            .ok_or_else(|| aura_core::AuraError::not_found("Choreography config not found"))?;

        // Get current phase
        let choreo_states = self
            .choreography_state
            .read()
            .map_err(|_| aura_core::AuraError::internal("Failed to acquire choreography read lock"))?;

        let state = choreo_states
            .get(&session_id)
            .ok_or_else(|| aura_core::AuraError::not_found("Choreography state not found"))?;

        let phase = match &state.choreography_phase {
            SessionChoreographyPhase::Invitation => "invitation",
            SessionChoreographyPhase::ParticipantResponse => "participant_response",
            SessionChoreographyPhase::Active => "active",
            SessionChoreographyPhase::Ending => "ending",
            SessionChoreographyPhase::Ended => "ended",
        }
        .to_string();

        // Create choreographic message
        let device_id_uuid: uuid::Uuid = self.device_id.into();
        let choreographic_message = ChoreographicMessage {
            from: self.device_id,
            to: target_role.as_ref().map(|r| DeviceId::from(r.device_id)),
            source_role: ChoreographicRole::new(device_id_uuid, 0),
            target_role,
            protocol_namespace: config.namespace.clone(),
            phase,
            message_type: message_type.to_string(),
            payload: payload.to_vec(),
            timestamp: self.current_timestamp().await,
            sequence_number: 0, // Would need proper sequencing in production
            guard_capabilities: config.guard_capabilities.clone(),
        };

        // Store the message
        let mut messages = self
            .choreographic_messages
            .write()
            .map_err(|_| aura_core::AuraError::internal("Failed to acquire choreographic messages write lock"))?;

        messages
            .entry(session_id)
            .or_insert_with(Vec::new)
            .push(choreographic_message);

        Ok(())
    }

    async fn receive_choreographic_messages(
        &self,
        session_id: SessionId,
        role_filter: Option<ChoreographicRole>,
    ) -> Result<Vec<ChoreographicMessage>> {
        let messages = self
            .choreographic_messages
            .read()
            .map_err(|_| aura_core::AuraError::internal("Failed to acquire choreographic messages read lock"))?;

        let session_messages = messages.get(&session_id).cloned().unwrap_or_default();

        // Filter by role if specified
        if let Some(role) = role_filter {
            Ok(session_messages
                .into_iter()
                .filter(|msg| {
                    msg.target_role
                        .as_ref()
                        .map(|r| r.device_id == role.device_id && r.role_index == role.role_index)
                        .unwrap_or(false)
                })
                .collect())
        } else {
            Ok(session_messages)
        }
    }

    async fn get_choreography_phase(&self, session_id: SessionId) -> Result<Option<String>> {
        let choreo_states = self
            .choreography_state
            .read()
            .map_err(|_| aura_core::AuraError::internal("Failed to acquire choreography read lock"))?;

        let phase_string = choreo_states.get(&session_id).map(|state| {
            match &state.choreography_phase {
                SessionChoreographyPhase::Invitation => "invitation",
                SessionChoreographyPhase::ParticipantResponse => "participant_response",
                SessionChoreographyPhase::Active => "active",
                SessionChoreographyPhase::Ending => "ending",
                SessionChoreographyPhase::Ended => "ended",
            }
            .to_string()
        });

        Ok(phase_string)
    }

    async fn update_choreography_state(
        &self,
        session_id: SessionId,
        phase: &str,
        _state_data: &[u8],
    ) -> Result<()> {
        let mut choreo_states = self
            .choreography_state
            .write()
            .map_err(|_| aura_core::AuraError::internal("Failed to acquire choreography write lock"))?;

        let state = choreo_states
            .get_mut(&session_id)
            .ok_or_else(|| aura_core::AuraError::not_found("Choreography state not found"))?;

        // Update phase based on string
        state.choreography_phase = match phase {
            "invitation" => SessionChoreographyPhase::Invitation,
            "participant_response" => SessionChoreographyPhase::ParticipantResponse,
            "active" => SessionChoreographyPhase::Active,
            "ending" => SessionChoreographyPhase::Ending,
            "ended" => SessionChoreographyPhase::Ended,
            _ => {
                return Err(aura_core::AuraError::invalid(format!(
                    "Unknown choreography phase: {}",
                    phase
                )))
            }
        };

        Ok(())
    }

    async fn validate_choreographic_message(
        &self,
        session_id: SessionId,
        message: &ChoreographicMessage,
    ) -> Result<bool> {
        // Get choreography config
        let configs = self
            .choreography_configs
            .read()
            .map_err(|_| aura_core::AuraError::internal("Failed to acquire config read lock"))?;

        let config = configs
            .get(&session_id)
            .ok_or_else(|| aura_core::AuraError::not_found("Choreography config not found"))?;

        // Get current phase
        let current_phase = self.get_choreography_phase(session_id).await?;

        // Validate namespace
        if message.protocol_namespace != config.namespace {
            return Ok(false);
        }

        // Validate phase matches
        if Some(message.phase.clone()) != current_phase {
            return Ok(false);
        }

        // Validate guard capabilities are satisfied
        for required_cap in &config.guard_capabilities {
            if !message.guard_capabilities.contains(required_cap) {
                return Ok(false);
            }
        }

        Ok(true)
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
    impl aura_core::PhysicalTimeEffects for TestTimeEffects {
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

        // Create choreographic session with config
        let choreography_config = ChoreographyConfig {
            namespace: "test_messaging".to_string(),
            guard_capabilities: vec!["send_message".to_string()],
            flow_budget: Some(1000),
            journal_facts: vec!["message_sent".to_string()],
            timeout_seconds: 60,
            max_retries: 3,
        };

        let session_id = handler
            .create_choreographic_session(
                SessionType::Coordination,
                vec![],
                choreography_config,
            )
            .await
            .unwrap();

        let message_data = b"Hello, session!";

        // Test send choreographic message
        handler
            .send_choreographic_message(
                session_id,
                "test_message",
                message_data,
                None, // broadcast to all
            )
            .await
            .unwrap();

        // Test receive choreographic messages
        let messages = handler
            .receive_choreographic_messages(session_id, None)
            .await
            .unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].payload, message_data);
        assert_eq!(messages[0].from, device_id);
        assert_eq!(messages[0].message_type, "test_message");
        assert_eq!(messages[0].protocol_namespace, "test_messaging");
    }
}
