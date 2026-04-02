//! Runtime-owned local health observer.
//!
//! Derives smoothed local-only health snapshots for adaptive privacy policy.
#![allow(dead_code)]

use super::config_profiles::impl_service_config_profiles;
use super::traits::{RuntimeService, RuntimeServiceContext, ServiceError, ServiceHealth};
use async_trait::async_trait;
use aura_core::service::{LocalHealthSnapshot, ProviderCandidate};
use std::sync::Arc;
use tokio::sync::RwLock;

#[allow(dead_code, clippy::enum_variant_names)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocalHealthObserverCommand {
    ObserveProviderSet,
    ObserveRtt,
    ObserveLoss,
    ObserveTraffic,
    ObserveChurn,
    ObserveQueuePressure,
    ObserveHoldOutcome,
    ObserveSyncOpportunity,
}

#[derive(Debug, Clone)]
pub struct LocalHealthObserverConfig {
    pub ema_numerator: u32,
    pub ema_denominator: u32,
    pub queue_pressure_cap: u32,
    /// Small EMA deltas below these thresholds are suppressed to avoid thrash.
    pub rtt_hysteresis_ms: u32,
    pub loss_hysteresis_bps: u32,
    pub queue_pressure_hysteresis: u32,
    /// Minimum interval between noisy smoothed updates unless the change is large.
    pub min_smoothed_update_interval_ms: u64,
}

impl Default for LocalHealthObserverConfig {
    fn default() -> Self {
        Self {
            ema_numerator: 1,
            ema_denominator: 4,
            queue_pressure_cap: 100,
            rtt_hysteresis_ms: 10,
            loss_hysteresis_bps: 50,
            queue_pressure_hysteresis: 2,
            min_smoothed_update_interval_ms: 100,
        }
    }
}

impl_service_config_profiles!(LocalHealthObserverConfig {
    pub fn for_testing() -> Self {
        Self {
            ema_numerator: 1,
            ema_denominator: 2,
            queue_pressure_cap: 16,
            rtt_hysteresis_ms: 5,
            loss_hysteresis_bps: 10,
            queue_pressure_hysteresis: 1,
            min_smoothed_update_interval_ms: 25,
        }
    }
});

#[derive(Debug)]
struct LocalHealthObserverState {
    snapshot: Option<LocalHealthSnapshot>,
    hold_successes: u32,
    hold_failures: u32,
    last_smoothed_update_ms: Option<u64>,
    lifecycle: ServiceHealth,
}

impl Default for LocalHealthObserverState {
    fn default() -> Self {
        Self {
            snapshot: None,
            hold_successes: 0,
            hold_failures: 0,
            last_smoothed_update_ms: None,
            lifecycle: ServiceHealth::NotStarted,
        }
    }
}

#[aura_macros::actor_owned(
    owner = "local_health_observer",
    domain = "adaptive_privacy_health",
    gate = "local_health_command_ingress",
    command = LocalHealthObserverCommand,
    capacity = 64,
    category = "actor_owned"
)]
#[derive(Default)]
pub struct LocalHealthObserverService {
    config: LocalHealthObserverConfig,
    state: Arc<RwLock<LocalHealthObserverState>>,
}

impl LocalHealthObserverService {
    pub fn new(config: LocalHealthObserverConfig) -> Self {
        Self {
            config,
            state: Arc::new(RwLock::new(LocalHealthObserverState::default())),
        }
    }

    pub async fn snapshot(&self) -> LocalHealthSnapshot {
        self.state
            .read()
            .await
            .snapshot
            .clone()
            .unwrap_or(LocalHealthSnapshot {
                generated_at_ms: 0,
                reachable_provider_count: 0,
                ema_rtt_ms: 0,
                ema_loss_bps: 0,
                traffic_volume_bytes: 0,
                sync_blended_retrieval_bytes: 0,
                accountability_reply_bytes: 0,
                churn_events: 0,
                observed_route_diversity: 0,
                queue_pressure: 0,
                hold_success_bps: 10_000,
                sync_opportunity_count: 0,
            })
    }

    pub async fn observe_provider_set(
        &self,
        candidates: &[ProviderCandidate],
        route_diversity: u8,
        now_ms: u64,
    ) -> LocalHealthSnapshot {
        let mut state = self.state.write().await;
        let mut snapshot = state.snapshot.clone().unwrap_or(LocalHealthSnapshot {
            generated_at_ms: now_ms,
            reachable_provider_count: 0,
            ema_rtt_ms: 0,
            ema_loss_bps: 0,
            traffic_volume_bytes: 0,
            sync_blended_retrieval_bytes: 0,
            accountability_reply_bytes: 0,
            churn_events: 0,
            observed_route_diversity: route_diversity,
            queue_pressure: 0,
            hold_success_bps: 10_000,
            sync_opportunity_count: 0,
        });
        snapshot.generated_at_ms = now_ms;
        snapshot.reachable_provider_count = candidates
            .iter()
            .filter(|candidate| candidate.reachable)
            .count() as u32;
        snapshot.observed_route_diversity = route_diversity;
        state.snapshot = Some(snapshot.clone());
        snapshot
    }

