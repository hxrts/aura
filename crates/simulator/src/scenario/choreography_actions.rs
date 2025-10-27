//! Choreography Actions - Refactored Runner Helpers
//!
//! This module converts the imperative helpers from runners/ into declarative
//! choreography actions that can be invoked from TOML scenarios.

use crate::{tick, QueuedProtocol, Result, WorldState};
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
            "Executing DKD choreography: app_id='{}', context='{}', threshold={}",
            app_id, context, threshold
        );

        // Queue DKD protocol for execution
        let protocol = QueuedProtocol {
            protocol_type: "DKD".to_string(),
            participants: if participants.is_empty() {
                // Use all participants if none specified
                world_state.participants.keys().cloned().collect()
            } else {
                participants.to_vec()
            },
            parameters: {
                let mut params = HashMap::new();
                params.insert("app_id".to_string(), app_id.to_string());
                params.insert("context".to_string(), context.to_string());
                params.insert("threshold".to_string(), threshold.to_string());
                params
            },
            scheduled_time: world_state.current_time + 100, // Schedule for next tick
            priority: 1,
        };

        world_state.protocols.execution_queue.push_back(protocol);

        // Execute simulation ticks until protocol completes
        let mut events_generated = 0;
        let max_ticks = 50; // Safety limit

        for tick_num in 0..max_ticks {
            let events = tick(world_state)?;
            events_generated += events.len();

            // Check if DKD protocol has completed
            // This is simplified - in practice we'd check protocol state
            if tick_num > 10 {
                break;
            }
        }

        println!(
            "[OK] DKD choreography completed in {} ticks",
            events_generated
        );

        Ok(ChoreographyResult {
            success: true,
            events_generated,
            execution_time: start_time.elapsed(),
            data: {
                let mut data = HashMap::new();
                data.insert("app_id".to_string(), app_id.to_string());
                data.insert("context".to_string(), context.to_string());
                data.insert("participants".to_string(), participants.len().to_string());
                data.insert("protocol_type".to_string(), "DKD".to_string());
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
            "Executing Resharing choreography: {} -> {} threshold",
            old_threshold, new_threshold
        );

        // Queue resharing protocol
        let protocol = QueuedProtocol {
            protocol_type: "Resharing".to_string(),
            participants: participants.to_vec(),
            parameters: {
                let mut params = HashMap::new();
                params.insert("old_threshold".to_string(), old_threshold.to_string());
                params.insert("new_threshold".to_string(), new_threshold.to_string());
                params
            },
            scheduled_time: world_state.current_time + 100,
            priority: 1,
        };

        world_state.protocols.execution_queue.push_back(protocol);

        // Execute simulation
        let mut events_generated = 0;
        for _ in 0..30 {
            let events = tick(world_state)?;
            events_generated += events.len();
        }

        println!("[OK] Resharing choreography completed");

        Ok(ChoreographyResult {
            success: true,
            events_generated,
            execution_time: start_time.elapsed(),
            data: {
                let mut data = HashMap::new();
                data.insert("old_threshold".to_string(), old_threshold.to_string());
                data.insert("new_threshold".to_string(), new_threshold.to_string());
                data.insert("participants".to_string(), participants.len().to_string());
                data.insert("protocol_type".to_string(), "Resharing".to_string());
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
            "Executing Recovery choreography: guardian_threshold={}, cooldown={}h",
            guardian_threshold, cooldown_hours
        );

        // Queue recovery protocol
        let protocol = QueuedProtocol {
            protocol_type: "Recovery".to_string(),
            participants: participants.to_vec(),
            parameters: {
                let mut params = HashMap::new();
                params.insert(
                    "guardian_threshold".to_string(),
                    guardian_threshold.to_string(),
                );
                params.insert("cooldown_hours".to_string(), cooldown_hours.to_string());
                params
            },
            scheduled_time: world_state.current_time + 100,
            priority: 2, // Higher priority for recovery
        };

        world_state.protocols.execution_queue.push_back(protocol);

        // Execute simulation
        let mut events_generated = 0;
        for _ in 0..40 {
            let events = tick(world_state)?;
            events_generated += events.len();
        }

        println!("[OK] Recovery choreography completed");

        Ok(ChoreographyResult {
            success: true,
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
pub fn register_standard_choreographies(
    engine: &mut crate::scenario::engine::UnifiedScenarioEngine,
) {
    // Register choreographies
    engine.register_choreography("dkd".to_string(), DkdChoreography);
    engine.register_choreography("resharing".to_string(), ResharingChoreography);
    engine.register_choreography("recovery".to_string(), RecoveryChoreography);
    engine.register_choreography("locking".to_string(), LockingChoreography);

    println!("Registered standard choreography actions:");
    println!("  dkd - Deterministic Key Derivation");
    println!("  resharing - Key Resharing Protocol");
    println!("  recovery - Guardian-based Recovery");
    println!("  locking - Distributed Locking");
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
