//! Runtime-owned adaptive selection manager.
#![allow(dead_code)] // Cleanup target (2026-07): remove after adaptive selection migration finishes and unused staging helpers disappear.

use super::config_profiles::impl_service_config_profiles;
use super::local_health_observer::LocalHealthObserverService;
use super::service_registry::{ProviderHealthSnapshot, ServiceRegistry};
use super::traits::{RuntimeService, RuntimeServiceContext, ServiceError, ServiceHealth};
use async_trait::async_trait;
use aura_core::effects::RandomEffects;
use aura_core::service::{
    BootstrapContactHint, BootstrapIntroductionHint, LinkEndpoint, LocalEstablishDecision,
    LocalHealthSnapshot, LocalHoldDecision, LocalMoveDecision, LocalRoutingProfile,
    LocalSelectionProfile, MessageClassRoutingConstraint, MovePath, MovePathBinding,
    NeighborhoodReentryHint, PrivacyMessageClass, ProviderCandidate, ProviderEvidence, Route,
    SchedulerClass, SecurityControlClass, SelectionState, ServiceFamily, ServiceProfile,
};
use aura_core::types::identifiers::{AuthorityId, ContextId, DeviceId};
use aura_rendezvous::{
    validate_bootstrap_contact_hint, validate_bootstrap_introduction_hint,
    validate_neighborhood_reentry_hint,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;
use tokio::sync::RwLock;

const CANONICAL_COVER_PACKET_SIZE_BYTES: u64 = 1024;

#[allow(dead_code)] // Cleanup target (2026-07): drop after actor ingress is exercised outside local tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectionManagerCommand {
    SelectProfile,
    RecordProfile,
}

#[derive(Debug, Clone)]
pub struct SelectionManagerConfig {
    pub default_path_ttl_ms: u64,
    pub profile_change_min_interval_ms: u64,
    pub residency_turns: u32,
    pub security_control_floor: u32,
    pub privacy_mode_enabled: bool,
    pub min_mixing_depth: u8,
    pub max_mixing_depth: u8,
    pub path_diversity_floor: u8,
    pub cover_floor_per_second: u32,
    pub cover_gain_per_rtt_bucket: u32,
    pub delay_gain_numerator: u32,
    pub delay_gain_denominator: u32,
    pub delay_hysteresis_ms: u64,
    pub cover_hysteresis_per_second: u32,
    pub diversity_hysteresis: u8,
    pub tuning_enabled: bool,
}

impl Default for SelectionManagerConfig {
    fn default() -> Self {
        Self {
            default_path_ttl_ms: 30_000,
            profile_change_min_interval_ms: 1_000,
            residency_turns: 2,
            security_control_floor: 2,
            privacy_mode_enabled: false,
            min_mixing_depth: 1,
            max_mixing_depth: 3,
            // Phase-6 tuning keeps small/medium profiles off the degenerate
            // one-hop floor while still allowing larger reachable sets to
            // climb to the configured mixing-depth ceiling.
            path_diversity_floor: 2,
            // The fixed privacy policy keeps a non-zero baseline even when
            // organic traffic and sync windows are sparse.
            cover_floor_per_second: 2,
            cover_gain_per_rtt_bucket: 1,
            delay_gain_numerator: 1,
            // Phase-6 tuning reduced default delay growth after evidence from
            // partition-heal and ceremony-latency validation profiles.
            delay_gain_denominator: 3,
            delay_hysteresis_ms: 25,
            cover_hysteresis_per_second: 1,
            diversity_hysteresis: 1,
            tuning_enabled: false,
        }
    }
}

impl_service_config_profiles!(SelectionManagerConfig {
    pub fn for_testing() -> Self {
        Self {
            default_path_ttl_ms: 1_000,
            profile_change_min_interval_ms: 0,
            residency_turns: 1,
            security_control_floor: 1,
            privacy_mode_enabled: true,
            min_mixing_depth: 1,
            max_mixing_depth: 3,
            path_diversity_floor: 2,
            cover_floor_per_second: 1,
            cover_gain_per_rtt_bucket: 1,
            delay_gain_numerator: 1,
            delay_gain_denominator: 2,
            delay_hysteresis_ms: 5,
            cover_hysteresis_per_second: 1,
            diversity_hysteresis: 1,
            tuning_enabled: true,
        }
    }
});

#[derive(Debug)]
struct SelectionManagerState {
    last_profiles: HashMap<ContextId, LocalSelectionProfile>,
    residency: HashMap<(ContextId, AuthorityId), u32>,
    bootstrap: HashMap<ContextId, BootstrapScopeState>,
    lifecycle: ServiceHealth,
}

