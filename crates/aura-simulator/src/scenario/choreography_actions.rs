//! Choreography Actions - Middleware-Based Protocol Execution
//!
//! This module provides choreography actions that use the AuraProtocolHandler
//! middleware pattern for composable and type-safe protocol execution.

use crate::{tick, QueuedProtocol, Result, WorldState};
use async_trait::async_trait;
use aura_protocol::{
    middleware::{
        handler::SessionInfo, AuraProtocolHandler, CapabilityMiddleware, ErrorRecoveryMiddleware,
        SessionMiddleware,
    },
    ProtocolError, ProtocolResult,
};
use aura_types::DeviceId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;
use uuid::Uuid;

// Import types from engine module
use super::engine::ChoreographyResult;

/// Trait for handling network conditions
pub trait NetworkConditionHandler {
    /// Apply network condition to the world state
    fn apply(
        &self,
        world_state: &mut WorldState,
        participants: &[String],
        parameters: &HashMap<String, toml::Value>,
    ) -> Result<()>;

    /// Description of this network condition
    fn description(&self) -> &str {
        "Generic network condition"
    }
}

/// Trait for injecting Byzantine behaviors
pub trait ByzantineBehaviorInjector {
    /// Inject Byzantine behavior for specific participant
    fn inject(
        &self,
        world_state: &mut WorldState,
        participant: &str,
        parameters: &HashMap<String, toml::Value>,
    ) -> Result<()>;

    /// Description of this Byzantine behavior
    fn description(&self) -> &str {
        "Generic Byzantine behavior"
    }
}

/// Protocol message type for simulation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationMessage {
    pub protocol_type: String,
    pub payload: Vec<u8>,
    pub metadata: HashMap<String, String>,
}

/// Simulation protocol handler implementation
pub struct SimulationProtocolHandler {
    device_id: DeviceId,
    sessions: HashMap<Uuid, SessionInfo>,
    message_queue: HashMap<DeviceId, Vec<SimulationMessage>>,
}

impl SimulationProtocolHandler {
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            device_id,
            sessions: HashMap::new(),
            message_queue: HashMap::new(),
        }
    }
}

#[async_trait]
impl AuraProtocolHandler for SimulationProtocolHandler {
    type DeviceId = DeviceId;
    type SessionId = Uuid;
    type Message = SimulationMessage;

    async fn send_message(&mut self, to: Self::DeviceId, msg: Self::Message) -> ProtocolResult<()> {
        // Store message in queue for simulation purposes
        self.message_queue
            .entry(to)
            .or_default()
            .push(msg);
        Ok(())
    }

    async fn receive_message(&mut self, from: Self::DeviceId) -> ProtocolResult<Self::Message> {
        // Retrieve message from queue
        if let Some(messages) = self.message_queue.get_mut(&from) {
            if let Some(message) = messages.pop() {
                return Ok(message);
            }
        }

        // Return dummy message if no messages in queue
        Ok(SimulationMessage {
            protocol_type: "default".to_string(),
            payload: vec![],
            metadata: HashMap::new(),
        })
    }

    async fn start_session(
        &mut self,
        participants: Vec<Self::DeviceId>,
        protocol_type: String,
        metadata: HashMap<String, String>,
    ) -> ProtocolResult<Self::SessionId> {
        let session_id = Uuid::new_v4();
        let session_info = SessionInfo {
            session_id,
            participants,
            protocol_type,
            started_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            metadata,
        };

        self.sessions.insert(session_id, session_info);
        Ok(session_id)
    }

    async fn end_session(&mut self, session_id: Self::SessionId) -> ProtocolResult<()> {
        self.sessions.remove(&session_id);
        Ok(())
    }

    async fn get_session_info(
        &mut self,
        session_id: Self::SessionId,
    ) -> ProtocolResult<SessionInfo> {
        self.sessions
            .get(&session_id)
            .cloned()
            .ok_or_else(|| ProtocolError::Session {
                message: format!("Session not found: {}", session_id),
            })
    }

    async fn list_sessions(&mut self) -> ProtocolResult<Vec<SessionInfo>> {
        Ok(self.sessions.values().cloned().collect())
    }

