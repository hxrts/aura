//! Scenario definitions and types for simulator

pub mod types {
    use crate::telltale_parity::TelltaleControlPlaneLane;
    use serde::{Deserialize, Serialize};
    use std::collections::HashMap;

    /// Expected outcome of a scenario execution
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    pub enum ExpectedOutcome {
        /// Scenario should complete successfully
        Success,
        /// Scenario should fail with an error
        Failure,
        /// Scenario should timeout
        Timeout,
        /// Property violation should be detected
        PropertyViolation { property: String },
        /// Safety violation should be prevented
        SafetyViolationPrevented,
        /// Success when honest majority exists
        HonestMajoritySuccess,
        /// Chat group functionality validated
        ChatGroupSuccess,
        /// Recovery demo completed successfully
        RecoveryDemoSuccess,
    }

    /// Legacy Byzantine strategy kept for backward compatibility with older scenarios
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct LegacyByzantineStrategy {
        pub name: String,
        pub parameters: HashMap<String, String>,
    }

    /// Byzantine conditions for scenarios
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ByzantineConditions {
        pub strategies: Vec<LegacyByzantineStrategy>,
    }

    /// Network conditions for scenarios
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct NetworkConditions {
        pub latency_ms: Option<u64>,
        pub packet_loss: Option<f64>,
    }

    /// Scenario assertion
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ScenarioAssertion {
        pub property: String,
        pub expected: bool,
    }

    /// Chat group configuration for scenarios
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ChatGroupConfig {
        pub enabled: bool,
        pub multi_actor_support: bool,
        pub message_history_validation: bool,
        pub group_name: Option<String>,
        pub initial_messages: Vec<ChatMessage>,
    }

    /// Chat message for scenario testing
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ChatMessage {
        pub sender: String,
        pub content: String,
        pub timestamp: Option<u64>,
    }

    /// Data loss simulation configuration
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct DataLossSimulation {
        pub enabled: bool,
        pub target_participant: String,
        pub loss_type: DataLossType,
        pub recovery_validation: bool,
    }

    /// Types of data loss for simulation
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub enum DataLossType {
        /// Complete device loss with all data
        CompleteDeviceLoss,
        /// Partial key material corruption
        PartialKeyCorruption,
        /// Network partition simulation
        NetworkPartition,
        /// Storage corruption
        StorageCorruption,
    }

    /// Demo configuration for UX scenarios
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct DemoConfig {
        pub protagonist: Option<String>,
        pub guardians: Vec<String>,
        pub demo_type: DemoType,
        pub validation_steps: Vec<String>,
    }

    /// Types of demo scenarios
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub enum DemoType {
        /// Bob's recovery journey demo
        RecoveryJourney,
        /// Guardian setup demo
        GuardianSetup,
        /// Chat group demo
        ChatGroupDemo,
        /// Multi-actor coordination demo
        MultiActorDemo,
    }

    /// Scenario setup with extended capabilities
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ScenarioSetup {
        pub participants: u32,
        pub threshold: u32,
        pub chat_config: Option<ChatGroupConfig>,
        pub data_loss_config: Option<DataLossSimulation>,
        pub demo_config: Option<DemoConfig>,
    }

    /// Complete scenario definition
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Scenario {
        pub id: String,
        pub name: String,
        pub setup: ScenarioSetup,
        pub network_conditions: Option<NetworkConditions>,
        pub byzantine_conditions: Option<ByzantineConditions>,
        pub assertions: Vec<ScenarioAssertion>,
        pub expected_outcome: ExpectedOutcome,
    }

    /// Reachable set size bucket for adaptive privacy validation.
    #[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
    pub enum ReachableSetSize {
        Small,
        Medium,
        Large,
    }

    /// Topology family used by one adaptive privacy validation profile.
    #[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
    pub enum AdaptiveTopologyKind {
        ClusteredHumanSocial,
        ClusteredPartitioned,
    }

