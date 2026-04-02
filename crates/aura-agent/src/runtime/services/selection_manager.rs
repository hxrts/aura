//! Runtime-owned adaptive selection manager.
#![allow(dead_code)]

use super::config_profiles::impl_service_config_profiles;
use super::local_health_observer::LocalHealthObserverService;
use super::service_registry::{ProviderHealthSnapshot, ServiceRegistry};
use super::traits::{RuntimeService, RuntimeServiceContext, ServiceError, ServiceHealth};
use async_trait::async_trait;
use aura_core::effects::RandomEffects;
use aura_core::service::{
    LinkEndpoint, LocalEstablishDecision, LocalHealthSnapshot, LocalHoldDecision,
    LocalMoveDecision, LocalRoutingProfile, LocalSelectionProfile, MessageClassRoutingConstraint,
    MovePath, MovePathBinding, PrivacyMessageClass, ProviderCandidate, ProviderEvidence, Route,
    SchedulerClass, SecurityControlClass, SelectionState, ServiceFamily, ServiceProfile,
};
use aura_core::types::identifiers::{AuthorityId, ContextId};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use tokio::sync::RwLock;

#[allow(dead_code)]
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
    lifecycle: ServiceHealth,
}

impl Default for SelectionManagerState {
    fn default() -> Self {
        Self {
            last_profiles: HashMap::new(),
            residency: HashMap::new(),
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
        let selected_move_authorities: Vec<AuthorityId> = self
            .pick_authorities(scope, move_candidates, ServiceFamily::Move, &health, random)
            .await?;
        let selected_hold_authorities: Vec<AuthorityId> = self
            .pick_authorities(scope, hold_candidates, ServiceFamily::Hold, &health, random)
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
                &move_candidates_by_authority(move_candidates),
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

        let synthetic_cover_gap_per_second = routing_profile.cover_rate_per_second.saturating_sub(
            ((health.traffic_volume_bytes + health.sync_blended_retrieval_bytes) / 1024) as u32,
        );

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
        let residency = self.state.read().await.residency.clone();
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
                    ),
                )
            })
            .collect::<Vec<_>>();
        let mut selected = Vec::new();
        let max_select = if family == ServiceFamily::Hold {
            3
        } else {
            usize::from(self.config.max_mixing_depth.max(self.config.min_mixing_depth))
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
                    *weight = weight.saturating_sub(diversity_penalty);
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
    endpoints: &BTreeMap<AuthorityId, Vec<LinkEndpoint>>,
    destination: LinkEndpoint,
) -> Route {
    Route {
        hops: authorities
            .iter()
            .map(|authority_id| aura_core::service::RelayHop {
                authority_id: *authority_id,
                link_endpoint: endpoints
                    .get(authority_id)
                    .and_then(|values| values.first().cloned())
                    .unwrap_or_else(|| LinkEndpoint::relay(*authority_id)),
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
        .saturating_add(availability_score)
        .saturating_add(diversity_score)
        .saturating_sub(residency_penalty_turns.saturating_mul(20))
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
            reachable,
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
        assert!(
            profile.synthetic_cover_gap_per_second
                <= profile.move_decision.routing_profile.cover_rate_per_second
        );
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
}
