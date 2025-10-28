//! Standard Choreography Executors
//!
//! This module provides implementations of all standard choreographies
//! used in TOML scenarios.

use super::choreography_actions::run_protocol_with_scheduler;
use super::engine::{ChoreographyExecutor, ChoreographyResult};
use crate::{tick, QueuedProtocol, Result, WorldState};
use std::collections::HashMap;
use std::time::Instant;

// ============================================================================
// Core Protocol Choreographies
// ============================================================================

/// Handshake choreography - establishes secure communication channel
pub struct HandshakeChoreography;

impl ChoreographyExecutor for HandshakeChoreography {
    fn execute(
        &self,
        world_state: &mut WorldState,
        participants: &[String],
        _parameters: &HashMap<String, toml::Value>,
    ) -> Result<ChoreographyResult> {
        let start_time = Instant::now();

        let protocol = QueuedProtocol {
            protocol_type: "Handshake".to_string(),
            participants: participants.to_vec(),
            parameters: HashMap::new(),
            scheduled_time: world_state.current_time + 100,
            priority: 1,
        };

        world_state.protocols.execution_queue.push_back(protocol);

        let mut events_generated = 0;
        for _ in 0..10 {
            let events = tick(world_state)?;
            events_generated += events.len();
        }

        Ok(ChoreographyResult {
            success: true,
            events_generated,
            execution_time: start_time.elapsed(),
            data: HashMap::new(),
        })
    }

    fn description(&self) -> &str {
        "Handshake protocol for secure channel establishment"
    }
}

/// Session establishment choreography
pub struct SessionEstablishmentChoreography;

impl ChoreographyExecutor for SessionEstablishmentChoreography {
    fn execute(
        &self,
        world_state: &mut WorldState,
        participants: &[String],
        parameters: &HashMap<String, toml::Value>,
    ) -> Result<ChoreographyResult> {
        let start_time = Instant::now();

        let threshold = parameters
            .get("threshold")
            .and_then(|v| v.as_integer())
            .unwrap_or(participants.len() as i64) as usize;

        let mut params = HashMap::new();
        params.insert("threshold".to_string(), threshold.to_string());

        let protocol = QueuedProtocol {
            protocol_type: "SessionEstablishment".to_string(),
            participants: participants.to_vec(),
            parameters: params,
            scheduled_time: world_state.current_time + 100,
            priority: 1,
        };

        world_state.protocols.execution_queue.push_back(protocol);

        let mut events_generated = 0;
        for _ in 0..20 {
            let events = tick(world_state)?;
            events_generated += events.len();
        }

        Ok(ChoreographyResult {
            success: true,
            events_generated,
            execution_time: start_time.elapsed(),
            data: HashMap::new(),
        })
    }

    fn description(&self) -> &str {
        "Session establishment with threshold configuration"
    }
}

/// P2P DKD choreography - two-party key derivation
pub struct P2pDkdChoreography;

impl ChoreographyExecutor for P2pDkdChoreography {
    fn execute(
        &self,
        world_state: &mut WorldState,
        participants: &[String],
        parameters: &HashMap<String, toml::Value>,
    ) -> Result<ChoreographyResult> {
        let start_time = Instant::now();

        let app_id = parameters
            .get("app_id")
            .and_then(|v| v.as_str())
            .unwrap_or("default_app");
        let context = parameters
            .get("context")
            .and_then(|v| v.as_str())
            .unwrap_or("default_context");

        let mut params = HashMap::new();
        params.insert("app_id".to_string(), app_id.to_string());
        params.insert("context".to_string(), context.to_string());

        let protocol = QueuedProtocol {
            protocol_type: "P2pDKD".to_string(),
            participants: participants.to_vec(),
            parameters: params,
            scheduled_time: world_state.current_time + 100,
            priority: 1,
        };

        world_state.protocols.execution_queue.push_back(protocol);

        let mut events_generated = 0;
        for _ in 0..15 {
            let events = tick(world_state)?;
            events_generated += events.len();
        }

        Ok(ChoreographyResult {
            success: true,
            events_generated,
            execution_time: start_time.elapsed(),
            data: {
                let mut data = HashMap::new();
                data.insert("app_id".to_string(), app_id.to_string());
                data.insert("context".to_string(), context.to_string());
                data
            },
        })
    }

