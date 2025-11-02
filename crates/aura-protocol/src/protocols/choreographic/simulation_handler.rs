//! Simulation choreography handler for deterministic testing
//!
//! This handler integrates with Aura's simulation framework to enable:
//! - Deterministic message passing and timing
//! - Event recording for replay and debugging
//! - Integration with console visualization
//! - Time-travel debugging capabilities

use crate::{
    context::BaseContext, effects::ProtocolEffects, middleware::handler::AuraProtocolHandler,
};
use aura_types::DeviceId;
use rumpsteak_choreography::{ChoreoHandler, ChoreographyError, Label};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// Event types for choreographic protocols
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChoreoEvent {
    /// Protocol started
    ProtocolStarted {
        protocol_type: String,
        participants: Vec<BridgedRole>,
        timestamp: u64,
    },

    /// Message sent between participants
    MessageSent {
        from: BridgedRole,
        to: BridgedRole,
        message_type: String,
        payload_size: usize,
        timestamp: u64,
    },

    /// Message received by participant
    MessageReceived {
        from: BridgedRole,
        to: BridgedRole,
        message_type: String,
        timestamp: u64,
    },

    /// Choice made in choreography
    ChoiceMade {
        from: BridgedRole,
        to: BridgedRole,
        label: String,
        timestamp: u64,
    },

    /// Protocol completed
    ProtocolCompleted {
        protocol_type: String,
        success: bool,
        duration_ms: u64,
        timestamp: u64,
    },

    /// Protocol failed
    ProtocolFailed {
        protocol_type: String,
        error: String,
        timestamp: u64,
    },

    /// Coordinator elected via lottery
    CoordinatorElected {
        coordinator: BridgedRole,
        participants: Vec<BridgedRole>,
        timestamp: u64,
    },

    /// Session epoch bumped
    EpochBumped {
        old_epoch: u64,
        new_epoch: u64,
        reason: String,
        timestamp: u64,
    },
}

use super::{BridgedEndpoint, BridgedRole};

/// Simulation configuration for choreographic protocols
#[derive(Debug, Clone)]
pub struct SimulationConfig {
    /// Whether to record all events
    pub record_events: bool,

    /// Whether to enable deterministic timing
    pub deterministic_timing: bool,

    /// Message delay in milliseconds (for testing timeouts)
    pub message_delay_ms: u64,

    /// Random seed for deterministic randomness
    pub random_seed: u64,

    /// Whether to simulate network failures
    pub simulate_failures: bool,

    /// Failure rate (0.0 - 1.0)
    pub failure_rate: f64,
}

impl Default for SimulationConfig {
    fn default() -> Self {
        Self {
            record_events: true,
            deterministic_timing: true,
            message_delay_ms: 0,
            random_seed: 42,
            simulate_failures: false,
            failure_rate: 0.0,
        }
    }
}

/// Message queue entry
#[derive(Debug, Clone)]
struct QueuedMessage {
    from: BridgedRole,
    to: BridgedRole,
    payload: Vec<u8>,
    message_type: String,
    delivery_time: u64,
}

/// Simulation handler for choreographic protocols
pub struct SimulationChoreoHandler<H: AuraProtocolHandler, E: ProtocolEffects> {
    /// Underlying Aura handler
    handler: H,

    /// Protocol effects
    effects: E,

    /// Base context
    context: BaseContext,

    /// Simulation configuration
    config: SimulationConfig,

    /// Recorded events
    events: Arc<Mutex<Vec<ChoreoEvent>>>,

    /// Message queues for each participant
    message_queues: HashMap<DeviceId, VecDeque<QueuedMessage>>,

    /// Current simulation time
    current_time: u64,

    /// Active protocol type
    protocol_type: Option<String>,
}

impl<H: AuraProtocolHandler, E: ProtocolEffects> SimulationChoreoHandler<H, E> {
    pub fn new(handler: H, effects: E, context: BaseContext, config: SimulationConfig) -> Self {
        Self {
            handler,
            effects,
            context,
            config,
            events: Arc::new(Mutex::new(Vec::new())),
            message_queues: HashMap::new(),
            current_time: 0,
            protocol_type: None,
        }
    }

    /// Get recorded events
    pub fn get_events(&self) -> Vec<ChoreoEvent> {
        self.events.lock().unwrap().clone()
    }

