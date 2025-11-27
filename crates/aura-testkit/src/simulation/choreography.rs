//! Choreography Test Infrastructure
//!
//! This module provides multi-device test harnesses for choreographic protocol testing.
//! It extends aura-testkit with specialized infrastructure for testing distributed protocols
//! that require coordination across multiple simulated devices.
//!
//! Enhanced for stateless effect system architecture (work/021.md).

use crate::foundation::CompositeTestHandler;
use crate::{DeviceTestFixture, TestEffectsBuilder, TestExecutionMode};
use async_channel::{unbounded, Receiver, Sender};
use async_lock::RwLock;
use aura_core::DeviceId;
use std::collections::HashMap;
use std::sync::Arc;
// No futures import needed for sequential execution

/// Multi-device test harness for choreographic protocols
///
/// This harness manages multiple simulated devices with coordinated test contexts
/// and provides infrastructure for testing distributed choreographic protocols.
pub struct ChoreographyTestHarness {
    /// Test devices with their foundation-based contexts
    devices: Vec<(DeviceTestFixture, CompositeTestHandler)>,
    /// Transport coordinator for inter-device communication
    transport: Arc<MockChoreographyTransport>,
    /// Role mappings for choreographic execution
    role_mappings: HashMap<String, DeviceId>,
    /// Session coordinator for managing distributed sessions
    session_coordinator: Arc<MockSessionCoordinator>,
}

impl ChoreographyTestHarness {
    /// Create a new choreography test harness with the specified number of devices
    pub fn with_devices(count: usize) -> Self {
        let devices: Vec<(DeviceTestFixture, CompositeTestHandler)> = (0..count)
            .map(|i| {
                let device_fixture = DeviceTestFixture::new(i);
                let device_id = device_fixture.device_id();
                let test_context = TestEffectsBuilder::for_unit_tests(device_id)
                    .build()
                    .expect("Failed to create test context");
                (device_fixture, test_context)
            })
            .collect();

        let device_ids: Vec<DeviceId> = devices
            .iter()
            .map(|(fixture, _)| fixture.device_id())
            .collect();
        let transport = Arc::new(MockChoreographyTransport::new(device_ids.clone()));
        let session_coordinator = Arc::new(MockSessionCoordinator::new(device_ids));

        Self {
            devices,
            transport,
            role_mappings: HashMap::new(),
            session_coordinator,
        }
    }

    /// Create a harness with specific device labels for easier testing
    pub fn with_labeled_devices(labels: Vec<&str>) -> Self {
        let devices: Vec<(DeviceTestFixture, CompositeTestHandler)> = labels
            .into_iter()
            .enumerate()
            .map(|(i, label)| {
                let device_fixture = DeviceTestFixture::with_label(i, label.to_string());
                let device_id = device_fixture.device_id();
                let test_context = TestEffectsBuilder::for_unit_tests(device_id)
                    .build()
                    .expect("Failed to create test context");
                (device_fixture, test_context)
            })
            .collect();

        let device_ids: Vec<DeviceId> = devices
            .iter()
            .map(|(fixture, _)| fixture.device_id())
            .collect();
        let transport = Arc::new(MockChoreographyTransport::new(device_ids.clone()));
        let session_coordinator = Arc::new(MockSessionCoordinator::new(device_ids));

        Self {
            devices,
            transport,
            role_mappings: HashMap::new(),
            session_coordinator,
        }
    }

    /// Create harness from device fixtures using stateless effects (new API)
    ///
    /// This method uses the new stateless effect system architecture and is the
    /// recommended way to create choreography harnesses going forward.
    pub async fn from_fixtures(
        fixtures: Vec<DeviceTestFixture>,
        execution_mode: TestExecutionMode,
    ) -> Result<Self, TestError> {
        let mut devices = Vec::new();

        for fixture in fixtures {
            // Use the new stateless effect system
            let effects_builder = match execution_mode {
                TestExecutionMode::UnitTest => {
                    TestEffectsBuilder::for_unit_tests(fixture.device_id())
                }
                TestExecutionMode::Integration => {
                    TestEffectsBuilder::for_integration_tests(fixture.device_id())
                }
                TestExecutionMode::Simulation => {
                    TestEffectsBuilder::for_simulation(fixture.device_id())
                }
            };

            let test_context =
                effects_builder
                    .build()
                    .map_err(|e| TestError::ChoreographyExecution {
                        reason: format!("Failed to create test context: {}", e),
                    })?;
            devices.push((fixture, test_context));
        }

        let device_ids: Vec<DeviceId> = devices
            .iter()
            .map(|(fixture, _)| fixture.device_id())
            .collect();
        let transport = Arc::new(MockChoreographyTransport::new(device_ids.clone()));
        let session_coordinator = Arc::new(MockSessionCoordinator::new(device_ids));

        Ok(Self {
            devices,
            transport,
            role_mappings: HashMap::new(),
            session_coordinator,
        })
    }

