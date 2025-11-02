//! Command types for console-to-server communication.

use serde::{Deserialize, Serialize};

/// Commands sent from the console UI to backend servers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum ConsoleCommand {
    /// Advance the simulation by the specified number of steps.
    Step {
        /// Number of simulation steps to advance.
        count: u64,
    },
    /// Run the simulation until no further progress can be made.
    RunUntilIdle,
    /// Jump to a specific simulation tick.
    SeekToTick {
        /// Target simulation tick.
        tick: u64,
    },
    /// Create a checkpoint at the current simulation state.
    Checkpoint {
        /// Optional label for the checkpoint.
        label: Option<String>,
    },
    /// Restore simulation to a previously saved checkpoint.
    RestoreCheckpoint {
        /// Identifier of the checkpoint to restore.
        checkpoint_id: String,
    },

    /// Query the state of a specific device.
    QueryState {
        /// Identifier of the device to query.
        device_id: String,
    },
    /// Retrieve the current network topology.
    GetTopology,
    /// Get the ledger state for a specific device.
    GetLedger {
        /// Identifier of the device.
        device_id: String,
    },
    /// Get all detected protocol violations.
    GetViolations,

    /// Inject a message into the system for a specific recipient.
    InjectMessage {
        /// Recipient device identifier.
        to: String,
        /// Message content.
        message: String,
    },
    /// Broadcast a message to all participants.
    BroadcastMessage {
        /// Message content to broadcast.
        message: String,
    },

    /// Initiate a Deterministic Key Derivation (DKD) protocol.
    InitiateDkd {
        /// Devices participating in the DKD protocol.
        participants: Vec<String>,
        /// Derivation context.
        context: String,
    },
    /// Initiate a key resharing protocol.
    InitiateResharing {
        /// Devices participating in the resharing.
        participants: Vec<String>,
    },
    /// Initiate an account recovery protocol.
    InitiateRecovery {
        /// Guardian devices that will assist in recovery.
        guardians: Vec<String>,
    },

    /// Create a network partition isolating specified devices.
    CreatePartition {
        /// Devices to isolate.
        devices: Vec<String>,
    },
    /// Take a device offline.
    SetDeviceOffline {
        /// Device identifier to take offline.
        device_id: String,
    },
    /// Enable Byzantine fault behavior on a device.
    EnableByzantine {
        /// Device identifier.
        device_id: String,
        /// Byzantine behavior strategy.
        strategy: String,
    },

    /// List all available branches.
    ListBranches,
    /// Switch to a different simulation branch.
    CheckoutBranch {
        /// Branch identifier to switch to.
        branch_id: String,
    },
    /// Create a new branch from the current state.
    ForkBranch {
        /// Optional label for the new branch.
        label: Option<String>,
    },
    /// Delete a branch.
    DeleteBranch {
        /// Branch identifier to delete.
        branch_id: String,
    },
    /// Export a scenario from a branch to a file.
    ExportScenario {
        /// Source branch identifier.
        branch_id: String,
        /// Target filename.
        filename: String,
    },

    /// Load a scenario from a file.
    LoadScenario {
        /// Filename to load from.
        filename: String,
    },
    /// Load a trace from a file.
    LoadTrace {
        /// Filename to load from.
        filename: String,
    },

    /// Retrieve the causal path of an event.
    GetCausalityPath {
        /// Event identifier.
        event_id: u64,
    },
    /// Get all events within a time range.
    GetEventsInRange {
        /// Start tick (inclusive).
        start: u64,
        /// End tick (inclusive).
        end: u64,
    },
}

/// REPL commands (text-based interface).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplCommand {
    /// The command name.
    pub command: String,
    /// Command arguments.
    pub args: Vec<String>,
}

impl ReplCommand {
    /// Creates a new REPL command with the given name and arguments.
    pub fn new(command: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            command: command.into(),
            args,
        }
    }
}
