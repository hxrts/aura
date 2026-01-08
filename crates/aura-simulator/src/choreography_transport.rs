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

/// Message queue key type: (from_role, to_role)
type MessageQueueKey = (ChoreographicRole, ChoreographicRole);
/// Message queue storage type
type MessageQueues = HashMap<MessageQueueKey, VecDeque<Vec<u8>>>;

/// Shared message bus for simulation
///
/// This structure holds the message queues for all roles in a simulation.
/// Messages are keyed by (from_role, to_role) pairs for deterministic ordering.
#[derive(Debug, Default)]
pub struct SimulatedMessageBus {
    /// Message queues: (from_role, to_role) -> queue of messages
    queues: RwLock<MessageQueues>,
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

// ===========================================================================
// TestEffectSystem - Combined effects for choreography protocol testing
// ===========================================================================

use aura_core::effects::authorization::{AuthorizationDecision, AuthorizationError};
use aura_core::effects::leakage::{LeakageBudget, LeakageEvent};
use aura_core::effects::storage::{StorageError, StorageStats};
use aura_core::effects::time::{
    LogicalClockEffects, OrderClockEffects, PhysicalTimeEffects, TimeError,
};
use aura_core::effects::{
    BiscuitAuthorizationEffects, FlowBudgetEffects, JournalEffects, RandomCoreEffects,
    StorageCoreEffects, StorageExtendedEffects,
};
use aura_core::flow::{FlowBudget, Receipt};
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::scope::{AuthorizationOp, ResourceScope};
use aura_core::time::{LogicalTime, OrderTime, PhysicalTime, VectorClock};
use aura_core::ExecutionMode;
use aura_core::{AuraError, FlowCost, Journal, Result as AuraResult};
use aura_guards::guards::GuardContextProvider;

/// Combined effect system for choreography protocol testing.
///
/// This type combines `SimulatedTransport` (for choreographic effects) with
/// mock implementations of all other required traits (guard effects, time effects, etc.).
/// This allows protocol tests to use `AuraProtocolAdapter` with its full trait bounds.
///
/// # Example
///
/// ```ignore
/// let bus = Arc::new(SimulatedMessageBus::new());
/// let effects = TestEffectSystem::new(bus.clone(), device_id, role_index)?;
/// let adapter = AuraProtocolAdapter::new(Arc::new(effects), authority_id, role, role_map);
/// ```
pub struct TestEffectSystem {
    /// The simulated transport for choreographic effects
    transport: SimulatedTransport,
    /// Authority ID for guard context
    authority_id: AuthorityId,
    /// Storage backend (simple in-memory)
    storage: std::sync::Mutex<HashMap<String, Vec<u8>>>,
    /// Physical time counter (deterministic)
    physical_time_ms: std::sync::Mutex<u64>,
    /// Logical clock state
    logical_clock: std::sync::Mutex<LogicalTime>,
}

impl std::fmt::Debug for TestEffectSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TestEffectSystem")
            .field("transport", &self.transport)
            .field("authority_id", &self.authority_id)
            .finish()
    }
}

impl TestEffectSystem {
    /// Create a new test effect system
    pub fn new(
        bus: Arc<SimulatedMessageBus>,
        device_id: DeviceId,
        role_index: u32,
    ) -> Option<Self> {
        let transport = SimulatedTransport::new(bus, device_id, role_index)?;
        Some(Self {
            transport,
            authority_id: AuthorityId::from_uuid(device_id.uuid()),
            storage: std::sync::Mutex::new(HashMap::new()),
            physical_time_ms: std::sync::Mutex::new(1640995200000), // 2022-01-01
            logical_clock: std::sync::Mutex::new(LogicalTime {
                vector: VectorClock::default(),
                lamport: 0,
            }),
        })
    }

    /// Create with explicit authority ID
    pub fn with_authority(mut self, authority_id: AuthorityId) -> Self {
        self.authority_id = authority_id;
        self
    }

    /// Get the underlying transport
    pub fn transport(&self) -> &SimulatedTransport {
        &self.transport
    }
}

// Delegate ChoreographicEffects to transport
#[async_trait]
impl ChoreographicEffects for TestEffectSystem {
    async fn send_to_role_bytes(
        &self,
        role: ChoreographicRole,
        message: Vec<u8>,
    ) -> Result<(), ChoreographyError> {
        self.transport.send_to_role_bytes(role, message).await
    }

    async fn receive_from_role_bytes(
        &self,
        role: ChoreographicRole,
    ) -> Result<Vec<u8>, ChoreographyError> {
        self.transport.receive_from_role_bytes(role).await
    }

    async fn broadcast_bytes(&self, message: Vec<u8>) -> Result<(), ChoreographyError> {
        self.transport.broadcast_bytes(message).await
    }

    fn current_role(&self) -> ChoreographicRole {
        self.transport.current_role()
    }

    fn all_roles(&self) -> Vec<ChoreographicRole> {
        self.transport.all_roles()
    }

    async fn is_role_active(&self, role: ChoreographicRole) -> bool {
        self.transport.is_role_active(role).await
    }