    fn description(&self) -> &str {
        "Peer-to-peer deterministic key derivation"
    }
}

/// Context agreement choreography
pub struct ContextAgreementChoreography;

impl ChoreographyExecutor for ContextAgreementChoreography {
    fn execute(
        &self,
        world_state: &mut WorldState,
        participants: &[String],
        parameters: &HashMap<String, toml::Value>,
    ) -> Result<ChoreographyResult> {
        let start_time = Instant::now();

        let app_id = parameters
            .get("app_id")
            .and_then(|v| v.as_str())
            .unwrap_or("default_app");
        let context = parameters
            .get("context")
            .and_then(|v| v.as_str())
            .unwrap_or("default_context");

        let mut params = HashMap::new();
        params.insert("app_id".to_string(), app_id.to_string());
        params.insert("context".to_string(), context.to_string());

        let protocol = QueuedProtocol {
            protocol_type: "ContextAgreement".to_string(),
            participants: participants.to_vec(),
            parameters: params,
            scheduled_time: world_state.current_time + 100,
            priority: 1,
        };

        world_state.protocols.execution_queue.push_back(protocol);

        let mut events_generated = 0;
        for _ in 0..10 {
            let events = tick(world_state)?;
            events_generated += events.len();
        }

        Ok(ChoreographyResult {
            success: true,
            events_generated,
            execution_time: start_time.elapsed(),
            data: HashMap::new(),
        })
    }

    fn description(&self) -> &str {
        "Context agreement for DKD operations"
    }
}

/// Session operation choreography
pub struct SessionOperationChoreography;

impl ChoreographyExecutor for SessionOperationChoreography {
    fn execute(
        &self,
        world_state: &mut WorldState,
        participants: &[String],
        _parameters: &HashMap<String, toml::Value>,
    ) -> Result<ChoreographyResult> {
        let start_time = Instant::now();

        let protocol = QueuedProtocol {
            protocol_type: "SessionOperation".to_string(),
            participants: participants.to_vec(),
            parameters: HashMap::new(),
            scheduled_time: world_state.current_time + 100,
            priority: 1,
        };

        world_state.protocols.execution_queue.push_back(protocol);

        let mut events_generated = 0;
        for _ in 0..10 {
            let events = tick(world_state)?;
            events_generated += events.len();
        }

        Ok(ChoreographyResult {
            success: true,
            events_generated,
            execution_time: start_time.elapsed(),
            data: HashMap::new(),
        })
    }

    fn description(&self) -> &str {
        "Generic session operation"
    }
}

// ============================================================================
// FROST Protocol Choreographies
// ============================================================================

/// FROST key generation choreography
pub struct FrostKeyGenerationChoreography;

impl ChoreographyExecutor for FrostKeyGenerationChoreography {
    fn execute(
        &self,
        world_state: &mut WorldState,
        participants: &[String],
        parameters: &HashMap<String, toml::Value>,
    ) -> Result<ChoreographyResult> {
        let start_time = Instant::now();

        let threshold = parameters
            .get("threshold")
            .and_then(|v| v.as_integer())
            .unwrap_or(2) as usize;

        let mut params = HashMap::new();
        params.insert("threshold".to_string(), threshold.to_string());

        let protocol = QueuedProtocol {
            protocol_type: "FrostKeyGen".to_string(),
            participants: participants.to_vec(),
            parameters: params,
            scheduled_time: world_state.current_time + 100,
            priority: 1,
        };

        world_state.protocols.execution_queue.push_back(protocol);

        let mut events_generated = 0;
        for _ in 0..30 {
            let events = tick(world_state)?;
            events_generated += events.len();
        }

        Ok(ChoreographyResult {
            success: true,
            events_generated,
            execution_time: start_time.elapsed(),
            data: HashMap::new(),
        })
    }

    fn description(&self) -> &str {
        "FROST threshold key generation"
    }
}

/// FROST commitment choreography
pub struct FrostCommitmentChoreography;

