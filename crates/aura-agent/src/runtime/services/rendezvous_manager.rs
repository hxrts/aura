//! Rendezvous Service Manager
//!
//! Wraps `aura_rendezvous::RendezvousService` for integration with the agent runtime.
//! Provides lifecycle management, descriptor caching, and channel establishment.

use aura_core::effects::GuardSnapshot;
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_protocol::guards::pure::GuardChain;
use aura_rendezvous::{
    RendezvousDescriptor, RendezvousFact, RendezvousService, TransportHint,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

/// Configuration for the rendezvous service manager
#[derive(Debug, Clone)]
pub struct RendezvousManagerConfig {
    /// Enable automatic descriptor refresh
    pub auto_refresh_enabled: bool,

    /// Refresh window - refresh descriptors this long before expiry (default: 5 min)
    pub refresh_window: Duration,

    /// Default descriptor validity duration (default: 1 hour)
    pub descriptor_validity: Duration,

    /// Enable periodic cleanup of expired descriptors
    pub auto_cleanup_enabled: bool,

    /// Cleanup interval (default: 60s)
    pub cleanup_interval: Duration,

    /// Default transport hints for this node
    pub default_transport_hints: Vec<TransportHint>,
}

impl Default for RendezvousManagerConfig {
    fn default() -> Self {
        Self {
            auto_refresh_enabled: true,
            refresh_window: Duration::from_secs(300),      // 5 minutes
            descriptor_validity: Duration::from_secs(3600), // 1 hour
            auto_cleanup_enabled: true,
            cleanup_interval: Duration::from_secs(60),
            default_transport_hints: Vec::new(),
        }
    }
}

impl RendezvousManagerConfig {
    /// Create config for testing (shorter intervals)
    pub fn for_testing() -> Self {
        Self {
            auto_refresh_enabled: true,
            refresh_window: Duration::from_secs(30),
            descriptor_validity: Duration::from_secs(300),
            auto_cleanup_enabled: true,
            cleanup_interval: Duration::from_secs(10),
            default_transport_hints: vec![TransportHint::QuicDirect {
                addr: "127.0.0.1:0".to_string(),
            }],
        }
    }

    /// Create config with auto features disabled
    pub fn manual_only() -> Self {
        Self {
            auto_refresh_enabled: false,
            auto_cleanup_enabled: false,
            ..Default::default()
        }
    }

    /// Set default transport hints
    pub fn with_transport_hints(mut self, hints: Vec<TransportHint>) -> Self {
        self.default_transport_hints = hints;
        self
    }
}

/// State of the rendezvous service manager
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RendezvousManagerState {
    /// Service not yet started
    Stopped,
    /// Service starting up
    Starting,
    /// Service running
    Running,
    /// Service shutting down
    Stopping,
}

/// Manager for rendezvous operations
///
/// Integrates `aura_rendezvous::RendezvousService` into the agent runtime lifecycle.
/// Handles descriptor publication, caching, and channel establishment.
pub struct RendezvousManager {
    /// Inner rendezvous service
    service: Arc<RwLock<Option<RendezvousService>>>,

    /// Configuration
    config: RendezvousManagerConfig,

    /// Current state
    state: Arc<RwLock<RendezvousManagerState>>,

    /// Guard chain for authorization
    guard_chain: Arc<GuardChain>,

    /// Authority ID
    authority_id: AuthorityId,

    /// Background cleanup task handle
    cleanup_task: Arc<RwLock<Option<tokio::task::JoinHandle<()>>>>,
}

impl RendezvousManager {
    /// Create a new rendezvous manager
    pub fn new(
        authority_id: AuthorityId,
        guard_chain: Arc<GuardChain>,
        config: RendezvousManagerConfig,
    ) -> Self {
        Self {
            service: Arc::new(RwLock::new(None)),
            config,
            state: Arc::new(RwLock::new(RendezvousManagerState::Stopped)),
            guard_chain,
            authority_id,
            cleanup_task: Arc::new(RwLock::new(None)),
        }
    }

    /// Create with default configuration
    pub fn with_defaults(authority_id: AuthorityId, guard_chain: Arc<GuardChain>) -> Self {
        Self::new(authority_id, guard_chain, RendezvousManagerConfig::default())
    }

    /// Get the current state
    pub async fn state(&self) -> RendezvousManagerState {
        *self.state.read().await
    }

    /// Check if the service is running
    pub async fn is_running(&self) -> bool {
        *self.state.read().await == RendezvousManagerState::Running
    }

    /// Start the rendezvous manager
    pub async fn start(&self) -> Result<(), String> {
        let current_state = *self.state.read().await;
        if current_state == RendezvousManagerState::Running {
            return Ok(()); // Already running
        }

        *self.state.write().await = RendezvousManagerState::Starting;

        // Create the underlying rendezvous service
        let mut service = RendezvousService::new(self.authority_id, self.guard_chain.clone());

        // Configure transport selector if we have default hints
        if !self.config.default_transport_hints.is_empty() {
            service = service.with_transport_selector(
                aura_rendezvous::TransportSelector::default(),
            );
        }

        *self.service.write().await = Some(service);
        *self.state.write().await = RendezvousManagerState::Running;

        // Start background cleanup task if enabled
        if self.config.auto_cleanup_enabled {
            self.start_cleanup_task().await;
        }

        tracing::info!("Rendezvous manager started for authority {}", self.authority_id);
        Ok(())
    }

    /// Stop the rendezvous manager
    pub async fn stop(&self) -> Result<(), String> {
        let current_state = *self.state.read().await;
        if current_state == RendezvousManagerState::Stopped {
            return Ok(()); // Already stopped
        }

        *self.state.write().await = RendezvousManagerState::Stopping;

        // Cancel cleanup task if running
        if let Some(handle) = self.cleanup_task.write().await.take() {
            handle.abort();
        }

        *self.service.write().await = None;
        *self.state.write().await = RendezvousManagerState::Stopped;

        tracing::info!("Rendezvous manager stopped");
        Ok(())
    }

    /// Start the background cleanup task
    async fn start_cleanup_task(&self) {
        let service = self.service.clone();
        let interval = self.config.cleanup_interval;
        let state = self.state.clone();

        let handle = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                ticker.tick().await;

                // Check if still running
                if *state.read().await != RendezvousManagerState::Running {
                    break;
                }

                // Perform cleanup
                if let Some(ref mut svc) = *service.write().await {
                    // Get current time in ms (simplified - production would use TimeEffects)
                    let now_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);
                    svc.cleanup_expired_descriptors(now_ms);
                }
            }
        });

        *self.cleanup_task.write().await = Some(handle);
    }

    // ========================================================================
    // Descriptor Operations
    // ========================================================================

    /// Publish a transport descriptor for a context
    ///
    /// Returns the fact to be committed to the journal and the guard outcome.
    pub async fn publish_descriptor(
        &self,
        context_id: ContextId,
        transport_hints: Option<Vec<TransportHint>>,
        psk_commitment: [u8; 32],
        now_ms: u64,
        nonce: [u8; 32],
        snapshot: &GuardSnapshot,
    ) -> Result<(RendezvousFact, aura_core::effects::GuardOutcome), String> {
        let service = self.service.read().await;
        let service = service.as_ref().ok_or("Rendezvous manager not started")?;

        let hints = transport_hints.unwrap_or_else(|| self.config.default_transport_hints.clone());
        let validity_ms = self.config.descriptor_validity.as_millis() as u64;

        service
            .prepare_publish_descriptor(
                context_id,
                hints,
                psk_commitment,
                now_ms,
                now_ms.saturating_add(validity_ms),
                nonce,
                snapshot,
            )
            .map_err(|e| format!("Failed to prepare descriptor: {}", e))
    }

    /// Refresh a descriptor for a context
    ///
    /// Returns the new descriptor fact, optional revocation fact, and guard outcome.
    pub async fn refresh_descriptor(
        &self,
        context_id: ContextId,
        transport_hints: Option<Vec<TransportHint>>,
        psk_commitment: [u8; 32],
        now_ms: u64,
        new_nonce: [u8; 32],
        snapshot: &GuardSnapshot,
    ) -> Result<
        (
            RendezvousFact,
            Option<RendezvousFact>,
            aura_core::effects::GuardOutcome,
        ),
        String,
    > {
        let service = self.service.read().await;
        let service = service.as_ref().ok_or("Rendezvous manager not started")?;

        let hints = transport_hints.unwrap_or_else(|| self.config.default_transport_hints.clone());
        let validity_ms = self.config.descriptor_validity.as_millis() as u64;

        service
            .prepare_refresh_descriptor(
                context_id,
                hints,
                psk_commitment,
                now_ms,
                validity_ms,
                new_nonce,
                snapshot,
            )
            .map_err(|e| format!("Failed to prepare refresh: {}", e))
    }

    /// Cache a peer's descriptor
    pub async fn cache_descriptor(&self, descriptor: RendezvousDescriptor) -> Result<(), String> {
        let mut service = self.service.write().await;
        let service = service.as_mut().ok_or("Rendezvous manager not started")?;
        service.cache_descriptor(descriptor);
        Ok(())
    }

    /// Get a cached descriptor for a peer
    pub async fn get_descriptor(
        &self,
        context_id: ContextId,
        peer: AuthorityId,
    ) -> Option<RendezvousDescriptor> {
        let service = self.service.read().await;
        service
            .as_ref()
            .and_then(|s| s.get_cached_descriptor(context_id, peer).cloned())
    }

    /// Check if our descriptor needs refresh in a context
    pub async fn needs_refresh(&self, context_id: ContextId, now_ms: u64) -> bool {
        let service = self.service.read().await;
        service
            .as_ref()
            .map(|s| s.needs_refresh(context_id, now_ms, self.config.refresh_window.as_millis() as u64))
            .unwrap_or(true)
    }

    /// Get contexts needing descriptor refresh
    pub async fn contexts_needing_refresh(&self, now_ms: u64) -> Vec<ContextId> {
        let service = self.service.read().await;
        service
            .as_ref()
            .map(|s| s.contexts_needing_refresh(now_ms, self.config.refresh_window.as_millis() as u64))
            .unwrap_or_default()
    }

    // ========================================================================
    // Channel Operations
    // ========================================================================

    /// Prepare to establish a channel with a peer
    pub async fn prepare_establish_channel(
        &self,
        context_id: ContextId,
        peer: AuthorityId,
        snapshot: &GuardSnapshot,
    ) -> Result<
        (
            aura_rendezvous::SelectedTransport,
            aura_core::effects::GuardOutcome,
        ),
        String,
    > {
        let service = self.service.read().await;
        let service = service.as_ref().ok_or("Rendezvous manager not started")?;

        service
            .prepare_establish_channel(context_id, peer, snapshot)
            .map_err(|e| format!("Failed to prepare channel: {}", e))
    }

    /// Create a channel established fact
    pub async fn create_channel_fact(
        &self,
        context_id: ContextId,
        peer: AuthorityId,
        channel_id: [u8; 32],
        epoch: u64,
    ) -> Result<RendezvousFact, String> {
        let service = self.service.read().await;
        let service = service.as_ref().ok_or("Rendezvous manager not started")?;

        Ok(service.create_channel_established_fact(context_id, peer, channel_id, epoch))
    }

    // ========================================================================
    // Relay Operations
    // ========================================================================

    /// Prepare a relay request
    pub async fn prepare_relay_request(
        &self,
        context_id: ContextId,
        relay: AuthorityId,
        target: AuthorityId,
        snapshot: &GuardSnapshot,
    ) -> Result<aura_core::effects::GuardOutcome, String> {
        let service = self.service.read().await;
        let service = service.as_ref().ok_or("Rendezvous manager not started")?;

        service
            .prepare_relay_request(context_id, relay, target, snapshot)
            .map_err(|e| format!("Failed to prepare relay request: {}", e))
    }

    /// Prepare to forward a relay message
    pub async fn prepare_relay_forward(
        &self,
        context_id: ContextId,
        target: AuthorityId,
        snapshot: &GuardSnapshot,
    ) -> Result<aura_core::effects::GuardOutcome, String> {
        let service = self.service.read().await;
        let service = service.as_ref().ok_or("Rendezvous manager not started")?;

        service
            .prepare_relay_forward(context_id, target, snapshot)
            .map_err(|e| format!("Failed to prepare relay forward: {}", e))
    }

    // ========================================================================
    // Accessors
    // ========================================================================

    /// Get the configuration
    pub fn config(&self) -> &RendezvousManagerConfig {
        &self.config
    }

    /// Get the authority ID
    pub fn authority_id(&self) -> AuthorityId {
        self.authority_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::effects::{FlowBudgetView, MetadataView};
    use aura_core::time::{PhysicalTime, TimeStamp};
    use aura_core::Cap;
    use std::collections::HashMap;

    fn test_authority() -> AuthorityId {
        AuthorityId::new_from_entropy([1u8; 32])
    }

    fn test_peer() -> AuthorityId {
        AuthorityId::new_from_entropy([2u8; 32])
    }

    fn test_context() -> ContextId {
        ContextId::new_from_entropy([3u8; 32])
    }

    fn test_snapshot(authority: AuthorityId, context: ContextId) -> GuardSnapshot {
        let mut budgets = HashMap::new();
        budgets.insert((context, authority), 1000);

        GuardSnapshot {
            now: TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: 1000,
                uncertainty: None,
            }),
            caps: Cap::default(),
            budgets: FlowBudgetView::new(budgets),
            metadata: MetadataView::default(),
            rng_seed: [0u8; 32],
        }
    }

    #[tokio::test]
    async fn test_manager_creation() {
        let config = RendezvousManagerConfig::for_testing();
        let manager = RendezvousManager::new(
            test_authority(),
            Arc::new(GuardChain::standard()),
            config,
        );

        assert_eq!(manager.state().await, RendezvousManagerState::Stopped);
        assert!(!manager.is_running().await);
    }

    #[tokio::test]
    async fn test_manager_lifecycle() {
        let config = RendezvousManagerConfig::for_testing();
        let manager = RendezvousManager::new(
            test_authority(),
            Arc::new(GuardChain::standard()),
            config,
        );

        // Start
        manager.start().await.unwrap();
        assert!(manager.is_running().await);

        // Stop
        manager.stop().await.unwrap();
        assert!(!manager.is_running().await);
    }

    #[tokio::test]
    async fn test_descriptor_caching() {
        let config = RendezvousManagerConfig::for_testing();
        let manager = RendezvousManager::new(
            test_authority(),
            Arc::new(GuardChain::standard()),
            config,
        );
        manager.start().await.unwrap();

        let descriptor = RendezvousDescriptor {
            authority_id: test_peer(),
            context_id: test_context(),
            transport_hints: vec![TransportHint::QuicDirect {
                addr: "127.0.0.1:8443".to_string(),
            }],
            handshake_psk_commitment: [0u8; 32],
            valid_from: 0,
            valid_until: u64::MAX,
            nonce: [0u8; 32],
        };

        manager.cache_descriptor(descriptor.clone()).await.unwrap();

        let cached = manager
            .get_descriptor(test_context(), test_peer())
            .await
            .unwrap();
        assert_eq!(cached.authority_id, test_peer());

        manager.stop().await.unwrap();
    }

    #[tokio::test]
    async fn test_publish_descriptor() {
        let config = RendezvousManagerConfig::for_testing();
        let manager = RendezvousManager::new(
            test_authority(),
            Arc::new(GuardChain::standard()),
            config,
        );
        manager.start().await.unwrap();

        let snapshot = test_snapshot(test_authority(), test_context());
        let (fact, outcome) = manager
            .publish_descriptor(
                test_context(),
                Some(vec![TransportHint::QuicDirect {
                    addr: "127.0.0.1:8443".to_string(),
                }]),
                [0u8; 32],
                1000,
                [42u8; 32],
                &snapshot,
            )
            .await
            .unwrap();

        assert!(matches!(fact, RendezvousFact::Descriptor(_)));
        assert!(outcome.decision.is_authorized());

        manager.stop().await.unwrap();
    }

    #[tokio::test]
    async fn test_needs_refresh() {
        let config = RendezvousManagerConfig::for_testing();
        let manager = RendezvousManager::new(
            test_authority(),
            Arc::new(GuardChain::standard()),
            config,
        );
        manager.start().await.unwrap();

        // No descriptor cached - should need refresh
        assert!(manager.needs_refresh(test_context(), 1000).await);

        manager.stop().await.unwrap();
    }
}
