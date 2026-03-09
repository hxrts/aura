//! Session-local bridge effects for the Aura and Telltale runtime boundary.
//!
//! These effects intentionally remain synchronous. Telltale host callbacks are
//! synchronous and must not perform async work. The bridge surface therefore
//! exposes only immediate session-local state mutation and snapshot operations.

/// Maximum payload size, in bytes, carried through one synchronous VM bridge send record.
pub const MAX_VM_BRIDGE_PAYLOAD_BYTES: usize = 1024 * 1024;

/// Host-visible scheduler pressure and contention signals for one VM fragment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VmBridgeSchedulerSignals {
    /// Recent guard contention events observed by the host bridge.
    pub guard_contention_events: u64,
    /// Flow-budget pressure in basis points (`0..=10_000`).
    pub flow_budget_pressure_bps: u16,
    /// Leakage-budget pressure in basis points (`0..=10_000`).
    pub leakage_budget_pressure_bps: u16,
}

impl VmBridgeSchedulerSignals {
    /// Clamp pressure signals to the representable basis-point range.
    #[must_use]
    pub fn normalized(self) -> Self {
        Self {
            guard_contention_events: self.guard_contention_events,
            flow_budget_pressure_bps: self.flow_budget_pressure_bps.min(10_000),
            leakage_budget_pressure_bps: self.leakage_budget_pressure_bps.min(10_000),
        }
    }
}

/// One pending outbound send emitted by the synchronous VM boundary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VmBridgePendingSend {
    /// Source role name.
    pub from_role: String,
    /// Destination role name.
    pub to_role: String,
    /// Label selected by the protocol.
    pub label: String,
    /// Serialized outbound payload.
    ///
    /// Callers must keep this at or below `MAX_VM_BRIDGE_PAYLOAD_BYTES`.
    pub payload: Vec<u8>,
}

/// One blocked receive edge observed at the synchronous VM boundary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VmBridgeBlockedEdge {
    /// Source role name.
    pub from_role: String,
    /// Destination role name.
    pub to_role: String,
}

/// Snapshot of lease-related metadata visible at the bridge boundary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VmBridgeLeaseMetadataSnapshot {
    /// Stable lease descriptors for the active fragment.
    pub lease_descriptors: Vec<String>,
}

/// Snapshot of transfer-related metadata visible at the bridge boundary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VmBridgeTransferMetadataSnapshot {
    /// Stable transfer descriptors for the active fragment.
    pub transfer_descriptors: Vec<String>,
}

/// Synchronous session-local bridge operations used by the Telltale host boundary.
pub trait VmBridgeEffects: Send + Sync {
    /// Queue one outbound payload for the next VM send callback.
    fn enqueue_outbound_payload(&self, payload: Vec<u8>);

    /// Consume the next queued outbound payload, if any.
    fn dequeue_outbound_payload(&self) -> Option<Vec<u8>>;

    /// Queue one inbound payload that has completed async host delivery.
    fn enqueue_inbound_payload(&self, payload: Vec<u8>);

    /// Consume the next queued inbound payload, if any.
    fn dequeue_inbound_payload(&self) -> Option<Vec<u8>>;

    /// Queue one branch choice for the next VM choice callback.
    fn enqueue_branch_choice(&self, label: String);

    /// Consume the next queued branch choice, if any.
    fn dequeue_branch_choice(&self) -> Option<String>;

    /// Record one pending send emitted by the synchronous VM boundary.
    fn record_pending_send(&self, send: VmBridgePendingSend);

    /// Drain all pending sends accumulated for async host delivery.
    fn drain_pending_sends(&self) -> Vec<VmBridgePendingSend>;

    /// Record the currently blocked receive edge for this fragment.
    fn set_blocked_edge(&self, edge: Option<VmBridgeBlockedEdge>);

    /// Snapshot the currently blocked receive edge for this fragment.
    fn blocked_edge(&self) -> Option<VmBridgeBlockedEdge>;

    /// Override host-visible scheduler signals for this fragment.
    fn set_scheduler_signals(&self, signals: VmBridgeSchedulerSignals);

    /// Snapshot scheduler signals for this fragment.
    fn scheduler_signals(&self) -> VmBridgeSchedulerSignals;

    /// Snapshot lease metadata visible at the bridge boundary.
    fn lease_metadata_snapshot(&self) -> VmBridgeLeaseMetadataSnapshot {
        VmBridgeLeaseMetadataSnapshot::default()
    }

    /// Snapshot transfer metadata visible at the bridge boundary.
    fn transfer_metadata_snapshot(&self) -> VmBridgeTransferMetadataSnapshot {
        VmBridgeTransferMetadataSnapshot::default()
    }
}
