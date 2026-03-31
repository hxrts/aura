//! Social Topology Manager
//!
//! Wraps `aura_social` types for integration with the agent runtime.
//! Provides social topology, relay selection, and data availability.

use super::config_profiles::impl_service_config_profiles;
use super::traits::{RuntimeService, RuntimeServiceContext, ServiceError, ServiceHealth};
use async_trait::async_trait;
use aura_core::effects::relay::{RelayCandidate, RelayContext, RelaySelector};
use aura_core::service::{ProviderCandidate, ServiceFamily};
use aura_core::types::identifiers::AuthorityId;
use aura_relational::WebOfTrustEvidence;
use aura_social::{DiscoveryLayer, Home, Neighborhood, SocialTopology};
use aura_transport::relay::DeterministicRandomSelector;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[allow(dead_code)] // Declaration-layer ingress inventory; runtime actor wiring lands incrementally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SocialManagerCommand {
    Initialize,
    RefreshTopology,
    UpdateSelectionPolicy,
}

/// Configuration for the social topology manager
#[derive(Debug, Clone)]
pub struct SocialManagerConfig {
    /// Prefer proximity in relay selection (home peers before neighborhood)
    pub prefer_proximity: bool,

    /// Enable automatic topology refresh from journal
    pub auto_refresh_enabled: bool,
}

impl Default for SocialManagerConfig {
    fn default() -> Self {
        Self {
            prefer_proximity: true,
            auto_refresh_enabled: true,
        }
    }
}

impl_service_config_profiles!(SocialManagerConfig {
    /// Create config for testing
    pub fn for_testing() -> Self {
        Self {
            prefer_proximity: true,
            auto_refresh_enabled: false,
        }
    }
});

/// State of the social manager
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocialManagerState {
    /// Manager not initialized
    Uninitialized,
    /// Manager initialized and ready
    Ready,
}

/// Manager for social topology and related services
///
/// Integrates `aura_social` types into the agent runtime lifecycle.
/// Provides:
/// - Social topology for peer discovery
/// - Relay selector using deterministic selection
/// - Discovery layer determination
#[aura_macros::actor_owned(
    owner = "social_manager",
    domain = "social_topology",
    gate = "social_command_ingress",
    command = SocialManagerCommand,
    capacity = 64,
    category = "actor_owned"
)]
pub struct SocialManager {
    /// Social topology
    topology: RwLock<SocialTopology>,

    /// Relay selector
    relay_selector: Arc<DeterministicRandomSelector>,

    /// Configuration
    config: SocialManagerConfig,

    /// Current state
    state: RwLock<SocialManagerState>,

    /// Local authority
    authority_id: AuthorityId,
}

impl SocialManager {
    /// Create a new social manager with empty topology
    pub fn new(authority_id: AuthorityId, config: SocialManagerConfig) -> Self {
        let topology = SocialTopology::empty(authority_id);
        let relay_selector = DeterministicRandomSelector::new(config.prefer_proximity);

        Self {
            topology: RwLock::new(topology),
            relay_selector: Arc::new(relay_selector),
            config,
            state: RwLock::new(SocialManagerState::Uninitialized),
            authority_id,
        }
    }

    /// Create with default configuration
    pub fn with_defaults(authority_id: AuthorityId) -> Self {
        Self::new(authority_id, SocialManagerConfig::default())
    }

    /// Initialize the manager with a topology
    pub async fn initialize(&self, topology: SocialTopology) {
        *self.topology.write().await = topology;
        *self.state.write().await = SocialManagerState::Ready;
        tracing::info!(
            "Social manager initialized for authority {}",
            self.authority_id
        );
    }

    /// Initialize with home and neighborhoods
    pub async fn initialize_with_social(
        &self,
        home: Option<Home>,
        neighborhoods: Vec<Neighborhood>,
    ) {
        let topology = SocialTopology::new(self.authority_id, home, neighborhoods);
        self.initialize(topology).await;
    }

    /// Get the current state
    pub async fn state(&self) -> SocialManagerState {
        *self.state.read().await
    }

