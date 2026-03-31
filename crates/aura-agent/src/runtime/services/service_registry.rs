//! Unified runtime-owned local service registry.
//!
//! This registry owns mutable local service views such as descriptor snapshots,
//! provider health, runtime-local selection state, pending route handshakes,
//! and hold observations. These are all derived, actor-owned runtime views and
//! must not be treated as replicated truth.

use super::invariant::InvariantViolation;
use super::state::with_state_mut_validated;
use aura_core::service::{Route, SelectionState, ServiceFamily};
use aura_core::types::identifiers::{AuthorityId, ContextId, DeviceId};
use aura_rendezvous::RendezvousDescriptor;
use std::collections::HashMap;
use tokio::sync::RwLock;

#[allow(dead_code)] // Declaration-layer ingress inventory; sanctioned surfaces call methods directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServiceRegistryCommand {
    CacheDescriptor,
    RemoveDescriptor,
    RecordProviderHealth,
    RecordSelectionState,
    RecordHoldObservation,
    TrackPendingRoute,
    RemovePendingRoute,
    InvalidateScopeEpoch,
    CleanupExpiredDescriptors,
    ClearScope,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderHealthSnapshot {
    pub authority_id: AuthorityId,
    pub family: ServiceFamily,
    pub success_count: u32,
    pub failure_count: u32,
    pub last_observed_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoldObservation {
    pub scope: ContextId,
    pub authority_id: AuthorityId,
    pub observed_at_ms: u64,
    pub retention_until: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingRouteState {
    pub scope: ContextId,
    pub authority_id: AuthorityId,
    pub family: ServiceFamily,
    pub route: Option<Route>,
    pub initiated_at_ms: u64,
}

#[derive(Debug, Clone, Default)]
pub struct ServiceRegistryProjection {
    pub descriptors: Vec<RendezvousDescriptor>,
    pub provider_health: Vec<ProviderHealthSnapshot>,
    pub selection_state: Vec<SelectionState>,
    pub hold_observations: Vec<HoldObservation>,
    pub pending_routes: Vec<PendingRouteState>,
}

#[derive(Debug, Clone, Default)]
struct ProviderHealthRecord {
    success_count: u32,
    failure_count: u32,
    last_observed_ms: u64,
}

#[derive(Debug, Default)]
struct ServiceRegistryState {
    descriptors: HashMap<(ContextId, AuthorityId), RendezvousDescriptor>,
    provider_health: HashMap<(ServiceFamily, AuthorityId), ProviderHealthRecord>,
    selection_state: HashMap<(ContextId, ServiceFamily), SelectionState>,
    hold_observations: HashMap<(ContextId, AuthorityId), HoldObservation>,
    pending_routes: HashMap<(ContextId, AuthorityId), PendingRouteState>,
}

impl ServiceRegistryState {
    fn validate(&self) -> Result<(), InvariantViolation> {
        for ((scope, authority_id), descriptor) in &self.descriptors {
            if *scope != descriptor.context_id || *authority_id != descriptor.authority_id {
                return Err(InvariantViolation::new(
                    "ServiceRegistry",
                    format!(
                        "descriptor key mismatch: ({scope:?}, {authority_id:?}) vs ({:?}, {:?})",
                        descriptor.context_id, descriptor.authority_id
                    ),
                ));
            }
        }

        for ((scope, family), state) in &self.selection_state {
            if state.family != *family {
                return Err(InvariantViolation::new(
                    "ServiceRegistry",
                    format!(
                        "selection family mismatch for scope {scope:?}: expected {family:?}, found {:?}",
                        state.family
                    ),
                ));
            }
        }

        for ((scope, authority_id), observation) in &self.hold_observations {
            if observation.scope != *scope || observation.authority_id != *authority_id {
                return Err(InvariantViolation::new(
                    "ServiceRegistry",
                    format!(
                        "hold observation key mismatch: ({scope:?}, {authority_id:?}) vs ({:?}, {:?})",
                        observation.scope, observation.authority_id
                    ),
                ));
            }
        }

        for ((scope, authority_id), route) in &self.pending_routes {
            if route.scope != *scope || route.authority_id != *authority_id {
                return Err(InvariantViolation::new(
                    "ServiceRegistry",
                    format!(
                        "pending route key mismatch: ({scope:?}, {authority_id:?}) vs ({:?}, {:?})",
                        route.scope, route.authority_id
                    ),
                ));
            }
        }

        Ok(())
    }
}

fn family_sort_key(family: ServiceFamily) -> u8 {
    match family {
        ServiceFamily::Establish => 0,
        ServiceFamily::Move => 1,
        ServiceFamily::Hold => 2,
    }
}

/// Canonical runtime-owned local registry for service-derived state.
#[derive(Default)]
#[aura_macros::actor_owned(
    owner = "service_registry",
    domain = "service_registry",
    gate = "service_registry_command_ingress",
    command = ServiceRegistryCommand,
    capacity = 128,
    category = "actor_owned"
)]
pub struct ServiceRegistryService {
    state: RwLock<ServiceRegistryState>,
}

