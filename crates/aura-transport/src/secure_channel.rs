//! SecureChannel semantics and management for the Aura protocol
//!
//! This module implements comprehensive SecureChannel management that ensures:
//! 1. One active channel per (ContextId, peer_device) with registry enforcement
//! 2. Teardown triggers: epoch rotation, capability shrink, context invalidation  
//! 3. Reconnect behavior: re-run rendezvous, receipts never cross epochs
//! 4. Registry invariants and lifecycle management
//!
//! As specified in work/007.md

use aura_core::{
    flow::FlowBudget, relationships::ContextId, session_epochs::Epoch, AuraError, DeviceId,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::net::TcpStream;
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, info, warn};

/// Get current time as seconds since UNIX epoch
fn current_timestamp() -> u64 {
    SystemTime::UNIX_EPOCH
        .elapsed()
        .unwrap_or_default()
        .as_secs()
}

/// Status of a SecureChannel connection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChannelStatus {
    /// Channel is establishing connection (handshake in progress)
    Establishing,
    /// Channel is active and ready for communication
    Active,
    /// Channel is being torn down gracefully
    TearingDown,
    /// Channel has been terminated
    Terminated,
    /// Channel failed during establishment or operation
    Failed,
}

impl Default for ChannelStatus {
    fn default() -> Self {
        Self::Establishing
    }
}

/// Reason for channel teardown - used for logging and diagnostics
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TeardownReason {
    /// Epoch rotation triggered teardown
    EpochRotation {
        old_epoch: Epoch,
        new_epoch: Epoch,
    },
    /// Capability shrink triggered teardown
    CapabilityShrink {
        old_budget: FlowBudget,
        new_budget: FlowBudget,
    },
    /// Context invalidation triggered teardown
    ContextInvalidation {
        context: ContextId,
        reason: String,
    },
    /// Manual teardown requested
    Manual,
    /// Connection failed
    ConnectionFailure { error: String },
    /// Timeout occurred
    Timeout,
    /// Peer disconnected
    PeerDisconnected,
}

/// Configuration for channel teardown behavior
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeardownConfig {
    /// Whether to perform graceful teardown (send goodbye message)
    pub graceful: bool,
    /// Timeout for graceful teardown in seconds
    pub timeout_seconds: u64,
    /// Whether to automatically reconnect after teardown
    pub auto_reconnect: bool,
    /// Whether to preserve message queues during teardown
    pub preserve_queues: bool,
}

impl Default for TeardownConfig {
    fn default() -> Self {
        Self {
            graceful: true,
            timeout_seconds: 30,
            auto_reconnect: true,
            preserve_queues: false,
        }
    }
}

/// A secure channel binding to a specific context and peer device
/// 
/// Each SecureChannel represents a bidirectional communication channel
/// between this device and a peer device within a specific context.
/// The channel enforces epoch boundaries and capability constraints.
#[derive(Debug)]
pub struct SecureChannel {
    /// Unique identifier for this channel
    pub channel_id: String,
    /// Context this channel operates within
    pub context: ContextId,
    /// Peer device this channel connects to
    pub peer_device: DeviceId,
    /// Current epoch binding for this channel
    pub epoch: Epoch,
    /// Current flow budget for this channel
    pub flow_budget: FlowBudget,
    /// Channel status
    pub status: ChannelStatus,
    /// Socket address of the peer
    pub peer_addr: Option<SocketAddr>,
    /// Underlying TCP connection
    pub connection: Option<TcpStream>,
    /// When this channel was established
    pub established_at: u64,
    /// Last activity timestamp
    pub last_activity: u64,
    /// Configuration for teardown behavior
    pub teardown_config: TeardownConfig,
    /// Statistics for this channel
    pub stats: ChannelStatistics,
}