impl ChoreographyExecutor for FrostCommitmentChoreography {
    fn execute(
        &self,
        world_state: &mut WorldState,
        participants: &[String],
        parameters: &HashMap<String, toml::Value>,
    ) -> Result<ChoreographyResult> {
        let start_time = Instant::now();

        let threshold = parameters
            .get("threshold")
            .and_then(|v| v.as_integer())
            .unwrap_or(2) as usize;

        let mut params = HashMap::new();
        params.insert("threshold".to_string(), threshold.to_string());

        let protocol = QueuedProtocol {
            protocol_type: "FrostCommitment".to_string(),
            participants: participants.to_vec(),
            parameters: params,
            scheduled_time: world_state.current_time + 100,
            priority: 1,
        };

        world_state.protocols.execution_queue.push_back(protocol);

        let mut events_generated = 0;
        for _ in 0..15 {
            let events = tick(world_state)?;
            events_generated += events.len();
        }

        Ok(ChoreographyResult {
            success: true,
            events_generated,
            execution_time: start_time.elapsed(),
            data: HashMap::new(),
        })
    }

    fn description(&self) -> &str {
        "FROST commitment phase"
    }
}

/// FROST signing choreography
pub struct FrostSigningChoreography;

impl ChoreographyExecutor for FrostSigningChoreography {
    fn execute(
        &self,
        world_state: &mut WorldState,
        participants: &[String],
        parameters: &HashMap<String, toml::Value>,
    ) -> Result<ChoreographyResult> {
        let start_time = Instant::now();

        let threshold = parameters
            .get("threshold")
            .and_then(|v| v.as_integer())
            .unwrap_or(2) as usize;

        let mut params = HashMap::new();
        params.insert("threshold".to_string(), threshold.to_string());

        let protocol = QueuedProtocol {
            protocol_type: "FrostSigning".to_string(),
            participants: participants.to_vec(),
            parameters: params,
            scheduled_time: world_state.current_time + 100,
            priority: 1,
        };

        world_state.protocols.execution_queue.push_back(protocol);

        let mut events_generated = 0;
        for _ in 0..20 {
            let events = tick(world_state)?;
            events_generated += events.len();
        }

        Ok(ChoreographyResult {
            success: true,
            events_generated,
            execution_time: start_time.elapsed(),
            data: HashMap::new(),
        })
    }

    fn description(&self) -> &str {
        "FROST threshold signing"
    }
}

// ============================================================================
// Account Lifecycle Choreographies
// ============================================================================

/// Account creation choreography
pub struct AccountCreationChoreography;

impl ChoreographyExecutor for AccountCreationChoreography {
    fn execute(
        &self,
        world_state: &mut WorldState,
        participants: &[String],
        parameters: &HashMap<String, toml::Value>,
    ) -> Result<ChoreographyResult> {
        let start_time = Instant::now();

        let threshold = parameters
            .get("threshold")
            .and_then(|v| v.as_integer())
            .unwrap_or(2) as usize;

        let mut params = HashMap::new();
        params.insert("threshold".to_string(), threshold.to_string());

        let protocol = QueuedProtocol {
            protocol_type: "AccountCreation".to_string(),
            participants: participants.to_vec(),
            parameters: params,
            scheduled_time: world_state.current_time + 100,
            priority: 1,
        };

        world_state.protocols.execution_queue.push_back(protocol);

        let mut events_generated = 0;
        for _ in 0..40 {
            let events = tick(world_state)?;
            events_generated += events.len();
        }

        Ok(ChoreographyResult {
            success: true,
            events_generated,
            execution_time: start_time.elapsed(),
            data: HashMap::new(),
        })
    }

    fn description(&self) -> &str {
        "Complete account creation with threshold setup"
    }
}

/// Guardian configuration choreography
pub struct GuardianConfigurationChoreography;

