//! Pure simulation world state
//!
//! This module implements a functional approach to simulation state management
//! by separating the "what" (WorldState) from the "how" (pure tick function).
//!
//! This design provides several benefits:
//! - Pure, testable state transitions
//! - Deterministic execution
//! - Easy state snapshots and restoration
//! - Clear separation of concerns

use crate::Result;
use aura_console_types::trace::{ParticipantStatus, ParticipantType};
use aura_console_types::TraceEvent;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use uuid::Uuid;

/// Pure simulation world state containing all simulation data
///
/// This struct is a simple container for the entire state of the simulated world.
/// It contains only data and no logic, enabling pure functional state transitions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldState {
    /// Unique simulation identifier
    pub simulation_id: Uuid,
    /// Current simulation tick
    pub current_tick: u64,
    /// Current simulation time (milliseconds since epoch)
    pub current_time: u64,
    /// Random seed for deterministic execution
    pub seed: u64,
    /// All participants in the simulation
    pub participants: HashMap<String, ParticipantState>,
    /// Network fabric state
    pub network: NetworkFabric,
    /// Protocol execution state
    pub protocols: ProtocolExecutionState,
    /// Byzantine adversary state
    pub byzantine: ByzantineAdversaryState,
    /// Simulation configuration
    pub config: SimulationConfiguration,
    /// Events generated during the last tick
    pub last_tick_events: Vec<TraceEvent>,
}

/// Individual participant state in the simulation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipantState {
    /// Unique participant identifier
    pub id: String,
    /// Device identifier
    pub device_id: String,
    /// Account identifier
    pub account_id: String,
    /// Participant type
    pub participant_type: ParticipantType,
    /// Current operational status
    pub status: ParticipantStatus,
    /// Message count for monitoring
    pub message_count: u64,
    /// Current protocol sessions this participant is in
    pub active_sessions: HashMap<String, SessionParticipation>,
    /// Participant's CRDT ledger state
    pub ledger_state: LedgerState,
    /// Key shares held by this participant
    pub key_shares: KeyShareState,
    /// Message inbox for async delivery
    pub message_inbox: VecDeque<Message>,
    /// Last active timestamp
    pub last_active: u64,
}

/// Session participation information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionParticipation {
    /// Session identifier
    pub session_id: String,
    /// Protocol type
    pub protocol_type: String,
    /// Current phase within the protocol
    pub current_phase: String,
    /// Participant's role in this session
    pub role: ParticipantRole,
    /// Session-specific state data
    pub state_data: Vec<u8>,
    /// When this participant joined the session
    pub joined_at: u64,
}

/// Role of a participant in a protocol session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ParticipantRole {
    /// Coordinator initiating the protocol
    Coordinator,
    /// Regular participant
    Participant,
    /// Observer (read-only)
    Observer,
    /// Recovery coordinator
    RecoveryCoordinator,
}

/// CRDT ledger state for a participant
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerState {
    /// Current heads of the ledger
    pub heads: Vec<String>,
    /// Total number of events in the ledger
    pub event_count: u64,
    /// Serialized ledger data (Automerge document)
    pub serialized_ledger: Vec<u8>,
    /// Last update timestamp
    pub last_updated: u64,
}

/// Key share state for threshold operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyShareState {
    /// Root key share for threshold signatures
    pub root_share: Option<Vec<u8>>,
    /// Derived context keys
    pub derived_keys: HashMap<String, Vec<u8>>,
    /// Key generation commitments
    pub commitments: HashMap<String, Vec<u8>>,
    /// Share verification data
    pub verification_data: Vec<u8>,
}

/// Unified message type for all message states (pending, in-flight, session)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Message identifier
    pub message_id: String,
    /// Sender participant ID
    pub from: String,
    /// Recipient participant ID (optional for broadcasts)
    pub to: Option<String>,
    /// Message type
    pub message_type: String,
    /// Message payload
    pub payload: Vec<u8>,
    /// When message was sent
    pub sent_at: u64,
    /// Expected delivery time (for pending messages)
    pub deliver_at: Option<u64>,
    /// Whether message should be dropped (for network simulation)
    pub will_drop: bool,
}

