//! Response types for server-to-console communication.

use super::network::NetworkTopology;
use super::trace::{PropertyViolation, TraceEvent};
use serde::{Deserialize, Serialize};

/// Response from backend servers to console commands.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Response {
    /// Command executed successfully.
    Success {
        /// Optional data payload.
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<ResponseData>,
    },
    /// Command execution failed.
    Error {
        /// Error message.
        message: String,
    },
    /// Fork is required to execute the command.
    ForkRequired {
        /// Message explaining why a fork is required.
        message: String,
        /// The command that requires a fork.
        command: String,
    },
}

/// Data returned in successful responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum ResponseData {
    /// Device or account state.
    State(serde_json::Value),
    /// Network topology information.
    Topology(NetworkTopology),
    /// Ledger events and state.
    Ledger(Vec<serde_json::Value>),
    /// Protocol property violations.
    Violations(Vec<PropertyViolation>),
    /// Trace events.
    Events(Vec<TraceEvent>),
    /// Branch information.
    Branches(Vec<BranchInfo>),
    /// Checkpoint created.
    Checkpoint {
        /// Checkpoint identifier.
        id: String,
        /// Simulation tick of the checkpoint.
        tick: u64,
    },
    /// Current simulation tick.
    Tick {
        /// The current tick value.
        current: u64,
    },
    /// Causality path between events.
    CausalityPath(Vec<u64>),
    /// Text response.
    Text(String),
}

/// Status of a response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseStatus {
    /// Response succeeded.
    Success,
    /// Response failed with an error.
    Error,
    /// Fork is required.
    ForkRequired,
}

/// Information about a simulation branch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchInfo {
    /// Branch identifier.
    pub id: String,
    /// Branch name.
    pub name: String,
    /// Whether this is the main branch.
    pub is_main: bool,
    /// Whether this branch is currently active.
    pub is_current: bool,
    /// Parent branch information if forked.
    pub parent: Option<BranchParent>,
    /// Current simulation tick in this branch.
    pub current_tick: u64,
    /// Random seed used in this branch.
    pub seed: u64,
}

/// Parent branch information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchParent {
    /// ID of the parent branch.
    pub branch_id: String,
    /// Simulation tick at which the fork occurred.
    pub fork_tick: u64,
}

impl Response {
    /// Creates a successful response with no data.
    pub fn success() -> Self {
        Response::Success { data: None }
    }

    /// Creates a successful response with data.
    pub fn success_with_data(data: ResponseData) -> Self {
        Response::Success { data: Some(data) }
    }

    /// Creates an error response.
    pub fn error(message: impl Into<String>) -> Self {
        Response::Error {
            message: message.into(),
        }
    }

    /// Creates a fork required response.
    pub fn fork_required(message: impl Into<String>, command: impl Into<String>) -> Self {
        Response::ForkRequired {
            message: message.into(),
            command: command.into(),
        }
    }
}
