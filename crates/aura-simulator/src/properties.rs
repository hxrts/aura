//! Online property definitions for simulation-time monitoring.

use aura_core::identifiers::{ContextId, SessionId};
use std::collections::HashMap;
use std::sync::Arc;

/// Guard-chain stages expected before transport send.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GuardStage {
    /// Capability authorization gate.
    CapGuard,
    /// Flow-budget charge gate.
    FlowGuard,
    /// Leakage-budget accounting gate.
    LeakageTracker,
    /// Journal-coupling gate.
    JournalCoupler,
    /// Final transport send.
    TransportSend,
}

/// Event stream consumed by [`AuraPropertyMonitor`](crate::property_monitor::AuraPropertyMonitor).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PropertyEvent {
    /// Session fault was observed.
    Faulted {
        /// Session where the fault occurred.
        session: SessionId,
        /// Human-readable reason.
        reason: String,
    },
    /// Message send was observed.
    Sent {
        /// Session associated with the message.
        session: SessionId,
        /// Stable message identifier.
        message_id: String,
    },
    /// Message receive was observed.
    Received {
        /// Session associated with the message.
        session: SessionId,
        /// Stable message identifier.
        message_id: String,
    },
    /// Journal divergence was detected.
    JournalDiverged {
        /// Context whose journals diverged.
        context: ContextId,
    },
    /// Journal convergence was observed.
    JournalConverged {
        /// Context whose journals converged.
        context: ContextId,
    },
    /// Consensus session was started.
    ConsensusStarted {
        /// Session that entered consensus.
        session: SessionId,
    },
    /// Consensus session committed successfully.
    ConsensusCommitted {
        /// Session that committed.
        session: SessionId,
    },
    /// One guard stage executed for a message.
    GuardStage {
        /// Session associated with guard evaluation.
        session: SessionId,
        /// Stable message identifier.
        message_id: String,
        /// Stage that executed.
        stage: GuardStage,
    },
}

/// Snapshot consumed by the property monitor once per tick.
#[derive(Debug, Clone, Default)]
pub struct PropertyStateSnapshot {
    /// Tick-local events to append to the property context trace.
    pub events: Vec<PropertyEvent>,
    /// Current buffer occupancy per session.
    pub buffer_sizes: HashMap<SessionId, usize>,
    /// Current local-type depth per session.
    pub session_depths: HashMap<SessionId, u64>,
    /// Current flow-budget balance per context.
    pub flow_budget_balances: HashMap<ContextId, i64>,
    /// Optional session-store state snapshot (opaque payloads).
    pub session_store: HashMap<String, String>,
    /// Optional coroutine-state snapshot (opaque payloads).
    pub coroutine_states: HashMap<String, String>,
    /// Optional journal-state snapshot (opaque payloads).
    pub journal_state: HashMap<String, String>,
}

/// Mutable context shared with every property evaluation.
#[derive(Debug, Clone, Default)]
pub struct PropertyContext {
    /// Current simulation tick.
    pub tick: u64,
    /// Full event trace observed so far.
    pub events: Vec<PropertyEvent>,
    /// Latest buffer occupancy by session.
    pub buffer_sizes: HashMap<SessionId, usize>,
    /// Latest local-type depth by session.
    pub session_depths: HashMap<SessionId, u64>,
    /// Latest flow-budget balances by context.
    pub flow_budget_balances: HashMap<ContextId, i64>,
    /// Latest session-store snapshot.
    pub session_store: HashMap<String, String>,
    /// Latest coroutine-state snapshot.
    pub coroutine_states: HashMap<String, String>,
    /// Latest journal-state snapshot.
    pub journal_state: HashMap<String, String>,
}

impl PropertyContext {
    /// Apply one tick snapshot to the context.
    pub fn apply_snapshot(&mut self, tick: u64, snapshot: &PropertyStateSnapshot) {
        self.tick = tick;
        self.events.extend(snapshot.events.clone());
        self.buffer_sizes
            .extend(snapshot.buffer_sizes.iter().map(|(k, v)| (*k, *v)));
        self.session_depths
            .extend(snapshot.session_depths.iter().map(|(k, v)| (*k, *v)));
        self.flow_budget_balances
            .extend(snapshot.flow_budget_balances.iter().map(|(k, v)| (*k, *v)));
        self.session_store.extend(
            snapshot
                .session_store
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );
        self.coroutine_states.extend(
            snapshot
                .coroutine_states
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );
        self.journal_state.extend(
            snapshot
                .journal_state
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );
    }
}

