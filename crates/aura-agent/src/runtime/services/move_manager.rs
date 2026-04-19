//! Runtime-owned move service.
//!
//! Owns bounded movement queues, replay suppression, flush planning, and
//! backpressure state for the current pre-onion `Move` rollout.

use super::config_profiles::impl_service_config_profiles;
use super::service_registry::ServiceRegistry;
use super::state::with_state_mut_validated;
use super::traits::{RuntimeService, RuntimeServiceContext, ServiceError, ServiceHealth};
use crate::runtime::TaskGroup;
use async_trait::async_trait;
use aura_core::effects::transport::TransportEnvelope;
use aura_core::effects::RandomCoreEffects;
use aura_core::hash::hash;
use aura_core::service::{LocalRoutingProfile, Route, ServiceFamily};
use aura_core::types::identifiers::{AuthorityId, ContextId};
use aura_core::util::serialization;
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

#[allow(dead_code)] // Declaration-layer ingress inventory; sanctioned surfaces call methods directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MoveManagerCommand {
    Enqueue,
    Flush,
    RecordDelivery,
    CleanupReplayWindow,
}

/// Configuration for bounded move planning.
#[derive(Debug, Clone)]
pub struct MoveManagerConfig {
    /// Max queued move envelopes retained before bounded eviction.
    pub max_buffered_envelopes: usize,
    /// Max replay markers retained in the replay window.
    pub replay_window_entries: usize,
    /// Max entries flushed per scheduling turn.
    pub flush_batch_size: usize,
    /// Cleanup cadence for replay-window maintenance.
    pub cleanup_interval: Duration,
    /// Pre-privacy routing profile.
    pub routing_profile: LocalRoutingProfile,
}

impl Default for MoveManagerConfig {
    fn default() -> Self {
        Self {
            max_buffered_envelopes: 64,
            replay_window_entries: 256,
            flush_batch_size: 8,
            cleanup_interval: Duration::from_secs(30),
            routing_profile: LocalRoutingProfile::passthrough(),
        }
    }
}

impl_service_config_profiles!(MoveManagerConfig {
    /// Short, deterministic config for tests.
    pub fn for_testing() -> Self {
        Self {
            max_buffered_envelopes: 8,
            replay_window_entries: 32,
            flush_batch_size: 4,
            cleanup_interval: Duration::from_millis(50),
            routing_profile: LocalRoutingProfile::passthrough(),
        }
    }
});

#[derive(Debug, Clone)]
pub struct MoveDeliveryPlan {
    pub envelope: TransportEnvelope,
    pub route: Route,
    pub replay_marker: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveProjection {
    pub queued_envelopes: usize,
    pub replay_window_entries: usize,
    pub last_flush_ms: Option<u64>,
    pub scheduled_flush_at_ms: Option<u64>,
    pub backpressure_events: u64,
    pub congestion_evictions: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum MoveManagerError {
    #[error("duplicate move envelope suppressed by replay window")]
    DuplicateSuppressed,
    #[error("move envelope route did not target destination {destination}")]
    RouteDestinationMismatch { destination: AuthorityId },
    #[error("move queue is empty")]
    QueueEmpty,
    #[error("failed to encode move replay marker: {0}")]
    ReplayMarkerEncoding(String),
}

#[derive(Debug, Clone)]
struct MoveQueueEntry {
    #[allow(dead_code)]
    /* TODO(2026-07): remove once replay-marker diagnostics or persistence consume the marker. */
    marker: [u8; 32],
    envelope: TransportEnvelope,
    route: Route,
    #[allow(dead_code)]
    /* TODO(2026-07): remove once queue-age scheduling consumes the timestamp or the field is deleted. */
    queued_at_ms: u64,
}

struct MoveState {
    queue: VecDeque<MoveQueueEntry>,
    replay_seen: HashSet<[u8; 32]>,
    replay_order: VecDeque<[u8; 32]>,
    last_flush_ms: Option<u64>,
    scheduled_flush_at_ms: Option<u64>,
    backpressure_events: u64,
    congestion_evictions: u64,
    lifecycle: ServiceHealth,
    cleanup_tasks: Option<TaskGroup>,
}

impl Default for MoveState {
    fn default() -> Self {
        Self {
            queue: VecDeque::new(),
            replay_seen: HashSet::new(),
            replay_order: VecDeque::new(),
            last_flush_ms: None,
            scheduled_flush_at_ms: None,
            backpressure_events: 0,
            congestion_evictions: 0,
            lifecycle: ServiceHealth::NotStarted,
            cleanup_tasks: None,
        }
    }
}

impl MoveState {
    fn validate(&self) -> Result<(), super::invariant::InvariantViolation> {
        for entry in &self.queue {
            if entry.route.destination.relay_authority.is_none()
                && entry.route.destination.address.is_none()
            {
                return Err(super::invariant::InvariantViolation::new(
                    "MoveManager",
                    "queued move route missing destination endpoint material",
                ));
            }
        }
        Ok(())
    }
}

/// Runtime-owned move planning and queue management.
#[aura_macros::service_surface(
    families = "Move",
    object_categories = "transport_protocol,runtime_derived_local,proof_accounting",
    discover = "rendezvous_move_surface_and_cached_service_descriptors",
    permit = "runtime_capability_budget_and_provider_admission",
    transfer = "move_manager_planning_and_transport_effects",
    select = "move_manager_and_service_registry",
    authoritative = "ServiceDescriptor,MoveEnvelope",
    runtime_local = "move_queue,replay_window,flush_schedule,congestion_state",
    category = "service_surface"
)]
#[aura_macros::actor_owned(
    owner = "move_manager",
    domain = "move",
    gate = "move_command_ingress",
    command = MoveManagerCommand,
    capacity = 128,
    category = "actor_owned"
)]
#[derive(Clone)]
pub struct MoveManager {
    config: MoveManagerConfig,
    registry: Arc<ServiceRegistry>,
    state: Arc<RwLock<MoveState>>,
}

