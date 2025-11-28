//! Session management handlers for agent operations with choreographic patterns

use async_lock::RwLock;
use async_trait::async_trait;
use aura_macros::choreography;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::effects::{
    SessionHandle, SessionInfo, SessionManagementEffects, SessionMessage, SessionRole,
    SessionStatus, SessionType,
};
use aura_core::{
    effects::agent::{ChoreographicMessage, ChoreographicRole, ChoreographyConfig},
    identifiers::{DeviceId, SessionId},
    AuraResult as Result, PhysicalTimeEffects, TimeEffects,
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
#[derive(Clone)]
pub struct MemorySessionHandler {
    device_id: DeviceId,
    sessions: Arc<RwLock<HashMap<SessionId, SessionData>>>,
    session_messages: Arc<RwLock<HashMap<SessionId, Vec<SessionMessage>>>>,
    choreographic_messages: Arc<RwLock<HashMap<SessionId, Vec<ChoreographicMessage>>>>,
    choreography_state: Arc<RwLock<HashMap<SessionId, ChoreographySessionState>>>,
    choreography_configs: Arc<RwLock<HashMap<SessionId, ChoreographyConfig>>>,
    time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync>,
}

/// Extended session data with choreography support
#[derive(Debug, Clone)]
struct ChoreographySessionState {
    manager_id: DeviceId,
    invited_participants: Vec<DeviceId>,
    active_participants: Vec<DeviceId>,
    declined_participants: Vec<DeviceId>,
    choreography_phase: SessionChoreographyPhase,
    metadata: Vec<u8>,
}

/// Current phase in the session choreography
#[derive(Debug, Clone, PartialEq)]
pub enum SessionChoreographyPhase {
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

impl MemorySessionHandler {
    /// Create a new choreography-aware memory session handler
    pub fn new(device_id: DeviceId, time_effects: Arc<dyn PhysicalTimeEffects>) -> Self {
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
        if let Some(mut sessions) = self.sessions.try_write() {
            sessions.clear();
        }
        if let Some(mut messages) = self.session_messages.try_write() {
            messages.clear();
        }
        if let Some(mut choreo_messages) = self.choreographic_messages.try_write() {
            choreo_messages.clear();
        }
        if let Some(mut choreo_state) = self.choreography_state.try_write() {
            choreo_state.clear();
        }
        if let Some(mut choreo_configs) = self.choreography_configs.try_write() {
            choreo_configs.clear();
        }
    }

    /// Get session count
    pub fn session_count(&self) -> usize {
        self.sessions
            .try_read()
            .map(|sessions| sessions.len())
            .unwrap_or(0)
    }

    /// Get current timestamp using TimeEffects
    async fn current_timestamp(&self) -> u64 {
        self.time_effects.current_timestamp().await
    }

    /// Simulate participant joining choreographically
    pub async fn choreographic_join(
        &self,
        session_id: SessionId,
        participant_id: DeviceId,
    ) -> Result<()> {
        let timestamp = self.current_timestamp().await;

        // Update choreography state
        if let Some(mut choreo_states) = self.choreography_state.try_write() {
            if let Some(state) = choreo_states.get_mut(&session_id) {
                if state.invited_participants.contains(&participant_id) {
                    state.active_participants.push(participant_id);
                    state
                        .invited_participants
                        .retain(|&id| id != participant_id);

                    // If all participants responded, move to active phase
                    if state.invited_participants.is_empty() {
                        state.choreography_phase = SessionChoreographyPhase::Active;
                    }
                }
            }
        }

        // Update session data
        if let Some(mut sessions) = self.sessions.try_write() {
            if let Some(session_data) = sessions.get_mut(&session_id) {
                session_data.participants.push(participant_id);
                session_data.updated_at = timestamp;
                session_data.status = SessionStatus::Active;
            }
        }

        Ok(())
    }

    /// Get choreography state for a session
    pub fn get_choreography_phase(
        &self,
        session_id: &SessionId,
    ) -> Option<SessionChoreographyPhase> {
        self.choreography_state
            .try_read()
            .and_then(|states| states.get(session_id).map(|s| s.choreography_phase.clone()))
    }
}

#[async_trait]
impl SessionManagementEffects for MemorySessionHandler {
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
            metadata: Vec::new(),
        };

        let mut sessions = self.sessions.write().await;
        let mut choreo_states = self.choreography_state.write().await;
        sessions.insert(session_id, session_data);
        choreo_states.insert(session_id, choreography_state);

        // Initialize empty message list for this session
        let mut messages = self.session_messages.write().await;
        messages.insert(session_id, Vec::new());

        Ok(session_id)
    }

    async fn join_session(&self, session_id: SessionId) -> Result<SessionHandle> {
        let timestamp = self.current_timestamp().await;
        let mut sessions = self.sessions.write().await;

        let session = sessions
            .get_mut(&session_id)
            .ok_or_else(|| aura_core::AuraError::not_found("Session not found"))?;

        // Add this device as participant
        if !session.participants.contains(&self.device_id) {
            session.participants.push(self.device_id);
        }

        session.status = SessionStatus::Active;
        session.updated_at = timestamp;

        Ok(SessionHandle {
            session_id,
            role: SessionRole::Participant,
            participants: session.participants.clone(),
            created_at: session.created_at,
        })
    }

    async fn leave_session(&self, session_id: SessionId) -> Result<()> {
        let timestamp = self.current_timestamp().await;
        let mut sessions = self.sessions.write().await;

        let session = sessions
            .get_mut(&session_id)
            .ok_or_else(|| aura_core::AuraError::not_found("Session not found"))?;

        // Remove this device from participants
        session.participants.retain(|&id| id != self.device_id);
        session.updated_at = timestamp;

        // If no participants left, mark as completed
        if session.participants.is_empty() {
            session.status = SessionStatus::Completed;
        }

        Ok(())
    }

    async fn end_session(&self, session_id: SessionId) -> Result<()> {
        let timestamp = self.current_timestamp().await;
        let mut sessions = self.sessions.write().await;

        let session = sessions
            .get_mut(&session_id)
            .ok_or_else(|| aura_core::AuraError::not_found("Session not found"))?;

        session.status = SessionStatus::Completed;
        session.updated_at = timestamp;

        Ok(())
    }

    async fn list_active_sessions(&self) -> Result<Vec<SessionInfo>> {
        let sessions = self.sessions.read().await;

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
        let sessions = self.sessions.read().await;

        let session = sessions
            .get(&session_id)
            .ok_or_else(|| aura_core::AuraError::not_found("Session not found"))?;

        Ok(session.status.clone())
    }

    // Choreographic session methods
    async fn create_choreographic_session(
        &self,
        session_type: SessionType,
        participants: Vec<aura_core::DeviceId>,
        choreography_config: ChoreographyConfig,
    ) -> Result<SessionId> {
        let session_id = self.create_session(session_type).await?;

        // Store choreography config
        let mut configs = self.choreography_configs.write().await;
        configs.insert(session_id, choreography_config.clone());
        drop(configs);

        // Initialize empty choreographic message list
        let mut choreo_messages = self.choreographic_messages.write().await;
        choreo_messages.insert(session_id, Vec::new());
        drop(choreo_messages);

        // Record invited participants and initial choreography metadata
        let mut choreo_states = self.choreography_state.write().await;
        if let Some(state) = choreo_states.get_mut(&session_id) {
            state.invited_participants = participants.clone();
            state.choreography_phase = SessionChoreographyPhase::ParticipantResponse;
            state.metadata = bincode::serialize(&choreography_config).unwrap_or_default();
        }
        drop(choreo_states);

        // Add participants to session list
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(&session_id) {
            for p in participants {
                if !session.participants.contains(&p) {
                    session.participants.push(p);
                }
            }
        }

        Ok(session_id)
    }

    async fn join_choreographic_session(
        &self,
        session_id: SessionId,
        role: ChoreographicRole,
    ) -> Result<SessionHandle> {
        let handle = self.join_session(session_id).await?;

        // Update choreography state
        let mut states = self.choreography_state.write().await;
        if let Some(state) = states.get_mut(&session_id) {
            let device = aura_core::DeviceId::from_uuid(role.device_id);
            if !state.active_participants.contains(&device) {
                state.active_participants.push(device);
            }
            state
                .invited_participants
                .retain(|participant| *participant != device);
            if state.invited_participants.is_empty() {
                state.choreography_phase = SessionChoreographyPhase::Active;
            }
        }

        Ok(SessionHandle {
            role: handle.role,
            ..handle
        })
    }

    async fn send_choreographic_message(
        &self,
        session_id: SessionId,
        message_type: &str,
        payload: &[u8],
        target_role: Option<ChoreographicRole>,
    ) -> Result<()> {
        // Get choreography config and state
        let configs = self.choreography_configs.read().await;
        let config = configs
            .get(&session_id)
            .ok_or_else(|| aura_core::AuraError::not_found("Choreography config not found"))?
            .clone();
        drop(configs);

        let states = self.choreography_state.read().await;
        let state = states
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
        drop(states);

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
            sequence_number: 0,
            guard_capabilities: config.guard_capabilities.clone(),
        };

        let mut messages = self.choreographic_messages.write().await;
        messages
            .entry(session_id)
            .or_default()
            .push(choreographic_message);
        Ok(())
    }

    async fn receive_choreographic_messages(
        &self,
        session_id: SessionId,
        role_filter: Option<ChoreographicRole>,
    ) -> Result<Vec<ChoreographicMessage>> {
        let messages = self.choreographic_messages.read().await;
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
        let states = self.choreography_state.read().await;
        let phase = states.get(&session_id).map(|s| match s.choreography_phase {
            SessionChoreographyPhase::Invitation => "Initial".to_string(),
            SessionChoreographyPhase::ParticipantResponse => "Handshake".to_string(),
            SessionChoreographyPhase::Active => "Active".to_string(),
            SessionChoreographyPhase::Ending => "Ending".to_string(),
            SessionChoreographyPhase::Ended => "Ended".to_string(),
        });
        Ok(phase)
    }

    async fn update_choreography_state(
        &self,
        session_id: SessionId,
        phase: &str,
        state_data: &[u8],
    ) -> Result<()> {
        let mut states = self.choreography_state.write().await;
        if let Some(state) = states.get_mut(&session_id) {
            state.choreography_phase = match phase {
                "Initial" => SessionChoreographyPhase::Invitation,
                "Handshake" => SessionChoreographyPhase::ParticipantResponse,
                "Active" => SessionChoreographyPhase::Active,
                "Ending" => SessionChoreographyPhase::Ending,
                "Ended" => SessionChoreographyPhase::Ended,
                _ => {
                    return Err(aura_core::AuraError::invalid(format!(
                        "Unknown choreography phase: {}",
                        phase
                    )))
                }
            };
            state.metadata = state_data.to_vec();
        }
        Ok(())
    }

    async fn validate_choreographic_message(
        &self,
        session_id: SessionId,
        message: &ChoreographicMessage,
    ) -> Result<bool> {
        let states = self.choreography_state.read().await;
        let state = states.get(&session_id);
        let phase = state
            .map(|s| s.choreography_phase.clone())
            .unwrap_or(SessionChoreographyPhase::Invitation);

        // Basic validation: ensure message roles align with phase expectations
        let valid_role = match phase {
            SessionChoreographyPhase::Invitation => message.target_role.is_none(),
            SessionChoreographyPhase::ParticipantResponse => {
                let manager = state.map(|s| s.manager_id);
                let target_matches_manager = message
                    .target_role
                    .as_ref()
                    .map(|role| Some(aura_core::DeviceId::from_uuid(role.device_id)) == manager)
                    .unwrap_or(false);
                target_matches_manager
            }
            SessionChoreographyPhase::Active => true,
            SessionChoreographyPhase::Ending | SessionChoreographyPhase::Ended => false,
        };

        Ok(valid_role)
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
        async fn physical_time(&self) -> std::result::Result<aura_core::time::PhysicalTime, aura_effects::TimeError> {
            Ok(aura_core::time::PhysicalTime {
                ts_ms: 1234567890000,
                uncertainty: None,
            })
        }

        async fn sleep_ms(&self, _ms: u64) -> std::result::Result<(), aura_effects::TimeError> {
            Ok(()) // No-op for tests
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
    }
}
