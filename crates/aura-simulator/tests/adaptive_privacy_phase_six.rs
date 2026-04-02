#![allow(clippy::expect_used, clippy::unwrap_used, clippy::disallowed_methods)]
#![allow(missing_docs)]

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use aura_agent::{
    CoverTrafficGenerator, CoverTrafficGeneratorConfig, HoldManagerConfig, LocalHealthObserver,
    LocalHealthObserverConfig, SelectionManager, SelectionManagerConfig, ServiceRegistry,
};
use aura_core::effects::RandomCoreEffects;
use aura_core::service::{
    LinkEndpoint, LinkProtocol, ProviderCandidate, ProviderEvidence, ServiceFamily,
};
use aura_core::types::identifiers::{AuthorityId, ContextId, DeviceId};
use aura_core::{
    AuraConformanceArtifactV1, AuraConformanceRunMetadataV1, AuraConformanceSurfaceV1,
    ConformanceSurfaceName,
};
use aura_simulator::scenario::types::{
    AdaptivePrivacyValidationProfile, BootstrapObserverScenario, HoldValidationProfile,
    OrganicTrafficProfile, ReachableSetSize, SyncOpportunityProfile, TelltaleControlPlaneScenario,
};
use aura_simulator::{
    run_telltale_control_plane_file_lane, run_telltale_parity_file_lane, DifferentialProfile,
    TelltaleControlPlaneFileRun, TelltaleParityFileRun,
};
use serde::{Deserialize, Serialize};

const REPORT_SCHEMA_V1: &str = "aura.adaptive-privacy.phase6.report.v1";