    /// Create harness using new stateless effects (new API)
    ///
    /// This provides a clean interface for creating choreography harnesses with
    /// the new stateless effect system architecture.
    pub async fn new_with_stateless_effects(
        devices: Vec<(DeviceTestFixture, CompositeTestHandler)>,
    ) -> Result<Self, TestError> {
        let device_ids: Vec<DeviceId> = devices
            .iter()
            .map(|(fixture, _)| fixture.device_id())
            .collect();

        let transport = Arc::new(MockChoreographyTransport::new(device_ids.clone()));
        let session_coordinator = Arc::new(MockSessionCoordinator::new(device_ids));

        Ok(Self {
            devices,
            transport,
            role_mappings: HashMap::new(),
            session_coordinator,
        })
    }

    /// Create for simulation scenarios with enhanced configuration
    pub async fn for_simulation(device_count: usize, _seed: u64) -> Result<Self, TestError> {
        let fixtures: Vec<DeviceTestFixture> =
            (0..device_count).map(DeviceTestFixture::new).collect();

        Self::from_fixtures(fixtures, TestExecutionMode::Simulation).await
    }

    /// Add simulator compatibility using stateless effects
    pub fn into_simulator_context(self) -> SimulatorCompatibleContext {
        SimulatorCompatibleContext {
            devices: self.devices,
            transport: self.transport,
            session_coordinator: self.session_coordinator,
        }
    }

    /// Add performance monitoring hooks
    pub fn with_performance_monitoring(self) -> Self {
        // Configure effect systems for performance tracking
        // This will be enhanced when the stateless effect system supports it
        self
    }

    pub fn get_performance_metrics(&self) -> PerformanceSnapshot {
        PerformanceSnapshot {
            device_count: self.devices.len(),
            // More metrics will be added when the stateless effect system supports them
        }
    }

    /// Map a choreographic role name to a device
    pub fn map_role(&mut self, role_name: &str, device_index: usize) -> Result<(), TestError> {
        if device_index >= self.devices.len() {
            return Err(TestError::InvalidDeviceIndex {
                index: device_index,
                max: self.devices.len(),
            });
        }

        let device_id = self.devices[device_index].0.device_id();
        self.role_mappings.insert(role_name.to_string(), device_id);
        Ok(())
    }

    /// Get the device ID for a choreographic role
    pub fn role_device_id(&self, role_name: &str) -> Option<DeviceId> {
        self.role_mappings.get(role_name).copied()
    }

    /// Get a device's test context by index
    pub fn device_context(&self, device_index: usize) -> Option<&CompositeTestHandler> {
        self.devices.get(device_index).map(|(_, context)| context)
    }

    /// Get a device fixture by index
    pub fn device_fixture(&self, device_index: usize) -> Option<&DeviceTestFixture> {
        self.devices.get(device_index).map(|(fixture, _)| fixture)
    }

    /// Get a device's test context for choreographic execution
    pub fn get_device_context(
        &self,
        device_index: usize,
    ) -> Result<&CompositeTestHandler, TestError> {
        if device_index >= self.devices.len() {
            return Err(TestError::InvalidDeviceIndex {
                index: device_index,
                max: self.devices.len(),
            });
        }

        Ok(&self.devices[device_index].1)
    }

