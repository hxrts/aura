//! Console-specific types for server-to-client communication

use serde::{Deserialize, Serialize};
use std::time::SystemTime;
use uuid::Uuid;

/// DevConsole-specific command responses
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ConsoleResponse {
    // Information responses
    Help {
        help_text: String,
    },
    Status {
        simulation_info: SimulationInfo,
    },
    Devices {
        devices: Vec<DeviceInfo>,
    },
    State {
        state: String,
    },
    Ledger {
        ledger_info: LedgerStateInfo,
    },
    Branches {
        branches: Vec<BranchInfo>,
    },
    Events {
        events: Vec<super::TraceEvent>,
    },
    NetworkTopology {
        topology: super::NetworkTopology,
    },

    // Control responses
    Step {
        new_tick: u64,
    },
    StepUntil {
        final_tick: u64,
        steps_taken: u64,
        condition_met: bool,
    },
    Reset,
    Fork {
        new_branch_id: Uuid,
        parent_branch_id: Uuid,
    },
    Switch {
        new_branch_id: Uuid,
    },

    // Protocol responses
    InitiateDkd {
        session_id: String,
        participants: Vec<String>,
    },
    InitiateRecovery {
        session_id: String,
        participants: Vec<String>,
    },

    // Network responses
    Partition {
        participants: Vec<String>,
    },
    Heal,
    Delay {
        from: String,
        to: String,
        delay_ms: u64,
    },

    // Testing responses
    Byzantine {
        participant: String,
        strategy: String,
    },
    Inject {
        participant: String,
        event_type: String,
    },

    // Scenario export response
    ExportScenario {
        toml_content: String,
        filename: String,
    },

    // Error response
    Error {
        message: String,
    },
}

/// Information about the current simulation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationInfo {
    pub id: Uuid,
    pub current_tick: u64,
    pub current_time: u64,
    pub seed: u64,
    pub is_recording: bool,
}

/// Information about a device/participant
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub id: String,
    pub device_id: String,
    pub account_id: String,
    pub participant_type: super::trace::ParticipantType,
    pub status: super::trace::ParticipantStatus,
    pub message_count: u64,
}

/// Information about ledger state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerStateInfo {
    pub head_count: u64,
    pub total_events: u64,
    pub participants: u64,
    pub latest_sequence: u64,
}

/// Information about a simulation branch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchInfo {
    pub id: Uuid,
    pub name: Option<String>,
    pub parent_branch: Option<Uuid>,
    pub simulation_info: SimulationInfo,
    pub created_at: SystemTime,
    pub last_activity: SystemTime,
    pub is_active: bool,
    pub event_count: u64,
}

/// Client-to-server message envelope
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMessage {
    Command {
        id: String,
        command: super::ConsoleCommand,
    },
    Subscribe {
        event_types: Vec<String>,
    },
    Unsubscribe {
        event_types: Vec<String>,
    },
}

/// Server-to-client message envelope
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerMessage {
    Response {
        id: String,
        response: ConsoleResponse,
    },
    Event(ConsoleEvent),
}

/// Real-time events sent to clients
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type")]
pub enum ConsoleEvent {
    TraceEvent {
        event: super::TraceEvent,
    },
    BranchSwitched {
        new_branch_id: Uuid,
        previous_branch_id: Option<Uuid>,
    },
    SubscriptionChanged {
        subscribed: Vec<String>,
        unsubscribed: Vec<String>,
    },
    SimulationStateChanged {
        branch_id: Uuid,
        new_tick: u64,
        new_time: u64,
    },
    NetworkTopologyChanged {
        topology: super::NetworkTopology,
    },
}