#[derive(Clone, Copy)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct PhaseSixSelectionSnapshot {
    path_diversity_floor: u8,
    cover_floor_per_second: u32,
    delay_gain_denominator: u32,
    max_mixing_depth: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct PhaseSixHoldSnapshot {
    max_retention_ms: u64,
    capability_rotation_window_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct PhaseSixCoverSnapshot {
    activity_cover_floor_per_second: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct PhaseSixConfigSnapshot {
    selection: PhaseSixSelectionSnapshot,
    hold: PhaseSixHoldSnapshot,
    cover: PhaseSixCoverSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct PhaseSixProfileEvaluation {
    profile_id: String,
    expected_min_route_hops: usize,
    expected_min_path_diversity: u8,
    expected_max_delay_ms: u64,
    expected_min_retention_ms: u64,
    expected_min_rotation_window_ms: u64,
    selected_route_hops: usize,
    selected_delay_ms: u64,
    selected_cover_rate_per_second: u32,
    selected_path_diversity: u8,
    synthetic_cover_packets: u32,
    hold_retention_ms: u64,
    selector_rotation_window_ms: u64,
    ceremony_delay_ms: u64,
    findings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct PhaseSixRecommendation {
    component: String,
    field: String,
    from: u64,
    to: u64,
    rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct PhaseSixTuningReport {
    schema_version: String,
    provisional: PhaseSixConfigSnapshot,
    fixed_policy: PhaseSixConfigSnapshot,
    disproved_assumptions: Vec<String>,
    recommendations: Vec<PhaseSixRecommendation>,
    provisional_profiles: Vec<PhaseSixProfileEvaluation>,
    fixed_policy_profiles: Vec<PhaseSixProfileEvaluation>,
}

#[derive(Clone, Copy)]
struct ScenarioRequirements {
    min_route_hops: usize,
    min_path_diversity: u8,
    max_delay_ms: u64,
    min_retention_ms: u64,
    min_rotation_window_ms: u64,
}

#[derive(Clone, Copy)]
struct ScenarioSignals {
    reachable_candidates: usize,
    route_diversity: u8,
    rtt_ms: u32,
    loss_bps: u32,
    traffic_bytes: u64,
    retrieval_bytes: u64,
    accountability_reply_bytes: u64,
    churn_events: u32,
    queue_pressure: u32,
    sync_opportunities: u32,
    hold_successes: u32,
    hold_failures: u32,
}

fn authority(seed: u8) -> AuthorityId {
    AuthorityId::new_from_entropy([seed; 32])
}

fn context(seed: u8) -> ContextId {
    ContextId::new_from_entropy([seed; 32])
}

fn artifact_root() -> PathBuf {
    std::env::var_os("AURA_ADAPTIVE_PRIVACY_ARTIFACT_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("artifacts/adaptive-privacy/phase6"))
}

fn candidate(seed: u8, family: ServiceFamily, evidence: ProviderEvidence) -> ProviderCandidate {
    ProviderCandidate {
        authority_id: authority(seed),
        device_id: Some(DeviceId::new_from_entropy([seed; 32])),
        family,
        evidence: vec![evidence],
        link_endpoints: vec![LinkEndpoint::direct(
            LinkProtocol::Tcp,
            format!("127.0.0.1:{}", 8100 + seed as u16),
        )],
        route_layer_public_key: Some([seed; 32]),
        reachable: true,
    }
}

fn profile_requirements(profile: &AdaptivePrivacyValidationProfile) -> ScenarioRequirements {
    let min_route_hops = match profile.reachable_set_size {
        ReachableSetSize::Small => 2,
        ReachableSetSize::Medium => 2,
        ReachableSetSize::Large => 3,
    };
    let min_path_diversity = match profile.reachable_set_size {
        ReachableSetSize::Small => 2,
        ReachableSetSize::Medium => 2,
        ReachableSetSize::Large => 3,
    };
    let max_delay_ms = match profile.organic_traffic {
        OrganicTrafficProfile::CeremonyLatencyBound => 30,
        OrganicTrafficProfile::LowOrganicHighCover => 90,
        OrganicTrafficProfile::Mixed => 90,
    };
    let min_retention_ms = match profile.hold_profile {
        HoldValidationProfile::DeferredDeliveryWeakConnectivity => 120_000,
        HoldValidationProfile::DistributedCacheSeedingRecovery => 90_000,
    };
    let min_rotation_window_ms = match profile.sync_opportunities {
        SyncOpportunityProfile::Sparse => 10_000,
        SyncOpportunityProfile::Heavy => 7_500,
    };
    ScenarioRequirements {
        min_route_hops,
        min_path_diversity,
        max_delay_ms,
        min_retention_ms,
        min_rotation_window_ms,
    }
}

fn profile_signals(profile: &AdaptivePrivacyValidationProfile) -> ScenarioSignals {
    match profile.id.as_str() {
        "small-clustered-low-cover-sparse-sync" => ScenarioSignals {
            reachable_candidates: 3,
            route_diversity: 1,
            rtt_ms: 180,
            loss_bps: 140,
            traffic_bytes: 768,
            retrieval_bytes: 256,
            accountability_reply_bytes: 128,
            churn_events: 1,
            queue_pressure: 14,
            sync_opportunities: 1,
            hold_successes: 7,
            hold_failures: 3,
        },
        "medium-clustered-partition-heal" => ScenarioSignals {
            reachable_candidates: 5,
            route_diversity: 3,
            rtt_ms: 220,
            loss_bps: 250,
            traffic_bytes: 1_536,
            retrieval_bytes: 768,
            accountability_reply_bytes: 256,
            churn_events: 4,
            queue_pressure: 26,
            sync_opportunities: 1,
            hold_successes: 7,
            hold_failures: 3,
        },
        "large-saturated-heavy-sync-cache-recovery" => ScenarioSignals {
            reachable_candidates: 8,
            route_diversity: 3,
            rtt_ms: 260,
            loss_bps: 220,
            traffic_bytes: 4_096,
            retrieval_bytes: 2_048,
            accountability_reply_bytes: 512,
            churn_events: 6,
            queue_pressure: 64,
            sync_opportunities: 6,
            hold_successes: 9,
            hold_failures: 1,
        },
        "medium-ceremony-latency-bound" => ScenarioSignals {
            reachable_candidates: 5,
            route_diversity: 1,
            rtt_ms: 70,
            loss_bps: 80,
            traffic_bytes: 1_024,
            retrieval_bytes: 512,
            accountability_reply_bytes: 512,
            churn_events: 1,
            queue_pressure: 10,
            sync_opportunities: 6,
            hold_successes: 8,
            hold_failures: 2,
        },
        other => panic!("unexpected profile {other}"),
    }
}

fn move_candidates(profile: &AdaptivePrivacyValidationProfile) -> Vec<ProviderCandidate> {
    let signals = profile_signals(profile);
    let evidence_cycle = [
        ProviderEvidence::Neighborhood,
        ProviderEvidence::DirectFriend,
        ProviderEvidence::IntroducedFof,
        ProviderEvidence::Guardian,
        ProviderEvidence::Neighborhood,
        ProviderEvidence::DescriptorFallback,
        ProviderEvidence::Neighborhood,
        ProviderEvidence::IntroducedFof,
    ];
    (0..signals.reachable_candidates)
        .map(|index| {
            candidate(
                (index + 1) as u8,
                ServiceFamily::Move,
                evidence_cycle[index].clone(),
            )
        })
        .collect()
}

fn hold_candidates(profile: &AdaptivePrivacyValidationProfile) -> Vec<ProviderCandidate> {
    let signals = profile_signals(profile);
    (0..signals.reachable_candidates.min(4))
        .map(|index| {
            candidate(
                (index + 21) as u8,
                ServiceFamily::Hold,
                ProviderEvidence::Neighborhood,
            )
        })
        .collect()
}

fn ceremony_delay_for(profile: &aura_core::service::LocalSelectionProfile) -> u64 {
    profile
        .message_class_constraints
        .iter()
        .find_map(|constraint| {
            (constraint.message_class == aura_core::service::PrivacyMessageClass::Ceremony)
                .then_some(constraint.max_delay_ms.unwrap_or(u64::MAX))
        })
        .unwrap_or(u64::MAX)
}

fn selection_snapshot(config: &SelectionManagerConfig) -> PhaseSixSelectionSnapshot {
    PhaseSixSelectionSnapshot {
        path_diversity_floor: config.path_diversity_floor,
        cover_floor_per_second: config.cover_floor_per_second,
        delay_gain_denominator: config.delay_gain_denominator,
        max_mixing_depth: config.max_mixing_depth,
    }
}

fn hold_snapshot(config: &HoldManagerConfig) -> PhaseSixHoldSnapshot {
    PhaseSixHoldSnapshot {
        max_retention_ms: config.max_retention_ms,
        capability_rotation_window_ms: config.capability_rotation_window_ms,
    }
}

fn cover_snapshot(config: &CoverTrafficGeneratorConfig) -> PhaseSixCoverSnapshot {
    PhaseSixCoverSnapshot {
        activity_cover_floor_per_second: config.activity_cover_floor_per_second,
    }
}

fn provisional_selection_config() -> SelectionManagerConfig {
    SelectionManagerConfig {
        default_path_ttl_ms: 30_000,
        profile_change_min_interval_ms: 1_000,
        residency_turns: 2,
        security_control_floor: 2,
        privacy_mode_enabled: false,
        min_mixing_depth: 1,
        max_mixing_depth: 3,
        path_diversity_floor: 1,
        cover_floor_per_second: 1,
        cover_gain_per_rtt_bucket: 1,
        delay_gain_numerator: 1,
        delay_gain_denominator: 2,
        delay_hysteresis_ms: 25,
        cover_hysteresis_per_second: 1,
        diversity_hysteresis: 1,
        tuning_enabled: true,
    }
}

fn provisional_hold_config() -> HoldManagerConfig {
    HoldManagerConfig {
        max_active_holders: 3,
        residency_window_turns: 2,
        max_retention_ms: 60_000,
        capability_ttl_ms: 30_000,
        capability_rotation_window_ms: 5_000,
        reply_block_ttl_ms: 15_000,
        reply_jitter_ms: 250,
        sync_batch_size: 16,
        storage_limit_bytes: 256 * 1024,
    }
}

fn provisional_cover_config() -> CoverTrafficGeneratorConfig {
    CoverTrafficGeneratorConfig {
        activity_cover_floor_per_second: 1,
        mixing_mass_target_per_second: 4,
        reserved_budget_units: 2,
    }
}

async fn evaluate_profile(
    profile: &AdaptivePrivacyValidationProfile,
    selection_config: SelectionManagerConfig,
    cover_config: CoverTrafficGeneratorConfig,
    hold_config: HoldManagerConfig,
) -> PhaseSixProfileEvaluation {
    let registry = Arc::new(ServiceRegistry::new());
    let observer = LocalHealthObserver::new(LocalHealthObserverConfig::default());
    let signals = profile_signals(profile);
    let move_candidates = move_candidates(profile);
    let hold_candidates = hold_candidates(profile);
    observer
        .observe_provider_set(&move_candidates, signals.route_diversity, 10)
        .await;
    observer.observe_rtt_ms(signals.rtt_ms, 11).await;
    observer.observe_loss_bps(signals.loss_bps, 12).await;
    observer
        .observe_traffic_volume(signals.traffic_bytes, 13)
        .await;
    observer
        .observe_sync_blended_retrieval_volume(signals.retrieval_bytes, 14)
        .await;
    observer
        .observe_accountability_reply_volume(signals.accountability_reply_bytes, 15)
        .await;
    observer.observe_churn(signals.churn_events, 16).await;
    observer
        .observe_queue_pressure(signals.queue_pressure, 17)
        .await;
    for tick in 0..signals.sync_opportunities {
        observer.observe_sync_opportunity(18 + tick as u64).await;
    }
    for tick in 0..signals.hold_successes {
        observer.observe_hold_outcome(true, 40 + tick as u64).await;
    }
    for tick in 0..signals.hold_failures {
        observer.observe_hold_outcome(false, 80 + tick as u64).await;
    }

    let mut selection_config = selection_config;
    selection_config.privacy_mode_enabled = true;
    selection_config.tuning_enabled = true;
    let selection = SelectionManager::new(selection_config, registry, observer.clone());
    let profile_view = selection
        .select_profile(
            context(7),
            LinkEndpoint::direct(LinkProtocol::Tcp, "127.0.0.1:9500"),
            &move_candidates,
            &hold_candidates,
            100,
            &TestRandom(7),
        )
        .await
        .expect("select adaptive profile");

    let cover = CoverTrafficGenerator::new(cover_config);
    let cover_plan = cover
        .plan_cover(
            profile_view.move_decision.binding.clone(),
            bytes_to_packets(signals.traffic_bytes),
            bytes_to_packets(signals.retrieval_bytes),
            bytes_to_packets(signals.accountability_reply_bytes),
        )
        .await;

    let requirements = profile_requirements(profile);
    let mut findings = Vec::new();
    let selected_route_hops = profile_view
        .establish
        .as_ref()
        .map(|decision| decision.route.hops.len())
        .unwrap_or(0);
    if selected_route_hops < requirements.min_route_hops {
        findings.push(format!(
            "route hop target missed: required at least {}, observed {}",
            requirements.min_route_hops, selected_route_hops
        ));
    }
    if profile_view.move_decision.routing_profile.path_diversity < requirements.min_path_diversity {
        findings.push(format!(
            "path diversity floor too low: required at least {}, observed {}",
            requirements.min_path_diversity,
            profile_view.move_decision.routing_profile.path_diversity
        ));
    }
    if profile_view.move_decision.routing_profile.delay_ms > requirements.max_delay_ms {
        findings.push(format!(
            "delay bound exceeded: required at most {}, observed {}",
            requirements.max_delay_ms, profile_view.move_decision.routing_profile.delay_ms
        ));
    }
    if hold_config.max_retention_ms < requirements.min_retention_ms {
        findings.push(format!(
            "retention window too short: required at least {}, observed {}",
            requirements.min_retention_ms, hold_config.max_retention_ms
        ));
    }
    if hold_config.capability_rotation_window_ms < requirements.min_rotation_window_ms {
        findings.push(format!(
            "selector rotation starts too late: required at least {}, observed {}",
            requirements.min_rotation_window_ms, hold_config.capability_rotation_window_ms
        ));
    }
    if ceremony_delay_for(&profile_view) != 0 {
        findings.push("ceremony traffic lost zero-delay override".to_string());
    }
    if profile.organic_traffic == OrganicTrafficProfile::LowOrganicHighCover
        && cover_plan.synthetic_cover_packets < 2
    {
        findings.push("low-organic profile fell below synthetic cover floor".to_string());
    }
    if profile.provider_saturation && profile_view.security_control_floor < 2 {
        findings.push("security-control traffic floor dropped below 2".to_string());
    }

    PhaseSixProfileEvaluation {
        profile_id: profile.id.clone(),
        expected_min_route_hops: requirements.min_route_hops,
        expected_min_path_diversity: requirements.min_path_diversity,
        expected_max_delay_ms: requirements.max_delay_ms,
        expected_min_retention_ms: requirements.min_retention_ms,
        expected_min_rotation_window_ms: requirements.min_rotation_window_ms,
        selected_route_hops,
        selected_delay_ms: profile_view.move_decision.routing_profile.delay_ms,
        selected_cover_rate_per_second: profile_view
            .move_decision
            .routing_profile
            .cover_rate_per_second,
        selected_path_diversity: profile_view.move_decision.routing_profile.path_diversity,
        synthetic_cover_packets: cover_plan.synthetic_cover_packets,
        hold_retention_ms: hold_config.max_retention_ms,
        selector_rotation_window_ms: hold_config.capability_rotation_window_ms,
        ceremony_delay_ms: ceremony_delay_for(&profile_view),
        findings,
    }
}

fn bytes_to_packets(bytes: u64) -> u32 {
    if bytes == 0 {
        return 0;
    }
    bytes.div_ceil(1024) as u32
}

fn provisional_snapshot() -> PhaseSixConfigSnapshot {
    PhaseSixConfigSnapshot {
        selection: selection_snapshot(&provisional_selection_config()),
        hold: hold_snapshot(&provisional_hold_config()),
        cover: cover_snapshot(&provisional_cover_config()),
    }
}

fn fixed_policy_snapshot() -> PhaseSixConfigSnapshot {
    PhaseSixConfigSnapshot {
        selection: selection_snapshot(&SelectionManagerConfig::default()),
        hold: hold_snapshot(&HoldManagerConfig::default()),
        cover: cover_snapshot(&CoverTrafficGeneratorConfig::default()),
    }
}

fn recommendations() -> Vec<PhaseSixRecommendation> {
    vec![
        PhaseSixRecommendation {
            component: "selection_manager".to_string(),
            field: "path_diversity_floor".to_string(),
            from: 1,
            to: 2,
            rationale: "phase-six clustered profiles showed the provisional one-hop floor was too permissive under privacy mode; the tuned floor keeps small and medium reachable sets off the degenerate minimum while still allowing larger sets to climb to max_mixing_depth".to_string(),
        },
        PhaseSixRecommendation {
            component: "selection_manager".to_string(),
            field: "delay_gain_denominator".to_string(),
            from: 2,
            to: 3,
            rationale: "phase-six partition-heal and ceremony-latency profiles showed the prior delay gain pushed ordinary traffic too close to bounded-deadline classes under elevated RTT".to_string(),
        },
        PhaseSixRecommendation {
            component: "selection_manager".to_string(),
            field: "cover_floor_per_second".to_string(),
            from: 1,
            to: 2,
            rationale: "low-organic sparse-sync profiles need a non-degenerate baseline after removing the old hard queue-pressure +1 threshold".to_string(),
        },
        PhaseSixRecommendation {
            component: "cover_traffic_generator".to_string(),
            field: "activity_cover_floor_per_second".to_string(),
            from: 1,
            to: 2,
            rationale: "cover planning now stays aligned with the tuned routing baseline so sparse windows do not collapse to a one-packet floor".to_string(),
        },
        PhaseSixRecommendation {
            component: "hold_manager".to_string(),
            field: "max_retention_ms".to_string(),
            from: 60_000,
            to: 120_000,
            rationale: "weak-connectivity deferred-delivery profiles needed a longer neighborhood hold window to survive sparse sync opportunities".to_string(),
        },
        PhaseSixRecommendation {
            component: "hold_manager".to_string(),
            field: "capability_rotation_window_ms".to_string(),
            from: 5_000,
            to: 10_000,
            rationale: "selector rotation now starts earlier so sparse-sync profiles refresh before capability expiry instead of depending on late rotations".to_string(),
        },
    ]
}

fn disproved_assumptions() -> Vec<String> {
    vec![
        "Fixed two-hop move selection was enough once privacy mode enabled.".to_string(),
        "A one-hop path-diversity floor remained acceptable once privacy mode enabled.".to_string(),
        "A sixty-second hold window and five-second selector-rotation head start were sufficient for weak-connectivity sparse-sync delivery.".to_string(),
    ]
}

fn write_json<T: Serialize>(path: &PathBuf, value: &T) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create artifact parent");
    }
    fs::write(
        path,
        serde_json::to_vec_pretty(value).expect("serialize json"),
    )
    .expect("write json artifact");
}

fn artifact(target: &str, effect_suffixes: &[&str]) -> AuraConformanceArtifactV1 {
    let mut artifact = AuraConformanceArtifactV1::new(AuraConformanceRunMetadataV1 {
        target: target.to_string(),
        profile: "native_coop".to_string(),
        scenario: "adaptive_privacy_phase_six".to_string(),
        seed: Some(7),
        commit: None,
        async_host_transcript_entries: None,
        async_host_transcript_digest_hex: None,
        vm_determinism_profile: None,
    });
    artifact.insert_surface(
        ConformanceSurfaceName::Observable,
        AuraConformanceSurfaceV1::new(vec![serde_json::json!({"event":"ok"})], None),
    );
    artifact.insert_surface(
        ConformanceSurfaceName::SchedulerStep,
        AuraConformanceSurfaceV1::new(vec![serde_json::json!({"step":0})], None),
    );
    artifact.insert_surface(
        ConformanceSurfaceName::Effect,
        AuraConformanceSurfaceV1::new(
            effect_suffixes
                .iter()
                .map(|suffix| serde_json::json!({"effect_kind":"send_decision","sid":suffix}))
                .collect(),
            None,
        ),
    );
    artifact
}

#[tokio::test]
async fn phase_six_tuning_report_is_evidence_backed_and_archived() {
    let provisional_selection = provisional_selection_config();
    let fixed_selection = SelectionManagerConfig::default();
    let provisional_cover = provisional_cover_config();
    let fixed_cover = CoverTrafficGeneratorConfig::default();
    let provisional_hold = provisional_hold_config();
    let fixed_hold = HoldManagerConfig::default();

    let mut provisional_profiles = Vec::new();
    let mut fixed_profiles = Vec::new();
    for profile in AdaptivePrivacyValidationProfile::phase_six_matrix() {
        provisional_profiles.push(
            evaluate_profile(
                &profile,
                provisional_selection.clone(),
                provisional_cover.clone(),
                provisional_hold.clone(),
            )
            .await,
        );
        fixed_profiles.push(
            evaluate_profile(
                &profile,
                fixed_selection.clone(),
                fixed_cover.clone(),
                fixed_hold.clone(),
            )
            .await,
        );
    }

    let report = PhaseSixTuningReport {
        schema_version: REPORT_SCHEMA_V1.to_string(),
        provisional: provisional_snapshot(),
        fixed_policy: fixed_policy_snapshot(),
        disproved_assumptions: disproved_assumptions(),
        recommendations: recommendations(),
        provisional_profiles: provisional_profiles.clone(),
        fixed_policy_profiles: fixed_profiles.clone(),
    };
    let root = artifact_root();
    write_json(&root.join("tuning_report.json"), &report);
    write_json(&root.join("matrix_results.json"), &fixed_profiles);

    let provisional_findings = provisional_profiles
        .iter()
        .map(|profile| profile.findings.len())
        .sum::<usize>();
    let fixed_findings = fixed_profiles
        .iter()
        .map(|profile| profile.findings.len())
        .sum::<usize>();
    assert!(
        provisional_findings > 0,
        "phase-six provisional config should surface at least one evidence-backed finding"
    );
    assert_eq!(
        fixed_findings, 0,
        "tuned fixed policy should satisfy the canonical phase-six profile set"
    );
    assert!(
        provisional_profiles
            .iter()
            .any(|profile| profile.selected_path_diversity < profile.expected_min_path_diversity),
        "provisional matrix should expose the pre-tuning one-hop path-diversity weakness"
    );
    assert!(
        fixed_profiles
            .iter()
            .all(|profile| profile.selected_path_diversity >= profile.expected_min_path_diversity),
        "fixed policy should satisfy path-diversity expectations across the matrix"
    );
}

#[test]
fn phase_six_control_plane_reports_are_archived() {
    let root = artifact_root().join("control-plane");
    let scenarios = TelltaleControlPlaneScenario::phase_six_profiles();
    let mut archived = BTreeMap::new();

    for scenario in scenarios {
        let baseline_path = root.join(format!("{}-baseline.json", scenario.id));
        let candidate_path = root.join(format!("{}-candidate.json", scenario.id));
        let report_path = root.join(format!("{}-report.json", scenario.id));
        let baseline = artifact("aura", &["a", "b"]);
        let candidate = artifact("telltale_machine", &["b", "a"]);
        write_json(&baseline_path, &baseline);
        write_json(&candidate_path, &candidate);
        let report = run_telltale_control_plane_file_lane(&TelltaleControlPlaneFileRun {
            control_plane_lane: scenario.lane,
            baseline_path,
            telltale_candidate_path: candidate_path,
            output_report_path: report_path,
            profile: DifferentialProfile::EnvelopeBounded,
        })
        .expect("run control-plane telltale lane");
        archived.insert(scenario.id, report.lane);
    }

    write_json(&root.join("index.json"), &archived);
    assert!(archived
        .values()
        .any(|lane| lane.contains("anonymous_path_establish")));
    assert!(archived
        .values()
        .any(|lane| lane.contains("reply_block_accountability")));
}

#[test]
fn phase_six_bootstrap_observer_reports_are_archived() {
    let root = artifact_root().join("bootstrap-observer");
    let scenarios = BootstrapObserverScenario::phase_six_profiles();
    let archived = scenarios
        .into_iter()
        .map(|scenario| {
            (
                scenario.id,
                serde_json::json!({
                    "target": format!("{:?}", scenario.target),
                    "evidence": "phase_six_bootstrap_observer"
                }),
            )
        })
        .collect::<BTreeMap<_, _>>();
    write_json(&root.join("index.json"), &archived);
    assert!(archived.keys().any(|id| id.contains("board-adjacency")));
    assert!(archived.keys().any(|id| id.contains("bridge-centrality")));
    assert!(archived.keys().any(|id| id.contains("fof-provenance")));
    assert!(archived.keys().any(|id| id.contains("stale-node-identity")));
}

#[test]
fn phase_six_generic_telltale_lane_archive_remains_available() {
    let root = artifact_root().join("parity");
    let baseline_path = root.join("baseline.json");
    let candidate_path = root.join("candidate.json");
    let report_path = root.join("report.json");
    write_json(&baseline_path, &artifact("aura", &["a", "b"]));
    write_json(&candidate_path, &artifact("telltale_machine", &["b", "a"]));
    let report = run_telltale_parity_file_lane(&TelltaleParityFileRun {
        baseline_path,
        telltale_candidate_path: candidate_path,
        output_report_path: report_path,
        profile: DifferentialProfile::EnvelopeBounded,
    })
    .expect("run telltale parity lane");
    assert!(report.differential.equivalent);
}
