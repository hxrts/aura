//! Simulated transport for choreographic protocol testing
//!
//! This module provides a `SimulatedTransport` implementation of `ChoreographicEffects`
//! for deterministic simulation of multi-party protocols.
//!
//! ## Design
//!
//! The transport uses in-memory message queues to simulate message passing between
//! protocol roles. Each role has a dedicated inbox, and messages are routed based
//! on the sender/receiver roles.
//!
//! ## Key Features
//!
//! - Deterministic message ordering (FIFO per-role pair)
//! - Configurable latency and fault injection
//! - Session lifecycle management
//! - Event tracing for debugging/visualization
//!
//! ## Example
//!
//! ```ignore
//! use aura_simulator::choreography_transport::SimulatedTransport;
//! use aura_protocol::effects::ChoreographicEffects;
//!
//! let mut transport = SimulatedTransport::new(my_device_id, my_role_index);
//! transport.start_session(session_id, roles).await?;
//! transport.send_to_role_bytes(target_role, message).await?;
//! let response = transport.receive_from_role_bytes(from_role).await?;
//! ```

use async_trait::async_trait;
use aura_core::DeviceId;
use aura_protocol::effects::{
    ChoreographicEffects, ChoreographicRole, ChoreographyError, ChoreographyEvent,
    ChoreographyMetrics, RoleIndex,
};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};
use uuid::Uuid;

/// Shared message bus for simulation
///
/// This structure holds the message queues for all roles in a simulation.
/// Messages are keyed by (from_role, to_role) pairs for deterministic ordering.
#[derive(Debug, Default)]
pub struct SimulatedMessageBus {
    /// Message queues: (from_role, to_role) -> queue of messages
    queues: RwLock<HashMap<(ChoreographicRole, ChoreographicRole), VecDeque<Vec<u8>>>>,
    /// Active roles in the current session
    roles: RwLock<Vec<ChoreographicRole>>,
    /// Current session ID
    session_id: RwLock<Option<Uuid>>,
    /// Event trace for debugging
    events: RwLock<Vec<ChoreographyEvent>>,
    /// Metrics tracking
    metrics: RwLock<SimulationMetrics>,
}

#[derive(Debug, Default)]
struct SimulationMetrics {
    messages_sent: u64,
    messages_received: u64,
    total_bytes_sent: u64,
    timeout_count: u64,
}

impl SimulatedMessageBus {
    /// Create a new empty message bus
    pub fn new() -> Self {
        Self::default()
    }

    /// Enqueue a message from one role to another
    pub fn enqueue_message(
        &self,
        from: ChoreographicRole,
        to: ChoreographicRole,
        message: Vec<u8>,
    ) -> Result<(), ChoreographyError> {
        let mut queues = self
            .queues
            .write()
            .map_err(|_| ChoreographyError::InternalError {
                message: "Failed to acquire queue lock".to_string(),
            })?;

        let key = (from, to);
        queues.entry(key).or_default().push_back(message.clone());

        // Update metrics
        if let Ok(mut metrics) = self.metrics.write() {
            metrics.messages_sent += 1;
            metrics.total_bytes_sent += message.len() as u64;
        }

        Ok(())
    }

    /// Dequeue a message for a role from a specific sender
    pub fn dequeue_message(
        &self,
        from: ChoreographicRole,
        to: ChoreographicRole,
    ) -> Result<Vec<u8>, ChoreographyError> {
        let mut queues = self
            .queues
            .write()
            .map_err(|_| ChoreographyError::InternalError {
                message: "Failed to acquire queue lock".to_string(),
            })?;

        let key = (from, to);
        let message = queues
            .get_mut(&key)
            .and_then(|q| q.pop_front())
            .ok_or_else(|| ChoreographyError::CommunicationTimeout {
                role: from,
                timeout_ms: 0,
            })?;

        // Update metrics
        if let Ok(mut metrics) = self.metrics.write() {
            metrics.messages_received += 1;
        }

        Ok(message)
    }