/// Network fabric simulation state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkFabric {
    /// Active network partitions
    pub partitions: Vec<NetworkPartition>,
    /// Message delivery delays between participants
    pub message_delays: HashMap<(String, String), u64>,
    /// Pending messages in flight
    pub in_flight_messages: VecDeque<Message>,
    /// Network failure configuration
    pub failure_config: NetworkFailureConfig,
    /// Connection topology
    pub connections: HashMap<String, Vec<String>>,
}

/// Network partition isolating participants
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkPartition {
    /// Partition identifier
    pub id: String,
    /// Participants isolated in this partition
    pub participants: Vec<String>,
    /// When the partition started
    pub started_at: u64,
    /// Expected duration (None = permanent)
    pub duration: Option<u64>,
}

/// Network failure simulation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkFailureConfig {
    /// Probability of dropping messages (0.0 to 1.0)
    pub drop_rate: f64,
    /// Base latency range (min_ms, max_ms)
    pub latency_range: (u64, u64),
    /// Additional jitter (max_ms)
    pub jitter_ms: u64,
    /// Bandwidth limits per participant (bytes per tick)
    pub bandwidth_limits: HashMap<String, u64>,
}

/// Protocol execution state across all sessions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolExecutionState {
    /// Currently active protocol sessions
    pub active_sessions: HashMap<String, ProtocolSession>,
    /// Completed protocol sessions
    pub completed_sessions: Vec<CompletedSession>,
    /// Queued protocols waiting to start
    pub execution_queue: VecDeque<QueuedProtocol>,
    /// Global protocol state (shared across sessions)
    pub global_state: HashMap<String, Vec<u8>>,
}

/// Active protocol session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolSession {
    /// Session identifier
    pub session_id: String,
    /// Protocol type being executed
    pub protocol_type: String,
    /// Current phase of the protocol
    pub current_phase: String,
    /// Participants in this session
    pub participants: Vec<String>,
    /// Session coordinator
    pub coordinator: String,
    /// Session start timestamp
    pub started_at: u64,
    /// Expected completion time
    pub expected_completion: Option<u64>,
    /// Current session status
    pub status: SessionStatus,
    /// Protocol-specific state data
    pub state_data: HashMap<String, Vec<u8>>,
    /// Messages exchanged in this session
    pub session_messages: Vec<Message>,
}

/// Status of a protocol session
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SessionStatus {
    /// Session is initializing
    Initializing,
    /// Session is actively running
    Active,
    /// Session is waiting for responses
    Waiting,
    /// Session completed successfully
    Completed,
    /// Session failed with error
    Failed {
        /// Reason for failure
        reason: String,
    },
    /// Session timed out
    TimedOut,
    /// Session was cancelled
    Cancelled,
}

/// Completed protocol session record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletedSession {
    /// Session information
    pub session: ProtocolSession,
    /// Final result
    pub result: SessionResult,
    /// Completion timestamp
    pub completed_at: u64,
}

/// Result of a completed protocol session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionResult {
    /// Session completed successfully with result data
    Success {
        /// Result data from the session
        result_data: Vec<u8>,
    },
    /// Session failed with error
    Failure {
        /// Error message
        error: String,
    },
    /// Session timed out
    Timeout,
}

/// Queued protocol waiting for execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedProtocol {
    /// Protocol type to execute
    pub protocol_type: String,
    /// Participants for this protocol
    pub participants: Vec<String>,
    /// Protocol parameters
    pub parameters: HashMap<String, String>,
    /// When protocol should start
    pub scheduled_time: u64,
    /// Priority for execution ordering
    pub priority: u32,
}

/// Byzantine adversary simulation state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ByzantineAdversaryState {
    /// Participants behaving byzantinely
    pub byzantine_participants: Vec<String>,
    /// Active attack strategies
    pub active_strategies: HashMap<String, ByzantineStrategy>,
    /// Strategy parameters
    pub strategy_parameters: HashMap<String, HashMap<String, String>>,
    /// Attack targets
    pub targets: HashMap<String, Vec<String>>,
}

