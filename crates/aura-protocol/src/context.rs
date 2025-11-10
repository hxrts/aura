//! Unified AuraContext for effect system operations
//!
//! This module provides the unified context that flows through all effect operations,
//! maintaining state, device identity, execution mode, and operation tracking.

use crate::guards::flow::FlowHint;
use aura_core::{identifiers::SessionId, AccountId, DeviceId};
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
            operation_id: Uuid::new_v4(),
            metadata: HashMap::new(),
            epoch: 0,
             flow_hint: None,
        }
    }

    /// Create a new context for production
    pub fn for_production(device_id: DeviceId) -> Self {
        Self {
            device_id,
            account_id: None,
            session_id: None,
            mode: ExecutionMode::Production,
            operation_id: Uuid::new_v4(),
            metadata: HashMap::new(),
            epoch: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            flow_hint: None,
        }
    }

    /// Create a new context for simulation
    pub fn for_simulation(device_id: DeviceId, seed: u64) -> Self {
        Self {
            device_id,
            account_id: None,
            session_id: None,
            mode: ExecutionMode::Simulation { seed },
            operation_id: Uuid::new_v4(),
            metadata: HashMap::new(),
            epoch: 0,
            flow_hint: None,
        }
    }

    /// Create a child context for a new operation
    pub fn child_operation(&self) -> Self {
        let mut child = self.clone();
        child.operation_id = Uuid::new_v4();
        child.flow_hint = None;
        child
    }

    /// Create a child context for a session
    pub fn with_session(&self, session_id: SessionId) -> Self {
        let mut child = self.clone();
        child.session_id = Some(session_id);
        child.operation_id = Uuid::new_v4();
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