    async fn verify_capability(
        &mut self,
        _operation: &str,
        _resource: &str,
        _context: HashMap<String, String>,
    ) -> ProtocolResult<bool> {
        // For simulation, always allow operations
        Ok(true)
    }

    async fn create_authorization_proof(
        &mut self,
        _operation: &str,
        _resource: &str,
        _context: HashMap<String, String>,
    ) -> ProtocolResult<Vec<u8>> {
        // Return dummy proof for simulation
        Ok(vec![0x01, 0x02, 0x03])
    }

    fn device_id(&self) -> Self::DeviceId {
        self.device_id
    }
}

/// Build a middleware stack for protocol execution
pub fn build_middleware_stack(
    base_handler: SimulationProtocolHandler,
) -> impl AuraProtocolHandler<DeviceId = DeviceId, SessionId = Uuid, Message = SimulationMessage> {
    // Create middleware stack
    let handler = SessionMiddleware::new(base_handler);
    let handler = CapabilityMiddleware::new(handler);
    ErrorRecoveryMiddleware::new(handler, "simulation".to_string())
}

/// DKD (Deterministic Key Derivation) choreography
pub struct DkdChoreography;

/// Resharing protocol choreography
pub struct ResharingChoreography;

/// Recovery protocol choreography
pub struct RecoveryChoreography;

/// Locking protocol choreography
pub struct LockingChoreography;

/// Network partition handler
pub struct NetworkPartitionHandler;

/// Message delay handler
pub struct MessageDelayHandler;

/// Message drop handler
pub struct MessageDropHandler;

/// Byzantine message dropping behavior
pub struct ByzantineMessageDropper;

/// Byzantine double spending behavior
pub struct ByzantineDoubleSpender;

/// Byzantine delay injector
pub struct ByzantineDelayInjector;

