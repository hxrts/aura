//! Message types for WebSocket communication

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Unique identifier for a message
pub type MessageId = Uuid;

/// Base message envelope for all WebSocket communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketEnvelope {
    /// Unique message identifier
    pub id: MessageId,
    /// Message type identifier
    pub message_type: String,
    /// Timestamp when message was created
    pub timestamp: u64,
    /// Source that created the message
    pub source: String,
    /// Target destination (optional)
    pub target: Option<String>,
    /// Message payload
    pub payload: serde_json::Value,
    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

/// Command messages sent to servers
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Command {
    /// Start a simulation
    StartSimulation {
        /// Name of the simulation scenario to run
        scenario_name: String,
        /// Configuration parameters for the simulation
        parameters: HashMap<String, serde_json::Value>,
    },
    /// Pause a running simulation
    PauseSimulation,
    /// Resume a paused simulation
    ResumeSimulation,
    /// Stop a simulation
    StopSimulation,
    /// Execute a single simulation step
    Step,
    /// Query trace data
    QueryTrace {
        /// Query parameters for retrieving trace data
        query: crate::trace::TraceQuery,
    },
    /// Evaluate properties
    EvaluateProperties {
        /// List of property identifiers to evaluate
        property_ids: Vec<crate::properties::PropertyId>,
    },
    /// Subscribe to event stream
    Subscribe {
        /// Types of event streams to subscribe to
        stream_types: Vec<String>,
        /// Filters to apply to the event stream
        filters: HashMap<String, String>,
    },
    /// Unsubscribe from event stream
    Unsubscribe {
        /// Types of event streams to unsubscribe from
        stream_types: Vec<String>,
    },
}

/// Response messages sent from servers
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Response {
    /// Command executed successfully
    Success {
        /// Identifier of the request that succeeded
        request_id: MessageId,
        /// Optional response data
        data: Option<serde_json::Value>,
    },
    /// Command failed
    Error {
        /// Identifier of the request that failed
        request_id: MessageId,
        /// Machine-readable error code
        error_code: String,
        /// Human-readable error message
        error_message: String,
        /// Additional error details and context
        details: HashMap<String, String>,
    },
    /// Trace data response
    TraceData {
        /// Identifier of the trace query request
        request_id: MessageId,
        /// The retrieved trace data
        trace: crate::trace::Trace,
    },
    /// Property evaluation results
    PropertyResults {
        /// Identifier of the property evaluation request
        request_id: MessageId,
        /// Results of property evaluations
        results: crate::properties::PropertyEvaluationSet,
    },
    /// Event stream data
    EventStream {
        /// Type of event stream
        stream_type: String,
        /// List of events in the stream
        events: Vec<crate::events::Event>,
    },
    /// Status update
    StatusUpdate {
        /// Component that sent the status update
        component: String,
        /// Current status of the component
        status: String,
        /// Additional status data
        data: HashMap<String, serde_json::Value>,
    },
}

/// Client-specific message types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "client_type")]
pub enum ClientMessage {
    /// Simulation client messages
    Simulation {
        #[serde(flatten)]
        /// Message payload for simulation clients
        message: SimulationClientMessage,
    },
    /// Live network client messages
    LiveNetwork {
        #[serde(flatten)]
        /// Message payload for live network clients
        message: LiveNetworkClientMessage,
    },
    /// Analysis client messages
    Analysis {
        #[serde(flatten)]
        /// Message payload for analysis clients
        message: AnalysisClientMessage,
    },
}

/// Messages specific to simulation clients
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SimulationClientMessage {
    /// Request simulation status
    GetStatus,
    /// Configure simulation parameters
    Configure {
        /// Configuration parameters for the simulation
        parameters: HashMap<String, serde_json::Value>,
    },
    /// Load a scenario
    LoadScenario {
        /// Name of the scenario to load
        scenario_name: String,
        /// Scenario configuration data
        scenario_data: serde_json::Value,
    },
}

/// Messages specific to live network clients
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum LiveNetworkClientMessage {
    /// Connect to a specific node
    ConnectToNode {
        /// Identifier of the node to connect to
        node_id: String,
        /// Connection parameters and configuration
        connection_params: HashMap<String, String>,
    },
    /// Request node status
    GetNodeStatus {
        /// Identifier of the node to query
        node_id: String,
    },
    /// Send command to node
    SendNodeCommand {
        /// Identifier of the target node
        node_id: String,
        /// Command to execute on the node
        command: String,
        /// Parameters for the command
        parameters: HashMap<String, serde_json::Value>,
    },
}

/// Messages specific to analysis clients
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AnalysisClientMessage {
    /// Start analysis session
    StartAnalysis {
        /// Identifier of the trace to analyze
        trace_id: crate::trace::TraceId,
        /// Type of analysis to perform
        analysis_type: String,
    },
    /// Query causality graph
    QueryCausality {
        /// Identifier of the trace to query
        trace_id: crate::trace::TraceId,
        /// Parameters for the causality query
        query_params: HashMap<String, String>,
    },
    /// Request property monitoring
    MonitorProperties {
        /// List of properties to monitor
        property_ids: Vec<crate::properties::PropertyId>,
        /// Configuration for property monitoring
        monitoring_config: HashMap<String, serde_json::Value>,
    },
}

impl WebSocketEnvelope {
    /// Create a new message envelope
    #[allow(clippy::disallowed_methods)]
    pub fn new(message_type: String, source: String, payload: serde_json::Value) -> Self {
        Self {
            id: Uuid::new_v4(),
            message_type,
            timestamp: 0, // Should be set by caller
            source,
            target: None,
            payload,
            metadata: HashMap::new(),
        }
    }

    /// Set the target for this message
    pub fn with_target(mut self, target: String) -> Self {
        self.target = Some(target);
        self
    }

    /// Add metadata to the message
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }

    /// Convert to JSON string
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Create from JSON string
    pub fn from_json(data: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(data)
    }
}