    /// Clear recorded events
    pub fn clear_events(&self) {
        self.events.lock().unwrap().clear();
    }

    /// Advance simulation time
    pub fn advance_time(&mut self, delta_ms: u64) {
        self.current_time += delta_ms;
        self.process_message_queues();
    }

    /// Set the active protocol type
    pub fn set_protocol_type(&mut self, protocol_type: String) {
        self.protocol_type = Some(protocol_type);
    }

    /// Process message queues and deliver messages that are ready
    fn process_message_queues(&mut self) {
        let mut events_to_record = Vec::new();
        let record_events = self.config.record_events;
        let current_time = self.current_time;

        for queue in self.message_queues.values_mut() {
            while let Some(msg) = queue.front() {
                if msg.delivery_time <= current_time {
                    let msg = queue.pop_front().unwrap();

                    if record_events {
                        events_to_record.push(ChoreoEvent::MessageReceived {
                            from: msg.from,
                            to: msg.to,
                            message_type: msg.message_type,
                            timestamp: current_time,
                        });
                    }
                } else {
                    break;
                }
            }
        }

        // Record events after borrowing is done
        for event in events_to_record {
            self.record_event(event);
        }
    }

    /// Record an event
    pub fn record_event(&self, event: ChoreoEvent) {
        if self.config.record_events {
            self.events.lock().unwrap().push(event);
        }
    }

    /// Record an event through effects
    async fn record_via_effects(&mut self, event: ChoreoEvent) {
        if self.config.record_events {
            self.effects.emit_choreo_event(event.clone()).await;
            self.events.lock().unwrap().push(event);
        }
    }

    /// Check if we should simulate a failure
    fn should_fail(&self) -> bool {
        if !self.config.simulate_failures {
            return false;
        }

        // Use deterministic randomness based on seed and current time
        let hash = self.config.random_seed.wrapping_mul(self.current_time + 1);
        let random = (hash as f64) / (u64::MAX as f64);

        random < self.config.failure_rate
    }

    /// Get the current simulation time
    pub fn current_time(&self) -> u64 {
        self.current_time
    }

    /// Set the current simulation time
    pub fn set_current_time(&mut self, time: u64) {
        self.current_time = time;
    }
}