    /// Organic traffic baseline for one validation profile.
    #[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
    pub enum OrganicTrafficProfile {
        LowOrganicHighCover,
        Mixed,
        CeremonyLatencyBound,
    }

    /// Sync opportunity class for one validation profile.
    #[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
    pub enum SyncOpportunityProfile {
        Sparse,
        Heavy,
    }

    /// Deferred-delivery / cache-recovery emphasis for one validation profile.
    #[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
    pub enum HoldValidationProfile {
        DeferredDeliveryWeakConnectivity,
        DistributedCacheSeedingRecovery,
    }

    /// Canonical adaptive privacy validation profile. This is the matrix row
    /// later phases run through simulator and telltale-backed coverage.
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    pub struct AdaptivePrivacyValidationProfile {
        pub id: String,
        pub reachable_set_size: ReachableSetSize,
        pub topology: AdaptiveTopologyKind,
        pub partition_heal_cycles: bool,
        pub provider_saturation: bool,
        pub churn_spikes: bool,
        pub organic_traffic: OrganicTrafficProfile,
        pub sync_opportunities: SyncOpportunityProfile,
        pub hold_profile: HoldValidationProfile,
    }

    impl AdaptivePrivacyValidationProfile {
        /// Canonical Phase 6 validation matrix. Keep this list explicit so the
        /// simulator and later CI lanes can share one matrix definition.
        pub fn phase_six_matrix() -> Vec<Self> {
            vec![
                Self {
                    id: "small-clustered-low-cover-sparse-sync".to_string(),
                    reachable_set_size: ReachableSetSize::Small,
                    topology: AdaptiveTopologyKind::ClusteredHumanSocial,
                    partition_heal_cycles: false,
                    provider_saturation: false,
                    churn_spikes: false,
                    organic_traffic: OrganicTrafficProfile::LowOrganicHighCover,
                    sync_opportunities: SyncOpportunityProfile::Sparse,
                    hold_profile: HoldValidationProfile::DeferredDeliveryWeakConnectivity,
                },
                Self {
                    id: "medium-clustered-partition-heal".to_string(),
                    reachable_set_size: ReachableSetSize::Medium,
                    topology: AdaptiveTopologyKind::ClusteredPartitioned,
                    partition_heal_cycles: true,
                    provider_saturation: false,
                    churn_spikes: true,
                    organic_traffic: OrganicTrafficProfile::Mixed,
                    sync_opportunities: SyncOpportunityProfile::Sparse,
                    hold_profile: HoldValidationProfile::DeferredDeliveryWeakConnectivity,
                },
                Self {
                    id: "large-saturated-heavy-sync-cache-recovery".to_string(),
                    reachable_set_size: ReachableSetSize::Large,
                    topology: AdaptiveTopologyKind::ClusteredPartitioned,
                    partition_heal_cycles: true,
                    provider_saturation: true,
                    churn_spikes: true,
                    organic_traffic: OrganicTrafficProfile::Mixed,
                    sync_opportunities: SyncOpportunityProfile::Heavy,
                    hold_profile: HoldValidationProfile::DistributedCacheSeedingRecovery,
                },
                Self {
                    id: "medium-ceremony-latency-bound".to_string(),
                    reachable_set_size: ReachableSetSize::Medium,
                    topology: AdaptiveTopologyKind::ClusteredHumanSocial,
                    partition_heal_cycles: false,
                    provider_saturation: true,
                    churn_spikes: false,
                    organic_traffic: OrganicTrafficProfile::CeremonyLatencyBound,
                    sync_opportunities: SyncOpportunityProfile::Heavy,
                    hold_profile: HoldValidationProfile::DistributedCacheSeedingRecovery,
                },
            ]
        }
    }

    /// Observer-model inference targets for adaptive privacy evaluation.
    #[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
    pub enum ObserverInferenceTarget {
        HomeMembership,
        DirectFriendEdge,
        IntroductionProvenance,
        PartialPathCompromiseLinkage,
    }

    /// One observer-model scenario row.
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    pub struct ObserverModelScenario {
        pub id: String,
        pub target: ObserverInferenceTarget,
    }