    /// Execute a choreography across all devices using foundation-based contexts
    pub async fn execute_choreography<F, R>(&self, choreography_fn: F) -> Result<Vec<R>, TestError>
    where
        F: Fn(
            usize,
            &CompositeTestHandler,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<R, TestError>> + Send>>,
        R: Send,
    {
        let mut tasks = Vec::new();

        for (i, (_, context)) in self.devices.iter().enumerate() {
            let task = choreography_fn(i, context);
            tasks.push(task);
        }

        // Execute all device roles sequentially for now (foundation approach)
        let mut results = Vec::new();
        for task in tasks {
            results.push(task.await?);
        }
        Ok(results)
    }

    /// Create a coordinated session across all devices
    pub async fn create_coordinated_session(
        &self,
        session_type: &str,
    ) -> Result<CoordinatedSession, TestError> {
        let participants: Vec<DeviceId> = self
            .devices
            .iter()
            .map(|(fixture, _)| fixture.device_id())
            .collect();

        let session_id = self
            .session_coordinator
            .create_multi_device_session(session_type, participants)
            .await?;

        Ok(CoordinatedSession {
            session_id,
            coordinator: self.session_coordinator.clone(),
            devices: self
                .devices
                .iter()
                .map(|(fixture, _)| fixture.device_id())
                .collect(),
        })
    }

    /// Get the number of devices in the harness
    pub fn device_count(&self) -> usize {
        self.devices.len()
    }

    /// List all device IDs in the harness
    pub fn device_ids(&self) -> Vec<DeviceId> {
        self.devices
            .iter()
            .map(|(fixture, _)| fixture.device_id())
            .collect()
    }
}

/// Mock transport for choreographic message passing between test devices
#[derive(Debug)]
pub struct MockChoreographyTransport {
    /// Device registry
    devices: Vec<DeviceId>,
    /// Message channels between devices
    channels: Arc<RwLock<HashMap<(DeviceId, DeviceId), MockChannel>>>,
}

impl MockChoreographyTransport {
    pub fn new(devices: Vec<DeviceId>) -> Self {
        let channels = Arc::new(RwLock::new(HashMap::new()));

        Self { devices, channels }
    }

    /// Send a message from one device to another
    pub async fn send_message(
        &self,
        from: DeviceId,
        to: DeviceId,
        message: Vec<u8>,
    ) -> Result<(), TransportError> {
        let mut channels = self.channels.write().await;
        let channel = channels.entry((from, to)).or_insert_with(MockChannel::new);
        channel.send(message).await
    }

    /// Receive a message for a device
    pub async fn receive_message(
        &self,
        device_id: DeviceId,
        from: Option<DeviceId>,
    ) -> Result<Option<(DeviceId, Vec<u8>)>, TransportError> {
        let mut channels = self.channels.write().await;

        // If specific sender requested, check that channel
        if let Some(sender) = from {
            if let Some(channel) = channels.get_mut(&(sender, device_id)) {
                if let Some(message) = channel.try_receive().await? {
                    return Ok(Some((sender, message)));
                }
            }
            return Ok(None);
        }

        // Otherwise check all channels for this device
        for other_device in &self.devices {
            if *other_device != device_id {
                if let Some(channel) = channels.get_mut(&(*other_device, device_id)) {
                    if let Some(message) = channel.try_receive().await? {
                        return Ok(Some((*other_device, message)));
                    }
                }
            }
        }

        Ok(None)
    }
}

/// Mock channel for device-to-device communication
#[derive(Debug)]
pub struct MockChannel {
    messages: Sender<Vec<u8>>,
    receiver: Arc<RwLock<Receiver<Vec<u8>>>>,
}

impl MockChannel {
    pub fn new() -> Self {
        let (sender, receiver) = unbounded();
        Self {
            messages: sender,
            receiver: Arc::new(RwLock::new(receiver)),
        }
    }

    pub async fn send(&self, message: Vec<u8>) -> Result<(), TransportError> {
        self.messages
            .send(message)
            .await
            .map_err(|_| TransportError::ChannelClosed)?;
        Ok(())
    }

    pub async fn try_receive(&self) -> Result<Option<Vec<u8>>, TransportError> {
        let receiver = self.receiver.write().await;
        match receiver.try_recv() {
            Ok(message) => Ok(Some(message)),
            Err(async_channel::TryRecvError::Empty) => Ok(None),
            Err(async_channel::TryRecvError::Closed) => Err(TransportError::ChannelClosed),
        }
    }
}

impl Default for MockChannel {
    fn default() -> Self {
        Self::new()
    }
}

/// Mock session coordinator for multi-device sessions
#[derive(Debug)]
pub struct MockSessionCoordinator {
    /// Available devices
    _devices: Vec<DeviceId>,
    /// Active sessions
    sessions: Arc<RwLock<HashMap<String, MockSessionState>>>,
    /// Session counter for unique IDs
    session_counter: Arc<std::sync::atomic::AtomicU64>,
}

impl MockSessionCoordinator {
    pub fn new(devices: Vec<DeviceId>) -> Self {
        Self {
            _devices: devices,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            session_counter: Arc::new(std::sync::atomic::AtomicU64::new(1)),
        }
    }

    /// Create a multi-device session
    pub async fn create_multi_device_session(
        &self,
        session_type: &str,
        participants: Vec<DeviceId>,
    ) -> Result<String, TestError> {
        let session_id = format!(
            "test-session-{}-{}",
            session_type,
            self.session_counter
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        );

        let session_state = MockSessionState {
            session_type: session_type.to_string(),
            participants: participants.clone(),
            status: SessionStatus::Active,
            created_at: 1000, // Fixed timestamp for testing
        };

        let mut sessions = self.sessions.write().await;
        sessions.insert(session_id.clone(), session_state);

        Ok(session_id)
    }