    /// Check if the manager is ready
    pub async fn is_ready(&self) -> bool {
        *self.state.read().await == SocialManagerState::Ready
    }

    // ========================================================================
    // Topology Access
    // ========================================================================

    /// Get a read lock on the topology
    pub async fn topology(&self) -> tokio::sync::RwLockReadGuard<'_, SocialTopology> {
        self.topology.read().await
    }

    /// Update the topology
    pub async fn update_topology(&self, topology: SocialTopology) {
        *self.topology.write().await = topology;
        tracing::debug!("Social topology updated");
    }

    /// Add a peer relationship
    pub async fn add_peer(
        &self,
        peer: AuthorityId,
        relationship: aura_core::effects::relay::RelayRelationship,
    ) {
        self.topology.write().await.add_peer(peer, relationship);
    }

    // ========================================================================
    // Discovery
    // ========================================================================

    /// Determine the discovery layer for reaching a target
    pub async fn discovery_layer(&self, target: &AuthorityId) -> DiscoveryLayer {
        self.topology.read().await.discovery_layer(target)
    }

    /// Get discovery context (layer + relevant peers)
    pub async fn discovery_context(
        &self,
        target: &AuthorityId,
    ) -> (DiscoveryLayer, Vec<AuthorityId>) {
        self.topology.read().await.discovery_context(target)
    }

    /// Check if we have social presence
    pub async fn has_social_presence(&self) -> bool {
        self.topology.read().await.has_social_presence()
    }

    // ========================================================================
    // Relay Selection
    // ========================================================================

    /// Select relays for a context
    pub async fn select_relays(
        &self,
        context: &RelayContext,
        candidates: &[RelayCandidate],
    ) -> Vec<AuthorityId> {
        self.relay_selector.select(context, candidates)
    }

    /// Build relay candidates from topology
    pub async fn build_relay_candidates<F>(
        &self,
        destination: &AuthorityId,
        reachability: F,
    ) -> Vec<RelayCandidate>
    where
        F: FnMut(&AuthorityId) -> bool,
    {
        self.topology
            .read()
            .await
            .build_relay_candidates(destination, reachability)
    }

    /// Build fused runtime-local provider candidates from neighborhood and WoT inputs.
    ///
    /// `aura-social` and `aura-relational` stay authoritative for their own
    /// facts and pure derivations; `aura-agent` owns the final fused permit view.
    pub async fn build_provider_candidates<F>(
        &self,
        family: ServiceFamily,
        destination: &AuthorityId,
        reachability: F,
        trust_evidence: &[WebOfTrustEvidence],
    ) -> Vec<ProviderCandidate>
    where
        F: Fn(&AuthorityId) -> bool + Copy,
    {
        let neighborhood_candidates =
            self.topology
                .read()
                .await
                .build_provider_candidates(family, destination, reachability);

        if family == ServiceFamily::Hold {
            return neighborhood_candidates;
        }

        let mut merged: BTreeMap<AuthorityId, ProviderCandidate> = neighborhood_candidates
            .into_iter()
            .map(|candidate| (candidate.authority_id, candidate))
            .collect();

        for evidence in trust_evidence {
            let entry = merged
                .entry(evidence.authority_id)
                .or_insert_with(|| ProviderCandidate {
                    authority_id: evidence.authority_id,
                    device_id: None,
                    family,
                    evidence: Vec::new(),
                    link_endpoints: Vec::new(),
                    reachable: reachability(&evidence.authority_id),
                });
            if !entry.evidence.contains(&evidence.evidence) {
                entry.evidence.push(evidence.evidence.clone());
            }
        }

        merged.into_values().collect()
    }

    /// Get the relay selector
    pub fn relay_selector(&self) -> Arc<DeterministicRandomSelector> {
        self.relay_selector.clone()
    }

    // ========================================================================
    // Peer Queries
    // ========================================================================

    /// Get all home peers
    pub async fn same_home_members(&self) -> Vec<AuthorityId> {
        self.topology.read().await.same_home_members()
    }

    /// Get all neighborhood peers
    pub async fn neighborhood_hop_members(&self) -> Vec<AuthorityId> {
        self.topology.read().await.neighborhood_hop_members()
    }

    /// Get all known peers
    pub async fn all_peers(&self) -> Vec<AuthorityId> {
        self.topology.read().await.all_peers()
    }

    /// Check if we know a peer
    pub async fn knows_peer(&self, peer: &AuthorityId) -> bool {
        self.topology.read().await.knows_peer(peer)
    }

    // ========================================================================
    // Accessors
    // ========================================================================

    /// Get the configuration
    pub fn config(&self) -> &SocialManagerConfig {
        &self.config
    }

    /// Get the authority ID
    pub fn authority_id(&self) -> AuthorityId {
        self.authority_id
    }
}