impl ServiceRegistryService {
    pub fn new() -> Self {
        Self {
            state: RwLock::new(ServiceRegistryState::default()),
        }
    }

    pub async fn cache_descriptor(&self, descriptor: RendezvousDescriptor) {
        with_state_mut_validated(
            &self.state,
            |state| {
                state
                    .descriptors
                    .insert((descriptor.context_id, descriptor.authority_id), descriptor);
            },
            ServiceRegistryState::validate,
        )
        .await;
    }

    pub async fn remove_descriptor(&self, context_id: ContextId, authority_id: AuthorityId) {
        let _ = with_state_mut_validated(
            &self.state,
            |state| state.descriptors.remove(&(context_id, authority_id)),
            ServiceRegistryState::validate,
        )
        .await;
    }

    pub async fn get_descriptor(
        &self,
        context_id: ContextId,
        authority_id: AuthorityId,
    ) -> Option<RendezvousDescriptor> {
        self.state
            .read()
            .await
            .descriptors
            .get(&(context_id, authority_id))
            .cloned()
    }

    pub async fn get_any_descriptor_for_authority(
        &self,
        authority_id: AuthorityId,
    ) -> Option<RendezvousDescriptor> {
        self.state
            .read()
            .await
            .descriptors
            .values()
            .find(|descriptor| descriptor.authority_id == authority_id)
            .cloned()
    }

    pub async fn descriptor_needs_refresh(
        &self,
        context_id: ContextId,
        authority_id: AuthorityId,
        refresh_window_ms: u64,
        now_ms: u64,
    ) -> bool {
        self.state
            .read()
            .await
            .descriptors
            .get(&(context_id, authority_id))
            .map(|descriptor| {
                let refresh_threshold = descriptor.valid_until.saturating_sub(refresh_window_ms);
                now_ms >= refresh_threshold
            })
            .unwrap_or(true)
    }

    pub async fn contexts_needing_refresh(
        &self,
        authority_id: AuthorityId,
        refresh_window_ms: u64,
        now_ms: u64,
    ) -> Vec<ContextId> {
        let mut contexts = self
            .state
            .read()
            .await
            .descriptors
            .iter()
            .filter(|((_, candidate_authority), descriptor)| {
                *candidate_authority == authority_id && {
                    let refresh_threshold =
                        descriptor.valid_until.saturating_sub(refresh_window_ms);
                    now_ms >= refresh_threshold
                }
            })
            .map(|((context_id, _), _)| *context_id)
            .collect::<Vec<_>>();
        contexts.sort();
        contexts.dedup();
        contexts
    }

    pub async fn list_descriptors_in_context(
        &self,
        context_id: ContextId,
        now_ms: u64,
    ) -> Vec<RendezvousDescriptor> {
        let mut descriptors = self
            .state
            .read()
            .await
            .descriptors
            .values()
            .filter(|descriptor| descriptor.context_id == context_id && descriptor.is_valid(now_ms))
            .cloned()
            .collect::<Vec<_>>();
        descriptors.sort_by_key(|descriptor| {
            (
                descriptor.context_id,
                descriptor.authority_id,
                descriptor.device_id,
            )
        });
        descriptors
    }