    impl ObserverModelScenario {
        pub fn phase_six_profiles() -> Vec<Self> {
            vec![
                Self {
                    id: "observer-home-membership".to_string(),
                    target: ObserverInferenceTarget::HomeMembership,
                },
                Self {
                    id: "observer-direct-friend-edge".to_string(),
                    target: ObserverInferenceTarget::DirectFriendEdge,
                },
                Self {
                    id: "observer-introduction-provenance".to_string(),
                    target: ObserverInferenceTarget::IntroductionProvenance,
                },
                Self {
                    id: "observer-partial-path-compromise-linkage".to_string(),
                    target: ObserverInferenceTarget::PartialPathCompromiseLinkage,
                },
            ]
        }
    }

    /// Bootstrap observer-model inference targets for stale-node re-entry.
    #[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
    pub enum BootstrapObserverInferenceTarget {
        NeighborhoodAdjacencyFromBoardContents,
        BridgeAuthorityCentralityFromRepeatedReentry,
        FofProvenanceFromBootstrapHintSelection,
        StaleNodeIdentityFromWidenedReentry,
    }

    /// One bootstrap observer scenario row.
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    pub struct BootstrapObserverScenario {
        pub id: String,
        pub target: BootstrapObserverInferenceTarget,
    }

    impl BootstrapObserverScenario {
        pub fn phase_six_profiles() -> Vec<Self> {
            vec![
                Self {
                    id: "bootstrap-observer-board-adjacency".to_string(),
                    target:
                        BootstrapObserverInferenceTarget::NeighborhoodAdjacencyFromBoardContents,
                },
                Self {
                    id: "bootstrap-observer-bridge-centrality".to_string(),
                    target: BootstrapObserverInferenceTarget::BridgeAuthorityCentralityFromRepeatedReentry,
                },
                Self {
                    id: "bootstrap-observer-fof-provenance".to_string(),
                    target: BootstrapObserverInferenceTarget::FofProvenanceFromBootstrapHintSelection,
                },
                Self {
                    id: "bootstrap-observer-stale-node-identity".to_string(),
                    target: BootstrapObserverInferenceTarget::StaleNodeIdentityFromWidenedReentry,
                },
            ]
        }
    }

    /// Security-control traffic classes that must resist starvation.
    #[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
    pub enum SecurityControlTrafficClass {
        AnonymousPathEstablishment,
        CapabilityTrustUpdates,
        AccountabilityReplies,
        RetrievalCapabilityRotation,
    }

    /// Scenario row for starvation testing under heavy ordinary traffic.
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    pub struct StarvationScenario {
        pub id: String,
        pub protected_class: SecurityControlTrafficClass,
    }

    impl StarvationScenario {
        pub fn phase_six_profiles() -> Vec<Self> {
            vec![
                Self {
                    id: "starvation-anonymous-path-establish".to_string(),
                    protected_class: SecurityControlTrafficClass::AnonymousPathEstablishment,
                },
                Self {
                    id: "starvation-capability-trust-updates".to_string(),
                    protected_class: SecurityControlTrafficClass::CapabilityTrustUpdates,
                },
                Self {
                    id: "starvation-accountability-replies".to_string(),
                    protected_class: SecurityControlTrafficClass::AccountabilityReplies,
                },
                Self {
                    id: "starvation-retrieval-capability-rotation".to_string(),
                    protected_class: SecurityControlTrafficClass::RetrievalCapabilityRotation,
                },
            ]
        }
    }

    /// Metadata-minimization focus for validation scenarios.
    #[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
    pub enum MetadataMinimizationFocus {
        RetrievalNotRecipientAddressed,
        DeliveryNotMailboxShaped,
    }

    /// Scenario row for metadata-minimization regression checks.
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    pub struct MetadataMinimizationScenario {
        pub id: String,
        pub focus: MetadataMinimizationFocus,
    }

