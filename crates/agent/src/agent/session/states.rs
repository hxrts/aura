//! Session state types and AgentProtocol struct
//!
//! This module defines the type-safe state machine for agent protocols.

use crate::agent::core::AgentCore;
use crate::{Storage, Transport};
use aura_protocol::SessionStatusInfo;
use aura_types::{AccountId, DeviceId};
use serde::{Deserialize, Serialize};

/// Session state trait - marker for type-safe state transitions
pub trait SessionState: Send + Sync + 'static {
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// Uninitialized state - agent created but not bootstrapped
pub struct Uninitialized;
impl SessionState for Uninitialized {}

/// Idle state - ready to perform operations
pub struct Idle;
impl SessionState for Idle {}

/// Coordinating state - running long-term protocols (limited API)
pub struct Coordinating;
impl SessionState for Coordinating {}

/// Failed state - error state (can attempt recovery)
pub struct Failed;
impl SessionState for Failed {
    const IS_FINAL: bool = true;
    const CAN_TERMINATE: bool = true;
}

/// Bootstrap configuration for agent initialization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapConfig {
    pub threshold: u16,
    pub share_count: u16,
    pub parameters: serde_json::Value,
}

impl Default for BootstrapConfig {
    fn default() -> Self {
        Self {
            threshold: 2,
            share_count: 3,
            parameters: serde_json::Value::Null,
        }
    }
}

/// Protocol execution status
#[derive(Debug, Clone)]
pub enum ProtocolStatus {
    Idle,
    InProgress {
        protocol_name: String,
        progress: f32,
    },
    Completed {
        protocol_name: String,
    },
    Failed {
        protocol_name: String,
        error: String,
    },
}

/// Agent protocol with type-safe state machine
///
/// The generic `State` parameter ensures only valid operations
/// are available in each state at compile time.
pub struct AgentProtocol<T: Transport, S: Storage, State: SessionState> {
    pub inner: AgentCore<T, S>,
    _state: std::marker::PhantomData<State>,
}

impl<T: Transport, S: Storage, State: SessionState> AgentProtocol<T, S, State> {
    /// Create a new agent protocol instance
    pub fn new(core: AgentCore<T, S>) -> Self {
        Self {
            inner: core,
            _state: std::marker::PhantomData,
        }
    }

    /// Transition to a new state (type-safe state transitions)
    pub fn transition_to<NewState: SessionState>(self) -> AgentProtocol<T, S, NewState> {
        AgentProtocol {
            inner: self.inner,
            _state: std::marker::PhantomData,
        }
    }

    /// Get the device ID (available in all states)
    pub fn device_id(&self) -> DeviceId {
        self.inner.device_id()
    }

    /// Get the account ID (available in all states)
    pub fn account_id(&self) -> AccountId {
        self.inner.account_id()
    }
}

/// Type alias for uninitialized agent
pub type UnifiedAgent<T, S> = AgentProtocol<T, S, Uninitialized>;

/// Witness that a protocol has completed successfully
#[derive(Debug)]
pub struct ProtocolCompleted {
    pub protocol_id: uuid::Uuid,
    pub result: serde_json::Value,
}

/// Detailed failure information for failed agents
#[derive(Debug, Clone)]
pub struct FailureInfo {
    pub device_id: DeviceId,
    pub failure_time: std::time::SystemTime,
    pub failed_sessions: Vec<SessionStatusInfo>,
    pub can_retry: bool,
    pub suggested_action: String,
}
