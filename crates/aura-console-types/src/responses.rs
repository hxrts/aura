//! Response types for server-to-console communication.

use super::network::NetworkTopology;
use super::trace::{PropertyViolation, TraceEvent};
use serde::{Deserialize, Serialize};

/// Response from backend servers to console commands.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Response {
    Success {
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<ResponseData>,
    },
    Error {
        message: String,
    },
    ForkRequired {
        message: String,
        command: String,
    },
}

/// Data returned in successful responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum ResponseData {
    State(serde_json::Value),
    Topology(NetworkTopology),
    Ledger(Vec<serde_json::Value>),
    Violations(Vec<PropertyViolation>),
    Events(Vec<TraceEvent>),
    Branches(Vec<BranchInfo>),
    Checkpoint { id: String, tick: u64 },
    Tick { current: u64 },
    CausalityPath(Vec<u64>),
    Text(String),
}

/// Status of a response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseStatus {
    Success,
    Error,
    ForkRequired,
}

/// Information about a simulation branch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchInfo {
    pub id: String,
    pub name: String,
    pub is_main: bool,
    pub is_current: bool,
    pub parent: Option<BranchParent>,
    pub current_tick: u64,
    pub seed: u64,
}

/// Parent branch information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchParent {
    pub branch_id: String,
    pub fork_tick: u64,
}

impl Response {
    pub fn success() -> Self {
        Response::Success { data: None }
    }

    pub fn success_with_data(data: ResponseData) -> Self {
        Response::Success { data: Some(data) }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Response::Error {
            message: message.into(),
        }
    }

    pub fn fork_required(message: impl Into<String>, command: impl Into<String>) -> Self {
        Response::ForkRequired {
            message: message.into(),
            command: command.into(),
        }
    }
}
