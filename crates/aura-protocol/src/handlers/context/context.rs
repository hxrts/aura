//! Main unified context for all Aura operations
//!
//! The `AuraContext` is the central immutable context that flows through all
//! handler operations without mutation, ensuring thread-safe access without locks.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use aura_core::identifiers::DeviceId;
use aura_core::{AccountId, SessionId};

use super::{AgentContext, ChoreographicContext, MiddlewareContext, SimulationContext};
use crate::handlers::ExecutionMode;

/// Immutable unified context for all Aura operations
///
/// This context flows through all handler operations without any mutation,
/// ensuring thread-safe access without locks. All modifications return new
/// instances rather than mutating in place.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuraContext {
    /// Core device identifier
    pub device_id: DeviceId,
    /// Execution mode (testing, production, simulation)
    pub execution_mode: ExecutionMode,
    /// Current session identifier
    pub session_id: Option<SessionId>,
    /// When this context was created (timestamp in milliseconds)
    pub created_at: u64,
    /// Active account identifier (if known)
    pub account_id: Option<AccountId>,
    /// Arbitrary metadata for higher-level components (immutable)
    pub metadata: Arc<HashMap<String, String>>,
    /// Unique operation identifier for tracing
    pub operation_id: Uuid,
    /// Epoch timestamp bound to the context
    pub epoch: u64,

    // Layer-specific contexts
    /// Choreographic operations context
    pub choreographic: Option<ChoreographicContext>,
    /// Simulation operations context
    pub simulation: Option<SimulationContext>,
    /// Agent operations context
    pub agent: Option<AgentContext>,

    // Cross-cutting context
    /// Middleware operations context
    pub middleware: MiddlewareContext,
}

impl AuraContext {
    /// Create a new context for testing mode
    pub fn for_testing(device_id: DeviceId) -> Self {
        let created_at = 0u64; // Fixed timestamp for deterministic testing
        let mut seed = Vec::with_capacity(24);
        seed.extend_from_slice(device_id.0.as_bytes());
        seed.extend_from_slice(&created_at.to_le_bytes());
        let digest = aura_core::hash::hash(&seed);
        let mut op_bytes = [0u8; 16];
        op_bytes.copy_from_slice(&digest[..16]);
        let operation_id = uuid::Uuid::from_bytes(op_bytes);
        Self {
            device_id,
            execution_mode: ExecutionMode::Testing,
            session_id: None,
            created_at,
            account_id: None,
            metadata: Arc::new(HashMap::new()),
            operation_id,
            epoch: created_at,
            choreographic: None,
            simulation: None,
            agent: Some(AgentContext::new(device_id)),
            middleware: MiddlewareContext::new(),
        }
    }

    /// Create a new context for production mode
    pub fn for_production(device_id: DeviceId, created_at: u64, operation_id: Uuid) -> Self {
        Self {
            device_id,
            execution_mode: ExecutionMode::Production,
            session_id: None,
            created_at,
            account_id: None,
            metadata: Arc::new(HashMap::new()),
            operation_id,
            epoch: created_at,
            choreographic: None,
            simulation: None,
            agent: Some(AgentContext::new(device_id)),
            middleware: MiddlewareContext::new(),
        }
    }

    /// Create a new context for simulation mode
    pub fn for_simulation(device_id: DeviceId, seed: u64) -> Self {
        let created_at = seed; // Use seed as deterministic timestamp
        Self {
            device_id,
            execution_mode: ExecutionMode::Simulation { seed },
            session_id: None,
            created_at,
            account_id: None,
            metadata: Arc::new(HashMap::new()),
            operation_id: uuid::Uuid::from_u128(seed as u128), // Deterministic UUID from seed
            epoch: created_at,
            choreographic: None,
            simulation: Some(SimulationContext::new(seed)),
            agent: Some(AgentContext::new(device_id)),
            middleware: MiddlewareContext::new(),
        }
    }

    /// Create new context with choreographic context
    pub fn with_choreographic(&self, context: ChoreographicContext) -> Self {
        Self {
            device_id: self.device_id,
            execution_mode: self.execution_mode,
            session_id: self.session_id,
            created_at: self.created_at,
            account_id: self.account_id,
            metadata: self.metadata.clone(),
            operation_id: self.operation_id,
            epoch: self.epoch,
            choreographic: Some(context),
            simulation: self.simulation.clone(),
            agent: self.agent.clone(),
            middleware: self.middleware.clone(),
        }
    }