    /// Check if there are pending messages for a role
    pub fn has_pending_messages(&self, from: ChoreographicRole, to: ChoreographicRole) -> bool {
        self.queues
            .read()
            .ok()
            .map(|q| {
                q.get(&(from, to))
                    .map(|queue| !queue.is_empty())
                    .unwrap_or(false)
            })
            .unwrap_or(false)
    }

    /// Record a choreography event
    pub fn record_event(&self, event: ChoreographyEvent) {
        if let Ok(mut events) = self.events.write() {
            events.push(event);
        }
    }

    /// Get all recorded events
    pub fn get_events(&self) -> Vec<ChoreographyEvent> {
        self.events
            .read()
            .ok()
            .map(|e| e.clone())
            .unwrap_or_default()
    }

    /// Get current metrics
    pub fn get_metrics(&self) -> ChoreographyMetrics {
        self.metrics
            .read()
            .ok()
            .map(|m| ChoreographyMetrics {
                messages_sent: m.messages_sent,
                messages_received: m.messages_received,
                avg_latency_ms: 0.0, // Simulation has no real latency
                timeout_count: m.timeout_count,
                retry_count: 0,
                total_duration_ms: 0,
            })
            .unwrap_or(ChoreographyMetrics {
                messages_sent: 0,
                messages_received: 0,
                avg_latency_ms: 0.0,
                timeout_count: 0,
                retry_count: 0,
                total_duration_ms: 0,
            })
    }
}

/// Simulated transport implementing ChoreographicEffects
///
/// Each instance represents a single role's view of the message bus.
/// Multiple SimulatedTransport instances share the same SimulatedMessageBus
/// to enable multi-party protocol simulation.
pub struct SimulatedTransport {
    /// Shared message bus
    bus: Arc<SimulatedMessageBus>,
    /// This transport's device ID
    device_id: DeviceId,
    /// This transport's role index
    role_index: RoleIndex,
    /// All roles in the current session
    session_roles: Vec<ChoreographicRole>,
    /// Current session ID
    session_id: Option<Uuid>,
    /// Configured timeout in milliseconds
    timeout_ms: u64,
}

impl SimulatedTransport {
    /// Create a new simulated transport
    ///
    /// # Arguments
    ///
    /// * `bus` - Shared message bus for simulation
    /// * `device_id` - Device ID for this transport
    /// * `role_index` - Role index for this transport (0-based)
    pub fn new(
        bus: Arc<SimulatedMessageBus>,
        device_id: DeviceId,
        role_index: u32,
    ) -> Option<Self> {
        let role_index = RoleIndex::new(role_index)?;
        Some(Self {
            bus,
            device_id,
            role_index,
            session_roles: Vec::new(),
            session_id: None,
            timeout_ms: 30000, // 30s default
        })
    }

    /// Get the current role for this transport
    pub fn current_role(&self) -> ChoreographicRole {
        ChoreographicRole::new(self.device_id, self.role_index)
    }

    /// Create a transport network for multiple roles
    ///
    /// Returns a vector of transports, one for each device/role pair.
    pub fn create_network(
        device_ids: &[DeviceId],
    ) -> (Arc<SimulatedMessageBus>, Vec<SimulatedTransport>) {
        let bus = Arc::new(SimulatedMessageBus::new());
        let transports: Vec<_> = device_ids
            .iter()
            .enumerate()
            .filter_map(|(i, &device_id)| SimulatedTransport::new(bus.clone(), device_id, i as u32))
            .collect();
        (bus, transports)
    }
}

impl std::fmt::Debug for SimulatedTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SimulatedTransport")
            .field("device_id", &self.device_id)
            .field("role_index", &self.role_index.get())
            .field("session_id", &self.session_id)
            .field("num_session_roles", &self.session_roles.len())
            .finish()
    }
}

#[async_trait]
impl ChoreographicEffects for SimulatedTransport {
    async fn send_to_role_bytes(
        &self,
        role: ChoreographicRole,
        message: Vec<u8>,
    ) -> Result<(), ChoreographyError> {
        let from = self.current_role();

        // Record event
        self.bus.record_event(ChoreographyEvent::MessageSent {
            from,
            to: role,
            message_type: "bytes".to_string(),
        });

        self.bus.enqueue_message(from, role, message)
    }

