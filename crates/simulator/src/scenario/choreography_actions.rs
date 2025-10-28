//! Choreography Actions - Scheduler-Backed Protocol Execution
//!
//! This module provides choreography actions that use the ProtocolLifecycle
//! scheduler approach for deterministic and type-safe protocol execution.

use crate::{tick, QueuedProtocol, Result, WorldState};
use async_trait::async_trait;
use aura_coordination::{LocalSessionRuntime, Transport as CoordinationTransport};
use aura_crypto::Effects as CoreEffects;
use aura_journal::{events::RelationshipId, AccountLedger};
use aura_types::{AccountId, DeviceId};
use blake3::Hasher;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
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

/// Simple stub transport for simulation
#[derive(Debug, Clone)]
pub struct SimulationMemoryTransport {
    device_id: DeviceId,
}

impl SimulationMemoryTransport {
    pub fn new(device_id: DeviceId) -> Self {
        Self { device_id }
    }
}

#[async_trait]
impl CoordinationTransport for SimulationMemoryTransport {
    async fn send_message(
        &self,
        _peer_id: &str,
        _message: &[u8],
    ) -> std::result::Result<(), String> {
        // Stub implementation - just log for simulation
        println!("SimulationMemoryTransport: sending message in simulation");
        Ok(())
    }

    async fn broadcast_message(&self, _message: &[u8]) -> std::result::Result<(), String> {
        // Stub implementation - just log for simulation
        println!("SimulationMemoryTransport: broadcasting message in simulation");
        Ok(())
    }

    async fn is_peer_reachable(&self, _peer_id: &str) -> bool {
        // Stub implementation - always reachable in simulation
        true
    }
}

/// Helper function to create stub transport for simulation
pub fn create_stub_transport(device_id: DeviceId) -> SimulationMemoryTransport {
    SimulationMemoryTransport::new(device_id)
}

/// Helper to create a session runtime for simulation participants
async fn create_session_runtime_for_participant(
    _participant_id: &str,
    device_id: DeviceId,
    account_id: AccountId,
    ledger: Arc<RwLock<AccountLedger>>,
) -> Result<LocalSessionRuntime> {
    let effects = Arc::new(CoreEffects::production());
    let mut runtime = LocalSessionRuntime::new(device_id, account_id, effects);

    // For simulation, we use a minimal transport adapter
    // In production tests, this would be connected to actual network transports
    let stub_transport = create_stub_transport(device_id);

    runtime
        .set_environment(ledger, Arc::new(stub_transport))
        .await;

    Ok(runtime)
}