    /// Create new context with session ID
    pub fn with_session(&self, session_id: SessionId) -> Self {
        Self {
            device_id: self.device_id,
            execution_mode: self.execution_mode,
            session_id: Some(session_id),
            created_at: self.created_at,
            account_id: self.account_id,
            metadata: self.metadata.clone(),
            operation_id: self.operation_id,
            epoch: self.epoch,
            choreographic: self.choreographic.clone(),
            simulation: self.simulation.clone(),
            agent: self.agent.clone(),
            middleware: self.middleware.clone(),
        }
    }

    /// Create new context with account identifier
    pub fn with_account(&self, account_id: AccountId) -> Self {
        Self {
            device_id: self.device_id,
            execution_mode: self.execution_mode,
            session_id: self.session_id,
            created_at: self.created_at,
            account_id: Some(account_id),
            metadata: self.metadata.clone(),
            operation_id: self.operation_id,
            epoch: self.epoch,
            choreographic: self.choreographic.clone(),
            simulation: self.simulation.clone(),
            agent: self.agent.clone(),
            middleware: self.middleware.clone(),
        }
    }

    /// Create new context with metadata entry
    pub fn with_metadata(&self, key: impl Into<String>, value: impl Into<String>) -> Self {
        let mut new_metadata = (*self.metadata).clone();
        new_metadata.insert(key.into(), value.into());

        Self {
            device_id: self.device_id,
            execution_mode: self.execution_mode,
            session_id: self.session_id,
            created_at: self.created_at,
            account_id: self.account_id,
            metadata: Arc::new(new_metadata),
            operation_id: self.operation_id,
            epoch: self.epoch,
            choreographic: self.choreographic.clone(),
            simulation: self.simulation.clone(),
            agent: self.agent.clone(),
            middleware: self.middleware.clone(),
        }
    }

    /// Create new context with tracing
    pub fn with_tracing(&self, trace_id: String, span_id: String) -> Self {
        Self {
            device_id: self.device_id,
            execution_mode: self.execution_mode,
            session_id: self.session_id,
            created_at: self.created_at,
            account_id: self.account_id,
            metadata: self.metadata.clone(),
            operation_id: self.operation_id,
            epoch: self.epoch,
            choreographic: self.choreographic.clone(),
            simulation: self.simulation.clone(),
            agent: self.agent.clone(),
            middleware: self.middleware.with_tracing(trace_id, span_id),
        }
    }

    /// Create new context with metrics enabled
    pub fn with_metrics(&self) -> Self {
        Self {
            device_id: self.device_id,
            execution_mode: self.execution_mode,
            session_id: self.session_id,
            created_at: self.created_at,
            account_id: self.account_id,
            metadata: self.metadata.clone(),
            operation_id: self.operation_id,
            epoch: self.epoch,
            choreographic: self.choreographic.clone(),
            simulation: self.simulation.clone(),
            agent: self.agent.clone(),
            middleware: self.middleware.with_metrics(),
        }
    }

    /// Create a derived context for a new operation
    pub fn child_operation(&self, operation_id: Uuid) -> Self {
        Self {
            device_id: self.device_id,
            execution_mode: self.execution_mode,
            session_id: self.session_id,
            created_at: self.created_at,
            account_id: self.account_id,
            metadata: self.metadata.clone(),
            operation_id,
            epoch: self.epoch,
            choreographic: self.choreographic.clone(),
            simulation: self.simulation.clone(),
            agent: self.agent.clone(),
            middleware: self.middleware.clone(),
        }
    }

    /// Check if this is a deterministic execution mode
    pub fn is_deterministic(&self) -> bool {
        self.execution_mode.is_deterministic()
    }

    /// Get the simulation seed if in simulation mode
    pub fn simulation_seed(&self) -> Option<u64> {
        self.execution_mode.seed()
    }

    /// Calculate elapsed time since context creation
    pub fn elapsed_millis(&self, current_time: u64) -> u64 {
        current_time.saturating_sub(self.created_at)
    }
}

impl Default for AuraContext {
    fn default() -> Self {
        Self::for_testing(DeviceId::deterministic_test_id())
    }
}
