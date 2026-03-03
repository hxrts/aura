//! Runtime-owned session state manager.

use super::state::with_state_mut_validated;
use aura_core::identifiers::{DeviceId, SessionId};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Default)]
struct SessionState {
    sessions: HashMap<SessionId, SessionRecord>,
}

#[derive(Debug, Default, Clone)]
struct SessionRecord {
    participants: Vec<DeviceId>,
    metadata: HashMap<String, Value>,
}

impl SessionState {
    fn validate(&self) -> Result<(), String> {
        for (session_id, record) in &self.sessions {
            let mut dedup = std::collections::HashSet::new();
            for device_id in &record.participants {
                if !dedup.insert(*device_id) {
                    return Err(format!(
                        "Duplicate participant {device_id} in session {session_id}"
                    ));
                }
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

    pub(crate) async fn register_session(
        &self,
        session_id: SessionId,
        participants: Vec<DeviceId>,
    ) {
        with_state_mut_validated(
            &self.state,
            |state| {
                state
                    .sessions
                    .entry(session_id)
                    .or_insert_with(|| SessionRecord {
                        participants,
                        metadata: HashMap::new(),
                    });
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
                    .sessions
                    .entry(session_id)
                    .or_insert_with(SessionRecord::default);
                entry.metadata.extend(metadata);
                entry.metadata.clone()
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
                    .sessions
                    .entry(session_id)
                    .or_insert_with(SessionRecord::default);
                if !participants.participants.contains(&device_id) {
                    participants.participants.push(device_id);
                }
                participants.participants.clone()
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
                if let Some(record) = state.sessions.get_mut(&session_id) {
                    record.participants.retain(|id| id != &device_id);
                    return Some(record.participants.clone());
                }
                None
            },
            |state| state.validate(),
        )
        .await
    }
}