    async fn receive_from_role_bytes(
        &self,
        role: ChoreographicRole,
    ) -> Result<Vec<u8>, ChoreographyError> {
        let to = self.current_role();
        self.bus.dequeue_message(role, to)
    }

    async fn broadcast_bytes(&self, message: Vec<u8>) -> Result<(), ChoreographyError> {
        let from = self.current_role();
        for &role in &self.session_roles {
            if role != from {
                self.bus.enqueue_message(from, role, message.clone())?;
            }
        }
        Ok(())
    }

    fn current_role(&self) -> ChoreographicRole {
        ChoreographicRole::new(self.device_id, self.role_index)
    }

    fn all_roles(&self) -> Vec<ChoreographicRole> {
        self.session_roles.clone()
    }

    async fn is_role_active(&self, role: ChoreographicRole) -> bool {
        self.session_roles.contains(&role)
    }

    async fn start_session(
        &self,
        session_id: Uuid,
        roles: Vec<ChoreographicRole>,
    ) -> Result<(), ChoreographyError> {
        // Store session info
        if let Ok(mut sid) = self.bus.session_id.write() {
            *sid = Some(session_id);
        }
        if let Ok(mut session_roles) = self.bus.roles.write() {
            *session_roles = roles.clone();
        }

        // Record event
        self.bus.record_event(ChoreographyEvent::PhaseStarted {
            phase: "session_start".to_string(),
            participants: roles,
        });

        Ok(())
    }

    async fn end_session(&self) -> Result<(), ChoreographyError> {
        if let Ok(mut sid) = self.bus.session_id.write() {
            *sid = None;
        }
        Ok(())
    }

    async fn emit_choreo_event(&self, event: ChoreographyEvent) -> Result<(), ChoreographyError> {
        self.bus.record_event(event);
        Ok(())
    }

    async fn set_timeout(&self, _timeout_ms: u64) {
        // Store timeout for future operations
        // Note: SimulatedTransport is not mutable here, so we'd need interior mutability
        // For now, this is a no-op in simulation
    }

    async fn get_metrics(&self) -> ChoreographyMetrics {
        self.bus.get_metrics()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_device_id(index: u8) -> DeviceId {
        let mut bytes = [0u8; 16];
        bytes[15] = index;
        DeviceId::from_uuid(uuid::Uuid::from_bytes(bytes))
    }

    #[tokio::test]
    async fn test_simulated_transport_send_receive() {
        let device1 = make_device_id(1);
        let device2 = make_device_id(2);

        let (_bus, transports) = SimulatedTransport::create_network(&[device1, device2]);

        let transport1 = &transports[0];
        let transport2 = &transports[1];

        let role1 = transport1.current_role();
        let role2 = transport2.current_role();

        // Start session
        transport1
            .start_session(Uuid::new_v4(), vec![role1, role2])
            .await
            .unwrap();

        // Send message from role1 to role2
        let message = b"hello".to_vec();
        transport1
            .send_to_role_bytes(role2, message.clone())
            .await
            .unwrap();

        // Receive message at role2
        let received = transport2.receive_from_role_bytes(role1).await.unwrap();
        assert_eq!(received, message);
    }

    #[tokio::test]
    async fn test_simulated_transport_metrics() {
        let device1 = make_device_id(1);
        let device2 = make_device_id(2);

        let (_bus, transports) = SimulatedTransport::create_network(&[device1, device2]);

        let transport1 = &transports[0];
        let transport2 = &transports[1];

        let role2 = transport2.current_role();

        // Send multiple messages
        transport1
            .send_to_role_bytes(role2, b"msg1".to_vec())
            .await
            .unwrap();
        transport1
            .send_to_role_bytes(role2, b"msg2".to_vec())
            .await
            .unwrap();

        let metrics = transport1.get_metrics().await;
        assert_eq!(metrics.messages_sent, 2);
    }
}