/// Helper to execute protocol using scheduler approach
pub async fn execute_protocol_with_scheduler(
    world_state: &mut WorldState,
    protocol_type: &str,
    participants: &[String],
    parameters: &HashMap<String, String>,
) -> Result<bool> {
    // For simulation, we'll create temporary session runtimes for participants
    // In production, these would already exist and be managed by agents

    let mut session_runtimes = HashMap::new();
    let mut participant_devices = HashMap::new();

    for participant_id in participants {
        if let Some(_participant) = world_state.participants.get(participant_id) {
            // Create minimal device and account IDs for simulation
            let device_id = DeviceId::new(); // This would come from participant in real scenario
            let account_id = AccountId::new(); // This would come from participant in real scenario

            // Create a minimal ledger for the simulation participant
            use aura_journal::{AccountState, DeviceMetadata, DeviceType};
            use ed25519_dalek::VerifyingKey;

            let dummy_key_bytes = [0u8; 32];
            let verifying_key = VerifyingKey::from_bytes(&dummy_key_bytes).map_err(|e| {
                crate::SimError::RuntimeError(format!("Failed to create verifying key: {}", e))
            })?;

            let device_metadata = DeviceMetadata {
                device_id,
                device_name: participant_id.clone(),
                device_type: DeviceType::Native,
                public_key: verifying_key,
                added_at: world_state.current_time,
                last_seen: world_state.current_time,
                dkd_commitment_proofs: Default::default(),
                next_nonce: 0,
                used_nonces: Default::default(),
            };

            let account_state = AccountState::new(
                account_id,
                verifying_key,
                device_metadata,
                2, // threshold
                3, // share_count
            );

            let ledger = AccountLedger::new(account_state).map_err(|e| {
                crate::SimError::RuntimeError(format!("Failed to create ledger: {:?}", e))
            })?;
            let ledger_arc = Arc::new(RwLock::new(ledger));

            let runtime = create_session_runtime_for_participant(
                participant_id,
                device_id,
                account_id,
                ledger_arc,
            )
            .await?;

            session_runtimes.insert(participant_id.clone(), runtime);
            participant_devices.insert(participant_id.clone(), device_id);
        }
    }

    // Execute protocol using session runtime command
    if let Some(runtime) = session_runtimes.values().next() {
        let command_sender = runtime.command_sender();

        let command = match protocol_type {
            "DKD" => {
                let app_id = parameters
                    .get("app_id")
                    .cloned()
                    .unwrap_or_else(|| "sim_app".to_string());
                let context = parameters
                    .get("context")
                    .cloned()
                    .unwrap_or_else(|| "sim_context".to_string());
                let threshold = parameters
                    .get("threshold")
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(2);

                aura_coordination::SessionCommand::StartDkdWithContext {
                    app_id,
                    context_label: context.clone(),
                    participants: participants
                        .iter()
                        .map(|_| DeviceId::new()) // Simplified for simulation
                        .collect(),
                    threshold,
                    context_bytes: context.into_bytes(),
                    with_binding_proof: true,
                }
            }
            "Resharing" => {
                let new_threshold = parameters
                    .get("new_threshold")
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(3);

                aura_coordination::SessionCommand::StartResharing {
                    new_participants: participants
                        .iter()
                        .map(|_| DeviceId::new()) // Simplified for simulation
                        .collect(),
                    new_threshold,
                }
            }
            "Recovery" => {
                let guardian_threshold = parameters
                    .get("guardian_threshold")
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(2);
                let cooldown_seconds = parameters
                    .get("cooldown_seconds")
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(300);

                aura_coordination::SessionCommand::StartRecovery {
                    guardian_threshold,
                    cooldown_seconds,
                }
            }
            "Counter" | "CounterInit" | "CounterIncrement" => {
                let count = parameters
                    .get("count")
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(1);
                let ttl_epochs = parameters
                    .get("ttl_epochs")
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(100);

                let seed = parameters
                    .get("relationship_seed")
                    .cloned()
                    .unwrap_or_else(|| participants.join(","));
                let mut hasher = Hasher::new();
                hasher.update(seed.as_bytes());
                let hash = hasher.finalize();
                let mut relationship_bytes = [0u8; 32];
                relationship_bytes.copy_from_slice(hash.as_bytes());
                let relationship_id = RelationshipId(relationship_bytes);

                let requesting_device = participant_devices
                    .values()
                    .copied()
                    .next()
                    .unwrap_or_else(DeviceId::new);

                aura_coordination::SessionCommand::StartCounter {
                    relationship_id,
                    requesting_device,
                    count,
                    ttl_epochs,
                }
            }
            _ => {
                return Err(crate::SimError::RuntimeError(format!(
                    "Unknown protocol type: {}",
                    protocol_type
                )));
            }
        };

        command_sender.send(command).map_err(|_| {
            crate::SimError::RuntimeError("Failed to send protocol command".to_string())
        })?;

        // Allow some time for protocol to start
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        return Ok(true);
    }

    Ok(false)
}

/// Blocking helper for scheduler-backed execution.
pub fn run_protocol_with_scheduler(
    world_state: &mut WorldState,
    protocol_type: &str,
    participants: &[String],
    parameters: &HashMap<String, String>,
) -> Result<bool> {
    tokio::task::block_in_place(|| {
        let handle = tokio::runtime::Handle::current();
        handle.block_on(async {
            execute_protocol_with_scheduler(world_state, protocol_type, participants, parameters)
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
            "Executing DKD choreography with scheduler: app_id='{}', context='{}', threshold={}",
            app_id, context, threshold
        );

        // Convert toml parameters to string parameters for scheduler
        let mut string_params = HashMap::new();
        string_params.insert("app_id".to_string(), app_id.to_string());
        string_params.insert("context".to_string(), context.to_string());
        string_params.insert("threshold".to_string(), threshold.to_string());

        // Use scheduler-backed execution in a blocking manner
        let success = run_protocol_with_scheduler(world_state, "DKD", participants, &string_params)
            .unwrap_or(false);

        // Still execute some simulation ticks for compatibility with existing tests
        let mut events_generated = 0;
        let max_ticks = if success { 20 } else { 5 }; // Fewer ticks if scheduler failed

        for tick_num in 0..max_ticks {
            let events = tick(world_state)?;
            events_generated += events.len();

            // Break early if using scheduler approach
            if success && tick_num > 5 {
                break;
            }
        }

        println!(
            "[{}] DKD choreography completed using scheduler approach",
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
                data.insert("scheduler_used".to_string(), "true".to_string());
                data
            },
        })
    }

    fn description(&self) -> &str {
        "Deterministic Key Derivation (DKD) protocol choreography"
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
            "Executing Resharing choreography with scheduler: {} -> {} threshold",
            old_threshold, new_threshold
        );

        // Convert parameters for scheduler
        let mut string_params = HashMap::new();
        string_params.insert("old_threshold".to_string(), old_threshold.to_string());
        string_params.insert("new_threshold".to_string(), new_threshold.to_string());

        // Use scheduler-backed execution
        let success =
            run_protocol_with_scheduler(world_state, "Resharing", participants, &string_params)
                .unwrap_or(false);

        // Execute simulation ticks
        let mut events_generated = 0;
        let max_ticks = if success { 15 } else { 5 };

        for _ in 0..max_ticks {
            let events = tick(world_state)?;
            events_generated += events.len();
        }

        println!(
            "[{}] Resharing choreography completed using scheduler approach",
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
                data.insert("scheduler_used".to_string(), "true".to_string());
                data
            },
        })
    }

    fn description(&self) -> &str {
        "Key resharing protocol choreography for threshold updates"
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
            "Executing Recovery choreography with scheduler: guardian_threshold={}, cooldown={}h",
            guardian_threshold, cooldown_hours
        );

        // Convert parameters for scheduler
        let mut string_params = HashMap::new();
        string_params.insert(
            "guardian_threshold".to_string(),
            guardian_threshold.to_string(),
        );
        string_params.insert(
            "cooldown_seconds".to_string(),
            (cooldown_hours * 3600).to_string(),
        ); // Convert to seconds

        // Use scheduler-backed execution
        let success =
            run_protocol_with_scheduler(world_state, "Recovery", participants, &string_params)
                .unwrap_or(false);

        // Execute simulation ticks
        let mut events_generated = 0;
        let max_ticks = if success { 20 } else { 5 };

        for _ in 0..max_ticks {
            let events = tick(world_state)?;
            events_generated += events.len();
        }

        println!(
            "[{}] Recovery choreography completed using scheduler approach",
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
                data.insert("scheduler_used".to_string(), "true".to_string());
                data
            },
        })
    }

    fn description(&self) -> &str {
        "Guardian-based account recovery choreography"
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
            "Executing Locking choreography: operation_type='{}'",
            operation_type
        );

        // Queue locking protocol
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

        println!("[OK] Locking choreography completed");

        Ok(ChoreographyResult {
            success: true,
            events_generated,
            execution_time: start_time.elapsed(),
            data: {
                let mut data = HashMap::new();
                data.insert("operation_type".to_string(), operation_type.to_string());
                data.insert("participants".to_string(), participants.len().to_string());
                data.insert("protocol_type".to_string(), "Locking".to_string());
                data
            },
        })
    }

    fn description(&self) -> &str {
        "Distributed locking choreography for coordinated operations"
    }
}

