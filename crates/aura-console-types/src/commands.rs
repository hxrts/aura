//! Command types for console-to-server communication.

use serde::{Deserialize, Serialize};

/// Commands sent from the console UI to backend servers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum ConsoleCommand {
    // Simulation control
    Step {
        count: u64,
    },
    RunUntilIdle,
    SeekToTick {
        tick: u64,
    },
    Checkpoint {
        label: Option<String>,
    },
    RestoreCheckpoint {
        checkpoint_id: String,
    },

    // State queries
    QueryState {
        device_id: String,
    },
    GetTopology,
    GetLedger {
        device_id: String,
    },
    GetViolations,

    // Message operations
    InjectMessage {
        to: String,
        message: String,
    },
    BroadcastMessage {
        message: String,
    },

    // Protocol operations
    InitiateDkd {
        participants: Vec<String>,
        context: String,
    },
    InitiateResharing {
        participants: Vec<String>,
    },
    InitiateRecovery {
        guardians: Vec<String>,
    },

    // Network manipulation
    CreatePartition {
        devices: Vec<String>,
    },
    SetDeviceOffline {
        device_id: String,
    },
    EnableByzantine {
        device_id: String,
        strategy: String,
    },

    // Branch management
    ListBranches,
    CheckoutBranch {
        branch_id: String,
    },
    ForkBranch {
        label: Option<String>,
    },
    DeleteBranch {
        branch_id: String,
    },
    ExportScenario {
        branch_id: String,
        filename: String,
    },

    // Scenario management
    LoadScenario {
        filename: String,
    },
    LoadTrace {
        filename: String,
    },

    // Analysis
    GetCausalityPath {
        event_id: u64,
    },
    GetEventsInRange {
        start: u64,
        end: u64,
    },
}

/// REPL commands (text-based interface).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplCommand {
    pub command: String,
    pub args: Vec<String>,
}

impl ReplCommand {
    pub fn new(command: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            command: command.into(),
            args,
        }
    }
}