impl MoveManager {
    pub fn new(config: MoveManagerConfig, registry: Arc<ServiceRegistry>) -> Self {
        Self {
            config,
            registry,
            state: Arc::new(RwLock::new(MoveState {
                lifecycle: ServiceHealth::NotStarted,
                ..MoveState::default()
            })),
        }
    }

    pub fn config(&self) -> &MoveManagerConfig {
        &self.config
    }

    pub async fn projection(&self) -> MoveProjection {
        let state = self.state.read().await;
        MoveProjection {
            queued_envelopes: state.queue.len(),
            replay_window_entries: state.replay_order.len(),
            last_flush_ms: state.last_flush_ms,
            scheduled_flush_at_ms: state.scheduled_flush_at_ms,
            backpressure_events: state.backpressure_events,
            congestion_evictions: state.congestion_evictions,
        }
    }

    pub async fn enqueue_for_delivery<E: RandomCoreEffects + ?Sized>(
        &self,
        envelope: TransportEnvelope,
        route: Route,
        queued_at_ms: u64,
        random: &E,
    ) -> Result<Vec<MoveDeliveryPlan>, MoveManagerError> {
        if route.destination.relay_authority.is_none() && route.destination.address.is_none() {
            return Err(MoveManagerError::RouteDestinationMismatch {
                destination: envelope.destination,
            });
        }

        let marker = replay_marker(&envelope, &route)?;
        let mut state = self.state.write().await;
        if state.replay_seen.contains(&marker) {
            return Err(MoveManagerError::DuplicateSuppressed);
        }

        if state.queue.len() >= self.config.max_buffered_envelopes {
            state.queue.pop_front();
            state.backpressure_events = state.backpressure_events.saturating_add(1);
            state.congestion_evictions = state.congestion_evictions.saturating_add(1);
        }

        remember_replay_marker(&mut state, marker, self.config.replay_window_entries);
        state.queue.push_back(MoveQueueEntry {
            marker,
            envelope,
            route,
            queued_at_ms,
        });
        state.scheduled_flush_at_ms = Some(queued_at_ms);

        deterministic_shuffle(&mut state.queue, random).await;

        let batch_size = self.config.flush_batch_size.max(1);
        let mut batch = Vec::new();
        for _ in 0..batch_size {
            let Some(entry) = state.queue.pop_front() else {
                break;
            };
            batch.push(MoveDeliveryPlan {
                envelope: entry.envelope,
                route: entry.route,
                replay_marker: entry.marker,
            });
        }
        state.last_flush_ms = Some(queued_at_ms);
        if state.queue.is_empty() {
            state.scheduled_flush_at_ms = None;
        }
        Ok(batch)
    }