#[async_trait::async_trait]
impl<H, E> ChoreoHandler for SimulationChoreoHandler<H, E>
where
    H: AuraProtocolHandler<Message = Vec<u8>> + Send + Sync + 'static,
    E: ProtocolEffects + Send + Sync + 'static,
    H::DeviceId: From<Uuid> + Into<Uuid>,
{
    type Role = BridgedRole;
    type Endpoint = BridgedEndpoint;

    async fn send<T: serde::Serialize + Send + Sync>(
        &mut self,
        _ep: &mut Self::Endpoint,
        to: Self::Role,
        msg: &T,
    ) -> Result<(), ChoreographyError> {
        // Simulate failure if configured
        if self.should_fail() {
            return Err(ChoreographyError::Transport(
                "Simulated send failure".to_string(),
            ));
        }

        // Serialize message
        let payload =
            serde_json::to_vec(msg).map_err(|e| ChoreographyError::Serialization(e.to_string()))?;

        let message_type = std::any::type_name::<T>().to_string();

        // Record send event
        let from = BridgedRole {
            device_id: self.context.device_id,
            role_index: 0, // TODO: Get actual role index
        };

        self.record_event(ChoreoEvent::MessageSent {
            from,
            to,
            message_type: message_type.clone(),
            payload_size: payload.len(),
            timestamp: self.current_time,
        });

        // Queue message for delivery
        let delivery_time = self.current_time + self.config.message_delay_ms;
        let queued_msg = QueuedMessage {
            from,
            to,
            payload: payload.clone(),
            message_type,
            delivery_time,
        };

        self.message_queues
            .entry(DeviceId::from(to.device_id))
            .or_default()
            .push_back(queued_msg);

        // Send through underlying handler
        self.handler
            .send_message(to.device_id.into(), payload)
            .await
            .map_err(|e| ChoreographyError::Transport(format!("{:?}", e)))?;

        Ok(())
    }

    async fn recv<T: serde::de::DeserializeOwned + Send>(
        &mut self,
        _ep: &mut Self::Endpoint,
        from: Self::Role,
    ) -> Result<T, ChoreographyError> {
        // Simulate failure if configured
        if self.should_fail() {
            return Err(ChoreographyError::Transport(
                "Simulated recv failure".to_string(),
            ));
        }

        // Process message queues to deliver any ready messages
        self.process_message_queues();

        // Check if we have a queued message
        if let Some(queue) = self
            .message_queues
            .get_mut(&DeviceId::from(self.context.device_id))
        {
            if let Some(pos) = queue.iter().position(|m| m.from == from) {
                let msg = queue.remove(pos).unwrap();

                let value = serde_json::from_slice(&msg.payload)
                    .map_err(|e| ChoreographyError::Serialization(e.to_string()))?;

                return Ok(value);
            }
        }

        // Fall back to underlying handler
        let payload = self
            .handler
            .receive_message(from.device_id.into())
            .await
            .map_err(|e| ChoreographyError::Transport(format!("{:?}", e)))?;

        serde_json::from_slice(payload.as_slice())
            .map_err(|e| ChoreographyError::Serialization(e.to_string()))
    }

    async fn choose(
        &mut self,
        _ep: &mut Self::Endpoint,
        to: Self::Role,
        choice: Label,
    ) -> Result<(), ChoreographyError> {
        // Record choice event
        let from = BridgedRole {
            device_id: self.context.device_id,
            role_index: 0, // TODO: Get actual role index
        };

        self.record_event(ChoreoEvent::ChoiceMade {
            from,
            to,
            label: choice.0.to_string(),
            timestamp: self.current_time,
        });

        // Serialize and send the label
        let serialized = bincode::serialize(&choice.0)
            .map_err(|e| ChoreographyError::Serialization(e.to_string()))?;

        self.handler
            .send_message(to.device_id.into(), serialized)
            .await
            .map_err(|e| ChoreographyError::Transport(format!("{:?}", e)))?;

        Ok(())
    }

    async fn offer(
        &mut self,
        _ep: &mut Self::Endpoint,
        from: Self::Role,
    ) -> Result<Label, ChoreographyError> {
        // Receive the choice
        let payload = self
            .handler
            .receive_message(from.device_id.into())
            .await
            .map_err(|e| ChoreographyError::Transport(format!("{:?}", e)))?;

        // Deserialize the label string
        let label_str: String = bincode::deserialize(payload.as_slice())
            .map_err(|e| ChoreographyError::Serialization(e.to_string()))?;

        // Simple validation
        if label_str.len() > 1000 {
            return Err(ChoreographyError::ProtocolViolation(
                "Label string too long".to_string(),
            ));
        }

        // Find matching static string
        let static_str = Box::leak(label_str.into_boxed_str());

        Ok(Label(static_str))
    }

    async fn with_timeout<F, T>(
        &mut self,
        _ep: &mut Self::Endpoint,
        _at: Self::Role,
        _dur: std::time::Duration,
        body: F,
    ) -> Result<T, ChoreographyError>
    where
        F: std::future::Future<Output = Result<T, ChoreographyError>> + Send,
    {
        // For simulation, we just execute without actual timeout
        // The timeout behavior is simulated via the TimeoutManager
        body.await
    }
}

impl<H, E> SimulationChoreoHandler<H, E>
where
    H: AuraProtocolHandler + Send + Sync + 'static,
    E: ProtocolEffects + Send + Sync + 'static,
    H::DeviceId: From<Uuid> + Into<Uuid>,
{
    /// Setup method that can be called manually before protocol execution
    pub async fn setup(&mut self) -> Result<(), ChoreographyError> {
        // Record protocol start if protocol type is set
        if let Some(ref protocol_type) = self.protocol_type {
            self.record_event(ChoreoEvent::ProtocolStarted {
                protocol_type: protocol_type.clone(),
                participants: vec![], // Will be filled by actual protocol
                timestamp: self.current_time,
            });
        }
        Ok(())
    }

    /// Teardown method that can be called manually after protocol execution
    pub async fn teardown(&mut self) -> Result<(), ChoreographyError> {
        // Record protocol completion
        if let Some(ref protocol_type) = self.protocol_type {
            self.record_event(ChoreoEvent::ProtocolCompleted {
                protocol_type: protocol_type.clone(),
                success: true,
                duration_ms: self.current_time,
                timestamp: self.current_time,
            });
        }
        Ok(())
    }

    /// Get the current role for this handler
    pub fn current(&self, _ep: &mut BridgedEndpoint) -> BridgedRole {
        BridgedRole {
            device_id: self.context.device_id,
            role_index: 0, // TODO: Get actual role index
        }
    }
}