impl ChoreographyExecutor for GuardianConfigurationChoreography {
    fn execute(
        &self,
        world_state: &mut WorldState,
        participants: &[String],
        parameters: &HashMap<String, toml::Value>,
    ) -> Result<ChoreographyResult> {
        let start_time = Instant::now();

        let threshold = parameters
            .get("threshold")
            .and_then(|v| v.as_integer())
            .unwrap_or(2) as usize;

        let mut params = HashMap::new();
        params.insert("threshold".to_string(), threshold.to_string());

        let protocol = QueuedProtocol {
            protocol_type: "GuardianConfiguration".to_string(),
            participants: participants.to_vec(),
            parameters: params,
            scheduled_time: world_state.current_time + 100,
            priority: 1,
        };

        world_state.protocols.execution_queue.push_back(protocol);

        let mut events_generated = 0;
        for _ in 0..25 {
            let events = tick(world_state)?;
            events_generated += events.len();
        }

        Ok(ChoreographyResult {
            success: true,
            events_generated,
            execution_time: start_time.elapsed(),
            data: HashMap::new(),
        })
    }

    fn description(&self) -> &str {
        "Configure guardian-based recovery"
    }
}

/// Guardian recovery choreography
pub struct GuardianRecoveryChoreography;

impl ChoreographyExecutor for GuardianRecoveryChoreography {
    fn execute(
        &self,
        world_state: &mut WorldState,
        participants: &[String],
        parameters: &HashMap<String, toml::Value>,
    ) -> Result<ChoreographyResult> {
        let start_time = Instant::now();

        let threshold = parameters
            .get("threshold")
            .and_then(|v| v.as_integer())
            .unwrap_or(2) as usize;

        let mut params = HashMap::new();
        params.insert("threshold".to_string(), threshold.to_string());

        let protocol = QueuedProtocol {
            protocol_type: "GuardianRecovery".to_string(),
            participants: participants.to_vec(),
            parameters: params,
            scheduled_time: world_state.current_time + 100,
            priority: 2,
        };

        world_state.protocols.execution_queue.push_back(protocol);

        let mut events_generated = 0;
        for _ in 0..30 {
            let events = tick(world_state)?;
            events_generated += events.len();
        }

        Ok(ChoreographyResult {
            success: true,
            events_generated,
            execution_time: start_time.elapsed(),
            data: HashMap::new(),
        })
    }

    fn description(&self) -> &str {
        "Execute guardian-based account recovery"
    }
}

// ============================================================================
// Session Management Choreographies
// ============================================================================

/// Epoch increment choreography
pub struct EpochIncrementChoreography;

impl ChoreographyExecutor for EpochIncrementChoreography {
    fn execute(
        &self,
        world_state: &mut WorldState,
        participants: &[String],
        parameters: &HashMap<String, toml::Value>,
    ) -> Result<ChoreographyResult> {
        let start_time = Instant::now();

        let threshold = parameters
            .get("threshold")
            .and_then(|v| v.as_integer())
            .unwrap_or(2) as usize;

        let mut params = HashMap::new();
        params.insert("threshold".to_string(), threshold.to_string());

        let protocol = QueuedProtocol {
            protocol_type: "EpochIncrement".to_string(),
            participants: participants.to_vec(),
            parameters: params,
            scheduled_time: world_state.current_time + 100,
            priority: 1,
        };

        world_state.protocols.execution_queue.push_back(protocol);

        let mut events_generated = 0;
        for _ in 0..15 {
            let events = tick(world_state)?;
            events_generated += events.len();
        }

        Ok(ChoreographyResult {
            success: true,
            events_generated,
            execution_time: start_time.elapsed(),
            data: HashMap::new(),
        })
    }

    fn description(&self) -> &str {
        "Increment session epoch for credential rotation"
    }
}

/// Presence ticket distribution choreography
pub struct PresenceTicketDistributionChoreography;

impl ChoreographyExecutor for PresenceTicketDistributionChoreography {
    fn execute(
        &self,
        world_state: &mut WorldState,
        participants: &[String],
        _parameters: &HashMap<String, toml::Value>,
    ) -> Result<ChoreographyResult> {
        let start_time = Instant::now();

        let protocol = QueuedProtocol {
            protocol_type: "PresenceTicketDistribution".to_string(),
            participants: participants.to_vec(),
            parameters: HashMap::new(),
            scheduled_time: world_state.current_time + 100,
            priority: 1,
        };

        world_state.protocols.execution_queue.push_back(protocol);

        let mut events_generated = 0;
        for _ in 0..20 {
            let events = tick(world_state)?;
            events_generated += events.len();
        }

        Ok(ChoreographyResult {
            success: true,
            events_generated,
            execution_time: start_time.elapsed(),
            data: HashMap::new(),
        })
    }