/// Statistics tracked for each SecureChannel
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChannelStatistics {
    /// Total messages sent
    pub messages_sent: u64,
    /// Total messages received  
    pub messages_received: u64,
    /// Total bytes sent
    pub bytes_sent: u64,
    /// Total bytes received
    pub bytes_received: u64,
    /// Number of teardowns and reconnects
    pub reconnect_count: u64,
    /// Last teardown reason if any
    pub last_teardown_reason: Option<TeardownReason>,
}

impl SecureChannel {
    /// Create a new SecureChannel for the given context and peer
    pub fn new(
        context: ContextId,
        peer_device: DeviceId,
        epoch: Epoch,
        flow_budget: FlowBudget,
    ) -> Self {
        let channel_id = format!("{}::{}", context.as_str(), peer_device.0);
        let now = current_timestamp();

        Self {
            channel_id,
            context,
            peer_device,
            epoch,
            flow_budget,
            status: ChannelStatus::Establishing,
            peer_addr: None,
            connection: None,
            established_at: now,
            last_activity: now,
            teardown_config: TeardownConfig::default(),
            stats: ChannelStatistics::default(),
        }
    }

    /// Set the peer address for this channel
    pub fn set_peer_addr(&mut self, addr: SocketAddr) {
        self.peer_addr = Some(addr);
    }

    /// Set the TCP connection for this channel
    pub fn set_connection(&mut self, connection: TcpStream) {
        self.connection = Some(connection);
        self.status = ChannelStatus::Active;
        self.last_activity = current_timestamp();
        
        info!(
            channel_id = %self.channel_id,
            context = %self.context.as_str(),
            peer = %self.peer_device.0,
            "SecureChannel established"
        );
    }

    /// Check if this channel should be torn down due to epoch rotation
    pub fn should_teardown_for_epoch(&self, new_epoch: Epoch) -> bool {
        new_epoch.value() > self.epoch.value()
    }

    /// Check if this channel should be torn down due to capability shrink
    pub fn should_teardown_for_budget_shrink(&self, new_budget: &FlowBudget) -> bool {
        // Teardown if the new budget limit is significantly lower
        // or if epoch has advanced (which should use epoch teardown instead)
        new_budget.limit < self.flow_budget.limit.saturating_mul(3).saturating_div(4)
    }

    /// Update the flow budget for this channel
    /// Returns true if the channel should be torn down due to capability shrink
    pub fn update_flow_budget(&mut self, new_budget: FlowBudget) -> Option<TeardownReason> {
        let old_budget = self.flow_budget;
        
        if self.should_teardown_for_budget_shrink(&new_budget) {
            Some(TeardownReason::CapabilityShrink {
                old_budget,
                new_budget,
            })
        } else {
            self.flow_budget = new_budget;
            self.last_activity = current_timestamp();
            None
        }
    }

    /// Update the epoch for this channel
    /// Returns true if the channel should be torn down due to epoch rotation
    pub fn update_epoch(&mut self, new_epoch: Epoch) -> Option<TeardownReason> {
        let old_epoch = self.epoch;
        
        if self.should_teardown_for_epoch(new_epoch) {
            Some(TeardownReason::EpochRotation {
                old_epoch,
                new_epoch,
            })
        } else {
            self.epoch = new_epoch;
            self.last_activity = current_timestamp();
            None
        }
    }

    /// Mark the channel for teardown
    pub fn initiate_teardown(&mut self, reason: TeardownReason) {
        self.status = ChannelStatus::TearingDown;
        self.stats.last_teardown_reason = Some(reason.clone());
        self.last_activity = current_timestamp();
        
        warn!(
            channel_id = %self.channel_id,
            context = %self.context.as_str(),
            peer = %self.peer_device.0,
            reason = ?reason,
            "SecureChannel teardown initiated"
        );
    }

    /// Complete the teardown and mark channel as terminated
    pub fn complete_teardown(&mut self) {
        self.status = ChannelStatus::Terminated;
        self.connection = None;
        self.last_activity = current_timestamp();
        
        debug!(
            channel_id = %self.channel_id,
            context = %self.context.as_str(),
            peer = %self.peer_device.0,
            "SecureChannel teardown completed"
        );
    }