    /// Get session state
    pub async fn get_session_state(&self, session_id: &str) -> Option<MockSessionState> {
        let sessions = self.sessions.read().await;
        sessions.get(session_id).cloned()
    }

    /// End a session
    pub async fn end_session(&self, session_id: &str) -> Result<(), TestError> {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.status = SessionStatus::Ended;
            Ok(())
        } else {
            Err(TestError::SessionNotFound {
                session_id: session_id.to_string(),
            })
        }
    }

    /// List all active sessions
    pub async fn list_active_sessions(&self) -> Vec<String> {
        let sessions = self.sessions.read().await;
        sessions
            .iter()
            .filter(|(_, state)| matches!(state.status, SessionStatus::Active))
            .map(|(id, _)| id.clone())
            .collect()
    }
}

/// Coordinated session spanning multiple devices
pub struct CoordinatedSession {
    session_id: String,
    coordinator: Arc<MockSessionCoordinator>,
    devices: Vec<DeviceId>,
}

impl CoordinatedSession {
    /// Get the session ID
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Get participating device IDs
    pub fn participants(&self) -> &[DeviceId] {
        &self.devices
    }

    /// End the coordinated session
    pub async fn end(self) -> Result<(), TestError> {
        self.coordinator.end_session(&self.session_id).await
    }

    /// Get session status
    pub async fn status(&self) -> Result<MockSessionState, TestError> {
        self.coordinator
            .get_session_state(&self.session_id)
            .await
            .ok_or_else(|| TestError::SessionNotFound {
                session_id: self.session_id.clone(),
            })
    }
}

/// Mock session state for testing
#[derive(Debug, Clone)]
pub struct MockSessionState {
    pub session_type: String,
    pub participants: Vec<DeviceId>,
    pub status: SessionStatus,
    pub created_at: u64,
}

/// Session status
#[derive(Debug, Clone, PartialEq)]
pub enum SessionStatus {
    Active,
    Ended,
}

/// Test-specific errors
#[derive(Debug)]
pub enum TestError {
    InvalidDeviceIndex { index: usize, max: usize },
    SessionNotFound { session_id: String },
    Transport(TransportError),
    ChoreographyExecution { reason: String },
}

impl std::fmt::Display for TestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TestError::InvalidDeviceIndex { index, max } => {
                write!(f, "Invalid device index {}, max is {}", index, max)
            }
            TestError::SessionNotFound { session_id } => {
                write!(f, "Session not found: {}", session_id)
            }
            TestError::Transport(err) => write!(f, "Transport error: {}", err),
            TestError::ChoreographyExecution { reason } => {
                write!(f, "Choreography execution failed: {}", reason)
            }
        }
    }
}

impl std::error::Error for TestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            TestError::Transport(err) => Some(err),
            _ => None,
        }
    }
}

impl From<TransportError> for TestError {
    fn from(err: TransportError) -> Self {
        TestError::Transport(err)
    }
}

/// Transport-specific errors
#[derive(Debug)]
pub enum TransportError {
    /// Communication channel was closed
    ChannelClosed,
    /// Message serialization failed
    SerializationError,
    /// Device not found in transport
    DeviceNotFound {
        /// The device ID that was not found
        device_id: DeviceId,
    },
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransportError::ChannelClosed => write!(f, "Communication channel closed"),
            TransportError::SerializationError => write!(f, "Message serialization failed"),
            TransportError::DeviceNotFound { device_id } => {
                write!(f, "Device not found: {}", device_id)
            }
        }
    }
}

impl std::error::Error for TransportError {}

/// Simulator-compatible context for bridging to aura-simulator
///
/// This type provides the bridge between testkit choreography infrastructure
/// and the aura-simulator effect system.
pub struct SimulatorCompatibleContext {
    /// Test devices and their foundation test contexts
    pub devices: Vec<(DeviceTestFixture, CompositeTestHandler)>,
    /// Mock transport layer for choreography
    pub transport: Arc<MockChoreographyTransport>,
    /// Mock session coordinator
    pub session_coordinator: Arc<MockSessionCoordinator>,
}

impl SimulatorCompatibleContext {
    /// Get device count
    pub fn device_count(&self) -> usize {
        self.devices.len()
    }