    pub async fn cleanup_expired_descriptors(&self, now_ms: u64) -> usize {
        with_state_mut_validated(
            &self.state,
            |state| {
                let before = state.descriptors.len();
                state
                    .descriptors
                    .retain(|_, descriptor| descriptor.is_valid(now_ms));
                before.saturating_sub(state.descriptors.len())
            },
            ServiceRegistryState::validate,
        )
        .await
    }

    pub async fn record_provider_success(
        &self,
        family: ServiceFamily,
        authority_id: AuthorityId,
        observed_at_ms: u64,
    ) {
        self.record_provider_health(family, authority_id, true, observed_at_ms)
            .await;
    }

    pub async fn record_provider_failure(
        &self,
        family: ServiceFamily,
        authority_id: AuthorityId,
        observed_at_ms: u64,
    ) {
        self.record_provider_health(family, authority_id, false, observed_at_ms)
            .await;
    }

    async fn record_provider_health(
        &self,
        family: ServiceFamily,
        authority_id: AuthorityId,
        success: bool,
        observed_at_ms: u64,
    ) {
        with_state_mut_validated(
            &self.state,
            |state| {
                let entry = state
                    .provider_health
                    .entry((family, authority_id))
                    .or_default();
                if success {
                    entry.success_count = entry.success_count.saturating_add(1);
                } else {
                    entry.failure_count = entry.failure_count.saturating_add(1);
                }
                entry.last_observed_ms = observed_at_ms;
            },
            ServiceRegistryState::validate,
        )
        .await;
    }

    pub async fn provider_health(
        &self,
        family: ServiceFamily,
        authority_id: AuthorityId,
    ) -> Option<ProviderHealthSnapshot> {
        self.state
            .read()
            .await
            .provider_health
            .get(&(family, authority_id))
            .map(|record| ProviderHealthSnapshot {
                authority_id,
                family,
                success_count: record.success_count,
                failure_count: record.failure_count,
                last_observed_ms: record.last_observed_ms,
            })
    }

    pub async fn record_selection_state(&self, scope: ContextId, state: SelectionState) {
        with_state_mut_validated(
            &self.state,
            |registry| {
                registry
                    .selection_state
                    .insert((scope, state.family), state);
            },
            ServiceRegistryState::validate,
        )
        .await;
    }

    pub async fn selection_state(
        &self,
        scope: ContextId,
        family: ServiceFamily,
    ) -> Option<SelectionState> {
        self.state
            .read()
            .await
            .selection_state
            .get(&(scope, family))
            .cloned()
    }

    pub async fn track_pending_route(
        &self,
        scope: ContextId,
        authority_id: AuthorityId,
        family: ServiceFamily,
        route: Option<Route>,
        initiated_at_ms: u64,
    ) {
        with_state_mut_validated(
            &self.state,
            |state| {
                state.pending_routes.insert(
                    (scope, authority_id),
                    PendingRouteState {
                        scope,
                        authority_id,
                        family,
                        route,
                        initiated_at_ms,
                    },
                );
            },
            ServiceRegistryState::validate,
        )
        .await;
    }

    pub async fn remove_pending_route(&self, scope: ContextId, authority_id: AuthorityId) {
        let _ = with_state_mut_validated(
            &self.state,
            |state| state.pending_routes.remove(&(scope, authority_id)),
            ServiceRegistryState::validate,
        )
        .await;
    }

    pub async fn cleanup_stale_pending_routes(
        &self,
        now_ms: u64,
        pending_max_age_ms: u64,
    ) -> usize {
        with_state_mut_validated(
            &self.state,
            |state| {
                let before = state.pending_routes.len();
                state.pending_routes.retain(|_, pending| {
                    now_ms.saturating_sub(pending.initiated_at_ms) < pending_max_age_ms
                });
                before.saturating_sub(state.pending_routes.len())
            },
            ServiceRegistryState::validate,
        )
        .await
    }