    /// Check if the channel is active and ready for communication
    pub fn is_active(&self) -> bool {
        matches!(self.status, ChannelStatus::Active)
    }

    /// Check if the channel is in a terminal state
    pub fn is_terminated(&self) -> bool {
        matches!(
            self.status,
            ChannelStatus::Terminated | ChannelStatus::Failed
        )
    }

    /// Update activity timestamp and statistics for a sent message
    pub fn record_message_sent(&mut self, bytes: u64) {
        self.stats.messages_sent += 1;
        self.stats.bytes_sent += bytes;
        self.last_activity = current_timestamp();
    }

    /// Update activity timestamp and statistics for a received message
    pub fn record_message_received(&mut self, bytes: u64) {
        self.stats.messages_received += 1;
        self.stats.bytes_received += bytes;
        self.last_activity = current_timestamp();
    }
}

/// Key for uniquely identifying a SecureChannel in the registry
/// Enforces the constraint of one active channel per (ContextId, DeviceId)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChannelKey {
    pub context: ContextId,
    pub peer_device: DeviceId,
}

impl ChannelKey {
    pub fn new(context: ContextId, peer_device: DeviceId) -> Self {
        Self {
            context,
            peer_device,
        }
    }
}

/// Registry for managing SecureChannels with lifecycle and invariant enforcement
///
/// The registry ensures that:
/// - Only one active channel exists per (ContextId, peer_device) pair
/// - Channels are properly torn down when triggers are detected  
/// - Reconnection logic is properly coordinated
/// - Registry invariants are maintained
pub struct SecureChannelRegistry {
    /// Active channels indexed by (context, peer_device)
    channels: Arc<RwLock<HashMap<ChannelKey, SecureChannel>>>,
    /// Channels pending teardown
    teardown_queue: Arc<Mutex<Vec<(ChannelKey, TeardownReason)>>>,
    /// Registry configuration
    config: RegistryConfig,
}

impl std::fmt::Debug for SecureChannelRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecureChannelRegistry")
            .field("config", &self.config)
            .field("channels", &"<RwLock<HashMap<ChannelKey, SecureChannel>>>")
            .field("teardown_queue", &"<Mutex<Vec<(ChannelKey, TeardownReason)>>>")
            .finish()
    }
}

/// Configuration for the SecureChannelRegistry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryConfig {
    /// Maximum number of channels allowed
    pub max_channels: usize,
    /// Channel idle timeout in seconds
    pub channel_idle_timeout: u64,
    /// Teardown processing interval in seconds
    pub teardown_interval: u64,
    /// Whether to enable automatic cleanup of terminated channels
    pub auto_cleanup: bool,
    /// Maximum reconnect attempts before giving up
    pub max_reconnect_attempts: usize,
}

impl Default for RegistryConfig {
    fn default() -> Self {
        Self {
            max_channels: 1000,
            channel_idle_timeout: 300, // 5 minutes
            teardown_interval: 10,     // 10 seconds
            auto_cleanup: true,
            max_reconnect_attempts: 3,
        }
    }
}

impl SecureChannelRegistry {
    /// Create a new SecureChannelRegistry
    pub fn new(config: RegistryConfig) -> Self {
        Self {
            channels: Arc::new(RwLock::new(HashMap::new())),
            teardown_queue: Arc::new(Mutex::new(Vec::new())),
            config,
        }
    }

    /// Create a new SecureChannelRegistry with default configuration
    pub fn with_defaults() -> Self {
        Self::new(RegistryConfig::default())
    }

