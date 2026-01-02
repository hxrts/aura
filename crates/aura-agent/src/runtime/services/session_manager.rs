//! Runtime-owned session state manager.

use super::state::with_state_mut_validated;
use aura_core::identifiers::{DeviceId, SessionId};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Default)]
struct SessionState {
    participants: HashMap<SessionId, Vec<DeviceId>>,
    metadata: HashMap<SessionId, HashMap<String, Value>>,
}

impl SessionState {
    fn validate(&self) -> Result<(), String> {
        for session_id in self.participants.keys() {
            if !self.metadata.contains_key(session_id) {
                return Err(format!("Missing metadata entry for session {}", session_id));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SessionManager {
    state: Arc<RwLock<SessionState>>,
}

impl SessionManager {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) async fn register_session(&self, session_id: SessionId, participants: Vec<DeviceId>) {
        with_state_mut_validated(
            &self.state,
            |state| {
                state
                    .participants
                    .entry(session_id)
                    .or_insert_with(|| participants);
                state
                    .metadata
                    .entry(session_id)
                    .or_insert_with(HashMap::new);
            },
            |state| state.validate(),
        )
        .await;
    }

    pub(crate) async fn update_metadata(
        &self,
        session_id: SessionId,
        metadata: HashMap<String, Value>,
    ) -> HashMap<String, Value> {
        with_state_mut_validated(
            &self.state,
            |state| {
                let entry = state
                    .metadata
                    .entry(session_id)
                    .or_insert_with(HashMap::new);
                entry.extend(metadata);
                entry.clone()
            },
            |state| state.validate(),
        )
        .await
    }

    pub(crate) async fn add_participant(
        &self,
        session_id: SessionId,
        device_id: DeviceId,
    ) -> Vec<DeviceId> {
        with_state_mut_validated(
            &self.state,
            |state| {
                let participants = state
                    .participants
                    .entry(session_id)
                    .or_insert_with(Vec::new);
                if !participants.contains(&device_id) {
                    participants.push(device_id);
                }
                state
                    .metadata
                    .entry(session_id)
                    .or_insert_with(HashMap::new);
                participants.clone()
            },
            |state| state.validate(),
        )
        .await
    }

    pub(crate) async fn remove_participant(
        &self,
        session_id: SessionId,
        device_id: DeviceId,
    ) -> Option<Vec<DeviceId>> {
        with_state_mut_validated(
            &self.state,
            |state| {
                if let Some(participants) = state.participants.get_mut(&session_id) {
                    participants.retain(|id| id != &device_id);
                    return Some(participants.clone());
                }
                None
            },
            |state| state.validate(),
        )
        .await
    }
}