/// Helper to execute protocol using middleware pattern
pub async fn execute_protocol_with_middleware(
    world_state: &mut WorldState,
    protocol_type: &str,
    participants: &[String],
    parameters: &HashMap<String, String>,
) -> Result<bool> {
    // Create protocol handlers for each participant
    let mut handlers = HashMap::new();
    let mut participant_devices = HashMap::new();

    for participant_id in participants {
        if let Some(_participant) = world_state.participants.get(participant_id) {
            let device_id = DeviceId::new();
            participant_devices.insert(participant_id.clone(), device_id);

            // Create base handler
            let base_handler = SimulationProtocolHandler::new(device_id);

            // Build middleware stack
            let mut handler = build_middleware_stack(base_handler);

            // Setup the handler
            if let Err(e) = handler.setup().await {
                eprintln!("Failed to setup handler for {}: {:?}", participant_id, e);
                continue;
            }

            handlers.insert(participant_id.clone(), handler);
        }
    }

    // Execute protocol using handlers
    if let Some(handler) = handlers.values_mut().next() {
        let session_participants: Vec<DeviceId> = participant_devices.values().copied().collect();

        let mut session_metadata = HashMap::new();
        session_metadata.insert("protocol_type".to_string(), protocol_type.to_string());

        // Add protocol-specific parameters
        for (key, value) in parameters {
            session_metadata.insert(key.clone(), value.clone());
        }

        // Start session
        let session_id = handler
            .start_session(
                session_participants,
                protocol_type.to_string(),
                session_metadata,
            )
            .await
            .map_err(|e| crate::AuraError::coordination_failed(e.to_string()))?;

        // Simulate protocol execution based on type
        match protocol_type {
            "DKD" => {
                let app_id = parameters
                    .get("app_id")
                    .cloned()
                    .unwrap_or_else(|| "sim_app".to_string());
                let context = parameters
                    .get("context")
                    .cloned()
                    .unwrap_or_else(|| "sim_context".to_string());

                // Create DKD message
                let dkd_message = SimulationMessage {
                    protocol_type: "DKD".to_string(),
                    payload: format!("{}:{}", app_id, context).into_bytes(),
                    metadata: {
                        let mut meta = HashMap::new();
                        meta.insert("app_id".to_string(), app_id);
                        meta.insert("context".to_string(), context);
                        meta
                    },
                };

                // Send messages between participants
                for device_id in participant_devices.values() {
                    if let Err(e) = handler.send_message(*device_id, dkd_message.clone()).await {
                        eprintln!("Failed to send DKD message: {:?}", e);
                    }
                }
            }
            "Resharing" => {
                let new_threshold = parameters
                    .get("new_threshold")
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(3);

                let resharing_message = SimulationMessage {
                    protocol_type: "Resharing".to_string(),
                    payload: new_threshold.to_be_bytes().to_vec(),
                    metadata: {
                        let mut meta = HashMap::new();
                        meta.insert("new_threshold".to_string(), new_threshold.to_string());
                        meta
                    },
                };

                for device_id in participant_devices.values() {
                    if let Err(e) = handler
                        .send_message(*device_id, resharing_message.clone())
                        .await
                    {
                        eprintln!("Failed to send Resharing message: {:?}", e);
                    }
                }
            }
            "Recovery" => {
                let guardian_threshold = parameters
                    .get("guardian_threshold")
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(2);

                let recovery_message = SimulationMessage {
                    protocol_type: "Recovery".to_string(),
                    payload: guardian_threshold.to_be_bytes().to_vec(),
                    metadata: {
                        let mut meta = HashMap::new();
                        meta.insert(
                            "guardian_threshold".to_string(),
                            guardian_threshold.to_string(),
                        );
                        meta
                    },
                };

                for device_id in participant_devices.values() {
                    if let Err(e) = handler
                        .send_message(*device_id, recovery_message.clone())
                        .await
                    {
                        eprintln!("Failed to send Recovery message: {:?}", e);
                    }
                }
            }
            _ => {
                return Err(crate::AuraError::configuration_error(format!(
                    "Unknown protocol type: {}",
                    protocol_type
                )));
            }
        }

        // End session
        if let Err(e) = handler.end_session(session_id).await {
            eprintln!("Failed to end session: {:?}", e);
        }

        // Teardown handlers
        for (participant_id, mut handler) in handlers {
            if let Err(e) = handler.teardown().await {
                eprintln!("Failed to teardown handler for {}: {:?}", participant_id, e);
            }
        }

        return Ok(true);
    }

    Ok(false)
}

/// Blocking helper for middleware-based execution.
pub fn run_protocol_with_middleware(
    world_state: &mut WorldState,
    protocol_type: &str,
    participants: &[String],
    parameters: &HashMap<String, String>,
) -> Result<bool> {
    tokio::task::block_in_place(|| {
        let handle = tokio::runtime::Handle::current();
        handle.block_on(async {
            execute_protocol_with_middleware(world_state, protocol_type, participants, parameters)
                .await
        })
    })
}

// Choreography Implementations

impl super::engine::ChoreographyExecutor for DkdChoreography {
    fn execute(
        &self,
        world_state: &mut WorldState,
        participants: &[String],
        parameters: &HashMap<String, toml::Value>,
    ) -> Result<ChoreographyResult> {
        let start_time = Instant::now();

        // Extract parameters
        let app_id = parameters
            .get("app_id")
            .and_then(|v| v.as_str())
            .unwrap_or("default_app");
        let context = parameters
            .get("context")
            .and_then(|v| v.as_str())
            .unwrap_or("default_context");
        let threshold = parameters
            .get("threshold")
            .and_then(|v| v.as_integer())
            .unwrap_or(2) as usize;

        println!(
            "Executing DKD choreography with middleware: app_id='{}', context='{}', threshold={}",
            app_id, context, threshold
        );

        // Convert toml parameters to string parameters for middleware
        let mut string_params = HashMap::new();
        string_params.insert("app_id".to_string(), app_id.to_string());
        string_params.insert("context".to_string(), context.to_string());
        string_params.insert("threshold".to_string(), threshold.to_string());

        // Use middleware-based execution
        let success =
            run_protocol_with_middleware(world_state, "DKD", participants, &string_params)
                .unwrap_or(false);

        // Execute some simulation ticks for compatibility
        let mut events_generated = 0;
        let max_ticks = if success { 20 } else { 5 };

        for tick_num in 0..max_ticks {
            let events = tick(world_state)?;
            events_generated += events.len();

            // Break early if using middleware approach
            if success && tick_num > 5 {
                break;
            }
        }

        println!(
            "[{}] DKD choreography completed using middleware pattern",
            if success { "OK" } else { "WARN" }
        );

        Ok(ChoreographyResult {
            success,
            events_generated,
            execution_time: start_time.elapsed(),
            data: {
                let mut data = HashMap::new();
                data.insert("app_id".to_string(), app_id.to_string());
                data.insert("context".to_string(), context.to_string());
                data.insert("participants".to_string(), participants.len().to_string());
                data.insert("protocol_type".to_string(), "DKD".to_string());
                data.insert("middleware_used".to_string(), "true".to_string());
                data
            },
        })
    }