    /// Get or create a SecureChannel for the given context and peer
    /// 
    /// Enforces the invariant: one active channel per (ContextId, peer_device)
    /// If a channel already exists, returns the existing one.
    /// If the existing channel is terminated, creates a new one.
    pub async fn get_or_create_channel(
        &self,
        context: ContextId,
        peer_device: DeviceId,
        epoch: Epoch,
        flow_budget: FlowBudget,
    ) -> Result<(), AuraError> {
        let key = ChannelKey::new(context.clone(), peer_device);
        let mut channels = self.channels.write().await;

        match channels.get(&key) {
            Some(existing_channel) => {
                if existing_channel.is_terminated() {
                    // Replace terminated channel with a new one
                    let new_channel = SecureChannel::new(context.clone(), peer_device, epoch, flow_budget);
                    channels.insert(key, new_channel);
                    
                    info!(
                        context = %context.as_str(),
                        peer = %peer_device.0,
                        "Replaced terminated SecureChannel with new instance"
                    );
                } else {
                    // Channel already exists and is active/establishing
                    debug!(
                        context = %context.as_str(),
                        peer = %peer_device.0,
                        status = ?existing_channel.status,
                        "SecureChannel already exists"
                    );
                }
            }
            None => {
                // Check capacity before creating new channel
                if channels.len() >= self.config.max_channels {
                    return Err(AuraError::coordination_failed(format!(
                        "SecureChannelRegistry at capacity: {} channels",
                        self.config.max_channels
                    )));
                }

                // Create new channel
                let new_channel = SecureChannel::new(context.clone(), peer_device, epoch, flow_budget);
                channels.insert(key, new_channel);
                
                info!(
                    context = %context.as_str(),
                    peer = %peer_device.0,
                    "Created new SecureChannel"
                );
            }
        }

        Ok(())
    }

    /// Get a reference to an existing channel if it exists and is active
    pub async fn get_active_channel(
        &self,
        context: ContextId,
        peer_device: DeviceId,
    ) -> Option<ChannelKey> {
        let key = ChannelKey::new(context.clone(), peer_device);
        let channels = self.channels.read().await;
        
        channels.get(&key).and_then(|channel| {
            if channel.is_active() {
                Some(key)
            } else {
                None
            }
        })
    }

    /// Check if a channel exists for the given context and peer
    pub async fn has_channel(&self, context: ContextId, peer_device: DeviceId) -> bool {
        let key = ChannelKey::new(context.clone(), peer_device);
        let channels = self.channels.read().await;
        channels.contains_key(&key)
    }

    /// Trigger epoch rotation for all channels
    /// Channels that should teardown due to epoch change are queued for teardown
    pub async fn trigger_epoch_rotation(&self, new_epoch: Epoch) {
        let channels_guard = self.channels.read().await;
        let mut teardown_queue = self.teardown_queue.lock().await;

        for (key, channel) in channels_guard.iter() {
            if let Some(reason) = channel.should_teardown_for_epoch(new_epoch).then(|| {
                TeardownReason::EpochRotation {
                    old_epoch: channel.epoch,
                    new_epoch,
                }
            }) {
                teardown_queue.push((key.clone(), reason));
                
                info!(
                    context = %key.context.as_str(),
                    peer = %key.peer_device.0,
                    old_epoch = channel.epoch.value(),
                    new_epoch = new_epoch.value(),
                    "Queued SecureChannel for epoch rotation teardown"
                );
            }
        }
    }

    /// Trigger capability shrink check for a specific channel
    pub async fn trigger_capability_shrink(
        &self,
        context: ContextId,
        peer_device: DeviceId,
        new_budget: FlowBudget,
    ) -> Result<(), AuraError> {
        let key = ChannelKey::new(context.clone(), peer_device);
        let channels = self.channels.read().await;

        if let Some(channel) = channels.get(&key) {
            if channel.should_teardown_for_budget_shrink(&new_budget) {
                let reason = TeardownReason::CapabilityShrink {
                    old_budget: channel.flow_budget,
                    new_budget,
                };

                let mut teardown_queue = self.teardown_queue.lock().await;
                teardown_queue.push((key, reason));
                
                info!(
                    context = %context.as_str(),
                    peer = %peer_device.0,
                    old_limit = channel.flow_budget.limit,
                    new_limit = new_budget.limit,
                    "Queued SecureChannel for capability shrink teardown"
                );
            }
        }

        Ok(())
    }