    fn description(&self) -> &str {
        "Distribute presence tickets for session management"
    }
}

// ============================================================================
// CRDT and State Management Choreographies
// ============================================================================

/// CRDT initialization choreography
pub struct CrdtInitializationChoreography;

impl ChoreographyExecutor for CrdtInitializationChoreography {
    fn execute(
        &self,
        world_state: &mut WorldState,
        participants: &[String],
        _parameters: &HashMap<String, toml::Value>,
    ) -> Result<ChoreographyResult> {
        let start_time = Instant::now();

        let protocol = QueuedProtocol {
            protocol_type: "CrdtInitialization".to_string(),
            participants: participants.to_vec(),
            parameters: HashMap::new(),
            scheduled_time: world_state.current_time + 100,
            priority: 1,
        };

        world_state.protocols.execution_queue.push_back(protocol);

        let mut events_generated = 0;
        for _ in 0..15 {
            let events = tick(world_state)?;
            events_generated += events.len();
        }

        Ok(ChoreographyResult {
            success: true,
            events_generated,
            execution_time: start_time.elapsed(),
            data: HashMap::new(),
        })
    }

    fn description(&self) -> &str {
        "Initialize CRDT state across participants"
    }
}

/// CRDT update choreography
pub struct CrdtUpdateChoreography;

impl ChoreographyExecutor for CrdtUpdateChoreography {
    fn execute(
        &self,
        world_state: &mut WorldState,
        participants: &[String],
        _parameters: &HashMap<String, toml::Value>,
    ) -> Result<ChoreographyResult> {
        let start_time = Instant::now();

        let protocol = QueuedProtocol {
            protocol_type: "CrdtUpdate".to_string(),
            participants: participants.to_vec(),
            parameters: HashMap::new(),
            scheduled_time: world_state.current_time + 100,
            priority: 1,
        };

        world_state.protocols.execution_queue.push_back(protocol);

        let mut events_generated = 0;
        for _ in 0..10 {
            let events = tick(world_state)?;
            events_generated += events.len();
        }

        Ok(ChoreographyResult {
            success: true,
            events_generated,
            execution_time: start_time.elapsed(),
            data: HashMap::new(),
        })
    }

    fn description(&self) -> &str {
        "Update CRDT state with new values"
    }
}

/// Counter initialization choreography
pub struct CounterInitChoreography;

impl ChoreographyExecutor for CounterInitChoreography {
    fn execute(
        &self,
        world_state: &mut WorldState,
        participants: &[String],
        _parameters: &HashMap<String, toml::Value>,
    ) -> Result<ChoreographyResult> {
        let start_time = Instant::now();

        let mut string_params = HashMap::new();
        string_params.insert("count".to_string(), "1".to_string());
        string_params.insert("ttl_epochs".to_string(), "100".to_string());
        string_params.insert("relationship_seed".to_string(), participants.join(","));

        let success =
            run_protocol_with_scheduler(world_state, "CounterInit", participants, &string_params)
                .unwrap_or(false);

        let mut events_generated = 0;
        let max_ticks = if success { 10 } else { 5 };
        for tick_idx in 0..max_ticks {
            let events = tick(world_state)?;
            events_generated += events.len();
            if success && tick_idx > 3 {
                break;
            }
        }

        Ok(ChoreographyResult {
            success,
            events_generated,
            execution_time: start_time.elapsed(),
            data: {
                let mut data = HashMap::new();
                data.insert("protocol_type".to_string(), "CounterInit".to_string());
                data.insert("participants".to_string(), participants.len().to_string());
                data.insert("scheduler_used".to_string(), "true".to_string());
                data
            },
        })
    }

    fn description(&self) -> &str {
        "Initialize distributed counter"
    }
}

/// Counter increment choreography
pub struct CounterIncrementChoreography;