/// Byzantine attack strategy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ByzantineStrategy {
    /// Drop all messages
    DropAllMessages,
    /// Drop messages selectively
    SelectiveMessageDrop {
        /// Message types to drop
        target_types: Vec<String>,
    },
    /// Send invalid signatures
    InvalidSignatures,
    /// Delay messages beyond timeout
    DelayMessages {
        /// Delay duration in milliseconds
        delay_ms: u64,
    },
    /// Send conflicting protocol messages
    ConflictingMessages,
    /// Refuse to participate in protocols
    RefuseParticipation,
    /// Custom strategy with parameters
    Custom {
        /// Name of the custom strategy
        name: String,
        /// Implementation details
        implementation: String,
        /// Strategy parameters
        parameters: HashMap<String, String>,
    },
}

/// Simulation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationConfiguration {
    /// Maximum ticks to run
    pub max_ticks: u64,
    /// Maximum simulation time (milliseconds)
    pub max_time: u64,
    /// Time advancement per tick (milliseconds)
    pub tick_duration_ms: u64,
    /// Scenario name if loaded from scenario
    pub scenario_name: Option<String>,
    /// Random number generator state
    pub rng_state: Vec<u8>,
    /// Simulation properties to check
    pub properties: Vec<String>,
}

impl WorldState {
    /// Create a new empty world state
    pub fn new(seed: u64) -> Self {
        Self {
            simulation_id: Uuid::new_v4(),
            current_tick: 0,
            current_time: 0,
            seed,
            participants: HashMap::new(),
            network: NetworkFabric {
                partitions: Vec::new(),
                message_delays: HashMap::new(),
                in_flight_messages: VecDeque::new(),
                failure_config: NetworkFailureConfig {
                    drop_rate: 0.0,
                    latency_range: (10, 100),
                    jitter_ms: 10,
                    bandwidth_limits: HashMap::new(),
                },
                connections: HashMap::new(),
            },
            protocols: ProtocolExecutionState {
                active_sessions: HashMap::new(),
                completed_sessions: Vec::new(),
                execution_queue: VecDeque::new(),
                global_state: HashMap::new(),
            },
            byzantine: ByzantineAdversaryState {
                byzantine_participants: Vec::new(),
                active_strategies: HashMap::new(),
                strategy_parameters: HashMap::new(),
                targets: HashMap::new(),
            },
            config: SimulationConfiguration {
                max_ticks: 10000,
                max_time: 60000,       // 60 seconds
                tick_duration_ms: 100, // 100ms per tick
                scenario_name: None,
                rng_state: Vec::new(),
                properties: Vec::new(),
            },
            last_tick_events: Vec::new(),
        }
    }

    /// Add a participant to the world state
    pub fn add_participant(
        &mut self,
        id: String,
        device_id: String,
        account_id: String,
    ) -> Result<()> {
        let participant = ParticipantState {
            id: id.clone(),
            device_id,
            account_id,
            participant_type: ParticipantType::Honest,
            status: ParticipantStatus::Online,
            message_count: 0,
            active_sessions: HashMap::new(),
            ledger_state: LedgerState {
                heads: vec!["genesis".to_string()],
                event_count: 0,
                serialized_ledger: Vec::new(),
                last_updated: self.current_time,
            },
            key_shares: KeyShareState {
                root_share: None,
                derived_keys: HashMap::new(),
                commitments: HashMap::new(),
                verification_data: Vec::new(),
            },
            message_inbox: VecDeque::new(),
            last_active: self.current_time,
        };

        self.participants.insert(id.clone(), participant);
        self.network.connections.insert(id, Vec::new());
        Ok(())
    }

    /// Get participant state by ID
    pub fn get_participant(&self, participant_id: &str) -> Option<&ParticipantState> {
        self.participants.get(participant_id)
    }

    /// Get mutable participant state by ID
    pub fn get_participant_mut(&mut self, participant_id: &str) -> Option<&mut ParticipantState> {
        self.participants.get_mut(participant_id)
    }