    pub async fn observe_rtt_ms(&self, rtt_ms: u32, now_ms: u64) -> LocalHealthSnapshot {
        self.update_snapshot(now_ms, |snapshot, config, _state| {
            snapshot.ema_rtt_ms = apply_smoothed_value(
                snapshot.ema_rtt_ms,
                rtt_ms,
                config.rtt_hysteresis_ms,
                now_ms,
                config,
                _state,
            );
        })
        .await
    }

    pub async fn observe_loss_bps(&self, loss_bps: u32, now_ms: u64) -> LocalHealthSnapshot {
        self.update_snapshot(now_ms, |snapshot, config, _state| {
            snapshot.ema_loss_bps = apply_smoothed_value(
                snapshot.ema_loss_bps,
                loss_bps,
                config.loss_hysteresis_bps,
                now_ms,
                config,
                _state,
            );
        })
        .await
    }

    pub async fn observe_traffic_volume(
        &self,
        traffic_bytes: u64,
        now_ms: u64,
    ) -> LocalHealthSnapshot {
        self.update_snapshot(now_ms, |snapshot, _config, _state| {
            snapshot.traffic_volume_bytes =
                snapshot.traffic_volume_bytes.saturating_add(traffic_bytes);
        })
        .await
    }

    pub async fn observe_sync_blended_retrieval_volume(
        &self,
        retrieval_bytes: u64,
        now_ms: u64,
    ) -> LocalHealthSnapshot {
        self.update_snapshot(now_ms, |snapshot, _config, _state| {
            snapshot.sync_blended_retrieval_bytes = snapshot
                .sync_blended_retrieval_bytes
                .saturating_add(retrieval_bytes);
        })
        .await
    }

    pub async fn observe_accountability_reply_volume(
        &self,
        reply_bytes: u64,
        now_ms: u64,
    ) -> LocalHealthSnapshot {
        self.update_snapshot(now_ms, |snapshot, _config, _state| {
            snapshot.accountability_reply_bytes = snapshot
                .accountability_reply_bytes
                .saturating_add(reply_bytes);
        })
        .await
    }

    pub async fn observe_churn(&self, churn_events: u32, now_ms: u64) -> LocalHealthSnapshot {
        self.update_snapshot(now_ms, |snapshot, _config, _state| {
            snapshot.churn_events = snapshot.churn_events.saturating_add(churn_events);
        })
        .await
    }

    pub async fn observe_queue_pressure(&self, pressure: u32, now_ms: u64) -> LocalHealthSnapshot {
        self.update_snapshot(now_ms, |snapshot, config, _state| {
            snapshot.queue_pressure = apply_smoothed_value(
                snapshot.queue_pressure,
                pressure.min(config.queue_pressure_cap),
                config.queue_pressure_hysteresis,
                now_ms,
                config,
                _state,
            );
        })
        .await
    }

    pub async fn observe_hold_outcome(&self, success: bool, now_ms: u64) -> LocalHealthSnapshot {
        self.update_snapshot(now_ms, |snapshot, _config, state| {
            if success {
                state.hold_successes = state.hold_successes.saturating_add(1);
            } else {
                state.hold_failures = state.hold_failures.saturating_add(1);
            }
            let total = state
                .hold_successes
                .saturating_add(state.hold_failures)
                .max(1);
            snapshot.hold_success_bps = (state.hold_successes.saturating_mul(10_000)) / total;
        })
        .await
    }

    pub async fn observe_sync_opportunity(&self, now_ms: u64) -> LocalHealthSnapshot {
        self.update_snapshot(now_ms, |snapshot, _config, _state| {
            snapshot.sync_opportunity_count = snapshot.sync_opportunity_count.saturating_add(1);
        })
        .await
    }

    async fn update_snapshot<F>(&self, now_ms: u64, mut update: F) -> LocalHealthSnapshot
    where
        F: FnMut(
            &mut LocalHealthSnapshot,
            &LocalHealthObserverConfig,
            &mut LocalHealthObserverState,
        ),
    {
        let mut state = self.state.write().await;
        let mut snapshot = state.snapshot.clone().unwrap_or(LocalHealthSnapshot {
            generated_at_ms: now_ms,
            reachable_provider_count: 0,
            ema_rtt_ms: 0,
            ema_loss_bps: 0,
            traffic_volume_bytes: 0,
            sync_blended_retrieval_bytes: 0,
            accountability_reply_bytes: 0,
            churn_events: 0,
            observed_route_diversity: 0,
            queue_pressure: 0,
            hold_success_bps: 10_000,
            sync_opportunity_count: 0,
        });
        snapshot.generated_at_ms = now_ms;
        update(&mut snapshot, &self.config, &mut state);
        state.snapshot = Some(snapshot.clone());
        snapshot
    }
}

