//! Session management handlers for agent operations

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::effects::agent::{
    SessionHandle, SessionInfo, SessionManagementEffects, SessionMessage, SessionRole,
    SessionStatus, SessionType,
};
use aura_core::{
    identifiers::{DeviceId, SessionId},
    AuraResult as Result,
};

/// In-memory session handler for testing
#[derive(Debug, Clone)]
pub struct MemorySessionHandler {
    device_id: DeviceId,
    sessions: Arc<RwLock<HashMap<SessionId, SessionData>>>,
    session_messages: Arc<RwLock<HashMap<SessionId, Vec<SessionMessage>>>>,
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
    /// Create a new memory session handler
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            device_id,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            session_messages: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get the device ID this handler is configured for
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Clear all sessions (useful for testing)
    pub fn clear(&self) {
        if let Ok(mut sessions) = self.sessions.write() {
            sessions.clear();
        }
        if let Ok(mut messages) = self.session_messages.write() {
            messages.clear();
        }
    }

    /// Get session count
    pub fn session_count(&self) -> usize {
        self.sessions
            .read()
            .map(|sessions| sessions.len())
            .unwrap_or(0)
    }

    fn current_timestamp(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}

#[async_trait]
impl SessionManagementEffects for MemorySessionHandler {
    async fn create_session(&self, session_type: SessionType) -> Result<SessionId> {
        let session_id = SessionId::new();
        let timestamp = self.current_timestamp();

        let session_data = SessionData {
            session_id,
            session_type,
            role: SessionRole::Coordinator,
            participants: vec![self.device_id],
            status: SessionStatus::Created,
            created_at: timestamp,
            updated_at: timestamp,
        };

        let mut sessions = self
            .sessions
            .write()
            .map_err(|_| aura_core::AuraError::internal("Failed to acquire write lock"))?;

        sessions.insert(session_id, session_data);

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
        session.updated_at = self.current_timestamp();

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
        session.updated_at = self.current_timestamp();

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
        session.updated_at = self.current_timestamp();

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
            timestamp: self.current_timestamp(),
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

    #[tokio::test]
    async fn test_memory_session_operations() {
        let device_id = DeviceId::new();
        let handler = MemorySessionHandler::new(device_id);

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
        let handler = MemorySessionHandler::new(device_id);

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
        let handler = MemorySessionHandler::new(device_id);

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