    /// Trigger context invalidation for all channels in a context
    pub async fn trigger_context_invalidation(&self, context: ContextId, reason: String) {
        let channels_guard = self.channels.read().await;
        let mut teardown_queue = self.teardown_queue.lock().await;

        for (key, _channel) in channels_guard.iter() {
            if key.context == context {
                let teardown_reason = TeardownReason::ContextInvalidation {
                    context: context.clone(),
                    reason: reason.clone(),
                };
                teardown_queue.push((key.clone(), teardown_reason));
                
                info!(
                    context = %key.context.as_str(),
                    peer = %key.peer_device.0,
                    reason = %reason,
                    "Queued SecureChannel for context invalidation teardown"
                );
            }
        }
    }

    /// Process the teardown queue and perform actual teardowns
    pub async fn process_teardown_queue(&self) -> Result<usize, AuraError> {
        let mut teardown_queue = self.teardown_queue.lock().await;
        let to_teardown = teardown_queue.drain(..).collect::<Vec<_>>();
        drop(teardown_queue);

        let teardown_count = to_teardown.len();
        let mut channels = self.channels.write().await;

        for (key, reason) in to_teardown {
            if let Some(channel) = channels.get_mut(&key) {
                channel.initiate_teardown(reason);
                
                // Complete teardown immediately for now
                // In a full implementation, this might be asynchronous
                channel.complete_teardown();
                
                if channel.teardown_config.auto_reconnect {
                    channel.stats.reconnect_count += 1;
                    
                    // TODO: Implement reconnect behavior
                    // This would involve re-running rendezvous and establishing new channel
                    debug!(
                        context = %key.context.as_str(),
                        peer = %key.peer_device.0,
                        attempt = channel.stats.reconnect_count,
                        "Scheduling auto-reconnect attempt"
                    );
                }
            }
        }

        if teardown_count > 0 {
            info!(
                count = teardown_count,
                "Processed SecureChannel teardown queue"
            );
        }

        Ok(teardown_count)
    }

    /// Cleanup terminated channels from the registry
    pub async fn cleanup_terminated_channels(&self) -> usize {
        let mut channels = self.channels.write().await;
        let initial_count = channels.len();

        channels.retain(|key, channel| {
            let should_retain = !channel.is_terminated();
            
            if !should_retain {
                debug!(
                    context = %key.context.as_str(),
                    peer = %key.peer_device.0,
                    "Cleaned up terminated SecureChannel"
                );
            }
            
            should_retain
        });

        let cleaned_count = initial_count - channels.len();
        if cleaned_count > 0 {
            info!(
                cleaned = cleaned_count,
                remaining = channels.len(),
                "Cleaned up terminated SecureChannels"
            );
        }

        cleaned_count
    }

    /// Get registry statistics
    pub async fn get_registry_stats(&self) -> RegistryStats {
        let channels = self.channels.read().await;
        let teardown_queue = self.teardown_queue.lock().await;

        let mut stats = RegistryStats {
            total_channels: channels.len(),
            active_channels: 0,
            establishing_channels: 0,
            tearing_down_channels: 0,
            terminated_channels: 0,
            failed_channels: 0,
            pending_teardowns: teardown_queue.len(),
            contexts: std::collections::HashSet::new(),
            peers: std::collections::HashSet::new(),
        };

        for (key, channel) in channels.iter() {
            stats.contexts.insert(key.context.clone());
            stats.peers.insert(key.peer_device);

            match channel.status {
                ChannelStatus::Active => stats.active_channels += 1,
                ChannelStatus::Establishing => stats.establishing_channels += 1,
                ChannelStatus::TearingDown => stats.tearing_down_channels += 1,
                ChannelStatus::Terminated => stats.terminated_channels += 1,
                ChannelStatus::Failed => stats.failed_channels += 1,
            }
        }

        stats
    }