/// Predicate function type used by custom liveness properties.
pub type PropertyPredicate = Arc<dyn Fn(&PropertyContext) -> bool + Send + Sync>;

/// Online properties that can be checked every tick.
#[derive(Clone)]
pub enum AuraProperty {
    /// No sessions have faulted.
    NoFaults,
    /// Every send has a matching receive within a bound.
    SendRecvLiveness {
        /// Session under observation.
        session: SessionId,
        /// Maximum allowed send->recv distance in ticks.
        bound: u64,
    },
    /// Local type depth should not increase.
    TypeMonotonicity {
        /// Session under observation.
        session: SessionId,
    },
    /// Buffer occupancy stays below a limit.
    BufferBound {
        /// Session under observation.
        session: SessionId,
        /// Maximum allowed queue size.
        max_size: usize,
    },
    /// Generic liveness predicate with a precondition/goal pair.
    Liveness {
        /// Human-readable property name.
        name: String,
        /// Precondition activation predicate.
        precondition: PropertyPredicate,
        /// Goal predicate that must become true.
        goal: PropertyPredicate,
        /// Tick bound after precondition activation.
        bound: u64,
    },
    /// Journal divergence converges within a bound.
    JournalConvergence {
        /// Context under observation.
        context: ContextId,
        /// Tick bound after divergence.
        bound: u64,
    },
    /// Consensus session commits within a bound.
    ConsensusLiveness {
        /// Session under observation.
        session: SessionId,
        /// Tick bound after consensus start.
        bound: u64,
    },
    /// Flow-budget balances stay non-negative.
    FlowBudgetInvariant {
        /// Context under observation.
        context: ContextId,
    },
    /// Guard stages occur in the required order.
    GuardChainOrdering,
}

impl AuraProperty {
    /// Stable name for reporting.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::NoFaults => "NoFaults",
            Self::SendRecvLiveness { .. } => "SendRecvLiveness",
            Self::TypeMonotonicity { .. } => "TypeMonotonicity",
            Self::BufferBound { .. } => "BufferBound",
            Self::Liveness { .. } => "Liveness",
            Self::JournalConvergence { .. } => "JournalConvergence",
            Self::ConsensusLiveness { .. } => "ConsensusLiveness",
            Self::FlowBudgetInvariant { .. } => "FlowBudgetInvariant",
            Self::GuardChainOrdering => "GuardChainOrdering",
        }
    }
}

/// Suite selector for common protocol classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolPropertyClass {
    /// Consensus protocol suite.
    Consensus,
    /// Sync protocol suite.
    Sync,
    /// Chat protocol suite.
    Chat,
    /// Recovery protocol suite.
    Recovery,
}

/// Required identifiers to materialize a protocol suite.
#[derive(Debug, Clone, Copy)]
pub struct ProtocolPropertySuiteIds {
    /// Session under test.
    pub session: SessionId,
    /// Context under test.
    pub context: ContextId,
}

/// Build a default property suite for a protocol class.
#[must_use]
pub fn default_property_suite(
    class: ProtocolPropertyClass,
    ids: ProtocolPropertySuiteIds,
) -> Vec<AuraProperty> {
    match class {
        ProtocolPropertyClass::Consensus => vec![
            AuraProperty::NoFaults,
            AuraProperty::ConsensusLiveness {
                session: ids.session,
                bound: 128,
            },
            AuraProperty::TypeMonotonicity {
                session: ids.session,
            },
            AuraProperty::GuardChainOrdering,
        ],
        ProtocolPropertyClass::Sync => vec![
            AuraProperty::NoFaults,
            AuraProperty::JournalConvergence {
                context: ids.context,
                bound: 512,
            },
            AuraProperty::BufferBound {
                session: ids.session,
                max_size: 2048,
            },
            AuraProperty::FlowBudgetInvariant {
                context: ids.context,
            },
        ],
        ProtocolPropertyClass::Chat => vec![
            AuraProperty::NoFaults,
            AuraProperty::SendRecvLiveness {
                session: ids.session,
                bound: 64,
            },
            AuraProperty::BufferBound {
                session: ids.session,
                max_size: 1024,
            },
            AuraProperty::GuardChainOrdering,
        ],
        ProtocolPropertyClass::Recovery => vec![
            AuraProperty::NoFaults,
            AuraProperty::ConsensusLiveness {
                session: ids.session,
                bound: 256,
            },
            AuraProperty::JournalConvergence {
                context: ids.context,
                bound: 512,
            },
            AuraProperty::GuardChainOrdering,
        ],
    }
}