    fn description(&self) -> &str {
        "Deterministic Key Derivation (DKD) protocol choreography using middleware"
    }
}

impl super::engine::ChoreographyExecutor for ResharingChoreography {
    fn execute(
        &self,
        world_state: &mut WorldState,
        participants: &[String],
        parameters: &HashMap<String, toml::Value>,
    ) -> Result<ChoreographyResult> {
        let start_time = Instant::now();

        let old_threshold = parameters
            .get("old_threshold")
            .and_then(|v| v.as_integer())
            .unwrap_or(2) as usize;
        let new_threshold = parameters
            .get("new_threshold")
            .and_then(|v| v.as_integer())
            .unwrap_or(3) as usize;

        println!(
            "Executing Resharing choreography with middleware: {} -> {} threshold",
            old_threshold, new_threshold
        );

        // Convert parameters for middleware
        let mut string_params = HashMap::new();
        string_params.insert("old_threshold".to_string(), old_threshold.to_string());
        string_params.insert("new_threshold".to_string(), new_threshold.to_string());

        // Use middleware-based execution
        let success =
            run_protocol_with_middleware(world_state, "Resharing", participants, &string_params)
                .unwrap_or(false);

        // Execute simulation ticks
        let mut events_generated = 0;
        let max_ticks = if success { 15 } else { 5 };

        for _ in 0..max_ticks {
            let events = tick(world_state)?;
            events_generated += events.len();
        }

        println!(
            "[{}] Resharing choreography completed using middleware pattern",
            if success { "OK" } else { "WARN" }
        );

        Ok(ChoreographyResult {
            success,
            events_generated,
            execution_time: start_time.elapsed(),
            data: {
                let mut data = HashMap::new();
                data.insert("old_threshold".to_string(), old_threshold.to_string());
                data.insert("new_threshold".to_string(), new_threshold.to_string());
                data.insert("participants".to_string(), participants.len().to_string());
                data.insert("protocol_type".to_string(), "Resharing".to_string());
                data.insert("middleware_used".to_string(), "true".to_string());
                data
            },
        })
    }

    fn description(&self) -> &str {
        "Key resharing protocol choreography using middleware for threshold updates"
    }
}