// Network Condition Handlers

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
            duration: Some(duration_ticks * 100), // Convert ticks to time units
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

        // This would configure the network fabric to add delays
        // For now, we'll just log the configuration
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

        // This would configure the network fabric to drop messages
        // For now, we'll just log the configuration
        println!("Message drop configuration applied");

        Ok(())
    }

    fn description(&self) -> &str {
        "Randomly drops messages between participants"
    }
}

// Byzantine Behavior Injectors

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

        // Add participant to byzantine list if not already present
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

        // Set byzantine strategy
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
/// These choreographies now use the ProtocolLifecycle scheduler approach
/// for type-safe and deterministic protocol execution.
pub fn register_standard_choreographies(
    engine: &mut crate::scenario::engine::UnifiedScenarioEngine,
) {
    // Register choreographies (now using scheduler-backed execution)
    engine.register_choreography("dkd".to_string(), DkdChoreography);
    engine.register_choreography("resharing".to_string(), ResharingChoreography);
    engine.register_choreography("recovery".to_string(), RecoveryChoreography);
    engine.register_choreography("locking".to_string(), LockingChoreography);

    println!("Registered scheduler-backed choreography actions:");
    println!("  dkd - Deterministic Key Derivation (via ProtocolLifecycle)");
    println!("  resharing - Key Resharing Protocol (via ProtocolLifecycle)");
    println!("  recovery - Guardian-based Recovery (via ProtocolLifecycle)");
    println!("  locking - Distributed Locking (fallback to legacy)");
}

/// Helper function to register all standard network conditions
pub fn register_standard_network_conditions(
    _registry: &mut crate::scenario::engine::ChoreographyActionRegistry,
) {
    // Note: This would require extending the registry to support network conditions
    // For now, this is a placeholder showing the intended design
    println!("Standard network conditions available:");
    println!("  partition - Network partitioning");
    println!("  delay - Message delays");
    println!("  drop - Message drops");
}

/// Helper function to register all standard byzantine behaviors
pub fn register_standard_byzantine_behaviors(
    _registry: &mut crate::scenario::engine::ChoreographyActionRegistry,
) {
    // Note: This would require extending the registry to support byzantine behaviors
    // For now, this is a placeholder showing the intended design
    println!("Standard Byzantine behaviors available:");
    println!("  drop_messages - Drop all messages");
    println!("  double_spend - Attempt double spending");
    println!("  delay_messages - Delay message sending");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn create_test_world_state() -> WorldState {
        crate::test_utils::two_party_world_state()
    }

    #[test]
    fn test_dkd_choreography() {
        let choreography = DkdChoreography;
        let mut world_state = create_test_world_state();
        let participants = vec!["alice".to_string(), "bob".to_string()];
        let parameters = HashMap::new();

        let result = choreography
            .execute(&mut world_state, &participants, &parameters)
            .unwrap();

        assert!(result.success);
        assert!(result.events_generated > 0);
        assert!(!world_state.protocols.execution_queue.is_empty());
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
}