    pub async fn record_hold_observation(
        &self,
        scope: ContextId,
        authority_id: AuthorityId,
        observed_at_ms: u64,
        retention_until: Option<u64>,
    ) {
        with_state_mut_validated(
            &self.state,
            |state| {
                state.hold_observations.insert(
                    (scope, authority_id),
                    HoldObservation {
                        scope,
                        authority_id,
                        observed_at_ms,
                        retention_until,
                    },
                );
            },
            ServiceRegistryState::validate,
        )
        .await;
    }

    pub async fn hold_observations(&self, scope: ContextId) -> Vec<HoldObservation> {
        self.state
            .read()
            .await
            .hold_observations
            .values()
            .filter(|observation| observation.scope == scope)
            .cloned()
            .collect()
    }

    pub async fn invalidate_scope_epoch(&self, scope: ContextId, epoch: u64) {
        let _ = with_state_mut_validated(
            &self.state,
            |state| {
                state
                    .selection_state
                    .retain(|(candidate_scope, _), selection| {
                        *candidate_scope != scope
                            || selection.epoch.map_or(true, |value| value == epoch)
                    });
                state
                    .hold_observations
                    .retain(|(candidate_scope, _), _| *candidate_scope != scope);
                state
                    .pending_routes
                    .retain(|(candidate_scope, _), _| *candidate_scope != scope);
            },
            ServiceRegistryState::validate,
        )
        .await;
    }

    pub async fn clear_scope(&self, scope: ContextId) {
        let _ = with_state_mut_validated(
            &self.state,
            |state| {
                state
                    .descriptors
                    .retain(|(candidate_scope, _), _| *candidate_scope != scope);
                state
                    .selection_state
                    .retain(|(candidate_scope, _), _| *candidate_scope != scope);
                state
                    .hold_observations
                    .retain(|(candidate_scope, _), _| *candidate_scope != scope);
                state
                    .pending_routes
                    .retain(|(candidate_scope, _), _| *candidate_scope != scope);
            },
            ServiceRegistryState::validate,
        )
        .await;
    }

    pub async fn projection(
        &self,
        scope: Option<ContextId>,
        now_ms: u64,
    ) -> ServiceRegistryProjection {
        let state = self.state.read().await;
        let mut descriptors = state
            .descriptors
            .values()
            .filter(|descriptor| descriptor.is_valid(now_ms))
            .filter(|descriptor| scope.map_or(true, |value| descriptor.context_id == value))
            .cloned()
            .collect::<Vec<_>>();
        descriptors.sort_by_key(|descriptor| {
            (
                descriptor.context_id,
                descriptor.authority_id,
                descriptor.device_id,
            )
        });
        let mut provider_health = state
            .provider_health
            .iter()
            .map(|((family, authority_id), record)| ProviderHealthSnapshot {
                authority_id: *authority_id,
                family: *family,
                success_count: record.success_count,
                failure_count: record.failure_count,
                last_observed_ms: record.last_observed_ms,
            })
            .collect::<Vec<_>>();
        provider_health.sort_by_key(|entry| (family_sort_key(entry.family), entry.authority_id));
        let mut selection_state = state
            .selection_state
            .iter()
            .filter(|((candidate_scope, _), _)| {
                scope.map_or(true, |value| *candidate_scope == value)
            })
            .map(|(_, value)| value.clone())
            .collect::<Vec<_>>();
        selection_state.sort_by_key(|entry| {
            (
                family_sort_key(entry.family),
                entry.selected_authorities.clone(),
            )
        });
        let mut hold_observations = state
            .hold_observations
            .iter()
            .filter(|((candidate_scope, _), _)| {
                scope.map_or(true, |value| *candidate_scope == value)
            })
            .map(|(_, value)| value.clone())
            .collect::<Vec<_>>();
        hold_observations.sort_by_key(|entry| (entry.scope, entry.authority_id));
        let mut pending_routes = state
            .pending_routes
            .iter()
            .filter(|((candidate_scope, _), _)| {
                scope.map_or(true, |value| *candidate_scope == value)
            })
            .map(|(_, value)| value.clone())
            .collect::<Vec<_>>();
        pending_routes.sort_by_key(|entry| (entry.scope, entry.authority_id));
        ServiceRegistryProjection {
            descriptors,
            provider_health,
            selection_state,
            hold_observations,
            pending_routes,
        }
    }