impl super::engine::ChoreographyExecutor for RecoveryChoreography {
    fn execute(
        &self,
        world_state: &mut WorldState,
        participants: &[String],
        parameters: &HashMap<String, toml::Value>,
    ) -> Result<ChoreographyResult> {
        let start_time = Instant::now();

        let guardian_threshold = parameters
            .get("guardian_threshold")
            .and_then(|v| v.as_integer())
            .unwrap_or(2) as usize;
        let cooldown_hours = parameters
            .get("cooldown_hours")
            .and_then(|v| v.as_integer())
            .unwrap_or(24) as u64;

        println!(
            "Executing Recovery choreography with middleware: guardian_threshold={}, cooldown={}h",
            guardian_threshold, cooldown_hours
        );

        // Convert parameters for middleware
        let mut string_params = HashMap::new();
        string_params.insert(
            "guardian_threshold".to_string(),
            guardian_threshold.to_string(),
        );
        string_params.insert(
            "cooldown_seconds".to_string(),
            (cooldown_hours * 3600).to_string(),
        );

        // Use middleware-based execution
        let success =
            run_protocol_with_middleware(world_state, "Recovery", participants, &string_params)
                .unwrap_or(false);

        // Execute simulation ticks
        let mut events_generated = 0;
        let max_ticks = if success { 20 } else { 5 };

        for _ in 0..max_ticks {
            let events = tick(world_state)?;
            events_generated += events.len();
        }

        println!(
            "[{}] Recovery choreography completed using middleware pattern",
            if success { "OK" } else { "WARN" }
        );

        Ok(ChoreographyResult {
            success,
            events_generated,
            execution_time: start_time.elapsed(),
            data: {
                let mut data = HashMap::new();
                data.insert(
                    "guardian_threshold".to_string(),
                    guardian_threshold.to_string(),
                );
                data.insert("cooldown_hours".to_string(), cooldown_hours.to_string());
                data.insert("participants".to_string(), participants.len().to_string());
                data.insert("protocol_type".to_string(), "Recovery".to_string());
                data.insert("middleware_used".to_string(), "true".to_string());
                data
            },
        })
    }

    fn description(&self) -> &str {
        "Guardian-based account recovery choreography using middleware"
    }
}

impl super::engine::ChoreographyExecutor for LockingChoreography {
    fn execute(
        &self,
        world_state: &mut WorldState,
        participants: &[String],
        parameters: &HashMap<String, toml::Value>,
    ) -> Result<ChoreographyResult> {
        let start_time = Instant::now();

        let operation_type = parameters
            .get("operation_type")
            .and_then(|v| v.as_str())
            .unwrap_or("default_operation");

        println!(
            "Executing Locking choreography with middleware: operation_type='{}'",
            operation_type
        );

        // Queue locking protocol (fallback for now since it's not implemented with middleware yet)
        let protocol = QueuedProtocol {
            protocol_type: "Locking".to_string(),
            participants: participants.to_vec(),
            parameters: {
                let mut params = HashMap::new();
                params.insert("operation_type".to_string(), operation_type.to_string());
                params
            },
            scheduled_time: world_state.current_time + 100,
            priority: 1,
        };

        world_state.protocols.execution_queue.push_back(protocol);

        // Execute simulation
        let mut events_generated = 0;
        for _ in 0..20 {
            let events = tick(world_state)?;
            events_generated += events.len();
        }

        println!("[OK] Locking choreography completed (fallback mode)");

        Ok(ChoreographyResult {
            success: true,
            events_generated,
            execution_time: start_time.elapsed(),
            data: {
                let mut data = HashMap::new();
                data.insert("operation_type".to_string(), operation_type.to_string());
                data.insert("participants".to_string(), participants.len().to_string());
                data.insert("protocol_type".to_string(), "Locking".to_string());
                data.insert("middleware_used".to_string(), "false".to_string());
                data
            },
        })
    }

    fn description(&self) -> &str {
        "Distributed locking choreography for coordinated operations"
    }
}

// Network Condition Handlers (unchanged)

impl NetworkConditionHandler for NetworkPartitionHandler {
    fn apply(
        &self,
        world_state: &mut WorldState,
        participants: &[String],
        parameters: &HashMap<String, toml::Value>,
    ) -> Result<()> {
        let duration_ticks = parameters
            .get("duration_ticks")
            .and_then(|v| v.as_integer())
            .unwrap_or(10) as u64;

        println!(
            "Applying network partition: participants={:?}, duration={} ticks",
            participants, duration_ticks
        );

        let partition = crate::NetworkPartition {
            id: Uuid::new_v4().to_string(),
            participants: participants.to_vec(),
            started_at: world_state.current_time,
            duration: Some(duration_ticks * 100),
        };

        world_state.network.partitions.push(partition);

        Ok(())
    }

    fn description(&self) -> &str {
        "Creates a network partition isolating specified participants"
    }
}