impl ChoreographyExecutor for CounterIncrementChoreography {
    fn execute(
        &self,
        world_state: &mut WorldState,
        participants: &[String],
        _parameters: &HashMap<String, toml::Value>,
    ) -> Result<ChoreographyResult> {
        let start_time = Instant::now();

        let mut string_params = HashMap::new();
        string_params.insert("count".to_string(), "1".to_string());
        string_params.insert("ttl_epochs".to_string(), "50".to_string());
        string_params.insert("relationship_seed".to_string(), participants.join(","));

        let success = run_protocol_with_scheduler(
            world_state,
            "CounterIncrement",
            participants,
            &string_params,
        )
        .unwrap_or(false);

        let mut events_generated = 0;
        let max_ticks = if success { 8 } else { 5 };
        for tick_idx in 0..max_ticks {
            let events = tick(world_state)?;
            events_generated += events.len();
            if success && tick_idx > 2 {
                break;
            }
        }

        Ok(ChoreographyResult {
            success,
            events_generated,
            execution_time: start_time.elapsed(),
            data: {
                let mut data = HashMap::new();
                data.insert("protocol_type".to_string(), "CounterIncrement".to_string());
                data.insert("participants".to_string(), participants.len().to_string());
                data.insert("scheduler_used".to_string(), "true".to_string());
                data
            },
        })
    }

    fn description(&self) -> &str {
        "Increment distributed counter"
    }
}

// ============================================================================
// Group Communication Choreographies
// ============================================================================

/// Group initialization choreography
pub struct GroupInitializationChoreography;

impl ChoreographyExecutor for GroupInitializationChoreography {
    fn execute(
        &self,
        world_state: &mut WorldState,
        participants: &[String],
        parameters: &HashMap<String, toml::Value>,
    ) -> Result<ChoreographyResult> {
        let start_time = Instant::now();

        let threshold = parameters
            .get("threshold")
            .and_then(|v| v.as_integer())
            .unwrap_or(2) as usize;

        let mut params = HashMap::new();
        params.insert("threshold".to_string(), threshold.to_string());

        let protocol = QueuedProtocol {
            protocol_type: "GroupInitialization".to_string(),
            participants: participants.to_vec(),
            parameters: params,
            scheduled_time: world_state.current_time + 100,
            priority: 1,
        };

        world_state.protocols.execution_queue.push_back(protocol);

        let mut events_generated = 0;
        for _ in 0..25 {
            let events = tick(world_state)?;
            events_generated += events.len();
        }

        Ok(ChoreographyResult {
            success: true,
            events_generated,
            execution_time: start_time.elapsed(),
            data: HashMap::new(),
        })
    }

    fn description(&self) -> &str {
        "Initialize secure group communication"
    }
}

/// Group broadcast choreography
pub struct GroupBroadcastChoreography;

impl ChoreographyExecutor for GroupBroadcastChoreography {
    fn execute(
        &self,
        world_state: &mut WorldState,
        participants: &[String],
        _parameters: &HashMap<String, toml::Value>,
    ) -> Result<ChoreographyResult> {
        let start_time = Instant::now();

        let protocol = QueuedProtocol {
            protocol_type: "GroupBroadcast".to_string(),
            participants: participants.to_vec(),
            parameters: HashMap::new(),
            scheduled_time: world_state.current_time + 100,
            priority: 1,
        };

        world_state.protocols.execution_queue.push_back(protocol);

        let mut events_generated = 0;
        for _ in 0..15 {
            let events = tick(world_state)?;
            events_generated += events.len();
        }

        Ok(ChoreographyResult {
            success: true,
            events_generated,
            execution_time: start_time.elapsed(),
            data: HashMap::new(),
        })
    }

    fn description(&self) -> &str {
        "Broadcast message to group members"
    }
}

// ============================================================================
// Transport Choreographies
// ============================================================================

/// Gossip setup choreography
pub struct GossipSetupChoreography;