impl Clone for LocalHealthObserverService {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            state: self.state.clone(),
        }
    }
}

fn ema(previous: u32, sample: u32, config: &LocalHealthObserverConfig) -> u32 {
    if previous == 0 {
        return sample;
    }
    let numerator = previous
        .saturating_mul(config.ema_denominator.saturating_sub(config.ema_numerator))
        .saturating_add(sample.saturating_mul(config.ema_numerator));
    numerator / config.ema_denominator.max(1)
}

fn apply_smoothed_value(
    previous: u32,
    sample: u32,
    hysteresis: u32,
    now_ms: u64,
    config: &LocalHealthObserverConfig,
    state: &mut LocalHealthObserverState,
) -> u32 {
    let candidate = ema(previous, sample, config);
    let within_interval = state
        .last_smoothed_update_ms
        .map(|last| now_ms.saturating_sub(last) < config.min_smoothed_update_interval_ms)
        .unwrap_or(false);
    if within_interval && previous.abs_diff(candidate) < hysteresis {
        return previous;
    }
    state.last_smoothed_update_ms = Some(now_ms);
    candidate
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl RuntimeService for LocalHealthObserverService {
    fn name(&self) -> &'static str {
        "local_health_observer"
    }

    fn dependencies(&self) -> &[&'static str] {
        &["rendezvous_manager", "move_manager", "hold_manager"]
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
    use aura_core::service::{
        LinkEndpoint, LinkProtocol, ProviderCandidate, ProviderEvidence, ServiceFamily,
    };
    use aura_core::types::identifiers::AuthorityId;

    fn authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn candidate(seed: u8, reachable: bool) -> ProviderCandidate {
        ProviderCandidate {
            authority_id: authority(seed),
            device_id: None,
            family: ServiceFamily::Move,
            evidence: vec![ProviderEvidence::Neighborhood],
            link_endpoints: vec![LinkEndpoint::direct(
                LinkProtocol::Tcp,
                format!("127.0.0.1:{}", 8000 + seed as u16),
            )],
            reachable,
        }
    }

    #[tokio::test]
    async fn local_health_observer_smooths_rtt_and_tracks_local_signals() {
        let observer = LocalHealthObserverService::new(LocalHealthObserverConfig::for_testing());
        observer
            .observe_provider_set(&[candidate(1, true), candidate(2, false)], 2, 10)
            .await;
        let first = observer.observe_rtt_ms(100, 11).await;
        let second = observer.observe_rtt_ms(200, 12).await;
        assert_eq!(first.reachable_provider_count, 1);
        assert_eq!(second.observed_route_diversity, 2);
        assert!(second.ema_rtt_ms > 100);
        assert!(second.ema_rtt_ms < 200);
    }

    #[tokio::test]
    async fn local_health_observer_tracks_hold_success_ratio_and_sync_opportunities() {
        let observer = LocalHealthObserverService::new(LocalHealthObserverConfig::for_testing());
        observer.observe_hold_outcome(true, 10).await;
        observer.observe_hold_outcome(false, 11).await;
        let snapshot = observer.observe_sync_opportunity(12).await;
        assert_eq!(snapshot.hold_success_bps, 5000);
        assert_eq!(snapshot.sync_opportunity_count, 1);
    }

    #[tokio::test]
    async fn local_health_observer_tracks_loss_traffic_churn_and_queue_pressure() {
        let observer = LocalHealthObserverService::new(LocalHealthObserverConfig::for_testing());
        observer.observe_loss_bps(120, 10).await;
        observer.observe_traffic_volume(2048, 11).await;
        observer
            .observe_sync_blended_retrieval_volume(512, 12)
            .await;
        observer.observe_accountability_reply_volume(256, 13).await;
        observer.observe_churn(2, 14).await;
        let snapshot = observer.observe_queue_pressure(99, 15).await;
        assert_eq!(snapshot.traffic_volume_bytes, 2048);
        assert_eq!(snapshot.sync_blended_retrieval_bytes, 512);
        assert_eq!(snapshot.accountability_reply_bytes, 256);
        assert_eq!(snapshot.churn_events, 2);
        assert_eq!(snapshot.ema_loss_bps, 120);
        assert!(snapshot.queue_pressure <= 16);
    }

    #[tokio::test]
    async fn local_health_observer_applies_hysteresis_and_rate_limit_to_smoothed_updates() {
        let observer = LocalHealthObserverService::new(LocalHealthObserverConfig::for_testing());
        let first = observer.observe_rtt_ms(100, 10).await;
        let second = observer.observe_rtt_ms(102, 20).await;
        let third = observer.observe_rtt_ms(140, 40).await;
        assert_eq!(first.ema_rtt_ms, 100);
        assert_eq!(second.ema_rtt_ms, 100);
        assert!(third.ema_rtt_ms > 100);
    }
}