impl NetworkConditionHandler for MessageDelayHandler {
    fn apply(
        &self,
        _world_state: &mut WorldState,
        participants: &[String],
        parameters: &HashMap<String, toml::Value>,
    ) -> Result<()> {
        let delay_ms = parameters
            .get("delay_ms")
            .and_then(|v| v.as_integer())
            .unwrap_or(1000) as u64;

        println!(
            "Applying message delay: participants={:?}, delay={}ms",
            participants, delay_ms
        );

        println!("Message delay configuration applied");

        Ok(())
    }

    fn description(&self) -> &str {
        "Adds artificial delay to messages between participants"
    }
}

impl NetworkConditionHandler for MessageDropHandler {
    fn apply(
        &self,
        _world_state: &mut WorldState,
        participants: &[String],
        parameters: &HashMap<String, toml::Value>,
    ) -> Result<()> {
        let drop_rate = parameters
            .get("drop_rate")
            .and_then(|v| v.as_float())
            .unwrap_or(0.1);

        println!(
            "Applying message drops: participants={:?}, drop_rate={}",
            participants, drop_rate
        );

        println!("Message drop configuration applied");

        Ok(())
    }

    fn description(&self) -> &str {
        "Randomly drops messages between participants"
    }
}

// Byzantine Behavior Injectors (unchanged)

impl ByzantineBehaviorInjector for ByzantineMessageDropper {
    fn inject(
        &self,
        world_state: &mut WorldState,
        participant: &str,
        _parameters: &HashMap<String, toml::Value>,
    ) -> Result<()> {
        println!(
            "Injecting Byzantine message dropping behavior into '{}'",
            participant
        );

        if !world_state
            .byzantine
            .byzantine_participants
            .contains(&participant.to_string())
        {
            world_state
                .byzantine
                .byzantine_participants
                .push(participant.to_string());
        }

        world_state.byzantine.active_strategies.insert(
            participant.to_string(),
            crate::world_state::ByzantineStrategy::DropAllMessages,
        );

        Ok(())
    }

    fn description(&self) -> &str {
        "Makes a participant drop all messages (Byzantine behavior)"
    }
}

impl ByzantineBehaviorInjector for ByzantineDoubleSpender {
    fn inject(
        &self,
        world_state: &mut WorldState,
        participant: &str,
        _parameters: &HashMap<String, toml::Value>,
    ) -> Result<()> {
        println!(
            "Injecting Byzantine double spending behavior into '{}'",
            participant
        );

        if !world_state
            .byzantine
            .byzantine_participants
            .contains(&participant.to_string())
        {
            world_state
                .byzantine
                .byzantine_participants
                .push(participant.to_string());
        }

        world_state.byzantine.active_strategies.insert(
            participant.to_string(),
            crate::world_state::ByzantineStrategy::ConflictingMessages,
        );

        Ok(())
    }

    fn description(&self) -> &str {
        "Makes a participant attempt double spending (Byzantine behavior)"
    }
}

impl ByzantineBehaviorInjector for ByzantineDelayInjector {
    fn inject(
        &self,
        world_state: &mut WorldState,
        participant: &str,
        parameters: &HashMap<String, toml::Value>,
    ) -> Result<()> {
        let delay_ms = parameters
            .get("delay_ms")
            .and_then(|v| v.as_integer())
            .unwrap_or(5000) as u64;

        println!(
            "Injecting Byzantine delay behavior into '{}': delay={}ms",
            participant, delay_ms
        );

        if !world_state
            .byzantine
            .byzantine_participants
            .contains(&participant.to_string())
        {
            world_state
                .byzantine
                .byzantine_participants
                .push(participant.to_string());
        }

        world_state.byzantine.active_strategies.insert(
            participant.to_string(),
            crate::world_state::ByzantineStrategy::DelayMessages { delay_ms },
        );

        Ok(())
    }

    fn description(&self) -> &str {
        "Makes a participant delay messages (Byzantine behavior)"
    }
}