impl ChoreographyExecutor for GossipSetupChoreography {
    fn execute(
        &self,
        world_state: &mut WorldState,
        participants: &[String],
        _parameters: &HashMap<String, toml::Value>,
    ) -> Result<ChoreographyResult> {
        let start_time = Instant::now();

        let protocol = QueuedProtocol {
            protocol_type: "GossipSetup".to_string(),
            participants: participants.to_vec(),
            parameters: HashMap::new(),
            scheduled_time: world_state.current_time + 100,
            priority: 1,
        };

        world_state.protocols.execution_queue.push_back(protocol);

        let mut events_generated = 0;
        for _ in 0..20 {
            let events = tick(world_state)?;
            events_generated += events.len();
        }

        Ok(ChoreographyResult {
            success: true,
            events_generated,
            execution_time: start_time.elapsed(),
            data: HashMap::new(),
        })
    }

    fn description(&self) -> &str {
        "Setup gossip protocol for distributed communication"
    }
}

/// Gossip broadcast choreography
pub struct GossipBroadcastChoreography;

impl ChoreographyExecutor for GossipBroadcastChoreography {
    fn execute(
        &self,
        world_state: &mut WorldState,
        participants: &[String],
        _parameters: &HashMap<String, toml::Value>,
    ) -> Result<ChoreographyResult> {
        let start_time = Instant::now();

        let protocol = QueuedProtocol {
            protocol_type: "GossipBroadcast".to_string(),
            participants: participants.to_vec(),
            parameters: HashMap::new(),
            scheduled_time: world_state.current_time + 100,
            priority: 1,
        };

        world_state.protocols.execution_queue.push_back(protocol);

        let mut events_generated = 0;
        for _ in 0..10 {
            let events = tick(world_state)?;
            events_generated += events.len();
        }

        Ok(ChoreographyResult {
            success: true,
            events_generated,
            execution_time: start_time.elapsed(),
            data: HashMap::new(),
        })
    }

    fn description(&self) -> &str {
        "Broadcast message via gossip protocol"
    }
}

// ============================================================================
// Multi-Round Protocol Choreographies
// ============================================================================

/// Multi-round protocol choreography
pub struct MultiRoundProtocolChoreography;

impl ChoreographyExecutor for MultiRoundProtocolChoreography {
    fn execute(
        &self,
        world_state: &mut WorldState,
        participants: &[String],
        parameters: &HashMap<String, toml::Value>,
    ) -> Result<ChoreographyResult> {
        let start_time = Instant::now();

        let threshold = parameters
            .get("threshold")
            .and_then(|v| v.as_integer())
            .unwrap_or(2) as usize;

        let mut params = HashMap::new();
        params.insert("threshold".to_string(), threshold.to_string());

        let protocol = QueuedProtocol {
            protocol_type: "MultiRoundProtocol".to_string(),
            participants: participants.to_vec(),
            parameters: params,
            scheduled_time: world_state.current_time + 100,
            priority: 1,
        };

        world_state.protocols.execution_queue.push_back(protocol);

        let mut events_generated = 0;
        for _ in 0..35 {
            let events = tick(world_state)?;
            events_generated += events.len();
        }

        Ok(ChoreographyResult {
            success: true,
            events_generated,
            execution_time: start_time.elapsed(),
            data: HashMap::new(),
        })
    }

    fn description(&self) -> &str {
        "Execute multi-round protocol with threshold participants"
    }
}

// ============================================================================
// Template Choreographies
// ============================================================================

/// Template protocol choreography
pub struct TemplateProtocolChoreography;

impl ChoreographyExecutor for TemplateProtocolChoreography {
    fn execute(
        &self,
        world_state: &mut WorldState,
        participants: &[String],
        parameters: &HashMap<String, toml::Value>,
    ) -> Result<ChoreographyResult> {
        let start_time = Instant::now();

        let threshold = parameters
            .get("threshold")
            .and_then(|v| v.as_integer())
            .unwrap_or(2) as usize;

        let mut params = HashMap::new();
        params.insert("threshold".to_string(), threshold.to_string());

        let protocol = QueuedProtocol {
            protocol_type: "TemplateProtocol".to_string(),
            participants: participants.to_vec(),
            parameters: params,
            scheduled_time: world_state.current_time + 100,
            priority: 1,
        };

        world_state.protocols.execution_queue.push_back(protocol);

        let mut events_generated = 0;
        for _ in 0..20 {
            let events = tick(world_state)?;
            events_generated += events.len();
        }

        Ok(ChoreographyResult {
            success: true,
            events_generated,
            execution_time: start_time.elapsed(),
            data: HashMap::new(),
        })
    }