// =============================================================================
// RuntimeService Implementation
// =============================================================================

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl RuntimeService for SocialManager {
    fn name(&self) -> &'static str {
        "social_manager"
    }

    async fn start(&self, _context: &RuntimeServiceContext) -> Result<(), ServiceError> {
        // Mark as ready - topology will be populated by journal sync
        *self.state.write().await = SocialManagerState::Ready;
        Ok(())
    }

    async fn stop(&self) -> Result<(), ServiceError> {
        *self.state.write().await = SocialManagerState::Uninitialized;
        Ok(())
    }

    async fn health(&self) -> ServiceHealth {
        self.health_async().await
    }
}

impl SocialManager {
    /// Get the service health status asynchronously
    pub async fn health_async(&self) -> ServiceHealth {
        let state = *self.state.read().await;
        match state {
            SocialManagerState::Uninitialized => ServiceHealth::NotStarted,
            SocialManagerState::Ready => ServiceHealth::Healthy,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::effects::relay::RelayRelationship;
    use aura_core::service::ProviderEvidence;
    use aura_core::time::PhysicalTime;
    use aura_social::facts::HomeId;

    fn test_authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    #[tokio::test]
    async fn test_manager_creation() {
        let manager = SocialManager::with_defaults(test_authority(1));
        assert_eq!(manager.state().await, SocialManagerState::Uninitialized);
        assert!(!manager.is_ready().await);
    }

    #[tokio::test]
    async fn test_manager_initialization() {
        let authority = test_authority(1);
        let manager = SocialManager::with_defaults(authority);

        // Initialize with empty topology
        let topology = SocialTopology::empty(authority);
        manager.initialize(topology).await;

        assert!(manager.is_ready().await);
        assert!(!manager.has_social_presence().await);
    }

    #[tokio::test]
    async fn test_manager_with_block() {
        let local = test_authority(1);
        let peer = test_authority(2);

        let manager = SocialManager::with_defaults(local);

        let home_id = HomeId::from_bytes([1u8; 32]);
        let mut home_state = Home::new_empty(home_id);
        home_state.members = vec![local, peer];

        manager
            .initialize_with_social(Some(home_state), vec![])
            .await;

        assert!(manager.is_ready().await);
        assert!(manager.has_social_presence().await);

        let peers = manager.same_home_members().await;
        assert_eq!(peers.len(), 1);
        assert!(peers.contains(&peer));
    }

    #[tokio::test]
    async fn build_provider_candidates_fuses_neighborhood_and_wot_inputs() {
        let local = test_authority(1);
        let neighborhood_peer = test_authority(2);
        let introduced_peer = test_authority(3);
        let target = test_authority(9);
        let manager = SocialManager::with_defaults(local);

        manager
            .add_peer(
                neighborhood_peer,
                RelayRelationship::SameHome {
                    home_id: *HomeId::from_bytes([4u8; 32]).as_bytes(),
                },
            )
            .await;

        let trust = vec![WebOfTrustEvidence {
            authority_id: introduced_peer,
            evidence: ProviderEvidence::IntroducedFof,
            context_id: aura_core::ContextId::new_from_entropy([5u8; 32]),
            introduced_by: Some(neighborhood_peer),
            expires_at: Some(PhysicalTime {
                ts_ms: 1000,
                uncertainty: None,
            }),
            remaining_depth: 1,
            max_fanout: 1,
        }];

        let candidates = manager
            .build_provider_candidates(ServiceFamily::Move, &target, |_| true, &trust)
            .await;

        assert!(candidates.iter().any(|candidate| {
            candidate.authority_id == neighborhood_peer
                && candidate.evidence.contains(&ProviderEvidence::Neighborhood)
        }));
        assert!(candidates.iter().any(|candidate| {
            candidate.authority_id == introduced_peer
                && candidate
                    .evidence
                    .contains(&ProviderEvidence::IntroducedFof)
        }));
    }

    #[tokio::test]
    async fn build_provider_candidates_keeps_family_shape_neutral() {
        let local = test_authority(1);
        let peer = test_authority(2);
        let target = test_authority(9);
        let manager = SocialManager::with_defaults(local);

        manager
            .add_peer(
                peer,
                RelayRelationship::SameHome {
                    home_id: *HomeId::from_bytes([4u8; 32]).as_bytes(),
                },
            )
            .await;

        for family in [
            ServiceFamily::Establish,
            ServiceFamily::Move,
            ServiceFamily::Hold,
        ] {
            let candidates = manager
                .build_provider_candidates(family, &target, |_| true, &[])
                .await;
            assert_eq!(candidates.len(), 1);
            assert_eq!(candidates[0].family, family);
            assert_eq!(candidates[0].evidence, vec![ProviderEvidence::Neighborhood]);
            assert!(candidates[0].link_endpoints.is_empty());
        }
    }

    #[tokio::test]
    async fn hold_provider_candidates_ignore_wot_overlays() {
        let local = test_authority(1);
        let neighborhood_peer = test_authority(2);
        let introduced_peer = test_authority(3);
        let target = test_authority(9);
        let manager = SocialManager::with_defaults(local);

        manager
            .add_peer(
                neighborhood_peer,
                RelayRelationship::SameHome {
                    home_id: *HomeId::from_bytes([4u8; 32]).as_bytes(),
                },
            )
            .await;

        let trust = vec![WebOfTrustEvidence {
            authority_id: introduced_peer,
            evidence: ProviderEvidence::IntroducedFof,
            context_id: aura_core::ContextId::new_from_entropy([5u8; 32]),
            introduced_by: Some(neighborhood_peer),
            expires_at: Some(PhysicalTime {
                ts_ms: 1000,
                uncertainty: None,
            }),
            remaining_depth: 1,
            max_fanout: 1,
        }];

        let candidates = manager
            .build_provider_candidates(ServiceFamily::Hold, &target, |_| true, &trust)
            .await;

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].authority_id, neighborhood_peer);
        assert_eq!(candidates[0].evidence, vec![ProviderEvidence::Neighborhood]);
    }