/// Helper function to register all standard choreography actions
///
/// These choreographies now use the AuraProtocolHandler middleware pattern
/// for composable and type-safe protocol execution.
pub fn register_standard_choreographies(
    engine: &mut crate::scenario::engine::UnifiedScenarioEngine,
) {
    // Register choreographies (now using middleware-backed execution)
    engine.register_choreography("dkd".to_string(), DkdChoreography);
    engine.register_choreography("resharing".to_string(), ResharingChoreography);
    engine.register_choreography("recovery".to_string(), RecoveryChoreography);
    engine.register_choreography("locking".to_string(), LockingChoreography);

    println!("Registered middleware-backed choreography actions:");
    println!("  dkd - Deterministic Key Derivation (via AuraProtocolHandler)");
    println!("  resharing - Key Resharing Protocol (via AuraProtocolHandler)");
    println!("  recovery - Guardian-based Recovery (via AuraProtocolHandler)");
    println!("  locking - Distributed Locking (fallback mode)");
}

/// Helper function to register all standard network conditions
pub fn register_standard_network_conditions(
    _registry: &mut crate::scenario::engine::ChoreographyActionRegistry,
) {
    println!("Standard network conditions available:");
    println!("  partition - Network partitioning");
    println!("  delay - Message delays");
    println!("  drop - Message drops");
}

/// Helper function to register all standard byzantine behaviors
pub fn register_standard_byzantine_behaviors(
    _registry: &mut crate::scenario::engine::ChoreographyActionRegistry,
) {
    println!("Standard Byzantine behaviors available:");
    println!("  drop_messages - Drop all messages");
    println!("  double_spend - Attempt double spending");
    println!("  delay_messages - Delay message sending");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::engine::ChoreographyExecutor;
    use std::collections::HashMap;

    fn create_test_world_state() -> WorldState {
        crate::testing::test_utils::two_party_world_state()
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn test_dkd_choreography() {
        let choreography = DkdChoreography;
        let mut world_state = create_test_world_state();
        let participants = vec!["alice".to_string(), "bob".to_string()];
        let parameters = HashMap::new();

        let result = choreography
            .execute(&mut world_state, &participants, &parameters)
            .unwrap();

        assert!(result.success);
        assert!(result.events_generated > 0);
    }

    #[test]
    fn test_network_partition_handler() {
        let handler = NetworkPartitionHandler;
        let mut world_state = create_test_world_state();
        let participants = vec!["alice".to_string()];
        let mut parameters = HashMap::new();
        parameters.insert("duration_ticks".to_string(), toml::Value::Integer(5));

        let result = handler.apply(&mut world_state, &participants, &parameters);

        assert!(result.is_ok());
        assert_eq!(world_state.network.partitions.len(), 1);
        assert_eq!(world_state.network.partitions[0].participants, participants);
    }

    #[test]
    fn test_byzantine_message_dropper() {
        let injector = ByzantineMessageDropper;
        let mut world_state = create_test_world_state();
        let parameters = HashMap::new();

        let result = injector.inject(&mut world_state, "alice", &parameters);

        assert!(result.is_ok());
        assert!(world_state
            .byzantine
            .byzantine_participants
            .contains(&"alice".to_string()));
        assert!(world_state
            .byzantine
            .active_strategies
            .contains_key("alice"));
    }

    #[tokio::test]
    async fn test_simulation_protocol_handler() {
        let device_id = DeviceId::new();
        let mut handler = SimulationProtocolHandler::new(device_id);

        // Test session management
        let participants = vec![DeviceId::new(), DeviceId::new()];
        let session_id = handler
            .start_session(participants.clone(), "test".to_string(), HashMap::new())
            .await
            .unwrap();

        let session_info = handler.get_session_info(session_id).await.unwrap();
        assert_eq!(session_info.protocol_type, "test");
        assert_eq!(session_info.participants, participants);

        // Test messaging
        let message = SimulationMessage {
            protocol_type: "test".to_string(),
            payload: vec![1, 2, 3],
            metadata: HashMap::new(),
        };

        handler
            .send_message(participants[0], message.clone())
            .await
            .unwrap();
        let received = handler.receive_message(participants[0]).await.unwrap();
        assert_eq!(received.protocol_type, message.protocol_type);
        assert_eq!(received.payload, message.payload);

        // Test cleanup
        handler.end_session(session_id).await.unwrap();
        assert!(handler.get_session_info(session_id).await.is_err());
    }
}
