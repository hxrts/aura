//! Console-specific types for server-to-client communication

use serde::{Deserialize, Serialize};
use std::time::SystemTime;
use uuid::Uuid;

/// DevConsole-specific command responses
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ConsoleResponse {
    /// Help text response.
    Help {
        /// The help text.
        help_text: String,
    },
    /// Current simulation status.
    Status {
        /// Simulation information.
        simulation_info: SimulationInfo,
    },
    /// List of devices in the system.
    Devices {
        /// Information about each device.
        devices: Vec<DeviceInfo>,
    },
    /// Device state information.
    State {
        /// Serialized state.
        state: String,
    },
    /// Ledger state information.
    Ledger {
        /// Ledger state details.
        ledger_info: LedgerStateInfo,
    },
    /// List of available branches.
    Branches {
        /// Information about each branch.
        branches: Vec<BranchInfo>,
    },
    /// Trace events.
    Events {
        /// Events to report.
        events: Vec<super::TraceEvent>,
    },
    /// Current network topology.
    NetworkTopology {
        /// Network topology information.
        topology: super::NetworkTopology,
    },

    /// Simulation step completed.
    Step {
        /// New simulation tick.
        new_tick: u64,
    },
    /// Simulation stepped until condition met.
    StepUntil {
        /// Final simulation tick.
        final_tick: u64,
        /// Number of steps taken.
        steps_taken: u64,
        /// Whether the stopping condition was met.
        condition_met: bool,
    },
    /// Simulation reset.
    Reset,
    /// New branch created.
    Fork {
        /// ID of the new branch.
        new_branch_id: Uuid,
        /// ID of the parent branch.
        parent_branch_id: Uuid,
    },
    /// Switched to a different branch.
    Switch {
        /// ID of the new branch.
        new_branch_id: Uuid,
    },

    /// DKD protocol initiated.
    InitiateDkd {
        /// Session identifier.
        session_id: String,
        /// Participating device IDs.
        participants: Vec<String>,
    },
    /// Recovery protocol initiated.
    InitiateRecovery {
        /// Session identifier.
        session_id: String,
        /// Participating device IDs.
        participants: Vec<String>,
    },

    /// Network partition created.
    Partition {
        /// Devices in the partition.
        participants: Vec<String>,
    },
    /// Network partition healed.
    Heal,
    /// Network delay applied.
    Delay {
        /// Source device.
        from: String,
        /// Target device.
        to: String,
        /// Delay in milliseconds.
        delay_ms: u64,
    },

    /// Byzantine behavior enabled.
    Byzantine {
        /// Affected participant.
        participant: String,
        /// Byzantine strategy name.
        strategy: String,
    },
    /// Event injection completed.
    Inject {
        /// Target participant.
        participant: String,
        /// Type of event injected.
        event_type: String,
    },

    /// Scenario exported.
    ExportScenario {
        /// TOML content of the scenario.
        toml_content: String,
        /// Target filename.
        filename: String,
    },

    /// Error response.
    Error {
        /// Error message.
        message: String,
    },
}

/// Information about the current simulation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationInfo {
    /// Unique simulation identifier.
    pub id: Uuid,
    /// Current simulation tick.
    pub current_tick: u64,
    /// Current simulation time.
    pub current_time: u64,
    /// Random seed for the simulation.
    pub seed: u64,
    /// Whether events are being recorded.
    pub is_recording: bool,
}

/// Information about a device/participant
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// Unique device identifier.
    pub id: String,
    /// Device identifier (may differ from id).
    pub device_id: String,
    /// Associated account identifier.
    pub account_id: String,
    /// Type of participant (e.g., device, guardian).
    pub participant_type: super::trace::ParticipantType,
    /// Current status of the participant.
    pub status: super::trace::ParticipantStatus,
    /// Number of messages processed.
    pub message_count: u64,
}

/// Information about ledger state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerStateInfo {
    /// Number of head entries in the ledger.
    pub head_count: u64,
    /// Total number of events recorded.
    pub total_events: u64,
    /// Number of participants in the system.
    pub participants: u64,
    /// Latest sequence number.
    pub latest_sequence: u64,
}

/// Information about a simulation branch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchInfo {
    /// Branch identifier.
    pub id: Uuid,
    /// Optional user-provided branch name.
    pub name: Option<String>,
    /// Parent branch if this was forked.
    pub parent_branch: Option<Uuid>,
    /// Associated simulation information.
    pub simulation_info: SimulationInfo,
    /// When the branch was created.
    pub created_at: SystemTime,
    /// When the branch was last active.
    pub last_activity: SystemTime,
    /// Whether this branch is currently active.
    pub is_active: bool,
    /// Number of events recorded in this branch.
    pub event_count: u64,
}

/// Client-to-server message envelope
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMessage {
    /// Command to execute.
    Command {
        /// Unique message ID for correlation.
        id: String,
        /// The command to execute.
        command: super::ConsoleCommand,
    },
    /// Subscribe to specific event types.
    Subscribe {
        /// Event types to subscribe to.
        event_types: Vec<String>,
    },
    /// Unsubscribe from specific event types.
    Unsubscribe {
        /// Event types to unsubscribe from.
        event_types: Vec<String>,
    },
}

/// Server-to-client message envelope
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerMessage {
    /// Response to a command.
    Response {
        /// Message ID correlating to the request.
        id: String,
        /// The response.
        response: ConsoleResponse,
    },
    /// Unsolicited event from the server.
    Event(ConsoleEvent),
}

/// Real-time events sent to clients
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type")]
pub enum ConsoleEvent {
    /// A trace event occurred.
    TraceEvent {
        /// The trace event.
        event: super::TraceEvent,
    },
    /// Active branch was changed.
    BranchSwitched {
        /// The new active branch.
        new_branch_id: Uuid,
        /// The previously active branch.
        previous_branch_id: Option<Uuid>,
    },
    /// Client subscriptions changed.
    SubscriptionChanged {
        /// Event types now subscribed to.
        subscribed: Vec<String>,
        /// Event types unsubscribed from.
        unsubscribed: Vec<String>,
    },
    /// Simulation state changed.
    SimulationStateChanged {
        /// Branch where state changed.
        branch_id: Uuid,
        /// New simulation tick.
        new_tick: u64,
        /// New simulation time.
        new_time: u64,
    },
    /// Network topology changed.
    NetworkTopologyChanged {
        /// The new topology.
        topology: super::NetworkTopology,
    },
}