    pub async fn list_cached_peers(
        &self,
        owner: AuthorityId,
        scope: Option<ContextId>,
    ) -> Vec<AuthorityId> {
        let mut peers = self
            .state
            .read()
            .await
            .descriptors
            .values()
            .filter(|descriptor| descriptor.authority_id != owner)
            .filter(|descriptor| scope.map_or(true, |value| descriptor.context_id == value))
            .map(|descriptor| descriptor.authority_id)
            .collect::<Vec<_>>();
        peers.sort();
        peers.dedup();
        peers
    }

    pub async fn list_cached_peer_devices(
        &self,
        owner: AuthorityId,
        scope: Option<ContextId>,
    ) -> Vec<DeviceId> {
        let mut devices = self
            .state
            .read()
            .await
            .descriptors
            .values()
            .filter(|descriptor| descriptor.authority_id != owner)
            .filter(|descriptor| scope.map_or(true, |value| descriptor.context_id == value))
            .filter_map(|descriptor| descriptor.device_id)
            .collect::<Vec<_>>();
        devices.sort();
        devices.dedup();
        devices
    }

    pub async fn list_cached_devices_for_authority(
        &self,
        authority_id: AuthorityId,
        scope: Option<ContextId>,
    ) -> Vec<DeviceId> {
        let mut devices = self
            .state
            .read()
            .await
            .descriptors
            .values()
            .filter(|descriptor| descriptor.authority_id == authority_id)
            .filter(|descriptor| scope.map_or(true, |value| descriptor.context_id == value))
            .filter_map(|descriptor| descriptor.device_id)
            .collect::<Vec<_>>();
        devices.sort();
        devices.dedup();
        devices
    }
}

pub type ServiceRegistry = ServiceRegistryService;

#[cfg(test)]
mod tests {
    use super::*;
    use aura_rendezvous::TransportHint;

    fn authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn context(seed: u8) -> ContextId {
        ContextId::new_from_entropy([seed; 32])
    }

    fn descriptor(
        authority_id: AuthorityId,
        context_id: ContextId,
        valid_until: u64,
    ) -> RendezvousDescriptor {
        RendezvousDescriptor {
            authority_id,
            device_id: None,
            context_id,
            transport_hints: vec![TransportHint::tcp_direct("127.0.0.1:8080").expect("hint")],
            handshake_psk_commitment: [1u8; 32],
            public_key: [2u8; 32],
            valid_from: 0,
            valid_until,
            nonce: [3u8; 32],
            nickname_suggestion: None,
        }
    }

    #[tokio::test]
    async fn descriptor_cleanup_drops_expired_entries() {
        let registry = ServiceRegistryService::new();
        let ctx = context(1);
        let alive = authority(2);
        let expired = authority(3);
        registry.cache_descriptor(descriptor(alive, ctx, 500)).await;
        registry
            .cache_descriptor(descriptor(expired, ctx, 100))
            .await;

        let removed = registry.cleanup_expired_descriptors(200).await;
        assert_eq!(removed, 1);
        assert!(registry.get_descriptor(ctx, alive).await.is_some());
        assert!(registry.get_descriptor(ctx, expired).await.is_none());
    }

    #[tokio::test]
    async fn epoch_invalidation_prunes_scope_local_state() {
        let registry = ServiceRegistryService::new();
        let ctx = context(1);
        let peer = authority(2);
        registry
            .record_selection_state(
                ctx,
                SelectionState {
                    family: ServiceFamily::Move,
                    selected_authorities: vec![peer],
                    epoch: Some(10),
                    bounded_residency_remaining: Some(2),
                },
            )
            .await;
        registry
            .record_hold_observation(ctx, peer, 55, Some(100))
            .await;
        registry
            .track_pending_route(ctx, peer, ServiceFamily::Move, None, 40)
            .await;

        registry.invalidate_scope_epoch(ctx, 11).await;

        assert!(registry
            .selection_state(ctx, ServiceFamily::Move)
            .await
            .is_none());
        assert!(registry.hold_observations(ctx).await.is_empty());
        assert!(registry
            .projection(Some(ctx), 0)
            .await
            .pending_routes
            .is_empty());
    }
}