    /// Check if simulation should continue running
    pub fn should_continue(&self) -> bool {
        self.current_tick < self.config.max_ticks
            && self.current_time < self.config.max_time
            && !self.is_idle()
    }

    /// Check if simulation is idle (no active work)
    pub fn is_idle(&self) -> bool {
        self.protocols.active_sessions.is_empty()
            && self.protocols.execution_queue.is_empty()
            && self.network.in_flight_messages.is_empty()
    }

    /// Get the number of active protocol sessions
    pub fn active_session_count(&self) -> usize {
        self.protocols.active_sessions.len()
    }

    /// Get the number of queued protocols
    pub fn queued_protocol_count(&self) -> usize {
        self.protocols.execution_queue.len()
    }

    /// Create a complete snapshot of the world state
    pub fn snapshot(&self) -> WorldStateSnapshot {
        WorldStateSnapshot {
            tick: self.current_tick,
            time: self.current_time,
            participant_count: self.participants.len(),
            active_sessions: self.protocols.active_sessions.len(),
            in_flight_messages: self.network.in_flight_messages.len(),
            state_hash: self.compute_state_hash(),
        }
    }

    /// Compute a hash of the current state for verification
    fn compute_state_hash(&self) -> String {
        // Simple hash based on key state indicators
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        self.current_tick.hash(&mut hasher);
        self.current_time.hash(&mut hasher);
        self.participants.len().hash(&mut hasher);
        self.protocols.active_sessions.len().hash(&mut hasher);

        format!("{:x}", hasher.finish())
    }
}

/// Lightweight snapshot of world state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldStateSnapshot {
    /// Current simulation tick
    pub tick: u64,
    /// Current simulation time in milliseconds
    pub time: u64,
    /// Number of participants
    pub participant_count: usize,
    /// Number of active protocol sessions
    pub active_sessions: usize,
    /// Number of messages in flight
    pub in_flight_messages: usize,
    /// Hash of the current state
    pub state_hash: String,
}

impl Default for WorldState {
    fn default() -> Self {
        Self::new(42) // Default seed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_world_state_creation() {
        let world = WorldState::new(42);
        assert_eq!(world.seed, 42);
        assert_eq!(world.current_tick, 0);
        assert_eq!(world.current_time, 0);
        assert!(world.participants.is_empty());
        assert!(world.is_idle());
    }

    #[test]
    fn test_participant_management() {
        let mut world = WorldState::new(42);

        world
            .add_participant(
                "alice".to_string(),
                "device_alice".to_string(),
                "account_1".to_string(),
            )
            .unwrap();

        assert_eq!(world.participants.len(), 1);
        assert!(world.get_participant("alice").is_some());

        let alice = world.get_participant("alice").unwrap();
        assert_eq!(alice.id, "alice");
        assert_eq!(alice.device_id, "device_alice");
        assert_eq!(alice.account_id, "account_1");
        assert_eq!(alice.participant_type, ParticipantType::Honest);
        assert_eq!(alice.status, ParticipantStatus::Online);
    }

    #[test]
    fn test_state_snapshot() {
        let mut world = WorldState::new(42);
        world
            .add_participant(
                "alice".to_string(),
                "device_alice".to_string(),
                "account_1".to_string(),
            )
            .unwrap();

        let snapshot = world.snapshot();
        assert_eq!(snapshot.tick, 0);
        assert_eq!(snapshot.time, 0);
        assert_eq!(snapshot.participant_count, 1);
        assert_eq!(snapshot.active_sessions, 0);
        assert_eq!(snapshot.in_flight_messages, 0);
        assert!(!snapshot.state_hash.is_empty());
    }

    #[test]
    fn test_idle_detection() {
        let world = WorldState::new(42);
        assert!(world.is_idle());
        // should_continue returns false when idle (no pending work)
        assert!(!world.should_continue());

        // Test max tick boundary
        let mut world_at_limit = world;
        world_at_limit.current_tick = world_at_limit.config.max_ticks;
        assert!(!world_at_limit.should_continue());
    }
}