    async fn start_session(
        &self,
        session_id: Uuid,
        roles: Vec<ChoreographicRole>,
    ) -> Result<(), ChoreographyError> {
        self.transport.start_session(session_id, roles).await
    }

    async fn end_session(&self) -> Result<(), ChoreographyError> {
        self.transport.end_session().await
    }

    async fn emit_choreo_event(&self, event: ChoreographyEvent) -> Result<(), ChoreographyError> {
        self.transport.emit_choreo_event(event).await
    }

    async fn set_timeout(&self, timeout_ms: u64) {
        self.transport.set_timeout(timeout_ms).await;
    }

    async fn get_metrics(&self) -> ChoreographyMetrics {
        self.transport.get_metrics().await
    }
}

// GuardContextProvider implementation
impl GuardContextProvider for TestEffectSystem {
    fn authority_id(&self) -> AuthorityId {
        self.authority_id
    }

    fn get_metadata(&self, _key: &str) -> Option<String> {
        None
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Testing
    }

    fn can_perform_operation(&self, _operation: &str) -> bool {
        true
    }
}

// PhysicalTimeEffects implementation
#[async_trait]
impl PhysicalTimeEffects for TestEffectSystem {
    async fn physical_time(&self) -> Result<PhysicalTime, TimeError> {
        let ts_ms = *self.physical_time_ms.lock().unwrap();
        Ok(PhysicalTime {
            ts_ms,
            uncertainty: None,
        })
    }

    async fn sleep_ms(&self, duration_ms: u64) -> Result<(), TimeError> {
        let mut ts = self.physical_time_ms.lock().unwrap();
        *ts += duration_ms;
        Ok(())
    }
}

// TimeEffects - extends PhysicalTimeEffects with current_timestamp (has default impl)
impl aura_core::TimeEffects for TestEffectSystem {}

// LogicalClockEffects implementation
#[async_trait]
impl LogicalClockEffects for TestEffectSystem {
    async fn logical_advance(
        &self,
        _observed: Option<&VectorClock>,
    ) -> Result<LogicalTime, TimeError> {
        let mut clock = self.logical_clock.lock().unwrap();
        clock.lamport += 1;
        Ok(clock.clone())
    }

    async fn logical_now(&self) -> Result<LogicalTime, TimeError> {
        let clock = self.logical_clock.lock().unwrap();
        Ok(clock.clone())
    }
}

#[async_trait]
impl OrderClockEffects for TestEffectSystem {
    async fn order_time(&self) -> Result<OrderTime, TimeError> {
        let ts = *self.physical_time_ms.lock().unwrap();
        let mut bytes = [0u8; 32];
        bytes[..8].copy_from_slice(&ts.to_le_bytes());
        Ok(OrderTime(bytes))
    }
}

// StorageEffects implementation
#[async_trait]
impl StorageCoreEffects for TestEffectSystem {
    async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
        let mut storage = self.storage.lock().unwrap();
        storage.insert(key.to_string(), value);
        Ok(())
    }

    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let storage = self.storage.lock().unwrap();
        Ok(storage.get(key).cloned())
    }

    async fn remove(&self, key: &str) -> Result<bool, StorageError> {
        let mut storage = self.storage.lock().unwrap();
        Ok(storage.remove(key).is_some())
    }

    async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
        let storage = self.storage.lock().unwrap();
        match prefix {
            Some(p) => Ok(storage
                .keys()
                .filter(|k| k.starts_with(p))
                .cloned()
                .collect()),
            None => Ok(storage.keys().cloned().collect()),
        }
    }
}

#[async_trait]
impl StorageExtendedEffects for TestEffectSystem {
    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        let storage = self.storage.lock().unwrap();
        Ok(storage.contains_key(key))
    }

    async fn store_batch(&self, pairs: HashMap<String, Vec<u8>>) -> Result<(), StorageError> {
        let mut storage = self.storage.lock().unwrap();
        for (key, value) in pairs {
            storage.insert(key, value);
        }
        Ok(())
    }

    async fn retrieve_batch(
        &self,
        keys: &[String],
    ) -> Result<HashMap<String, Vec<u8>>, StorageError> {
        let storage = self.storage.lock().unwrap();
        let mut result = HashMap::new();
        for key in keys {
            if let Some(value) = storage.get(key) {
                result.insert(key.clone(), value.clone());
            }
        }
        Ok(result)
    }

    async fn clear_all(&self) -> Result<(), StorageError> {
        let mut storage = self.storage.lock().unwrap();
        storage.clear();
        Ok(())
    }

    async fn stats(&self) -> Result<StorageStats, StorageError> {
        let storage = self.storage.lock().unwrap();
        Ok(StorageStats {
            key_count: storage.len() as u64,
            total_size: storage.values().map(|v| v.len() as u64).sum(),
            available_space: Some(u64::MAX),
            backend_type: "test".to_string(),
        })
    }
}