    fn description(&self) -> &str {
        "Template protocol for scenario testing"
    }
}

// ============================================================================
// Test Choreography
// ============================================================================

/// Simple test choreography
pub struct TestChoreography;

impl ChoreographyExecutor for TestChoreography {
    fn execute(
        &self,
        world_state: &mut WorldState,
        participants: &[String],
        _parameters: &HashMap<String, toml::Value>,
    ) -> Result<ChoreographyResult> {
        let start_time = Instant::now();

        let protocol = QueuedProtocol {
            protocol_type: "Test".to_string(),
            participants: participants.to_vec(),
            parameters: HashMap::new(),
            scheduled_time: world_state.current_time + 100,
            priority: 1,
        };

        world_state.protocols.execution_queue.push_back(protocol);

        let mut events_generated = 0;
        for _ in 0..5 {
            let events = tick(world_state)?;
            events_generated += events.len();
        }

        Ok(ChoreographyResult {
            success: true,
            events_generated,
            execution_time: start_time.elapsed(),
            data: HashMap::new(),
        })
    }

    fn description(&self) -> &str {
        "Simple test choreography"
    }
}

// ============================================================================
// Registration Helper
// ============================================================================

/// Register all standard choreographies with the engine
pub fn register_all_standard_choreographies(
    engine: &mut crate::scenario::engine::UnifiedScenarioEngine,
) {
    // Core protocols
    engine.register_choreography("handshake".to_string(), HandshakeChoreography);
    engine.register_choreography(
        "session_establishment".to_string(),
        SessionEstablishmentChoreography,
    );
    engine.register_choreography("p2p_dkd".to_string(), P2pDkdChoreography);
    engine.register_choreography(
        "context_agreement".to_string(),
        ContextAgreementChoreography,
    );
    engine.register_choreography(
        "session_operation".to_string(),
        SessionOperationChoreography,
    );

    // FROST protocols
    engine.register_choreography(
        "frost_key_generation".to_string(),
        FrostKeyGenerationChoreography,
    );
    engine.register_choreography("frost_commitment".to_string(), FrostCommitmentChoreography);
    engine.register_choreography("frost_signing".to_string(), FrostSigningChoreography);

    // Account lifecycle
    engine.register_choreography("account_creation".to_string(), AccountCreationChoreography);
    engine.register_choreography(
        "guardian_configuration".to_string(),
        GuardianConfigurationChoreography,
    );
    engine.register_choreography(
        "guardian_recovery".to_string(),
        GuardianRecoveryChoreography,
    );

    // Session management
    engine.register_choreography("epoch_increment".to_string(), EpochIncrementChoreography);
    engine.register_choreography(
        "presence_ticket_distribution".to_string(),
        PresenceTicketDistributionChoreography,
    );

    // CRDT and state
    engine.register_choreography(
        "crdt_initialization".to_string(),
        CrdtInitializationChoreography,
    );
    engine.register_choreography("crdt_update".to_string(), CrdtUpdateChoreography);
    engine.register_choreography("counter_init".to_string(), CounterInitChoreography);
    engine.register_choreography(
        "counter_increment".to_string(),
        CounterIncrementChoreography,
    );

    // Group communication
    engine.register_choreography(
        "group_initialization".to_string(),
        GroupInitializationChoreography,
    );
    engine.register_choreography("group_broadcast".to_string(), GroupBroadcastChoreography);

    // Transport
    engine.register_choreography("gossip_setup".to_string(), GossipSetupChoreography);
    engine.register_choreography("gossip_broadcast".to_string(), GossipBroadcastChoreography);

    // Multi-round
    engine.register_choreography(
        "multi_round_protocol".to_string(),
        MultiRoundProtocolChoreography,
    );

    // Template
    engine.register_choreography(
        "template_protocol".to_string(),
        TemplateProtocolChoreography,
    );

    // Test
    engine.register_choreography("test".to_string(), TestChoreography);
}