impl Default for SelectionManagerState {
    fn default() -> Self {
        Self {
            last_profiles: HashMap::new(),
            residency: HashMap::new(),
            bootstrap: HashMap::new(),
            lifecycle: ServiceHealth::NotStarted,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SelectionManagerError {
    #[error("no reachable candidates available for family {family:?}")]
    NoReachableCandidates { family: ServiceFamily },
    #[error("privacy mode requires at least one anonymous relay candidate")]
    NoAnonymousRelayCandidates,
    #[error("invalid bootstrap record ({kind}): {reason}")]
    InvalidBootstrapRecord { kind: &'static str, reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BootstrapHintSource {
    RememberedDirectContact,
    PriorProvider,
    NeighborhoodDiscoveryBoard,
    WebOfTrustIntroduction,
    BootstrapBridge,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapHint {
    pub authority_id: AuthorityId,
    pub device_id: Option<DeviceId>,
    pub link_endpoints: Vec<LinkEndpoint>,
    pub route_layer_public_key: Option<[u8; 32]>,
    pub source: BootstrapHintSource,
    pub observed_at_ms: u64,
    pub reliability_bps: u16,
    pub breadth_hint: u8,
    pub bridge_cluster: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BootstrapReentryStage {
    RememberedContacts,
    NeighborhoodBoards,
    WebOfTrustIntroductions,
    BootstrapBridges,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapReentryDecision {
    pub stage: BootstrapReentryStage,
    pub candidate_authorities: Vec<AuthorityId>,
}

#[derive(Debug, Clone, Default)]
struct BootstrapAttemptState {
    attempts: u32,
    last_attempt_at_ms: Option<u64>,
    last_success_at_ms: Option<u64>,
}

#[derive(Debug, Clone)]
struct BootstrapCandidateState {
    authority_id: AuthorityId,
    device_id: Option<DeviceId>,
    link_endpoints: Vec<LinkEndpoint>,
    route_layer_public_key: Option<[u8; 32]>,
    sources: BTreeSet<BootstrapHintSource>,
    freshest_observed_at_ms: u64,
    reliability_bps: u16,
    breadth_hint: u8,
    bridge_cluster: Option<u64>,
}

#[derive(Debug, Clone, Default)]
struct BootstrapScopeState {
    candidates: HashMap<AuthorityId, BootstrapCandidateState>,
    attempts: BTreeMap<BootstrapReentryStage, BootstrapAttemptState>,
}

#[aura_macros::service_surface(
    families = "Establish,Move,Hold",
    object_categories = "runtime_derived_local,transport_protocol",
    discover = "social_manager_and_service_registry",
    permit = "runtime_local_policy_and_health",
    transfer = "selection_manager",
    select = "selection_manager",
    authoritative = "",
    runtime_local = "selection_profiles,residency_windows,weighting_state",
    category = "service_surface"
)]
#[aura_macros::actor_owned(
    owner = "selection_manager",
    domain = "adaptive_privacy_selection",
    gate = "selection_manager_command_ingress",
    command = SelectionManagerCommand,
    capacity = 64,
    category = "actor_owned"
)]
pub struct SelectionManagerService {
    config: SelectionManagerConfig,
    registry: Arc<ServiceRegistry>,
    health: LocalHealthObserverService,
    state: RwLock<SelectionManagerState>,
}

impl SelectionManagerService {
    pub fn new(
        config: SelectionManagerConfig,
        registry: Arc<ServiceRegistry>,
        health: LocalHealthObserverService,
    ) -> Self {
        Self {
            config: sanitize_config_for_build(config),
            registry,
            health,
            state: RwLock::new(SelectionManagerState::default()),
        }
    }

    pub async fn select_profile<E: RandomEffects + ?Sized>(
        &self,
        scope: ContextId,
        destination: LinkEndpoint,
        move_candidates: &[ProviderCandidate],
        hold_candidates: &[ProviderCandidate],
        now_ms: u64,
        random: &E,
    ) -> Result<LocalSelectionProfile, SelectionManagerError> {
        if let Some(existing) = self.state.read().await.last_profiles.get(&scope).cloned() {
            if now_ms
                < existing
                    .health
                    .generated_at_ms
                    .saturating_add(self.config.profile_change_min_interval_ms)
            {
                return Ok(existing);
            }
        }
        let health = self.health.snapshot().await;
        let move_candidates = self
            .merge_bootstrap_candidates(scope, ServiceFamily::Move, move_candidates, now_ms)
            .await;
        let hold_candidates = self
            .merge_bootstrap_candidates(scope, ServiceFamily::Hold, hold_candidates, now_ms)
            .await;
        let selected_move_authorities: Vec<AuthorityId> = self
            .pick_authorities(
                scope,
                &move_candidates,
                ServiceFamily::Move,
                &health,
                now_ms,
                random,
            )
            .await?;
        let selected_hold_authorities: Vec<AuthorityId> = self
            .pick_authorities(
                scope,
                &hold_candidates,
                ServiceFamily::Hold,
                &health,
                now_ms,
                random,
            )
            .await
            .unwrap_or_default();

        let routing_profile = if self.config.privacy_mode_enabled {
            self.continuous_routing_profile(&health, selected_move_authorities.len() as u8)
        } else {
            LocalRoutingProfile::passthrough()
        };

        let establish = if self.config.privacy_mode_enabled {
            if selected_move_authorities.is_empty() {
                return Err(SelectionManagerError::NoAnonymousRelayCandidates);
            }
            let route = build_route(
                &selected_move_authorities,
                &move_candidates,
                &move_candidates_by_authority(&move_candidates),
                destination.clone(),
            );
            Some(LocalEstablishDecision {
                profile: ServiceProfile::AnonymousPathEstablish,
                route,
                retain_path_until_ms: Some(now_ms.saturating_add(self.config.default_path_ttl_ms)),
                scheduler_class: Some(SchedulerClass::BoundedDeadlineReply),
            })
        } else {
            None
        };

        let hold = if !selected_hold_authorities.is_empty() {
            Some(LocalHoldDecision {
                profile: ServiceProfile::DeferredDeliveryHold,
                selected_authorities: selected_hold_authorities.clone(),
                bounded_residency_remaining: Some(self.config.residency_turns),
            })
        } else {
            None
        };

        let synthetic_cover_gap_per_second = routing_profile
            .cover_rate_per_second
            .saturating_sub(organic_cover_packets_per_second(&health));

        let profile = LocalSelectionProfile {
            scope,
            health: health.clone(),
            establish,
            move_decision: LocalMoveDecision {
                routing_profile: apply_profile_hysteresis(
                    self.state
                        .read()
                        .await
                        .last_profiles
                        .get(&scope)
                        .map(|profile| &profile.move_decision.routing_profile),
                    routing_profile,
                    &self.config,
                ),
                binding: MovePathBinding::Direct(MovePath::direct(destination)),
                scheduler_class: SchedulerClass::SyncBlended,
                metadata_minimized: true,
            },
            hold,
            security_control_floor: self.config.security_control_floor,
            security_controls: vec![
                SecurityControlClass::AnonymousPathEstablish,
                SecurityControlClass::CapabilityTrustUpdate,
                SecurityControlClass::AccountabilityReply,
                SecurityControlClass::RetrievalCapabilityRotation,
            ],
            message_class_constraints: vec![
                MessageClassRoutingConstraint {
                    message_class: PrivacyMessageClass::Ceremony,
                    force_scheduler_class: Some(SchedulerClass::BoundedDeadlineReply),
                    max_mixing_depth: Some(1),
                    max_delay_ms: Some(0),
                },
                MessageClassRoutingConstraint {
                    message_class: PrivacyMessageClass::Consensus,
                    force_scheduler_class: Some(SchedulerClass::BoundedDeadlineReply),
                    max_mixing_depth: Some(1),
                    max_delay_ms: Some(0),
                },
                MessageClassRoutingConstraint {
                    message_class: PrivacyMessageClass::AccountabilityReply,
                    force_scheduler_class: Some(SchedulerClass::BoundedDeadlineReply),
                    max_mixing_depth: Some(1),
                    max_delay_ms: Some(self.config.delay_hysteresis_ms),
                },
            ],
            synthetic_cover_gap_per_second,
        };

        let profile = self.apply_profile_rate_limit(scope, profile, now_ms).await;

        self.registry
            .record_selection_state(
                scope,
                SelectionState {
                    family: ServiceFamily::Move,
                    selected_authorities: selected_move_authorities,
                    epoch: None,
                    bounded_residency_remaining: Some(self.config.residency_turns),
                },
            )
            .await;
        if !selected_hold_authorities.is_empty() {
            self.registry
                .record_selection_state(
                    scope,
                    SelectionState {
                        family: ServiceFamily::Hold,
                        selected_authorities: selected_hold_authorities,
                        epoch: None,
                        bounded_residency_remaining: Some(self.config.residency_turns),
                    },
                )
                .await;
        }
        self.state
            .write()
            .await
            .last_profiles
            .insert(scope, profile.clone());
        Ok(profile)
    }

    pub async fn remember_bootstrap_hints(&self, scope: ContextId, hints: &[BootstrapHint]) {
        let mut state = self.state.write().await;
        let scope_state = state.bootstrap.entry(scope).or_default();
        for hint in hints {
            let entry = scope_state
                .candidates
                .entry(hint.authority_id)
                .or_insert_with(|| BootstrapCandidateState {
                    authority_id: hint.authority_id,
                    device_id: hint.device_id,
                    link_endpoints: hint.link_endpoints.clone(),
                    route_layer_public_key: hint.route_layer_public_key,
                    sources: BTreeSet::new(),
                    freshest_observed_at_ms: hint.observed_at_ms,
                    reliability_bps: hint.reliability_bps,
                    breadth_hint: hint.breadth_hint,
                    bridge_cluster: hint.bridge_cluster,
                });
            entry.device_id = entry.device_id.or(hint.device_id);
            for endpoint in &hint.link_endpoints {
                if !entry.link_endpoints.contains(endpoint) {
                    entry.link_endpoints.push(endpoint.clone());
                }
            }
            if entry.route_layer_public_key.is_none() {
                entry.route_layer_public_key = hint.route_layer_public_key;
            }
            entry.sources.insert(hint.source);
            entry.freshest_observed_at_ms = entry.freshest_observed_at_ms.max(hint.observed_at_ms);
            entry.reliability_bps = entry.reliability_bps.max(hint.reliability_bps);
            entry.breadth_hint = entry.breadth_hint.max(hint.breadth_hint);
            entry.bridge_cluster = entry.bridge_cluster.or(hint.bridge_cluster);
        }
    }

    pub async fn remember_bootstrap_contact_records(
        &self,
        scope: ContextId,
        records: &[BootstrapContactHint],
    ) -> Result<usize, SelectionManagerError> {
        let hints = records
            .iter()
            .map(|record| {
                validate_bootstrap_contact_hint(record).map_err(|error| {
                    SelectionManagerError::InvalidBootstrapRecord {
                        kind: "bootstrap_contact_hint",
                        reason: format!("{error:?}"),
                    }
                })?;
                Ok(BootstrapHint {
                    authority_id: record.authority_id,
                    device_id: record.device_id,
                    link_endpoints: record.link_endpoints.clone(),
                    route_layer_public_key: record.route_layer_public_key,
                    source: BootstrapHintSource::RememberedDirectContact,
                    observed_at_ms: record.freshest_observed_at_ms,
                    reliability_bps: 9_000,
                    breadth_hint: 1,
                    bridge_cluster: None,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.remember_bootstrap_hints(scope, &hints).await;
        Ok(hints.len())
    }

    pub async fn remember_neighborhood_reentry_records(
        &self,
        scope: ContextId,
        records: &[NeighborhoodReentryHint],
    ) -> Result<usize, SelectionManagerError> {
        let hints = records
            .iter()
            .map(|record| {
                validate_neighborhood_reentry_hint(record).map_err(|error| {
                    SelectionManagerError::InvalidBootstrapRecord {
                        kind: "neighborhood_reentry_hint",
                        reason: format!("{error:?}"),
                    }
                })?;
                Ok(BootstrapHint {
                    authority_id: record.advertised_authority,
                    device_id: record.advertised_device,
                    link_endpoints: record.link_endpoints.clone(),
                    route_layer_public_key: record.route_layer_public_key,
                    source: BootstrapHintSource::NeighborhoodDiscoveryBoard,
                    observed_at_ms: record.published_at_ms,
                    reliability_bps: 7_000,
                    breadth_hint: 3,
                    bridge_cluster: None,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.remember_bootstrap_hints(scope, &hints).await;
        Ok(hints.len())
    }

    pub async fn remember_bootstrap_introduction_records(
        &self,
        scope: ContextId,
        records: &[BootstrapIntroductionHint],
        observed_at_ms: u64,
    ) -> Result<usize, SelectionManagerError> {
        let hints = records
            .iter()
            .map(|record| {
                validate_bootstrap_introduction_hint(record).map_err(|error| {
                    SelectionManagerError::InvalidBootstrapRecord {
                        kind: "bootstrap_introduction_hint",
                        reason: format!("{error:?}"),
                    }
                })?;
                Ok(BootstrapHint {
                    authority_id: record.introduced_authority,
                    device_id: record.introduced_device,
                    link_endpoints: record.link_endpoints.clone(),
                    route_layer_public_key: record.route_layer_public_key,
                    source: BootstrapHintSource::WebOfTrustIntroduction,
                    observed_at_ms,
                    reliability_bps: 8_000,
                    breadth_hint: record.max_fanout,
                    bridge_cluster: None,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.remember_bootstrap_hints(scope, &hints).await;
        Ok(hints.len())
    }

    pub async fn record_reentry_attempt(
        &self,
        scope: ContextId,
        stage: BootstrapReentryStage,
        now_ms: u64,
        succeeded: bool,
    ) {
        let mut state = self.state.write().await;
        let attempt = state
            .bootstrap
            .entry(scope)
            .or_default()
            .attempts
            .entry(stage)
            .or_default();
        attempt.attempts = attempt.attempts.saturating_add(1);
        attempt.last_attempt_at_ms = Some(now_ms);
        if succeeded {
            attempt.last_success_at_ms = Some(now_ms);
        }
    }

    pub async fn stale_reentry_decision(
        &self,
        scope: ContextId,
        now_ms: u64,
    ) -> Option<BootstrapReentryDecision> {
        let state = self.state.read().await;
        let scope_state = state.bootstrap.get(&scope)?;
        let stages = [
            BootstrapReentryStage::RememberedContacts,
            BootstrapReentryStage::NeighborhoodBoards,
            BootstrapReentryStage::WebOfTrustIntroductions,
            BootstrapReentryStage::BootstrapBridges,
        ];
        for stage in stages {
            let attempts = scope_state
                .attempts
                .get(&stage)
                .map(|attempt| attempt.attempts)
                .unwrap_or(0);
            if attempts >= 2 {
                continue;
            }
            let mut authorities = scope_state
                .candidates
                .values()
                .filter(|candidate| candidate_matches_reentry_stage(candidate, stage))
                .filter(|candidate| freshness_score(candidate, now_ms) > 0)
                .map(|candidate| candidate.authority_id)
                .collect::<Vec<_>>();
            authorities.sort();
            authorities.dedup();
            if !authorities.is_empty() {
                return Some(BootstrapReentryDecision {
                    stage,
                    candidate_authorities: authorities,
                });
            }
        }
        None
    }

    fn continuous_routing_profile(
        &self,
        health: &LocalHealthSnapshot,
        selected_move_count: u8,
    ) -> LocalRoutingProfile {
        let diversity_target = self
            .config
            .path_diversity_floor
            .max(health.observed_route_diversity)
            .min(self.config.max_mixing_depth.max(1));
        let mixing_depth = selected_move_count
            .max(self.config.min_mixing_depth)
            .min(self.config.max_mixing_depth)
            .min(diversity_target.max(1));
        let delay_ms = div_ceil_u64(
            (health.ema_rtt_ms as u64).saturating_mul(self.config.delay_gain_numerator as u64),
            self.config.delay_gain_denominator.max(1) as u64,
        );
        let cover_rate_per_second = self
            .config
            .cover_floor_per_second
            .saturating_add(
                div_ceil_u32(health.ema_rtt_ms, 100)
                    .saturating_mul(self.config.cover_gain_per_rtt_bucket),
            )
            .saturating_add(div_ceil_u32(health.queue_pressure, 32));
        LocalRoutingProfile {
            mixing_depth,
            delay_ms,
            cover_rate_per_second,
            path_diversity: diversity_target.max(1),
        }
    }

    async fn apply_profile_rate_limit(
        &self,
        scope: ContextId,
        candidate: LocalSelectionProfile,
        now_ms: u64,
    ) -> LocalSelectionProfile {
        let state = self.state.read().await;
        let Some(existing) = state.last_profiles.get(&scope) else {
            return candidate;
        };
        if now_ms
            < existing
                .health
                .generated_at_ms
                .saturating_add(self.config.profile_change_min_interval_ms)
        {
            return existing.clone();
        }
        candidate
    }

    async fn pick_authorities<E: RandomEffects + ?Sized>(
        &self,
        scope: ContextId,
        candidates: &[ProviderCandidate],
        family: ServiceFamily,
        local_health: &LocalHealthSnapshot,
        now_ms: u64,
        random: &E,
    ) -> Result<Vec<AuthorityId>, SelectionManagerError> {
        let reachable = candidates
            .iter()
            .filter(|candidate| candidate.reachable)
            .collect::<Vec<_>>();
        if reachable.is_empty() {
            return Err(SelectionManagerError::NoReachableCandidates { family });
        }

        let projection = self.registry.projection(Some(scope), u64::MAX).await;
        let state = self.state.read().await;
        let residency = state.residency.clone();
        let bootstrap_scope = state.bootstrap.get(&scope).cloned().unwrap_or_default();
        drop(state);
        let mut weighted = reachable
            .iter()
            .map(|candidate| {
                let health = projection.provider_health.iter().find(|entry| {
                    entry.family == family && entry.authority_id == candidate.authority_id
                });
                (
                    candidate.authority_id,
                    score_candidate(
                        candidate,
                        health,
                        local_health,
                        family,
                        residency
                            .get(&(scope, candidate.authority_id))
                            .copied()
                            .unwrap_or(0),
                        bootstrap_scope.candidates.get(&candidate.authority_id),
                        now_ms,
                    ),
                )
            })
            .collect::<Vec<_>>();
        let mut selected = Vec::new();
        let max_select = if family == ServiceFamily::Hold {
            3
        } else {
            usize::from(
                self.config
                    .max_mixing_depth
                    .max(self.config.min_mixing_depth),
            )
        };
        let mut selected_evidence = Vec::new();
        for _ in 0..max_select {
            if weighted.is_empty() {
                break;
            }
            let total_weight: u32 = weighted.iter().map(|(_, weight)| *weight).sum();
            if total_weight == 0 {
                break;
            }
            let draw = random.random_range(0, total_weight as u64).await as u32;
            let mut cursor = 0u32;
            let mut chosen_index = 0usize;
            for (index, (_, weight)) in weighted.iter().enumerate() {
                cursor = cursor.saturating_add(*weight);
                if draw < cursor {
                    chosen_index = index;
                    break;
                }
            }
            let (authority, _) = weighted.remove(chosen_index);
            selected.push(authority);
            if let Some(candidate) = candidates
                .iter()
                .find(|candidate| candidate.authority_id == authority)
            {
                for evidence in &candidate.evidence {
                    if !selected_evidence.contains(evidence) {
                        selected_evidence.push(evidence.clone());
                    }
                }
            }
            for (candidate_authority, weight) in &mut weighted {
                if let Some(candidate) = candidates
                    .iter()
                    .find(|candidate| candidate.authority_id == *candidate_authority)
                {
                    let diversity_penalty = candidate
                        .evidence
                        .iter()
                        .filter(|evidence| selected_evidence.iter().any(|seen| seen == *evidence))
                        .count() as u32
                        * 10;
                    let bridge_diversity_penalty =
                        bootstrap_scope
                            .candidates
                            .get(candidate_authority)
                            .and_then(|candidate_state| candidate_state.bridge_cluster)
                            .map(|cluster| {
                                selected
                                    .iter()
                                    .filter(|selected_authority| {
                                        bootstrap_scope.candidates.get(selected_authority).and_then(
                                            |selected_state| selected_state.bridge_cluster,
                                        ) == Some(cluster)
                                    })
                                    .count() as u32
                                    * 12
                            })
                            .unwrap_or(0);
                    *weight = weight
                        .saturating_sub(diversity_penalty)
                        .saturating_sub(bridge_diversity_penalty);
                }
            }
        }
        selected.sort();
        selected.dedup();
        let mut state = self.state.write().await;
        for value in state.residency.values_mut() {
            *value = value.saturating_sub(1);
        }
        for authority in &selected {
            state
                .residency
                .insert((scope, *authority), self.config.residency_turns);
        }
        Ok(selected)
    }

    async fn merge_bootstrap_candidates(
        &self,
        scope: ContextId,
        family: ServiceFamily,
        candidates: &[ProviderCandidate],
        now_ms: u64,
    ) -> Vec<ProviderCandidate> {
        if family == ServiceFamily::Hold {
            return candidates.to_vec();
        }
        let state = self.state.read().await;
        let Some(scope_state) = state.bootstrap.get(&scope) else {
            return candidates.to_vec();
        };
        let mut merged = candidates
            .iter()
            .cloned()
            .map(|candidate| (candidate.authority_id, candidate))
            .collect::<BTreeMap<_, _>>();
        for candidate_state in scope_state.candidates.values() {
            if freshness_score(candidate_state, now_ms) == 0 {
                continue;
            }
            let entry = merged
                .entry(candidate_state.authority_id)
                .or_insert_with(|| ProviderCandidate {
                    authority_id: candidate_state.authority_id,
                    device_id: candidate_state.device_id,
                    family,
                    evidence: vec![ProviderEvidence::DescriptorFallback],
                    link_endpoints: candidate_state.link_endpoints.clone(),
                    route_layer_public_key: None,
                    reachable: !candidate_state.link_endpoints.is_empty(),
                });
            if entry.device_id.is_none() {
                entry.device_id = candidate_state.device_id;
            }
            for endpoint in &candidate_state.link_endpoints {
                if !entry.link_endpoints.contains(endpoint) {
                    entry.link_endpoints.push(endpoint.clone());
                }
            }
            if !entry.reachable && !candidate_state.link_endpoints.is_empty() {
                entry.reachable = true;
            }
            if entry.route_layer_public_key.is_none() {
                entry.route_layer_public_key = candidate_state.route_layer_public_key;
            }
        }
        merged.into_values().collect()
    }
}

fn div_ceil_u32(value: u32, divisor: u32) -> u32 {
    if value == 0 {
        return 0;
    }
    (value.saturating_add(divisor.saturating_sub(1))) / divisor.max(1)
}

fn div_ceil_u64(value: u64, divisor: u64) -> u64 {
    if value == 0 {
        return 0;
    }
    (value.saturating_add(divisor.saturating_sub(1))) / divisor.max(1)
}

fn sanitize_config_for_build(config: SelectionManagerConfig) -> SelectionManagerConfig {
    #[cfg(not(any(test, debug_assertions)))]
    {
        let mut config = config;
        let fixed = SelectionManagerConfig::default();
        if config.tuning_enabled {
            config.min_mixing_depth = fixed.min_mixing_depth;
            config.max_mixing_depth = fixed.max_mixing_depth;
            config.path_diversity_floor = fixed.path_diversity_floor;
            config.cover_floor_per_second = fixed.cover_floor_per_second;
            config.cover_gain_per_rtt_bucket = fixed.cover_gain_per_rtt_bucket;
            config.delay_gain_numerator = fixed.delay_gain_numerator;
            config.delay_gain_denominator = fixed.delay_gain_denominator;
            config.delay_hysteresis_ms = fixed.delay_hysteresis_ms;
            config.cover_hysteresis_per_second = fixed.cover_hysteresis_per_second;
            config.diversity_hysteresis = fixed.diversity_hysteresis;
            config.tuning_enabled = false;
        }
        config
    }

    #[cfg(any(test, debug_assertions))]
    {
        config
    }
}

fn move_candidates_by_authority(
    candidates: &[ProviderCandidate],
) -> BTreeMap<AuthorityId, Vec<LinkEndpoint>> {
    let mut map = BTreeMap::new();
    for candidate in candidates {
        map.entry(candidate.authority_id)
            .or_insert_with(Vec::new)
            .extend(candidate.link_endpoints.clone());
    }
    map
}

fn build_route(
    authorities: &[AuthorityId],
    candidates: &[ProviderCandidate],
    endpoints: &BTreeMap<AuthorityId, Vec<LinkEndpoint>>,
    destination: LinkEndpoint,
) -> Route {
    let route_keys = candidates
        .iter()
        .filter_map(|candidate| {
            candidate
                .route_layer_public_key
                .map(|public_key| (candidate.authority_id, public_key))
        })
        .collect::<BTreeMap<_, _>>();
    Route {
        hops: authorities
            .iter()
            .map(|authority_id| aura_core::service::RelayHop {
                authority_id: *authority_id,
                link_endpoint: endpoints
                    .get(authority_id)
                    .and_then(|values| values.first().cloned())
                    .unwrap_or_else(|| LinkEndpoint::relay(*authority_id)),
                route_layer_public_key: route_keys.get(authority_id).copied(),
            })
            .collect(),
        destination,
    }
}

fn score_candidate(
    candidate: &ProviderCandidate,
    health: Option<&ProviderHealthSnapshot>,
    local_health: &LocalHealthSnapshot,
    family: ServiceFamily,
    residency_penalty_turns: u32,
    bootstrap: Option<&BootstrapCandidateState>,
    now_ms: u64,
) -> u32 {
    let evidence_score = candidate
        .evidence
        .iter()
        .map(|evidence| match evidence {
            ProviderEvidence::Neighborhood => 15,
            ProviderEvidence::DirectFriend => 30,
            ProviderEvidence::IntroducedFof => 20,
            ProviderEvidence::Guardian => 25,
            ProviderEvidence::DescriptorFallback => 5,
        })
        .sum::<u32>();
    let health_score = health
        .map(|entry| {
            let total = entry
                .success_count
                .saturating_add(entry.failure_count)
                .max(1);
            (entry.success_count.saturating_mul(100)) / total
        })
        .unwrap_or(50);
    let hold_quality = if family == ServiceFamily::Hold {
        local_health.hold_success_bps / 100
    } else {
        0
    };
    let bootstrap_freshness = bootstrap
        .map(|candidate| freshness_score(candidate, now_ms))
        .unwrap_or(0);
    let bootstrap_source = bootstrap.map(source_score).unwrap_or(0);
    let bootstrap_breadth = bootstrap
        .map(|candidate| u32::from(candidate.breadth_hint).saturating_mul(4))
        .unwrap_or(0);
    let availability_score = local_health
        .sync_opportunity_count
        .min(10)
        .saturating_mul(3);
    let diversity_score = local_health.observed_route_diversity as u32 * 5;
    let reachability_score: u32 = if candidate.reachable { 40 } else { 0 };
    reachability_score
        .saturating_add(evidence_score)
        .saturating_add(health_score)
        .saturating_add(hold_quality)
        .saturating_add(bootstrap_freshness)
        .saturating_add(bootstrap_source)
        .saturating_add(bootstrap_breadth)
        .saturating_add(availability_score)
        .saturating_add(diversity_score)
        .saturating_sub(residency_penalty_turns.saturating_mul(20))
}

fn freshness_score(candidate: &BootstrapCandidateState, now_ms: u64) -> u32 {
    let age_ms = now_ms.saturating_sub(candidate.freshest_observed_at_ms);
    let age_bucket: u32 = match age_ms {
        0..=5_000 => 30,
        5_001..=30_000 => 22,
        30_001..=120_000 => 14,
        120_001..=600_000 => 6,
        _ => 0,
    };
    let reliability = u32::from(candidate.reliability_bps) / 250;
    age_bucket.saturating_add(reliability)
}

fn source_score(candidate: &BootstrapCandidateState) -> u32 {
    candidate
        .sources
        .iter()
        .map(|source| match source {
            BootstrapHintSource::RememberedDirectContact => 24,
            BootstrapHintSource::PriorProvider => 18,
            BootstrapHintSource::NeighborhoodDiscoveryBoard => 12,
            BootstrapHintSource::WebOfTrustIntroduction => 16,
            BootstrapHintSource::BootstrapBridge => 8,
        })
        .max()
        .unwrap_or(0)
}

fn candidate_matches_reentry_stage(
    candidate: &BootstrapCandidateState,
    stage: BootstrapReentryStage,
) -> bool {
    match stage {
        BootstrapReentryStage::RememberedContacts => {
            candidate
                .sources
                .contains(&BootstrapHintSource::RememberedDirectContact)
                || candidate
                    .sources
                    .contains(&BootstrapHintSource::PriorProvider)
        }
        BootstrapReentryStage::NeighborhoodBoards => candidate
            .sources
            .contains(&BootstrapHintSource::NeighborhoodDiscoveryBoard),
        BootstrapReentryStage::WebOfTrustIntroductions => candidate
            .sources
            .contains(&BootstrapHintSource::WebOfTrustIntroduction),
        BootstrapReentryStage::BootstrapBridges => candidate
            .sources
            .contains(&BootstrapHintSource::BootstrapBridge),
    }
}

fn apply_profile_hysteresis(
    previous: Option<&LocalRoutingProfile>,
    mut candidate: LocalRoutingProfile,
    config: &SelectionManagerConfig,
) -> LocalRoutingProfile {
    let Some(previous) = previous else {
        return candidate;
    };
    if previous.mixing_depth.abs_diff(candidate.mixing_depth) <= 1 {
        candidate.mixing_depth = previous.mixing_depth;
    }
    if previous.delay_ms.abs_diff(candidate.delay_ms) <= config.delay_hysteresis_ms {
        candidate.delay_ms = previous.delay_ms;
    }
    if previous
        .cover_rate_per_second
        .abs_diff(candidate.cover_rate_per_second)
        <= config.cover_hysteresis_per_second
    {
        candidate.cover_rate_per_second = previous.cover_rate_per_second;
    }
    if previous.path_diversity.abs_diff(candidate.path_diversity) <= config.diversity_hysteresis {
        candidate.path_diversity = previous.path_diversity;
    }
    candidate
}

fn organic_cover_packets_per_second(health: &LocalHealthSnapshot) -> u32 {
    let organic_bytes = health
        .traffic_volume_bytes
        .saturating_add(health.sync_blended_retrieval_bytes);
    organic_bytes
        .saturating_add(CANONICAL_COVER_PACKET_SIZE_BYTES.saturating_sub(1))
        .checked_div(CANONICAL_COVER_PACKET_SIZE_BYTES)
        .unwrap_or(0)
        .min(u32::MAX as u64) as u32
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl RuntimeService for SelectionManagerService {
    fn name(&self) -> &'static str {
        "selection_manager"
    }

    fn dependencies(&self) -> &[&'static str] {
        &[
            "social_manager",
            "rendezvous_manager",
            "local_health_observer",
        ]
    }

    async fn start(&self, _ctx: &RuntimeServiceContext) -> Result<(), ServiceError> {
        self.state.write().await.lifecycle = ServiceHealth::Healthy;
        Ok(())
    }

    async fn stop(&self) -> Result<(), ServiceError> {
        self.state.write().await.lifecycle = ServiceHealth::Stopped;
        Ok(())
    }

    async fn health(&self) -> ServiceHealth {
        self.state.read().await.lifecycle.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::services::LocalHealthObserverConfig;
    use aura_core::effects::RandomCoreEffects;
    use aura_core::service::{LinkProtocol, ProviderEvidence};
    use aura_core::types::identifiers::DeviceId;

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

    fn candidate(seed: u8, reachable: bool, evidence: ProviderEvidence) -> ProviderCandidate {
        ProviderCandidate {
            authority_id: authority(seed),
            device_id: Some(DeviceId::new_from_entropy([seed; 32])),
            family: ServiceFamily::Move,
            evidence: vec![evidence],
            link_endpoints: vec![LinkEndpoint::direct(
                LinkProtocol::Tcp,
                format!("127.0.0.1:{}", 7000 + seed as u16),
            )],
            route_layer_public_key: Some([seed; 32]),
            reachable,
        }
    }

    fn bootstrap_hint(
        seed: u8,
        source: BootstrapHintSource,
        observed_at_ms: u64,
        reliability_bps: u16,
        breadth_hint: u8,
        bridge_cluster: Option<u64>,
    ) -> BootstrapHint {
        BootstrapHint {
            authority_id: authority(seed),
            device_id: Some(DeviceId::new_from_entropy([seed; 32])),
            link_endpoints: vec![LinkEndpoint::direct(
                LinkProtocol::Tcp,
                format!("127.0.0.1:{}", 7600 + seed as u16),
            )],
            route_layer_public_key: Some([seed; 32]),
            source,
            observed_at_ms,
            reliability_bps,
            breadth_hint,
            bridge_cluster,
        }
    }

    #[tokio::test]
    async fn selection_manager_prefers_stronger_candidates_without_total_concentration() {
        let registry = Arc::new(ServiceRegistry::new());
        let health = LocalHealthObserverService::new(LocalHealthObserverConfig::for_testing());
        health
            .observe_provider_set(
                &[
                    candidate(1, true, ProviderEvidence::DirectFriend),
                    candidate(2, true, ProviderEvidence::Neighborhood),
                    candidate(3, true, ProviderEvidence::Neighborhood),
                ],
                2,
                10,
            )
            .await;
        let manager =
            SelectionManagerService::new(SelectionManagerConfig::for_testing(), registry, health);
        let profile = manager
            .select_profile(
                context(1),
                LinkEndpoint::direct(LinkProtocol::Tcp, "127.0.0.1:9000"),
                &[
                    candidate(1, true, ProviderEvidence::DirectFriend),
                    candidate(2, true, ProviderEvidence::Neighborhood),
                    candidate(3, true, ProviderEvidence::Neighborhood),
                ],
                &[],
                20,
                &TestRandom(3),
            )
            .await
            .expect("select profile");
        let establish = profile.establish.expect("privacy mode establish");
        assert_eq!(establish.profile, ServiceProfile::AnonymousPathEstablish);
        assert!(!establish.route.hops.is_empty());
        assert!(establish
            .route
            .hops
            .iter()
            .any(|hop| hop.authority_id == authority(1)));
        assert!(establish
            .route
            .hops
            .iter()
            .any(|hop| hop.authority_id != authority(1)));
    }

    #[tokio::test]
    async fn selection_manager_keeps_passthrough_available() {
        let registry = Arc::new(ServiceRegistry::new());
        let health = LocalHealthObserverService::new(LocalHealthObserverConfig::for_testing());
        let mut config = SelectionManagerConfig::for_testing();
        config.privacy_mode_enabled = false;
        let manager = SelectionManagerService::new(config, registry, health);
        let profile = manager
            .select_profile(
                context(2),
                LinkEndpoint::direct(LinkProtocol::Tcp, "127.0.0.1:9100"),
                &[candidate(1, true, ProviderEvidence::Neighborhood)],
                &[],
                30,
                &TestRandom(1),
            )
            .await
            .expect("select passthrough");
        assert!(profile.establish.is_none());
        assert_eq!(
            profile.move_decision.routing_profile,
            LocalRoutingProfile::passthrough()
        );
    }

    #[tokio::test]
    async fn selection_manager_records_message_class_constraints_and_real_traffic_gap() {
        let registry = Arc::new(ServiceRegistry::new());
        let health = LocalHealthObserverService::new(LocalHealthObserverConfig::for_testing());
        health.observe_traffic_volume(1024, 10).await;
        health.observe_sync_blended_retrieval_volume(1024, 11).await;
        health.observe_accountability_reply_volume(2048, 12).await;
        health
            .observe_provider_set(&[candidate(1, true, ProviderEvidence::Neighborhood)], 2, 13)
            .await;
        let manager =
            SelectionManagerService::new(SelectionManagerConfig::for_testing(), registry, health);
        let profile = manager
            .select_profile(
                context(3),
                LinkEndpoint::direct(LinkProtocol::Tcp, "127.0.0.1:9200"),
                &[candidate(1, true, ProviderEvidence::Neighborhood)],
                &[],
                20,
                &TestRandom(2),
            )
            .await
            .expect("select profile");
        assert!(profile
            .message_class_constraints
            .iter()
            .any(|constraint| constraint.message_class == PrivacyMessageClass::Consensus));
        assert_eq!(
            profile.move_decision.routing_profile.cover_rate_per_second,
            1
        );
        assert_eq!(profile.synthetic_cover_gap_per_second, 0);
        assert_eq!(profile.health.accountability_reply_bytes, 2048);
    }

    #[tokio::test]
    async fn selection_manager_rotates_paths_under_residency_pressure() {
        let registry = Arc::new(ServiceRegistry::new());
        let health = LocalHealthObserverService::new(LocalHealthObserverConfig::for_testing());
        let manager =
            SelectionManagerService::new(SelectionManagerConfig::for_testing(), registry, health);
        let candidates = [
            candidate(1, true, ProviderEvidence::DirectFriend),
            candidate(2, true, ProviderEvidence::Neighborhood),
            candidate(3, true, ProviderEvidence::IntroducedFof),
            candidate(4, true, ProviderEvidence::Neighborhood),
        ];
        let first = manager
            .select_profile(
                context(4),
                LinkEndpoint::direct(LinkProtocol::Tcp, "127.0.0.1:9300"),
                &candidates,
                &[],
                10,
                &TestRandom(1),
            )
            .await
            .expect("first profile");
        let second = manager
            .select_profile(
                context(4),
                LinkEndpoint::direct(LinkProtocol::Tcp, "127.0.0.1:9300"),
                &candidates,
                &[],
                20,
                &TestRandom(99),
            )
            .await
            .expect("second profile");
        let first_route = first.establish.expect("first establish").route;
        let second_route = second.establish.expect("second establish").route;
        assert_ne!(first_route.hops, second_route.hops);
        assert!(first_route
            .hops
            .iter()
            .any(|hop| hop.authority_id == authority(1)));
        assert!(second_route
            .hops
            .iter()
            .any(|hop| hop.authority_id != authority(1)));
    }

    #[tokio::test]
    async fn selection_manager_consumes_bootstrap_hints_when_shared_candidates_are_empty() {
        let registry = Arc::new(ServiceRegistry::new());
        let health = LocalHealthObserverService::new(LocalHealthObserverConfig::for_testing());
        health.observe_provider_set(&[], 1, 50).await;
        let manager =
            SelectionManagerService::new(SelectionManagerConfig::for_testing(), registry, health);
        manager
            .remember_bootstrap_hints(
                context(5),
                &[
                    bootstrap_hint(
                        8,
                        BootstrapHintSource::RememberedDirectContact,
                        45,
                        9_000,
                        2,
                        None,
                    ),
                    bootstrap_hint(
                        9,
                        BootstrapHintSource::BootstrapBridge,
                        48,
                        6_000,
                        4,
                        Some(1),
                    ),
                ],
            )
            .await;

        let profile = manager
            .select_profile(
                context(5),
                LinkEndpoint::direct(LinkProtocol::Tcp, "127.0.0.1:9400"),
                &[],
                &[],
                50,
                &TestRandom(1),
            )
            .await
            .expect("bootstrap hints should provide move candidates");
        let establish = profile.establish.expect("establish decision");
        assert!(establish
            .route
            .hops
            .iter()
            .any(|hop| hop.authority_id == authority(8)));
    }

    #[tokio::test]
    async fn stale_reentry_decisions_progress_from_contacts_to_bridges() {
        let registry = Arc::new(ServiceRegistry::new());
        let health = LocalHealthObserverService::new(LocalHealthObserverConfig::for_testing());
        let manager =
            SelectionManagerService::new(SelectionManagerConfig::for_testing(), registry, health);
        manager
            .remember_bootstrap_hints(
                context(6),
                &[
                    bootstrap_hint(
                        1,
                        BootstrapHintSource::RememberedDirectContact,
                        100,
                        9_000,
                        1,
                        None,
                    ),
                    bootstrap_hint(
                        2,
                        BootstrapHintSource::NeighborhoodDiscoveryBoard,
                        100,
                        7_000,
                        3,
                        None,
                    ),
                    bootstrap_hint(
                        3,
                        BootstrapHintSource::WebOfTrustIntroduction,
                        100,
                        8_000,
                        2,
                        None,
                    ),
                    bootstrap_hint(
                        4,
                        BootstrapHintSource::BootstrapBridge,
                        100,
                        5_000,
                        4,
                        Some(7),
                    ),
                ],
            )
            .await;

        let first = manager
            .stale_reentry_decision(context(6), 120)
            .await
            .expect("contacts-first decision");
        assert_eq!(first.stage, BootstrapReentryStage::RememberedContacts);
        manager
            .record_reentry_attempt(context(6), first.stage, 121, false)
            .await;
        manager
            .record_reentry_attempt(context(6), first.stage, 122, false)
            .await;

        let second = manager
            .stale_reentry_decision(context(6), 123)
            .await
            .expect("board decision");
        assert_eq!(second.stage, BootstrapReentryStage::NeighborhoodBoards);
        manager
            .record_reentry_attempt(context(6), second.stage, 124, false)
            .await;
        manager
            .record_reentry_attempt(context(6), second.stage, 125, false)
            .await;

        let third = manager
            .stale_reentry_decision(context(6), 126)
            .await
            .expect("introduction decision");
        assert_eq!(third.stage, BootstrapReentryStage::WebOfTrustIntroductions);
        manager
            .record_reentry_attempt(context(6), third.stage, 127, false)
            .await;
        manager
            .record_reentry_attempt(context(6), third.stage, 128, false)
            .await;

        let fourth = manager
            .stale_reentry_decision(context(6), 129)
            .await
            .expect("bridge decision");
        assert_eq!(fourth.stage, BootstrapReentryStage::BootstrapBridges);
    }

    #[tokio::test]
    async fn bootstrap_weighting_prefers_fresher_hints_without_collapsing_to_one_bridge_cluster() {
        let registry = Arc::new(ServiceRegistry::new());
        let health = LocalHealthObserverService::new(LocalHealthObserverConfig::for_testing());
        health.observe_provider_set(&[], 2, 200).await;
        let manager =
            SelectionManagerService::new(SelectionManagerConfig::for_testing(), registry, health);
        manager
            .remember_bootstrap_hints(
                context(7),
                &[
                    bootstrap_hint(
                        10,
                        BootstrapHintSource::NeighborhoodDiscoveryBoard,
                        195,
                        8_500,
                        3,
                        Some(1),
                    ),
                    bootstrap_hint(
                        11,
                        BootstrapHintSource::NeighborhoodDiscoveryBoard,
                        194,
                        8_000,
                        3,
                        Some(2),
                    ),
                    bootstrap_hint(
                        12,
                        BootstrapHintSource::BootstrapBridge,
                        130,
                        5_500,
                        4,
                        Some(1),
                    ),
                    bootstrap_hint(
                        13,
                        BootstrapHintSource::BootstrapBridge,
                        192,
                        7_500,
                        4,
                        Some(3),
                    ),
                ],
            )
            .await;

        let profile = manager
            .select_profile(
                context(7),
                LinkEndpoint::direct(LinkProtocol::Tcp, "127.0.0.1:9500"),
                &[],
                &[],
                200,
                &TestRandom(42),
            )
            .await
            .expect("bootstrap weighted profile");
        let route = profile.establish.expect("establish").route;
        let unique_hops = route
            .hops
            .iter()
            .map(|hop| hop.authority_id)
            .collect::<std::collections::BTreeSet<_>>();
        assert!(unique_hops.len() > 1);
        assert!(route
            .hops
            .iter()
            .any(|hop| hop.authority_id == authority(10)));
        assert!(route
            .hops
            .iter()
            .any(|hop| hop.authority_id != authority(12)));
    }

    #[tokio::test]
    async fn selection_manager_merges_validated_shared_bootstrap_records_locally() {
        let registry = Arc::new(ServiceRegistry::new());
        let health = LocalHealthObserverService::new(LocalHealthObserverConfig::for_testing());
        health.observe_provider_set(&[], 3, 300).await;
        let manager =
            SelectionManagerService::new(SelectionManagerConfig::for_testing(), registry, health);

        manager
            .remember_bootstrap_contact_records(
                context(8),
                &[BootstrapContactHint {
                    scope: context(8),
                    authority_id: authority(20),
                    device_id: Some(DeviceId::new_from_entropy([20; 32])),
                    link_endpoints: vec![LinkEndpoint::direct(LinkProtocol::Tcp, "127.0.0.1:7620")],
                    route_layer_public_key: Some([20; 32]),
                    freshest_observed_at_ms: 290,
                    valid_until: 390,
                    replay_window_id: [20; 32],
                }],
            )
            .await
            .expect("contact hint should merge");
        manager
            .remember_neighborhood_reentry_records(
                context(8),
                &[NeighborhoodReentryHint {
                    scope: context(8),
                    publisher_authority: authority(21),
                    advertised_authority: authority(21),
                    advertised_device: Some(DeviceId::new_from_entropy([21; 32])),
                    link_endpoints: vec![LinkEndpoint::direct(LinkProtocol::Tcp, "127.0.0.1:7621")],
                    route_layer_public_key: Some([21; 32]),
                    published_at_ms: 291,
                    valid_until: 391,
                    replay_window_id: [21; 32],
                }],
            )
            .await
            .expect("reentry hint should merge");
        manager
            .remember_bootstrap_introduction_records(
                context(8),
                &[BootstrapIntroductionHint {
                    scope: context(8),
                    introducer_authority: authority(22),
                    introduced_authority: authority(22),
                    introduced_device: Some(DeviceId::new_from_entropy([22; 32])),
                    link_endpoints: vec![LinkEndpoint::direct(LinkProtocol::Tcp, "127.0.0.1:7622")],
                    route_layer_public_key: Some([22; 32]),
                    remaining_depth: 1,
                    max_fanout: 2,
                    valid_until: 392,
                    replay_window_id: [22; 32],
                }],
                295,
            )
            .await
            .expect("introduction hint should merge");

        let profile = manager
            .select_profile(
                context(8),
                LinkEndpoint::direct(LinkProtocol::Tcp, "127.0.0.1:9600"),
                &[],
                &[],
                300,
                &TestRandom(5),
            )
            .await
            .expect("shared bootstrap records should become local candidates");

        let route = profile.establish.expect("establish").route;
        let authorities = route
            .hops
            .iter()
            .map(|hop| hop.authority_id)
            .collect::<std::collections::BTreeSet<_>>();
        assert!(authorities.contains(&authority(20)));
        assert!(authorities.contains(&authority(21)) || authorities.contains(&authority(22)));
        assert!(route
            .hops
            .iter()
            .all(|hop| hop.route_layer_public_key.is_some()));
        assert!(matches!(
            profile.move_decision.binding,
            MovePathBinding::Established(_) | MovePathBinding::Direct(_)
        ));
        let reentry = manager
            .stale_reentry_decision(context(8), 300)
            .await
            .expect("shared records should support stale-node re-entry");
        assert_eq!(reentry.stage, BootstrapReentryStage::RememberedContacts);
    }

    #[tokio::test]
    async fn selection_manager_rejects_invalid_shared_bootstrap_records() {
        let registry = Arc::new(ServiceRegistry::new());
        let health = LocalHealthObserverService::new(LocalHealthObserverConfig::for_testing());
        let manager =
            SelectionManagerService::new(SelectionManagerConfig::for_testing(), registry, health);

        let error = manager
            .remember_neighborhood_reentry_records(
                context(9),
                &[NeighborhoodReentryHint {
                    scope: context(9),
                    publisher_authority: authority(30),
                    advertised_authority: authority(31),
                    advertised_device: None,
                    link_endpoints: Vec::new(),
                    route_layer_public_key: None,
                    published_at_ms: 10,
                    valid_until: 20,
                    replay_window_id: [30; 32],
                }],
            )
            .await
            .expect_err("invalid board hint should be rejected");

        assert!(matches!(
            error,
            SelectionManagerError::InvalidBootstrapRecord {
                kind: "neighborhood_reentry_hint",
                ..
            }
        ));

        assert!(manager
            .stale_reentry_decision(context(9), 30)
            .await
            .is_none());
    }
}