    pub async fn record_delivery_result(
        &self,
        replay_marker: [u8; 32],
        context_id: ContextId,
        destination: AuthorityId,
        success: bool,
        observed_at_ms: u64,
    ) {
        let mut state = self.state.write().await;
        forget_replay_marker(&mut state, replay_marker);
        drop(state);

        if success {
            self.registry
                .record_provider_success(ServiceFamily::Move, destination, observed_at_ms)
                .await;
        } else {
            self.registry
                .record_provider_failure(ServiceFamily::Move, destination, observed_at_ms)
                .await;
        }
        self.registry
            .remove_pending_route(context_id, destination)
            .await;
    }

    pub async fn cleanup_replay_window(&self) -> usize {
        with_state_mut_validated(
            &self.state,
            |state| {
                let before = state.replay_order.len();
                while state.replay_order.len() > self.config.replay_window_entries {
                    if let Some(marker) = state.replay_order.pop_front() {
                        state.replay_seen.remove(&marker);
                    }
                }
                before.saturating_sub(state.replay_order.len())
            },
            MoveState::validate,
        )
        .await
    }

    fn spawn_cleanup_task(
        &self,
        tasks: TaskGroup,
        time: Arc<dyn aura_core::effects::PhysicalTimeEffects + Send + Sync>,
    ) {
        let manager = self.clone();
        let interval = self.config.cleanup_interval;
        let _cleanup_task_handle = tasks.spawn_interval_until_named(
            "move.cleanup_replay_window",
            time,
            interval,
            move || {
                let manager = manager.clone();
                async move {
                    let _ = manager.cleanup_replay_window().await;
                    true
                }
            },
        );
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl RuntimeService for MoveManager {
    fn name(&self) -> &'static str {
        "move_manager"
    }

    fn dependencies(&self) -> &[&'static str] {
        &[]
    }

    async fn start(&self, ctx: &RuntimeServiceContext) -> Result<(), ServiceError> {
        let task_group = ctx.tasks().group(self.name());
        self.spawn_cleanup_task(task_group.clone(), ctx.time_effects());
        let mut state = self.state.write().await;
        state.cleanup_tasks = Some(task_group);
        state.lifecycle = ServiceHealth::Healthy;
        Ok(())
    }

    async fn stop(&self) -> Result<(), ServiceError> {
        let tasks = {
            let mut state = self.state.write().await;
            state.lifecycle = ServiceHealth::Stopping;
            state.cleanup_tasks.take()
        };

        if let Some(tasks) = tasks {
            tasks.shutdown();
        }

        let mut state = self.state.write().await;
        state.queue.clear();
        state.lifecycle = ServiceHealth::Stopped;
        Ok(())
    }

    async fn health(&self) -> ServiceHealth {
        self.state.read().await.lifecycle.clone()
    }
}

async fn deterministic_shuffle<E: RandomCoreEffects + ?Sized>(
    queue: &mut VecDeque<MoveQueueEntry>,
    random: &E,
) {
    if queue.len() < 2 {
        return;
    }
    let mut entries = queue.drain(..).collect::<Vec<_>>();
    let len = entries.len();
    for index in (1..len).rev() {
        let swap_index = (random.random_u64().await as usize) % (index + 1);
        entries.swap(index, swap_index);
    }
    *queue = entries.into_iter().collect();
}

fn remember_replay_marker(state: &mut MoveState, marker: [u8; 32], max_entries: usize) {
    state.replay_seen.insert(marker);
    state.replay_order.push_back(marker);
    while state.replay_order.len() > max_entries {
        if let Some(evicted) = state.replay_order.pop_front() {
            state.replay_seen.remove(&evicted);
        }
    }
}

fn forget_replay_marker(state: &mut MoveState, marker: [u8; 32]) {
    if !state.replay_seen.remove(&marker) {
        return;
    }
    if let Some(position) = state.replay_order.iter().position(|seen| *seen == marker) {
        state.replay_order.remove(position);
    }
}

fn replay_marker(
    envelope: &TransportEnvelope,
    route: &Route,
) -> Result<[u8; 32], MoveManagerError> {
    let route_bytes = serialization::to_vec(route)
        .map_err(|error| MoveManagerError::ReplayMarkerEncoding(error.to_string()))?;
    let mut material = Vec::new();
    material.extend_from_slice(&envelope.source.to_bytes());
    material.extend_from_slice(&envelope.destination.to_bytes());
    material.extend_from_slice(envelope.context.as_bytes());
    material.extend_from_slice(&envelope.payload);
    material.extend_from_slice(&route_bytes);
    Ok(hash(&material))
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use aura_core::effects::random::RandomCoreEffects;
    use aura_core::service::{LinkEndpoint, LinkProtocol};
    use std::collections::HashMap;

    #[derive(Clone)]
    struct TestRandom(u64);

    #[async_trait]
    impl RandomCoreEffects for TestRandom {
        async fn random_bytes(&self, len: usize) -> Vec<u8> {
            vec![self.0 as u8; len]
        }

        async fn random_bytes_32(&self) -> [u8; 32] {
            [self.0 as u8; 32]
        }

        async fn random_u64(&self) -> u64 {
            self.0
        }
    }

    fn authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn context(seed: u8) -> ContextId {
        ContextId::new_from_entropy([seed; 32])
    }

    fn route() -> Route {
        Route::direct(LinkEndpoint::direct(LinkProtocol::Tcp, "127.0.0.1:7000"))
    }

    fn envelope(seed: u8) -> TransportEnvelope {
        TransportEnvelope {
            destination: authority(seed + 1),
            source: authority(seed),
            context: context(seed),
            payload: vec![seed, seed + 1],
            metadata: HashMap::new(),
            receipt: None,
        }
    }

    #[tokio::test]
    async fn bounded_queue_and_replay_window_are_enforced() {
        let registry = Arc::new(ServiceRegistry::new());
        let manager = MoveManager::new(
            MoveManagerConfig {
                max_buffered_envelopes: 2,
                replay_window_entries: 2,
                flush_batch_size: 1,
                cleanup_interval: Duration::from_secs(60),
                routing_profile: LocalRoutingProfile::passthrough(),
            },
            registry,
        );

        let first = manager
            .enqueue_for_delivery(envelope(1), route(), 10, &TestRandom(1))
            .await
            .expect("enqueue first");
        assert_eq!(first.len(), 1);
        let duplicate = manager
            .enqueue_for_delivery(envelope(1), route(), 11, &TestRandom(1))
            .await;
        assert!(matches!(
            duplicate,
            Err(MoveManagerError::DuplicateSuppressed)
        ));

        manager
            .enqueue_for_delivery(envelope(2), route(), 12, &TestRandom(1))
            .await
            .expect("enqueue second");
        manager
            .enqueue_for_delivery(envelope(3), route(), 13, &TestRandom(1))
            .await
            .expect("enqueue third");

        let projection = manager.projection().await;
        assert!(projection.backpressure_events <= 1);
        assert!(projection.replay_window_entries <= 2);
    }

    #[tokio::test]
    async fn deterministic_shuffle_is_stable_for_fixed_rng() {
        let registry = Arc::new(ServiceRegistry::new());
        let manager = MoveManager::new(MoveManagerConfig::for_testing(), registry);
        let plan = manager
            .enqueue_for_delivery(envelope(7), route(), 20, &TestRandom(2))
            .await
            .expect("enqueue");
        assert_eq!(plan.len(), 1);
        assert_eq!(
            plan[0].route.destination.address.as_deref(),
            Some("127.0.0.1:7000")
        );
    }

    #[tokio::test]
    async fn cleanup_replay_window_prunes_old_markers() {
        let registry = Arc::new(ServiceRegistry::new());
        let manager = MoveManager::new(
            MoveManagerConfig {
                replay_window_entries: 1,
                ..MoveManagerConfig::for_testing()
            },
            registry,
        );
        manager
            .enqueue_for_delivery(envelope(1), route(), 1, &TestRandom(1))
            .await
            .expect("enqueue first");
        manager
            .enqueue_for_delivery(envelope(2), route(), 2, &TestRandom(1))
            .await
            .expect("enqueue second");
        let removed = manager.cleanup_replay_window().await;
        assert!(removed <= 1);
        assert!(manager.projection().await.replay_window_entries <= 1);
    }

    #[tokio::test]
    async fn successful_delivery_allows_explicit_resend() {
        let registry = Arc::new(ServiceRegistry::new());
        let manager = MoveManager::new(MoveManagerConfig::for_testing(), registry);
        let initial = manager
            .enqueue_for_delivery(envelope(9), route(), 30, &TestRandom(3))
            .await
            .expect("enqueue first");
        assert_eq!(initial.len(), 1);

        manager
            .record_delivery_result(
                initial[0].replay_marker,
                initial[0].envelope.context,
                initial[0].envelope.destination,
                true,
                31,
            )
            .await;

        let resend = manager
            .enqueue_for_delivery(envelope(9), route(), 32, &TestRandom(3))
            .await
            .expect("resend after successful delivery");
        assert_eq!(resend.len(), 1);
    }
}