    impl MetadataMinimizationScenario {
        pub fn phase_six_profiles() -> Vec<Self> {
            vec![
                Self {
                    id: "metadata-retrieval-selector-based".to_string(),
                    focus: MetadataMinimizationFocus::RetrievalNotRecipientAddressed,
                },
                Self {
                    id: "metadata-delivery-not-mailbox-shaped".to_string(),
                    focus: MetadataMinimizationFocus::DeliveryNotMailboxShaped,
                },
            ]
        }
    }

    /// Failure mode for telltale-backed control-plane validation.
    #[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
    pub enum ControlPlaneFailureMode {
        TimeoutAndCancellation,
        WitnessReturnSuccessFailure,
        StaleOwnerAfterHandoff,
        ReplayVisibleFailure,
    }

    /// Protocol-critical control-plane scenario row.
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    pub struct TelltaleControlPlaneScenario {
        pub id: String,
        pub lane: TelltaleControlPlaneLane,
        pub failure_mode: ControlPlaneFailureMode,
    }

    impl TelltaleControlPlaneScenario {
        pub fn phase_six_profiles() -> Vec<Self> {
            vec![
                Self {
                    id: "cp-anonymous-establish-timeout-cancel".to_string(),
                    lane: TelltaleControlPlaneLane::AnonymousPathEstablish,
                    failure_mode: ControlPlaneFailureMode::TimeoutAndCancellation,
                },
                Self {
                    id: "cp-reply-block-witness-success-failure".to_string(),
                    lane: TelltaleControlPlaneLane::ReplyBlockAccountability,
                    failure_mode: ControlPlaneFailureMode::WitnessReturnSuccessFailure,
                },
                Self {
                    id: "cp-anonymous-establish-stale-owner".to_string(),
                    lane: TelltaleControlPlaneLane::AnonymousPathEstablish,
                    failure_mode: ControlPlaneFailureMode::StaleOwnerAfterHandoff,
                },
                Self {
                    id: "cp-reply-block-replay-visible-failure".to_string(),
                    lane: TelltaleControlPlaneLane::ReplyBlockAccountability,
                    failure_mode: ControlPlaneFailureMode::ReplayVisibleFailure,
                },
            ]
        }
    }

    /// Boundary-region focus for controller-knee and selector-threshold checks.
    #[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
    pub enum BoundaryScenarioFocus {
        ControllerKnee,
        RotationThreshold,
    }

    /// Scenario row for boundary behavior checks.
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    pub struct BoundaryScenario {
        pub id: String,
        pub focus: BoundaryScenarioFocus,
    }

    impl BoundaryScenario {
        pub fn phase_six_profiles() -> Vec<Self> {
            vec![
                Self {
                    id: "boundary-controller-knee".to_string(),
                    focus: BoundaryScenarioFocus::ControllerKnee,
                },
                Self {
                    id: "boundary-rotation-threshold".to_string(),
                    focus: BoundaryScenarioFocus::RotationThreshold,
                },
            ]
        }
    }

    /// Metric inventory for adaptive privacy phase-6 validation.
    #[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
    pub enum AdaptivePrivacyMetric {
        ControllerConvergence,
        OscillationThrash,
        DeliveryLatencyLoss,
        BatchSizeDistribution,
        ReplayDropRates,
        CoverSpend,
        SyntheticCoverGap,
        AccountabilityReplyVolumeTiming,
        SecurityControlTrafficLatencyUnderLoad,
        RetrievalSuccessDelay,
        HoldSuccessRetentionQuality,
        AnonymityCorrelationProxies,
        HomeMembershipInferencePrecisionRecall,
        DirectFriendEdgeInferencePrecisionRecall,
        IntroductionProvenanceInferencePrecisionRecall,
        ReplyTimingCorrelationPrecisionRecall,
        PartialPathCompromiseLinkagePrecisionRecall,
    }