// FlowBudgetEffects implementation
#[async_trait]
impl FlowBudgetEffects for TestEffectSystem {
    async fn charge_flow(
        &self,
        _context: &ContextId,
        _authority: &AuthorityId,
        cost: FlowCost,
    ) -> AuraResult<Receipt> {
        use aura_core::types::Epoch;
        Ok(Receipt {
            ctx: ContextId::from_uuid(Uuid::nil()),
            src: self.authority_id,
            dst: self.authority_id,
            epoch: Epoch(0),
            cost,
            nonce: aura_core::FlowNonce::new(0),
            prev: aura_core::Hash32::new([0; 32]),
            sig: aura_core::ReceiptSig::new(vec![0xAB; 64])?,
        })
    }
}

// JournalEffects implementation
#[async_trait]
impl JournalEffects for TestEffectSystem {
    async fn merge_facts(&self, mut target: Journal, delta: Journal) -> Result<Journal, AuraError> {
        target.merge_facts(delta.facts);
        Ok(target)
    }

    async fn refine_caps(
        &self,
        mut target: Journal,
        refinement: Journal,
    ) -> Result<Journal, AuraError> {
        target.refine_caps(refinement.caps);
        Ok(target)
    }

    async fn get_journal(&self) -> Result<Journal, AuraError> {
        Ok(Journal::new())
    }

    async fn persist_journal(&self, _journal: &Journal) -> Result<(), AuraError> {
        Ok(())
    }

    async fn get_flow_budget(
        &self,
        _context: &ContextId,
        _authority: &AuthorityId,
    ) -> Result<FlowBudget, AuraError> {
        Ok(FlowBudget {
            limit: 1000,
            spent: 0,
            epoch: aura_core::types::Epoch(0),
        })
    }

    async fn update_flow_budget(
        &self,
        _context: &ContextId,
        _authority: &AuthorityId,
        budget: &FlowBudget,
    ) -> Result<FlowBudget, AuraError> {
        Ok(*budget)
    }

    async fn charge_flow_budget(
        &self,
        _context: &ContextId,
        _authority: &AuthorityId,
        _cost: FlowCost,
    ) -> Result<FlowBudget, AuraError> {
        Ok(FlowBudget {
            limit: 1000,
            spent: 0,
            epoch: aura_core::types::Epoch(0),
        })
    }
}

// RandomCoreEffects implementation
#[async_trait]
impl RandomCoreEffects for TestEffectSystem {
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        vec![0x42; len]
    }

    async fn random_bytes_32(&self) -> [u8; 32] {
        [0x42; 32]
    }

    async fn random_u64(&self) -> u64 {
        42
    }
}

// AuthorizationEffects implementation (capability lattice)
#[async_trait]
impl aura_core::effects::AuthorizationEffects for TestEffectSystem {
    async fn verify_capability(
        &self,
        _capabilities: &aura_core::Cap,
        _operation: AuthorizationOp,
        _scope: &ResourceScope,
    ) -> Result<bool, AuthorizationError> {
        // Test implementation: always authorize
        Ok(true)
    }

    async fn delegate_capabilities(
        &self,
        _source_capabilities: &aura_core::Cap,
        requested_capabilities: &aura_core::Cap,
        _target_authority: &AuthorityId,
    ) -> Result<aura_core::Cap, AuthorizationError> {
        // Test implementation: return requested capabilities as-is
        Ok(requested_capabilities.clone())
    }
}

// BiscuitAuthorizationEffects implementation
#[async_trait]
impl BiscuitAuthorizationEffects for TestEffectSystem {
    async fn authorize_biscuit(
        &self,
        _token_data: &[u8],
        _operation: AuthorizationOp,
        _scope: &ResourceScope,
    ) -> Result<AuthorizationDecision, AuthorizationError> {
        Ok(AuthorizationDecision {
            authorized: true,
            reason: Some("Test authorization".to_string()),
        })
    }

    async fn authorize_fact(
        &self,
        _token_data: &[u8],
        _fact_type: &str,
        _scope: &ResourceScope,
    ) -> Result<bool, AuthorizationError> {
        Ok(true)
    }
}

// LeakageEffects implementation
#[async_trait]
impl aura_core::effects::LeakageEffects for TestEffectSystem {
    async fn record_leakage(&self, _event: LeakageEvent) -> AuraResult<()> {
        Ok(())
    }

    async fn get_leakage_budget(&self, _context_id: ContextId) -> AuraResult<LeakageBudget> {
        Ok(LeakageBudget::default())
    }

    async fn check_leakage_budget(
        &self,
        _context_id: ContextId,
        _observer: aura_core::effects::leakage::ObserverClass,
        _amount: u64,
    ) -> AuraResult<bool> {
        Ok(true)
    }

    async fn get_leakage_history(
        &self,
        _context_id: ContextId,
        _since_timestamp: Option<&aura_core::time::PhysicalTime>,
    ) -> AuraResult<Vec<LeakageEvent>> {
        Ok(Vec::new())
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

        // Start session (use nil UUID for determinism in tests)
        transport1
            .start_session(Uuid::nil(), vec![role1, role2])
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