/// Create a simulation handler for testing
pub fn create_simulation_handler<H, E>(
    handler: H,
    effects: E,
    context: BaseContext,
    config: SimulationConfig,
) -> SimulationChoreoHandler<H, E>
where
    H: AuraProtocolHandler,
    E: ProtocolEffects,
{
    SimulationChoreoHandler::new(handler, effects, context, config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        effects::AuraEffectsAdapter, handlers::InMemoryHandler, middleware::EffectsMiddleware,
    };
    use aura_crypto::Effects;

    #[tokio::test]
    async fn test_simulation_handler_event_recording() {
        let effects = Effects::test(42);
        let device_id = Uuid::new_v4();

        let handler = EffectsMiddleware::new(InMemoryHandler::new(), effects.clone());

        let context = BaseContext::new(device_id, effects.clone(), None, None);

        let config = SimulationConfig {
            record_events: true,
            ..Default::default()
        };

        let mut sim_handler = SimulationChoreoHandler::new(
            handler,
            AuraEffectsAdapter::new(device_id, effects),
            context,
            config,
        );

        // Set protocol type
        sim_handler.set_protocol_type("TestProtocol".to_string());

        // Setup should record protocol start
        sim_handler.setup().await.unwrap();

        let events = sim_handler.get_events();
        assert_eq!(events.len(), 1);
        match &events[0] {
            ChoreoEvent::ProtocolStarted { protocol_type, .. } => {
                assert_eq!(protocol_type, "TestProtocol");
            }
            _ => panic!("Expected ProtocolStarted event"),
        }

        // Test message send recording
        let to_role = BridgedRole {
            device_id: Uuid::new_v4(),
            role_index: 1,
        };

        let mut endpoint = BridgedEndpoint::new(sim_handler.context.clone());

        sim_handler
            .send(&mut endpoint, to_role, &"test message")
            .await
            .unwrap();

        let events = sim_handler.get_events();
        assert_eq!(events.len(), 2);
        match &events[1] {
            ChoreoEvent::MessageSent {
                message_type,
                payload_size,
                ..
            } => {
                assert!(message_type.contains("&str"));
                assert!(*payload_size > 0);
            }
            _ => panic!("Expected MessageSent event"),
        }
    }

    #[tokio::test]
    async fn test_simulation_handler_deterministic_failures() {
        let effects = Effects::test(42);
        let device_id = Uuid::new_v4();

        let handler = InMemoryHandler::new();
        let context = BaseContext::new(device_id, effects.clone(), None, None);

        let config = SimulationConfig {
            simulate_failures: true,
            failure_rate: 0.5,
            random_seed: 12345,
            ..Default::default()
        };

        let mut sim_handler = SimulationChoreoHandler::new(
            handler,
            AuraEffectsAdapter::new(device_id, effects),
            context,
            config,
        );

        let to_role = BridgedRole {
            device_id: Uuid::new_v4(),
            role_index: 1,
        };

        let mut endpoint = BridgedEndpoint::new(sim_handler.context.clone());

        // With a fixed seed and failure rate, we should get deterministic failures
        let mut failures = 0;
        let mut successes = 0;

        for i in 0..10 {
            sim_handler.current_time = i * 100; // Change time to vary randomness

            match sim_handler.send(&mut endpoint, to_role, &"test").await {
                Ok(_) => successes += 1,
                Err(ChoreographyError::Transport(_)) => failures += 1,
                Err(e) => panic!("Unexpected error: {:?}", e),
            }
        }

        // With 50% failure rate, we should see some of each
        assert!(failures > 0);
        assert!(successes > 0);

        // Reset and run again with same seed - should get same results
        sim_handler.current_time = 0;
        let mut failures2 = 0;
        let mut successes2 = 0;

        for i in 0..10 {
            sim_handler.current_time = i * 100;

            match sim_handler.send(&mut endpoint, to_role, &"test").await {
                Ok(_) => successes2 += 1,
                Err(ChoreographyError::Transport(_)) => failures2 += 1,
                Err(e) => panic!("Unexpected error: {:?}", e),
            }
        }

        assert_eq!(failures, failures2);
        assert_eq!(successes, successes2);
    }
}