    #[tokio::test]
    async fn test_discovery_layer() {
        let local = test_authority(1);
        let peer = test_authority(2);
        let unknown = test_authority(99);

        let manager = SocialManager::with_defaults(local);

        let home_id = HomeId::from_bytes([1u8; 32]);
        let mut topology = SocialTopology::empty(local);
        topology.add_peer(
            peer,
            RelayRelationship::SameHome {
                home_id: *home_id.as_bytes(),
            },
        );

        manager.initialize(topology).await;

        // Known peer = Direct
        assert_eq!(manager.discovery_layer(&peer).await, DiscoveryLayer::Direct);

        // Unknown peer with no social presence = Rendezvous
        assert_eq!(
            manager.discovery_layer(&unknown).await,
            DiscoveryLayer::Rendezvous
        );
    }

    #[tokio::test]
    async fn test_add_peer() {
        let local = test_authority(1);
        let peer = test_authority(2);

        let manager = SocialManager::with_defaults(local);
        manager.initialize(SocialTopology::empty(local)).await;

        assert!(!manager.knows_peer(&peer).await);

        let home_id = HomeId::from_bytes([1u8; 32]);
        manager
            .add_peer(
                peer,
                RelayRelationship::SameHome {
                    home_id: *home_id.as_bytes(),
                },
            )
            .await;

        assert!(manager.knows_peer(&peer).await);
    }
}