    /// Get device IDs
    pub fn device_ids(&self) -> Vec<DeviceId> {
        self.devices
            .iter()
            .map(|(fixture, _)| fixture.device_id())
            .collect()
    }

    /// Get test context for a device
    pub fn device_context(&self, device_id: DeviceId) -> Option<&CompositeTestHandler> {
        self.devices
            .iter()
            .find(|(fixture, _)| fixture.device_id() == device_id)
            .map(|(_, effects)| effects)
    }
}

impl std::fmt::Debug for SimulatorCompatibleContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SimulatorCompatibleContext")
            .field("device_count", &self.devices.len())
            .field("device_ids", &self.device_ids())
            .field("transport", &self.transport)
            .field("session_coordinator", &self.session_coordinator)
            .finish()
    }
}

/// Performance snapshot for monitoring choreography execution
///
/// This will be enhanced as the stateless effect system gains performance
/// monitoring capabilities.
#[derive(Debug, Clone)]
pub struct PerformanceSnapshot {
    /// Number of devices in the test
    pub device_count: usize,
    // Additional metrics will be added when supported by stateless effects
}

impl PerformanceSnapshot {
    /// Create a new performance snapshot
    pub fn new() -> Self {
        Self { device_count: 0 }
    }

    /// Add metrics for a device
    ///
    /// This will be implemented when the stateless effect system provides metrics
    pub fn add_device_metrics(&mut self, _device_id: DeviceId, _metrics: ()) {
        // This will be implemented when the stateless effect system provides metrics
    }
}

impl Default for PerformanceSnapshot {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper functions for common test scenarios
/// Create a simple two-device test harness
pub fn test_device_pair() -> ChoreographyTestHarness {
    let mut harness = ChoreographyTestHarness::with_labeled_devices(vec!["alice", "bob"]);
    harness
        .map_role("Alice", 0)
        .expect("Failed to map Alice role");
    harness.map_role("Bob", 1).expect("Failed to map Bob role");
    harness
}

/// Create a three-device test harness
pub fn test_device_trio() -> ChoreographyTestHarness {
    let mut harness =
        ChoreographyTestHarness::with_labeled_devices(vec!["alice", "bob", "charlie"]);
    harness
        .map_role("Alice", 0)
        .expect("Failed to map Alice role");
    harness.map_role("Bob", 1).expect("Failed to map Bob role");
    harness
        .map_role("Charlie", 2)
        .expect("Failed to map Charlie role");
    harness
}

/// Create a threshold group test harness (typically 2-of-3)
pub fn test_threshold_group() -> ChoreographyTestHarness {
    let mut harness = ChoreographyTestHarness::with_labeled_devices(vec![
        "coordinator",
        "participant_1",
        "participant_2",
    ]);
    harness
        .map_role("Coordinator", 0)
        .expect("Failed to map Coordinator role");
    harness
        .map_role("Participant1", 1)
        .expect("Failed to map Participant1 role");
    harness
        .map_role("Participant2", 2)
        .expect("Failed to map Participant2 role");
    harness
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_harness_creation() {
        let harness = ChoreographyTestHarness::with_devices(3);
        assert_eq!(harness.device_count(), 3);
        assert_eq!(harness.device_ids().len(), 3);
    }

    #[tokio::test]
    async fn test_role_mapping() {
        let harness = test_device_pair();

        assert!(harness.role_device_id("Alice").is_some());
        assert!(harness.role_device_id("Bob").is_some());
        assert!(harness.role_device_id("Charlie").is_none());
    }

    #[tokio::test]
    async fn test_coordinated_session() {
        let harness = test_device_trio();

        let session = harness
            .create_coordinated_session("coordination")
            .await
            .expect("Failed to create coordinated session");

        assert_eq!(session.participants().len(), 3);

        let status = session
            .status()
            .await
            .expect("Failed to get session status");
        assert_eq!(status.session_type, "coordination");
        assert_eq!(status.status, SessionStatus::Active);

        session.end().await.expect("Failed to end session");
    }

    #[tokio::test]
    async fn test_transport_communication() {
        let devices = vec![DeviceId::new(), DeviceId::new()];
        let transport = MockChoreographyTransport::new(devices.clone());

        let message = b"hello".to_vec();
        transport
            .send_message(devices[0], devices[1], message.clone())
            .await
            .expect("Failed to send message");

        let received = transport
            .receive_message(devices[1], Some(devices[0]))
            .await
            .expect("Failed to receive message");

        assert!(received.is_some());
        let (sender, received_message) = received.unwrap();
        assert_eq!(sender, devices[0]);
        assert_eq!(received_message, message);
    }
}