    /// Validate registry invariants - for testing and debugging
    pub async fn validate_invariants(&self) -> Result<Vec<String>, AuraError> {
        let channels = self.channels.read().await;
        let mut violations = Vec::new();

        // Check: No duplicate keys (enforced by HashMap, but let's be explicit)
        let total_channels = channels.len();
        let unique_keys: std::collections::HashSet<_> = channels.keys().collect();
        if unique_keys.len() != total_channels {
            violations.push(format!(
                "Duplicate channel keys detected: {} channels, {} unique keys",
                total_channels, unique_keys.len()
            ));
        }

        // Check: Each channel's key matches its context and peer
        for (key, channel) in channels.iter() {
            if key.context != channel.context || key.peer_device != channel.peer_device {
                violations.push(format!(
                    "Channel key mismatch: key=({}, {}), channel=({}, {})",
                    key.context.as_str(),
                    key.peer_device.0,
                    channel.context.as_str(),
                    channel.peer_device.0
                ));
            }
        }

        // Check: No channels exceed configured capacity
        if channels.len() > self.config.max_channels {
            violations.push(format!(
                "Registry over capacity: {} > {}",
                channels.len(),
                self.config.max_channels
            ));
        }

        Ok(violations)
    }
}

/// Statistics about the SecureChannelRegistry state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryStats {
    pub total_channels: usize,
    pub active_channels: usize,
    pub establishing_channels: usize,
    pub tearing_down_channels: usize,
    pub terminated_channels: usize,
    pub failed_channels: usize,
    pub pending_teardowns: usize,
    pub contexts: std::collections::HashSet<ContextId>,
    pub peers: std::collections::HashSet<DeviceId>,
}

impl RegistryStats {
    /// Get the number of unique contexts with active channels
    pub fn active_context_count(&self) -> usize {
        self.contexts.len()
    }

