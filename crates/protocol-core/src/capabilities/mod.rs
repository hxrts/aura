//! Capability bundle available to protocol executions.

mod effects;
mod storage;
mod transport;

use crate::metadata::ProtocolType;
use aura_errors::Result;
use aura_types::{AccountId, DeviceId, SessionId};
pub use effects::{EffectsExt, EffectsProvider};
use serde::{Deserialize, Serialize};
use std::time::Duration;
pub use storage::{AccessController, AccessDecision, ChunkMetadata, StorageBackend};
pub use transport::{ProtocolMessage, ProtocolTransport, ProtocolTransportExt};
use uuid::Uuid;

/// Effect emitted by a protocol step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProtocolEffects {
    /// Send a transport message to a specific participant.
    Send { message: ProtocolMessage },
    /// Broadcast a payload to multiple participants.
    Broadcast {
        from: DeviceId,
        payload: Vec<u8>,
        session_id: Option<Uuid>,
    },
    /// Persist a journal event by identifier.
    AppendJournal {
        event_type: String,
        payload: serde_json::Value,
    },
    /// Schedule a timer for future delivery.
    ScheduleTimer { timer_id: Uuid, timeout: Duration },
    /// Cancel a previously scheduled timer.
    CancelTimer { timer_id: Uuid },
    /// Emit tracing metadata without side effects.
    Trace {
        message: String,
        protocol: ProtocolType,
    },
    /// Update relationship counter state (temporary effect until ledger events wired).
    UpdateCounter {
        relationship_hash: [u8; 32],
        previous_value: u64,
        reserved_values: Vec<u64>,
        ttl_epochs: u64,
        requested_epoch: u64,
        requesting_device: DeviceId,
    },
}

/// Bundle of capabilities wired into each protocol step.
pub struct ProtocolCapabilities<'a> {
    /// Effect provider for deterministic randomness and time.
    pub effects: &'a dyn EffectsProvider,
    /// Transport interface for network IO.
    pub transport: &'a dyn ProtocolTransport,
    /// Storage backend for shared state.
    pub storage: Option<&'a dyn StorageBackend>,
    /// Access controller for capability checks.
    pub access: Option<&'a dyn AccessController>,
    /// Ledger/session lookups.
    pub ledger: Option<&'a dyn ProtocolLedgerView>,
}

impl<'a> ProtocolCapabilities<'a> {
    /// Helper to append a journal event using the ledger view.
    pub fn append_event(
        &self,
        session_id: SessionId,
        event_type: &str,
        payload: serde_json::Value,
    ) -> Result<()> {
        if let Some(ledger) = self.ledger {
            ledger.append_event(session_id, event_type, payload)
        } else {
            Err(aura_errors::AuraError::coordination_failed(
                "ledger capability unavailable",
            ))
        }
    }
}

/// Minimal ledger view used by protocols.
pub trait ProtocolLedgerView: Send + Sync {
    /// Append a serialized event to the ledger.
    fn append_event(
        &self,
        session_id: SessionId,
        event_type: &str,
        payload: serde_json::Value,
    ) -> Result<()>;

    /// Retrieve participants for a session.
    fn session_participants(&self, session_id: SessionId) -> Result<Vec<DeviceId>>;

    /// Lookup account-level metadata.
    fn account_devices(&self, account: &AccountId) -> Result<Vec<DeviceId>>;
}
