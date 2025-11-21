//! Unified AuraContext for effect system operations
//!
//! This module provides the unified context that flows through all effect operations,
//! maintaining state, device identity, execution mode, and operation tracking.

use crate::guards::flow::FlowHint;
use aura_core::{
    effects::{RandomEffects, TimeEffects},
    identifiers::{DeviceId, SessionId},
    AccountId,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Execution mode for the effect system
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionMode {
    /// Testing mode with deterministic, mock handlers
    Testing,
    /// Production mode with real effect handlers
    Production,
    /// Simulation mode with controllable, seeded handlers
    Simulation { seed: u64 },
}

/// Unified context for all effect operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuraContext {
    /// Device executing the operations
    pub device_id: DeviceId,

    /// Account context (if known)
    pub account_id: Option<AccountId>,

    /// Current session (if in a protocol session)
    pub session_id: Option<SessionId>,

    /// Execution mode
    pub mode: ExecutionMode,

    /// Operation tracking ID for tracing/debugging
    pub operation_id: Uuid,

    /// Arbitrary metadata for extensions
    pub metadata: HashMap<String, String>,

    /// Epoch timestamp for time-based operations
    pub epoch: u64,

    /// Pending flow budget hint (consumed before the next transport send)
    #[serde(skip)]
    pub flow_hint: Option<FlowHint>,
}

impl AuraContext {
    /// Create a new context for testing
    pub fn for_testing(device_id: DeviceId) -> Self {
        Self {
            device_id,
            account_id: None,
            session_id: None,
            mode: ExecutionMode::Testing,
            operation_id: Uuid::nil(), // Use nil UUID for deterministic testing
            metadata: HashMap::new(),
            epoch: 0,
            flow_hint: None,
        }
    }

    /// Create a new context for production
    pub async fn for_production<R, T>(
        device_id: DeviceId,
        random_effects: &R,
        time_effects: &T,
    ) -> Self
    where
        R: RandomEffects,
        T: TimeEffects,
    {
        // Generate UUID using random effects
        let bytes_vec = random_effects.random_bytes(16).await;
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&bytes_vec);
        let operation_id = Uuid::from_bytes(bytes);

        // Get current time using time effects
        let epoch = time_effects.current_timestamp().await;

        Self {
            device_id,
            account_id: None,
            session_id: None,
            mode: ExecutionMode::Production,
            operation_id,
            metadata: HashMap::new(),
            epoch,
            flow_hint: None,
        }
    }

    /// Create a new context for simulation
    pub fn for_simulation(device_id: DeviceId, seed: u64) -> Self {
        // Create deterministic UUID from seed for simulation
        let uuid_bytes = [
            (seed >> 56) as u8,
            (seed >> 48) as u8,
            (seed >> 40) as u8,
            (seed >> 32) as u8,
            (seed >> 24) as u8,
            (seed >> 16) as u8,
            (seed >> 8) as u8,
            seed as u8,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ];
        let operation_id = Uuid::from_bytes(uuid_bytes);

        Self {
            device_id,
            account_id: None,
            session_id: None,
            mode: ExecutionMode::Simulation { seed },
            operation_id,
            metadata: HashMap::new(),
            epoch: 0,
            flow_hint: None,
        }
    }

    /// Create a child context for a new operation
    pub async fn child_operation<R>(&self, random_effects: &R) -> Self
    where
        R: RandomEffects,
    {
        let mut child = self.clone();

        match &self.mode {
            ExecutionMode::Testing => {
                child.operation_id = Uuid::nil(); // Deterministic for testing
            }
            ExecutionMode::Production => {
                // Generate UUID using random effects
                let bytes_vec = random_effects.random_bytes(16).await;
                let mut bytes = [0u8; 16];
                bytes.copy_from_slice(&bytes_vec);
                child.operation_id = Uuid::from_bytes(bytes);
            }
            ExecutionMode::Simulation { seed } => {
                // Create deterministic UUID from parent operation ID and seed
                let parent_bytes = self.operation_id.as_bytes();
                let mut uuid_bytes = [0u8; 16];
                for (i, &byte) in parent_bytes.iter().enumerate() {
                    uuid_bytes[i] = byte.wrapping_add((seed >> (i * 8)) as u8);
                }
                child.operation_id = Uuid::from_bytes(uuid_bytes);
            }
        }

        child.flow_hint = None;
        child
    }

    /// Create a child context for a session
    pub async fn with_session<R>(&self, session_id: SessionId, random_effects: &R) -> Self
    where
        R: RandomEffects,
    {
        let mut child = self.clone();
        child.session_id = Some(session_id);

        match &self.mode {
            ExecutionMode::Testing => {
                child.operation_id = Uuid::nil(); // Deterministic for testing
            }
            ExecutionMode::Production => {
                // Generate UUID using random effects
                let bytes_vec = random_effects.random_bytes(16).await;
                let mut bytes = [0u8; 16];
                bytes.copy_from_slice(&bytes_vec);
                child.operation_id = Uuid::from_bytes(bytes);
            }
            ExecutionMode::Simulation { seed } => {
                // Create deterministic UUID from session_id and seed
                let session_bytes = session_id.0.as_bytes();
                let mut uuid_bytes = [0u8; 16];
                for (i, &byte) in session_bytes.iter().take(16).enumerate() {
                    uuid_bytes[i] = byte.wrapping_add((seed >> (i * 8)) as u8);
                }
                child.operation_id = Uuid::from_bytes(uuid_bytes);
            }
        }

        child.flow_hint = None;
        child
    }

    /// Set account context
    pub fn with_account(&mut self, account_id: AccountId) -> &mut Self {
        self.account_id = Some(account_id);
        self
    }

    /// Add metadata
    pub fn with_metadata(&mut self, key: impl Into<String>, value: impl Into<String>) -> &mut Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Get metadata value
    pub fn get_metadata(&self, key: &str) -> Option<&str> {
        self.metadata.get(key).map(|s| s.as_str())
    }

    /// Set a flow hint that will be consumed before the next transport send.
    pub fn set_flow_hint(&mut self, hint: FlowHint) -> &mut Self {
        self.flow_hint = Some(hint);
        self
    }

    /// Take the pending flow hint (if any).
    pub fn take_flow_hint(&mut self) -> Option<FlowHint> {
        self.flow_hint.take()
    }

    /// Check if this is a testing context
    pub fn is_testing(&self) -> bool {
        matches!(self.mode, ExecutionMode::Testing)
    }

    /// Check if this is a production context
    pub fn is_production(&self) -> bool {
        matches!(self.mode, ExecutionMode::Production)
    }

    /// Check if this is a simulation context
    pub fn is_simulation(&self) -> bool {
        matches!(self.mode, ExecutionMode::Simulation { .. })
    }

    /// Get simulation seed (if in simulation mode)
    pub fn simulation_seed(&self) -> Option<u64> {
        match self.mode {
            ExecutionMode::Simulation { seed } => Some(seed),
            _ => None,
        }
    }
}

impl Default for AuraContext {
    fn default() -> Self {
        Self::for_testing(DeviceId::new())
    }
}