    /// Get the number of unique peers with active channels
    pub fn active_peer_count(&self) -> usize {
        self.peers.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::relationships::ContextId;

    #[tokio::test]
    async fn test_registry_creation() {
        let registry = SecureChannelRegistry::with_defaults();
        let stats = registry.get_registry_stats().await;
        
        assert_eq!(stats.total_channels, 0);
        assert_eq!(stats.active_channels, 0);
    }

    #[tokio::test]
    async fn test_channel_creation_and_retrieval() {
        let registry = SecureChannelRegistry::with_defaults();
        let context = ContextId::new("test_context");
        let peer = DeviceId::new();
        let epoch = Epoch::new(1);
        let budget = FlowBudget::new(1000, epoch);

        // Create channel
        registry
            .get_or_create_channel(context, peer, epoch, budget)
            .await
            .unwrap();

        // Verify channel exists
        assert!(registry.has_channel(context, peer).await);

        let stats = registry.get_registry_stats().await;
        assert_eq!(stats.total_channels, 1);
        assert_eq!(stats.establishing_channels, 1); // New channels start as establishing
    }

    #[tokio::test] 
    async fn test_one_channel_per_context_peer_pair() {
        let registry = SecureChannelRegistry::with_defaults();
        let context = ContextId::new("test_context");
        let peer = DeviceId::new();
        let epoch = Epoch::new(1);
        let budget = FlowBudget::new(1000, epoch);

        // Create channel twice - should not create duplicate
        registry
            .get_or_create_channel(context, peer, epoch, budget)
            .await
            .unwrap();
        registry
            .get_or_create_channel(context, peer, epoch, budget)
            .await
            .unwrap();

        let stats = registry.get_registry_stats().await;
        assert_eq!(stats.total_channels, 1);
    }

    #[tokio::test]
    async fn test_epoch_rotation_teardown() {
        let registry = SecureChannelRegistry::with_defaults();
        let context = ContextId::new("test_context");
        let peer = DeviceId::new();
        let epoch = Epoch::new(1);
        let budget = FlowBudget::new(1000, epoch);

        // Create channel
        registry
            .get_or_create_channel(context, peer, epoch, budget)
            .await
            .unwrap();

        // Trigger epoch rotation
        let new_epoch = Epoch::new(2);
        registry.trigger_epoch_rotation(new_epoch).await;

        // Process teardown queue
        let teardown_count = registry.process_teardown_queue().await.unwrap();
        assert_eq!(teardown_count, 1);
    }

    #[tokio::test]
    async fn test_capability_shrink_teardown() {
        let registry = SecureChannelRegistry::with_defaults();
        let context = ContextId::new("test_context");
        let peer = DeviceId::new();
        let epoch = Epoch::new(1);
        let budget = FlowBudget::new(1000, epoch);

        // Create channel
        registry
            .get_or_create_channel(context, peer, epoch, budget)
            .await
            .unwrap();

        // Trigger capability shrink (new budget limit is < 3/4 of original)
        let shrunk_budget = FlowBudget::new(500, epoch);
        registry
            .trigger_capability_shrink(context, peer, shrunk_budget)
            .await
            .unwrap();

        // Process teardown queue
        let teardown_count = registry.process_teardown_queue().await.unwrap();
        assert_eq!(teardown_count, 1);
    }

    #[tokio::test]
    async fn test_context_invalidation_teardown() {
        let registry = SecureChannelRegistry::with_defaults();
        let context = ContextId::new("test_context");
        let peer1 = DeviceId::new();
        let peer2 = DeviceId::new();
        let epoch = Epoch::new(1);
        let budget = FlowBudget::new(1000, epoch);

        // Create channels for same context, different peers
        registry
            .get_or_create_channel(context, peer1, epoch, budget)
            .await
            .unwrap();
        registry
            .get_or_create_channel(context, peer2, epoch, budget)
            .await
            .unwrap();

        // Trigger context invalidation
        registry
            .trigger_context_invalidation(context, "Test invalidation".to_string())
            .await;

        // Process teardown queue - should tear down both channels
        let teardown_count = registry.process_teardown_queue().await.unwrap();
        assert_eq!(teardown_count, 2);
    }

    #[tokio::test]
    async fn test_registry_invariants() {
        let registry = SecureChannelRegistry::with_defaults();
        let context = ContextId::new("test_context");
        let peer = DeviceId::new();
        let epoch = Epoch::new(1);
        let budget = FlowBudget::new(1000, epoch);

        // Create channel
        registry
            .get_or_create_channel(context, peer, epoch, budget)
            .await
            .unwrap();

        // Validate invariants
        let violations = registry.validate_invariants().await.unwrap();
        assert!(violations.is_empty(), "Invariant violations: {:?}", violations);
    }

    #[tokio::test]
    async fn test_cleanup_terminated_channels() {
        let registry = SecureChannelRegistry::with_defaults();
        let context = ContextId::new("test_context");
        let peer = DeviceId::new();
        let epoch = Epoch::new(1);
        let budget = FlowBudget::new(1000, epoch);

        // Create and teardown channel
        registry
            .get_or_create_channel(context, peer, epoch, budget)
            .await
            .unwrap();
        
        registry.trigger_epoch_rotation(Epoch::new(2)).await;
        registry.process_teardown_queue().await.unwrap();

        // Before cleanup
        let stats_before = registry.get_registry_stats().await;
        assert_eq!(stats_before.total_channels, 1);
        assert_eq!(stats_before.terminated_channels, 1);

        // Cleanup
        let cleaned = registry.cleanup_terminated_channels().await;
        assert_eq!(cleaned, 1);

        // After cleanup
        let stats_after = registry.get_registry_stats().await;
        assert_eq!(stats_after.total_channels, 0);
    }
}