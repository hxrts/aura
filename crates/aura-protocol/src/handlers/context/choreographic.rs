//! Choreographic context for multi-party protocols
//!
//! Immutable context for choreographic operations, tracking the current role,
//! participants, and protocol-specific state.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::effects::choreographic::ChoreographicRole;
use crate::handlers::AuraHandlerError;

/// Immutable context for choreographic operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChoreographicContext {
    /// Current role in the choreography
    pub current_role: ChoreographicRole,
    /// All participants in the choreography
    pub participants: Arc<Vec<ChoreographicRole>>,
    /// Current epoch for coordination
    pub epoch: u64,
    /// Protocol-specific state (immutable)
    pub protocol_state: Arc<HashMap<String, Vec<u8>>>,
}

impl ChoreographicContext {
    /// Create a new choreographic context
    pub fn new(
        current_role: ChoreographicRole,
        participants: Vec<ChoreographicRole>,
        epoch: u64,
    ) -> Self {
        Self {
            current_role,
            participants: Arc::new(participants),
            epoch,
            protocol_state: Arc::new(HashMap::new()),
        }
    }

    /// Create a new context with updated state
    pub fn with_state<T: serde::Serialize>(
        &self,
        key: &str,
        value: &T,
    ) -> Result<Self, AuraHandlerError> {
        let serialized = bincode::serialize(value).map_err(|e| {
            AuraHandlerError::context_error(format!("Failed to serialize state: {}", e))
        })?;

        let mut new_state = (*self.protocol_state).clone();
        new_state.insert(key.to_string(), serialized);

        Ok(Self {
            current_role: self.current_role,
            participants: self.participants.clone(),
            epoch: self.epoch,
            protocol_state: Arc::new(new_state),
        })
    }

    /// Get protocol-specific state
    pub fn get_state<T: serde::de::DeserializeOwned>(
        &self,
        key: &str,
    ) -> Result<Option<T>, AuraHandlerError> {
        match self.protocol_state.get(key) {
            Some(data) => {
                let value = bincode::deserialize(data).map_err(|e| {
                    AuraHandlerError::context_error(format!("Failed to deserialize state: {}", e))
                })?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    /// Check if we are the current deciding role
    pub fn is_decider(&self, decider: &ChoreographicRole) -> bool {
        &self.current_role == decider
    }

    /// Get the number of participants
    pub fn participant_count(&self) -> usize {
        self.participants.len()
    }
}
