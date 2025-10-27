//! Event types for real-time communication

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Unique identifier for an event
pub type EventId = Uuid;

/// Event severity level for filtering and prioritization
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventSeverity {
    /// Critical events that require immediate attention
    Critical,
    /// Warning events that indicate potential issues
    Warning,
    /// Informational events for monitoring
    Info,
    /// Debug events for development
    Debug,
}

/// Base event structure for all system events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// Unique event identifier
    pub id: EventId,
    /// Event type identifier
    pub event_type: String,
    /// Timestamp when event occurred
    pub timestamp: u64,
    /// Source that generated the event
    pub source: String,
    /// Event severity level
    pub severity: EventSeverity,
    /// Event data payload
    pub data: serde_json::Value,
    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

/// Simulation-specific events
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SimulationEvent {
    /// Simulation started
    Started {
        /// Name of the simulation scenario
        scenario_name: String,
        /// Number of participants in the simulation
        participant_count: usize,
        /// Additional simulation parameters
        parameters: HashMap<String, serde_json::Value>,
    },
    /// Simulation step completed
    StepCompleted {
        /// Unique identifier for the simulation step
        step_id: u64,
        /// Name or identifier of the actor performing the action
        actor: String,
        /// Description of the action performed
        action: String,
        /// Duration of the step in milliseconds
        duration_ms: u64,
    },
    /// Simulation paused
    Paused {
        /// Reason why the simulation was paused
        reason: String,
    },
    /// Simulation resumed
    Resumed,
    /// Simulation completed
    Completed {
        /// Total number of steps executed
        total_steps: usize,
        /// Total duration in milliseconds
        duration_ms: u64,
        /// Whether the simulation completed successfully
        success: bool,
    },
    /// Simulation error occurred
    Error {
        /// Error message describing what went wrong
        error_message: String,
        /// Additional context about the error
        context: HashMap<String, String>,
    },
}

/// Property monitoring events
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PropertyEvent {
    /// Property evaluation started
    EvaluationStarted {
        /// Unique identifier for the property being evaluated
        property_id: crate::properties::PropertyId,
        /// Human-readable name of the property
        property_name: String,
    },
    /// Property satisfied
    PropertySatisfied {
        /// Unique identifier for the property that was satisfied
        property_id: crate::properties::PropertyId,
        /// Human-readable name of the property
        property_name: String,
        /// Time taken to evaluate the property in milliseconds
        evaluation_time_ms: u64,
    },
    /// Property violated
    PropertyViolated {
        /// Unique identifier for the property that was violated
        property_id: crate::properties::PropertyId,
        /// Human-readable name of the property
        property_name: String,
        /// Detailed explanation of why the property was violated
        violation_reason: String,
        /// Additional context information about the violation
        context: HashMap<String, String>,
        /// Time taken to evaluate the property in milliseconds
        evaluation_time_ms: u64,
    },
    /// Property evaluation failed
    EvaluationFailed {
        /// Unique identifier for the property that failed evaluation
        property_id: crate::properties::PropertyId,
        /// Human-readable name of the property
        property_name: String,
        /// Error message describing what went wrong during evaluation
        error_message: String,
    },
}

/// Network communication events
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum NetworkEvent {
    /// Node connected to network
    NodeConnected {
        /// Unique identifier for the connected node
        node_id: String,
        /// Type of node that connected (e.g., "guardian", "device")
        node_type: String,
        /// List of capabilities supported by the node
        capabilities: Vec<String>,
    },
    /// Node disconnected from network
    NodeDisconnected {
        /// Unique identifier for the disconnected node
        node_id: String,
        /// Reason why the node disconnected
        reason: String,
    },
    /// Message sent between nodes
    MessageSent {
        /// Identifier of the node that sent the message
        from_node: String,
        /// Identifier of the node that received the message
        to_node: String,
        /// Type or category of the message
        message_type: String,
        /// Size of the message in bytes
        message_size: usize,
    },
    /// Message received by node
    MessageReceived {
        /// Identifier of the node that received the message
        node_id: String,
        /// Identifier of the node that sent the message
        from_node: String,
        /// Type or category of the message
        message_type: String,
        /// Time taken to process the message in milliseconds
        processing_time_ms: u64,
    },
    /// Network partition detected
    PartitionDetected {
        /// List of nodes affected by the partition
        affected_nodes: Vec<String>,
        /// Type of partition (e.g., "split-brain", "isolated")
        partition_type: String,
    },
    /// Network partition healed
    PartitionHealed {
        /// List of nodes that were affected by the partition
        affected_nodes: Vec<String>,
    },
}

/// Event stream for real-time event processing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventStream {
    /// Stream identifier
    pub stream_id: Uuid,
    /// Stream name
    pub name: String,
    /// All events in this stream
    pub events: Vec<Event>,
    /// Stream metadata
    pub metadata: HashMap<String, String>,
    /// Stream creation timestamp
    pub created_at: u64,
}

impl Event {
    /// Create a new event
    #[allow(clippy::disallowed_methods)]
    pub fn new(
        event_type: String,
        source: String,
        severity: EventSeverity,
        data: serde_json::Value,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            event_type,
            timestamp: 0, // Should be set by caller
            source,
            severity,
            data,
            metadata: HashMap::new(),
        }
    }

    /// Add metadata to the event
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }
}

impl EventStream {
    /// Create a new event stream
    #[allow(clippy::disallowed_methods)]
    pub fn new(name: String) -> Self {
        Self {
            stream_id: Uuid::new_v4(),
            name,
            events: Vec::new(),
            metadata: HashMap::new(),
            created_at: 0, // Should be set by caller
        }
    }

    /// Add an event to the stream
    pub fn add_event(&mut self, event: Event) {
        self.events.push(event);
    }

    /// Filter events by severity
    pub fn filter_by_severity(&self, severity: EventSeverity) -> Vec<&Event> {
        self.events
            .iter()
            .filter(|event| event.severity == severity)
            .collect()
    }

    /// Get events by type
    pub fn get_events_by_type(&self, event_type: &str) -> Vec<&Event> {
        self.events
            .iter()
            .filter(|event| event.event_type == event_type)
            .collect()
    }
}