    impl AdaptivePrivacyMetric {
        pub fn phase_six_inventory() -> Vec<Self> {
            vec![
                Self::ControllerConvergence,
                Self::OscillationThrash,
                Self::DeliveryLatencyLoss,
                Self::BatchSizeDistribution,
                Self::ReplayDropRates,
                Self::CoverSpend,
                Self::SyntheticCoverGap,
                Self::AccountabilityReplyVolumeTiming,
                Self::SecurityControlTrafficLatencyUnderLoad,
                Self::RetrievalSuccessDelay,
                Self::HoldSuccessRetentionQuality,
                Self::AnonymityCorrelationProxies,
                Self::HomeMembershipInferencePrecisionRecall,
                Self::DirectFriendEdgeInferencePrecisionRecall,
                Self::IntroductionProvenanceInferencePrecisionRecall,
                Self::ReplyTimingCorrelationPrecisionRecall,
                Self::PartialPathCompromiseLinkagePrecisionRecall,
            ]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::types::{
        AdaptivePrivacyMetric, AdaptivePrivacyValidationProfile, AdaptiveTopologyKind,
        BootstrapObserverInferenceTarget, BootstrapObserverScenario, BoundaryScenario,
        BoundaryScenarioFocus, ControlPlaneFailureMode, HoldValidationProfile,
        MetadataMinimizationFocus, MetadataMinimizationScenario, ObserverInferenceTarget,
        ObserverModelScenario, OrganicTrafficProfile, ReachableSetSize,
        SecurityControlTrafficClass, StarvationScenario, SyncOpportunityProfile,
        TelltaleControlPlaneScenario,
    };

    #[test]
    fn phase_six_validation_matrix_covers_required_dimensions() {
        let matrix = AdaptivePrivacyValidationProfile::phase_six_matrix();
        assert!(matrix
            .iter()
            .any(|profile| profile.reachable_set_size == ReachableSetSize::Small));
        assert!(matrix
            .iter()
            .any(|profile| profile.reachable_set_size == ReachableSetSize::Medium));
        assert!(matrix
            .iter()
            .any(|profile| profile.reachable_set_size == ReachableSetSize::Large));
        assert!(matrix
            .iter()
            .any(|profile| profile.topology == AdaptiveTopologyKind::ClusteredHumanSocial));
        assert!(matrix
            .iter()
            .any(|profile| profile.topology == AdaptiveTopologyKind::ClusteredPartitioned));
        assert!(matrix.iter().any(|profile| profile.partition_heal_cycles));
        assert!(matrix.iter().any(|profile| profile.provider_saturation));
        assert!(matrix.iter().any(|profile| profile.churn_spikes));
        assert!(matrix
            .iter()
            .any(|profile| profile.organic_traffic == OrganicTrafficProfile::LowOrganicHighCover));
        assert!(matrix
            .iter()
            .any(|profile| profile.sync_opportunities == SyncOpportunityProfile::Sparse));
        assert!(matrix
            .iter()
            .any(|profile| profile.sync_opportunities == SyncOpportunityProfile::Heavy));
        assert!(matrix.iter().any(|profile| profile.hold_profile
            == HoldValidationProfile::DeferredDeliveryWeakConnectivity));
        assert!(matrix.iter().any(|profile| profile.hold_profile
            == HoldValidationProfile::DistributedCacheSeedingRecovery));
        assert!(matrix
            .iter()
            .any(|profile| profile.organic_traffic == OrganicTrafficProfile::CeremonyLatencyBound));
    }

    #[test]
    fn phase_six_observer_and_starvation_profiles_cover_required_targets() {
        let observer_profiles = ObserverModelScenario::phase_six_profiles();
        assert!(observer_profiles
            .iter()
            .any(|profile| profile.target == ObserverInferenceTarget::HomeMembership));
        assert!(observer_profiles
            .iter()
            .any(|profile| profile.target == ObserverInferenceTarget::DirectFriendEdge));
        assert!(observer_profiles
            .iter()
            .any(|profile| profile.target == ObserverInferenceTarget::IntroductionProvenance));
        assert!(
            observer_profiles
                .iter()
                .any(|profile| profile.target
                    == ObserverInferenceTarget::PartialPathCompromiseLinkage)
        );

        let starvation_profiles = StarvationScenario::phase_six_profiles();
        assert!(starvation_profiles.iter().any(|profile| {
            profile.protected_class == SecurityControlTrafficClass::AnonymousPathEstablishment
        }));
        assert!(starvation_profiles.iter().any(|profile| {
            profile.protected_class == SecurityControlTrafficClass::CapabilityTrustUpdates
        }));
        assert!(starvation_profiles
            .iter()
            .any(|profile| profile.protected_class
                == SecurityControlTrafficClass::AccountabilityReplies));
        assert!(starvation_profiles.iter().any(|profile| {
            profile.protected_class == SecurityControlTrafficClass::RetrievalCapabilityRotation
        }));

        let bootstrap_observer_profiles = BootstrapObserverScenario::phase_six_profiles();
        assert!(bootstrap_observer_profiles.iter().any(|profile| {
            profile.target
                == BootstrapObserverInferenceTarget::NeighborhoodAdjacencyFromBoardContents
        }));
        assert!(bootstrap_observer_profiles.iter().any(|profile| {
            profile.target
                == BootstrapObserverInferenceTarget::BridgeAuthorityCentralityFromRepeatedReentry
        }));
        assert!(bootstrap_observer_profiles.iter().any(|profile| {
            profile.target
                == BootstrapObserverInferenceTarget::FofProvenanceFromBootstrapHintSelection
        }));
        assert!(bootstrap_observer_profiles.iter().any(|profile| {
            profile.target == BootstrapObserverInferenceTarget::StaleNodeIdentityFromWidenedReentry
        }));
    }

    #[test]
    fn phase_six_metadata_control_plane_boundary_and_metric_profiles_are_complete() {
        let metadata_profiles = MetadataMinimizationScenario::phase_six_profiles();
        assert!(metadata_profiles
            .iter()
            .any(|profile| profile.focus
                == MetadataMinimizationFocus::RetrievalNotRecipientAddressed));
        assert!(metadata_profiles
            .iter()
            .any(|profile| profile.focus == MetadataMinimizationFocus::DeliveryNotMailboxShaped));

        let control_plane_profiles = TelltaleControlPlaneScenario::phase_six_profiles();
        assert!(
            control_plane_profiles
                .iter()
                .any(|profile| profile.failure_mode
                    == ControlPlaneFailureMode::TimeoutAndCancellation)
        );
        assert!(control_plane_profiles
            .iter()
            .any(|profile| profile.failure_mode
                == ControlPlaneFailureMode::WitnessReturnSuccessFailure));
        assert!(
            control_plane_profiles
                .iter()
                .any(|profile| profile.failure_mode
                    == ControlPlaneFailureMode::StaleOwnerAfterHandoff)
        );
        assert!(control_plane_profiles
            .iter()
            .any(|profile| profile.failure_mode == ControlPlaneFailureMode::ReplayVisibleFailure));

        let boundary_profiles = BoundaryScenario::phase_six_profiles();
        assert!(boundary_profiles
            .iter()
            .any(|profile| profile.focus == BoundaryScenarioFocus::ControllerKnee));
        assert!(boundary_profiles
            .iter()
            .any(|profile| profile.focus == BoundaryScenarioFocus::RotationThreshold));

        let metrics = AdaptivePrivacyMetric::phase_six_inventory();
        assert!(metrics.contains(&AdaptivePrivacyMetric::ControllerConvergence));
        assert!(metrics.contains(&AdaptivePrivacyMetric::SecurityControlTrafficLatencyUnderLoad));
        assert!(metrics.contains(&AdaptivePrivacyMetric::HomeMembershipInferencePrecisionRecall));
        assert!(metrics.contains(&AdaptivePrivacyMetric::ReplyTimingCorrelationPrecisionRecall));
        assert!(
            metrics.contains(&AdaptivePrivacyMetric::PartialPathCompromiseLinkagePrecisionRecall)
        );
        assert_eq!(metrics.len(), 17);
    }
}
