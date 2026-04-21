//! Aura-owned experiment surfaces layered on Telltale simulator primitives.

use crate::scenario::types::{
    AdaptivePrivacyValidationProfile, SyncOpportunityProfile, TelltaleControlPlaneScenario,
};
use crate::telltale_parity::{TelltaleControlPlaneLane, TelltaleParityReportV1};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use telltale_machine::coroutine::Value;
use telltale_machine::model::effects::{
    EffectFailure, EffectResult, SendDecision, SendDecisionInput,
};
use telltale_simulator::decision::{
    decide_theorem_eligibility, theorem_eligibility_from_result, DecisionCounterexample,
    DecisionKind, DecisionOutcome,
};
use telltale_simulator::harness::{BatchConfig, DirectAdapter, HarnessSpec, SimulationHarness};
use telltale_simulator::scenario::Scenario;
use telltale_simulator::sweep::{
    compare_sweep_results, SweepAxis, SweepBinding, SweepConfig, SweepDiffReport, SweepManifest,
    SweepManifestEntry, SweepRunResult,
};
use telltale_simulator::EffectHandler;
use telltale_types::{GlobalType, Label, LocalTypeR};

/// Schema identifier for sweep archive artifacts.
pub const AURA_SWEEP_ARCHIVE_SCHEMA_V1: &str = "aura.simulator.sweep-archive.v1";
/// Schema identifier for policy diff artifacts.
pub const AURA_POLICY_DIFF_REPORT_SCHEMA_V1: &str = "aura.simulator.policy-diff.v1";
/// Schema identifier for theorem-aware counterexample artifacts.
pub const AURA_COUNTEREXAMPLE_REPORT_SCHEMA_V1: &str = "aura.simulator.counterexample.v1";
/// Schema identifier for suite tournament artifacts.
pub const AURA_SUITE_TOURNAMENT_REPORT_SCHEMA_V1: &str = "aura.simulator.suite-tournament.v1";

/// Aura policy families currently compared in experiment lanes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuraPolicyPreset {
    ProvisionalLegacy,
    PhaseSixTuned,
}

impl AuraPolicyPreset {
    fn as_str(self) -> &'static str {
        match self {
            Self::ProvisionalLegacy => "provisional_legacy",
            Self::PhaseSixTuned => "phase_six_tuned",
        }
    }

    fn theorem_scheduler_profile(self) -> &'static str {
        match self {
            Self::ProvisionalLegacy => "threaded_envelope",
            Self::PhaseSixTuned => "canonical_exact",
        }
    }

    fn theorem_envelope_profile(self) -> &'static str {
        match self {
            Self::ProvisionalLegacy => "protocol_machine_envelope_adherence",
            Self::PhaseSixTuned => "exact",
        }
    }

    fn theorem_assumption_bundle(self) -> &'static str {
        match self {
            Self::ProvisionalLegacy => "partial_synchrony",
            Self::PhaseSixTuned => "fault_free_transport",
        }
    }
}

/// Aura-owned suite catalogs built on the shared sweep/batch execution machinery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuraExperimentSuiteCatalog {
    AdaptivePrivacyCore,
    ControlPlaneCore,
}

impl AuraExperimentSuiteCatalog {
    fn as_str(self) -> &'static str {
        match self {
            Self::AdaptivePrivacyCore => "adaptive_privacy_core",
            Self::ControlPlaneCore => "control_plane_core",
        }
    }
}

/// Request for an adaptive-privacy policy sweep.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuraAdaptivePrivacySweepRequest {
    pub policy: AuraPolicyPreset,
    pub profiles: Vec<AdaptivePrivacyValidationProfile>,
    pub seeds: Vec<u64>,
    pub adversary_budgets: Vec<u64>,
}

impl AuraAdaptivePrivacySweepRequest {
    /// Canonical phase-six sweep request.
    #[must_use]
    pub fn phase_six(policy: AuraPolicyPreset) -> Self {
        Self {
            policy,
            profiles: AdaptivePrivacyValidationProfile::phase_six_matrix(),
            seeds: vec![7, 19],
            adversary_budgets: vec![0, 1],
        }
    }
}

/// Stable Aura wrapper around a shared sweep manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuraSweepArchiveV1 {
    pub schema_version: String,
    pub policy: AuraPolicyPreset,
    pub manifest: SweepManifest,
}

/// Policy-comparison artifact derived from shared sweep outputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuraPolicyDiffReportV1 {
    pub schema_version: String,
    pub baseline_policy: AuraPolicyPreset,
    pub candidate_policy: AuraPolicyPreset,
    pub baseline_manifest: SweepManifest,
    pub candidate_manifest: SweepManifest,
    pub diff: SweepDiffReport,
}

/// Structured counterexample report for one parity/control-plane lane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuraCounterexampleReportV1 {
    pub schema_version: String,
    pub lane: String,
    pub relation: crate::telltale_parity::TelltaleParitySemanticRelationV1,
    pub normalized_observability_match: bool,
    pub schedule_noise_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_kind: Option<DecisionKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_counterexample: Option<DecisionCounterexample>,
}

/// Structured semantic regression entry derived from suite comparisons.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuraSemanticRegressionV1 {
    pub input_index: u64,
    pub theorem_eligibility_changed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub productive_step_delta: Option<i64>,
    pub assumption_diagnostics_changed: bool,
}

/// Baseline-vs-candidate tournament over one Aura suite catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuraSuiteTournamentReportV1 {
    pub schema_version: String,
    pub catalog: AuraExperimentSuiteCatalog,
    pub baseline_policy: AuraPolicyPreset,
    pub candidate_policy: AuraPolicyPreset,
    pub baseline_manifest: SweepManifest,
    pub candidate_manifest: SweepManifest,
    pub diff: SweepDiffReport,
    pub semantic_regressions: Vec<AuraSemanticRegressionV1>,
}

/// Experiment-surface errors.
#[derive(Debug, thiserror::Error)]
pub enum AuraExperimentError {
    #[error("experiment execution failed: {0}")]
    Run(String),
    #[error("failed serializing experiment artifact: {message}")]
    Serialize { message: String },
    #[error("failed writing experiment artifact to {path}: {message}")]
    WriteArtifact { path: String, message: String },
}

#[derive(Debug, Clone, Copy)]
struct PassthroughHandler;

impl EffectHandler for PassthroughHandler {
    fn handle_send(
        &self,
        _role: &str,
        _partner: &str,
        label: &str,
        _state: &[Value],
    ) -> EffectResult<Value> {
        EffectResult::success(Value::Str(label.to_string()))
    }

    fn send_decision(&self, input: SendDecisionInput<'_>) -> EffectResult<SendDecision> {
        EffectResult::success(SendDecision::Deliver(input.payload.unwrap_or(Value::Unit)))
    }

    fn handle_recv(
        &self,
        _role: &str,
        _partner: &str,
        _label: &str,
        _state: &mut Vec<Value>,
        _payload: &Value,
    ) -> EffectResult<()> {
        EffectResult::success(())
    }

    fn handle_choose(
        &self,
        _role: &str,
        _partner: &str,
        labels: &[String],
        _state: &[Value],
    ) -> EffectResult<String> {
        match labels.first().cloned() {
            Some(label) => EffectResult::success(label),
            None => EffectResult::failure(EffectFailure::invalid_input("no labels available")),
        }
    }

    fn step(&self, _role: &str, _state: &mut Vec<Value>) -> EffectResult<()> {
        EffectResult::success(())
    }
}

/// Execute one adaptive-privacy policy sweep and return the shared sweep output.
pub fn run_adaptive_privacy_policy_sweep(
    request: &AuraAdaptivePrivacySweepRequest,
) -> Result<SweepRunResult, AuraExperimentError> {
    let handler = PassthroughHandler;
    let adapter = DirectAdapter::new(&handler);
    let harness = SimulationHarness::new(&adapter);

    let mut manifest_runs = Vec::new();
    let mut results = Vec::new();

    for profile in &request.profiles {
        let base = policy_harness_spec(
            &format!("{}-{}", request.policy.as_str(), profile.id),
            request.policy,
        )?;
        let sweep = harness
            .run_sweep(
                &base,
                &SweepConfig {
                    batch: BatchConfig {
                        parallelism: Some(1),
                    },
                    axes: vec![
                        SweepAxis::Seed {
                            values: request.seeds.clone(),
                        },
                        SweepAxis::AdversaryBudget {
                            adversary_id: "budgeted".to_string(),
                            totals: request.adversary_budgets.clone(),
                        },
                    ],
                },
            )
            .map_err(AuraExperimentError::Run)?;

        for mut entry in sweep.manifest.runs {
            entry.bindings.extend(profile_bindings(profile));
            manifest_runs.push(entry);
        }
        results.extend(sweep.results);
    }

    Ok(SweepRunResult {
        parallelism: 1,
        manifest: SweepManifest {
            parallelism: 1,
            runs: manifest_runs,
        },
        results,
    })
}

/// Convert a sweep run into a stable Aura archive artifact.
#[must_use]
pub fn archive_from_sweep(policy: AuraPolicyPreset, sweep: &SweepRunResult) -> AuraSweepArchiveV1 {
    AuraSweepArchiveV1 {
        schema_version: AURA_SWEEP_ARCHIVE_SCHEMA_V1.to_string(),
        policy,
        manifest: sweep.manifest.clone(),
    }
}

/// Compare two policy sweeps using the shared Telltale diff algorithm.
#[must_use]
pub fn compare_policy_sweeps(
    baseline_policy: AuraPolicyPreset,
    baseline: &SweepRunResult,
    candidate_policy: AuraPolicyPreset,
    candidate: &SweepRunResult,
) -> AuraPolicyDiffReportV1 {
    AuraPolicyDiffReportV1 {
        schema_version: AURA_POLICY_DIFF_REPORT_SCHEMA_V1.to_string(),
        baseline_policy,
        candidate_policy,
        baseline_manifest: baseline.manifest.clone(),
        candidate_manifest: candidate.manifest.clone(),
        diff: compare_sweep_results(baseline, candidate),
    }
}

/// Build one theorem-aware counterexample report from a parity report.
#[must_use]
pub fn counterexample_from_parity_report(
    report: &TelltaleParityReportV1,
) -> Option<AuraCounterexampleReportV1> {
    let decision = report
        .upstream
        .telltale_decision_report
        .as_ref()
        .or(report.upstream.baseline_decision_report.as_ref())?;
    let DecisionOutcome::Counterexample(counterexample) = &decision.outcome else {
        return None;
    };

    Some(AuraCounterexampleReportV1 {
        schema_version: AURA_COUNTEREXAMPLE_REPORT_SCHEMA_V1.to_string(),
        lane: report.lane.clone(),
        relation: report.semantic_summary.relation,
        normalized_observability_match: report.semantic_summary.normalized_observability_match,
        schedule_noise_only: !report.semantic_summary.normalized_observability_match
            && report.semantic_summary.relation
                != crate::telltale_parity::TelltaleParitySemanticRelationV1::SafetyVisibleDivergence,
        decision_kind: Some(decision.kind),
        decision_counterexample: Some(counterexample.clone()),
    })
}

/// Convenience wrapper for control-plane lanes.
#[must_use]
pub fn counterexample_from_control_plane_report(
    lane: TelltaleControlPlaneLane,
    report: &TelltaleParityReportV1,
) -> Option<AuraCounterexampleReportV1> {
    let mut counterexample = counterexample_from_parity_report(report)?;
    counterexample.lane = format!("{}:{}", counterexample.lane, lane_suffix(lane));
    Some(counterexample)
}

/// Run one Aura suite catalog using the shared sweep manifest format.
pub fn run_suite_catalog(
    catalog: AuraExperimentSuiteCatalog,
    policy: AuraPolicyPreset,
) -> Result<SweepRunResult, AuraExperimentError> {
    let handler = PassthroughHandler;
    let adapter = DirectAdapter::new(&handler);
    let harness = SimulationHarness::new(&adapter);

    let suite_specs = suite_specs(catalog, policy)?;
    let specs = suite_specs
        .iter()
        .map(|spec| spec.harness_spec.clone())
        .collect::<Vec<_>>();
    let batch = harness.run_batch_with(
        &specs,
        BatchConfig {
            parallelism: Some(1),
        },
    );
    let manifest = SweepManifest {
        parallelism: batch.parallelism,
        runs: suite_specs
            .iter()
            .zip(batch.results.iter())
            .enumerate()
            .map(|(idx, (suite, result))| SweepManifestEntry {
                input_index: idx,
                scenario_name: suite.harness_spec.scenario.name.clone(),
                bindings: suite.bindings.clone(),
                execution_regime: result
                    .as_ref()
                    .ok()
                    .map(|run| run.stats.execution_regime)
                    .or_else(|| {
                        suite
                            .harness_spec
                            .scenario
                            .resolved_execution()
                            .ok()
                            .map(|execution| execution.regime())
                    }),
                theorem_profile: result
                    .as_ref()
                    .ok()
                    .map(|run| run.stats.theorem_profile.clone()),
                scheduler_profile: result
                    .as_ref()
                    .ok()
                    .map(|run| run.stats.scheduler_profile.clone()),
                theorem_eligibility: match result {
                    Ok(run) => theorem_eligibility_from_result(run),
                    Err(_) => decide_theorem_eligibility(&suite.harness_spec.scenario),
                },
                capacity_report: None,
                result_error: result.as_ref().err().cloned(),
            })
            .collect(),
    };

    Ok(SweepRunResult {
        parallelism: batch.parallelism,
        manifest,
        results: batch.results,
    })
}

/// Compare two suite runs and collect semantic regressions above the threshold.
#[must_use]
pub fn compare_suite_catalogs(
    catalog: AuraExperimentSuiteCatalog,
    baseline_policy: AuraPolicyPreset,
    baseline: &SweepRunResult,
    candidate_policy: AuraPolicyPreset,
    candidate: &SweepRunResult,
    productive_step_threshold: i64,
) -> AuraSuiteTournamentReportV1 {
    let diff = compare_sweep_results(baseline, candidate);
    let semantic_regressions = diff
        .differing_runs
        .iter()
        .filter(|run| {
            run.theorem_eligibility_changed
                || run.assumption_diagnostics_changed
                || run
                    .productive_step_delta
                    .is_some_and(|delta| delta.abs() >= productive_step_threshold)
        })
        .map(|run| AuraSemanticRegressionV1 {
            input_index: run.input_index as u64,
            theorem_eligibility_changed: run.theorem_eligibility_changed,
            productive_step_delta: run.productive_step_delta,
            assumption_diagnostics_changed: run.assumption_diagnostics_changed,
        })
        .collect();

    AuraSuiteTournamentReportV1 {
        schema_version: AURA_SUITE_TOURNAMENT_REPORT_SCHEMA_V1.to_string(),
        catalog,
        baseline_policy,
        candidate_policy,
        baseline_manifest: baseline.manifest.clone(),
        candidate_manifest: candidate.manifest.clone(),
        diff,
        semantic_regressions,
    }
}

/// Write one serializable experiment artifact to disk.
pub fn write_experiment_artifact<T>(path: &Path, artifact: &T) -> Result<(), AuraExperimentError>
where
    T: Serialize,
{
    let payload =
        serde_json::to_vec_pretty(artifact).map_err(|error| AuraExperimentError::Serialize {
            message: error.to_string(),
        })?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| AuraExperimentError::WriteArtifact {
            path: path.display().to_string(),
            message: error.to_string(),
        })?;
    }
    std::fs::write(path, payload).map_err(|error| AuraExperimentError::WriteArtifact {
        path: path.display().to_string(),
        message: error.to_string(),
    })
}

fn lane_suffix(lane: TelltaleControlPlaneLane) -> &'static str {
    match lane {
        TelltaleControlPlaneLane::AnonymousPathEstablish => "anonymous_path_establish",
        TelltaleControlPlaneLane::ReplyBlockAccountability => "reply_block_accountability",
    }
}

fn profile_bindings(profile: &AdaptivePrivacyValidationProfile) -> Vec<SweepBinding> {
    vec![
        SweepBinding {
            axis: "mobility".to_string(),
            value: format!("{:?}", profile.topology).to_lowercase(),
        },
        SweepBinding {
            axis: "sync_density".to_string(),
            value: match profile.sync_opportunities {
                SyncOpportunityProfile::Sparse => "sparse".to_string(),
                SyncOpportunityProfile::Heavy => "heavy".to_string(),
            },
        },
        SweepBinding {
            axis: "provider_saturation".to_string(),
            value: profile.provider_saturation.to_string(),
        },
    ]
}

fn policy_harness_spec(
    name: &str,
    policy: AuraPolicyPreset,
) -> Result<HarnessSpec, AuraExperimentError> {
    let (global_type, local_types) = simple_protocol();
    let scenario_toml = format!(
        r#"
name = "{name}"
roles = ["A", "B"]
steps = 6
seed = 7

[execution]
backend = "canonical"
scheduler_concurrency = 1
worker_threads = 1

[theorem]
scheduler_profile = "{scheduler_profile}"
envelope_profile = "{envelope_profile}"
assumption_bundle = "{assumption_bundle}"

[field]
layer = "mean_field"

[field.params]
beta = "1.0"
species = ["up", "down"]
initial_state = ["0.5", "0.5"]
step_size = "0.01"

[[adversaries]]
id = "budgeted"
trigger = {{ immediate = true }}
action = {{ type = "withholding" }}
budget = {{ total = 1, mode = "activation", assumption_failure = "fairness_failure" }}
"#,
        scheduler_profile = policy.theorem_scheduler_profile(),
        envelope_profile = policy.theorem_envelope_profile(),
        assumption_bundle = policy.theorem_assumption_bundle(),
    );
    let scenario = Scenario::parse(&scenario_toml).map_err(AuraExperimentError::Run)?;
    Ok(HarnessSpec::new(local_types, global_type, scenario))
}

fn simple_protocol() -> (GlobalType, BTreeMap<String, LocalTypeR>) {
    let global = GlobalType::mu(
        "loop",
        GlobalType::send(
            "A",
            "B",
            Label::new("msg"),
            GlobalType::send("B", "A", Label::new("ack"), GlobalType::var("loop")),
        ),
    );

    let mut local_types = BTreeMap::new();
    local_types.insert(
        "A".to_string(),
        LocalTypeR::mu(
            "loop",
            LocalTypeR::Send {
                partner: "B".into(),
                branches: vec![(
                    Label::new("msg"),
                    None,
                    LocalTypeR::Recv {
                        partner: "B".into(),
                        branches: vec![(Label::new("ack"), None, LocalTypeR::var("loop"))],
                    },
                )],
            },
        ),
    );
    local_types.insert(
        "B".to_string(),
        LocalTypeR::mu(
            "loop",
            LocalTypeR::Recv {
                partner: "A".into(),
                branches: vec![(
                    Label::new("msg"),
                    None,
                    LocalTypeR::Send {
                        partner: "A".into(),
                        branches: vec![(Label::new("ack"), None, LocalTypeR::var("loop"))],
                    },
                )],
            },
        ),
    );

    (global, local_types)
}

#[derive(Clone)]
struct SuiteSpec {
    harness_spec: HarnessSpec,
    bindings: Vec<SweepBinding>,
}

fn suite_specs(
    catalog: AuraExperimentSuiteCatalog,
    policy: AuraPolicyPreset,
) -> Result<Vec<SuiteSpec>, AuraExperimentError> {
    match catalog {
        AuraExperimentSuiteCatalog::AdaptivePrivacyCore => {
            AdaptivePrivacyValidationProfile::phase_six_matrix()
                .into_iter()
                .map(|profile| {
                    let spec = policy_harness_spec(
                        &format!("suite-{}-{}", policy.as_str(), profile.id),
                        policy,
                    )?;
                    Ok(SuiteSpec {
                        harness_spec: spec,
                        bindings: {
                            let mut bindings = profile_bindings(&profile);
                            bindings.push(SweepBinding {
                                axis: "suite_catalog".to_string(),
                                value: catalog.as_str().to_string(),
                            });
                            bindings.push(SweepBinding {
                                axis: "suite_entry".to_string(),
                                value: profile.id,
                            });
                            bindings
                        },
                    })
                })
                .collect()
        }
        AuraExperimentSuiteCatalog::ControlPlaneCore => {
            TelltaleControlPlaneScenario::phase_six_profiles()
                .into_iter()
                .map(|scenario| {
                    let spec = policy_harness_spec(
                        &format!("suite-{}-{}", policy.as_str(), scenario.id),
                        policy,
                    )?;
                    Ok(SuiteSpec {
                        harness_spec: spec,
                        bindings: vec![
                            SweepBinding {
                                axis: "suite_catalog".to_string(),
                                value: catalog.as_str().to_string(),
                            },
                            SweepBinding {
                                axis: "suite_entry".to_string(),
                                value: scenario.id,
                            },
                            SweepBinding {
                                axis: "control_plane_lane".to_string(),
                                value: lane_suffix(scenario.lane).to_string(),
                            },
                        ],
                    })
                })
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telltale_parity::{
        TelltaleParitySemanticRelationV1, TelltaleUpstreamComparisonV1, TelltaleUpstreamReportV1,
    };
    use telltale_simulator::decision::{
        DecisionCertificate, DecisionKind, DecisionOutcome, DecisionReport,
        TheoremEligibilityCounterexample,
    };
    use telltale_simulator::environment::EnvironmentTrace;
    use telltale_simulator::reconfiguration::ReconfigurationSummary;
    use telltale_simulator::runner::{
        CriticalCapacityPhase, CriticalCapacitySummary, SchedulerBoundMode,
        SchedulerEnvelopeStatus, SchedulerFairnessRequirement, SchedulerProfileSummary,
        TheoremProgressSummary,
    };
    use telltale_simulator::scenario::{
        ExecutionRegime, ResolvedExecutionBackend, ResolvedSchedulerPolicy, ResolvedTheoremProfile,
        TheoremAssumptionBundle, TheoremEligibility, TheoremEnvelopeProfile,
        TheoremSchedulerProfile,
    };
    use tempfile::tempdir;

    fn sample_run_summary(
        profile: TheoremSchedulerProfile,
    ) -> crate::telltale_parity::TelltaleRunSummaryV1 {
        crate::telltale_parity::TelltaleRunSummaryV1 {
            execution_regime: ExecutionRegime::CanonicalExact,
            backend: ResolvedExecutionBackend::Canonical,
            theorem_profile: ResolvedTheoremProfile {
                scheduler_profile: profile,
                envelope_profile: TheoremEnvelopeProfile::Exact,
                assumption_bundle: TheoremAssumptionBundle::FaultFreeTransport,
                eligibility: if profile == TheoremSchedulerProfile::CanonicalExact {
                    TheoremEligibility::Exact
                } else {
                    TheoremEligibility::EnvelopeBounded
                },
                eligibility_reason: None,
            },
            theorem_progress: TheoremProgressSummary {
                initial_weighted_measure: 8,
                initial_depth_budget: 4,
                productive_step_count: 3,
                remaining_weighted_measure: 2,
                weighted_measure_consumed: 6,
                critical_capacity: CriticalCapacitySummary {
                    threshold: Some(4),
                    phase: CriticalCapacityPhase::BelowThreshold,
                },
            },
            scheduler_profile: SchedulerProfileSummary {
                implementation_policy: ResolvedSchedulerPolicy::Cooperative,
                theorem_profile: profile,
                productive_exactness: profile == TheoremSchedulerProfile::CanonicalExact,
                total_step_mode: SchedulerBoundMode::ProductiveExactOnly,
                total_step_upper_bound: None,
                fairness_requirement: SchedulerFairnessRequirement::ExplicitYieldFairness,
                envelope_status: SchedulerEnvelopeStatus::Exact,
            },
            reconfiguration_summary: ReconfigurationSummary {
                applied_operations: 1,
                pure_operations: 1,
                transition_operations: 0,
                transition_budget_consumed: 0,
            },
            environment_trace: EnvironmentTrace::default(),
            assumption_diagnostics: Vec::new(),
            scheduler_concurrency: 1,
            worker_threads: 1,
            normalized_observability: telltale_simulator::analysis::NormalizedObservability {
                schema_version: 1,
                raw_observable_event_count: 1,
                raw_reconfiguration_count: 0,
                normalized_event_class: vec!["sent:x".to_string()],
                normalized_reconfiguration_class: Vec::new(),
            },
        }
    }

    #[test]
    fn adaptive_privacy_sweeps_are_deterministic() {
        let request = AuraAdaptivePrivacySweepRequest::phase_six(AuraPolicyPreset::PhaseSixTuned);
        let first = run_adaptive_privacy_policy_sweep(&request).expect("first sweep");
        let second = run_adaptive_privacy_policy_sweep(&request).expect("second sweep");

        assert_eq!(first.manifest, second.manifest);
        assert_eq!(first.results.len(), second.results.len());
    }

    #[test]
    fn policy_diff_reports_use_shared_sweep_diff() {
        let baseline = run_adaptive_privacy_policy_sweep(
            &AuraAdaptivePrivacySweepRequest::phase_six(AuraPolicyPreset::ProvisionalLegacy),
        )
        .expect("baseline");
        let candidate = run_adaptive_privacy_policy_sweep(
            &AuraAdaptivePrivacySweepRequest::phase_six(AuraPolicyPreset::PhaseSixTuned),
        )
        .expect("candidate");
        let diff = compare_policy_sweeps(
            AuraPolicyPreset::ProvisionalLegacy,
            &baseline,
            AuraPolicyPreset::PhaseSixTuned,
            &candidate,
        );

        assert!(!diff.diff.differing_runs.is_empty());
        let dir = tempdir().expect("tempdir");
        write_experiment_artifact(&dir.path().join("policy-diff.json"), &diff).expect("write diff");
    }

    #[test]
    fn counterexample_reports_preserve_telltale_witnesses() {
        let report = TelltaleParityReportV1 {
            schema_version: "schema".to_string(),
            lane: "aura-simulator:telltale-control-plane:reply_block_accountability".to_string(),
            comparison_classification: "envelope_bounded".to_string(),
            first_mismatch_surface: None,
            first_mismatch_step_index: None,
            differential: crate::differential_tester::DifferentialReport {
                profile: crate::differential_tester::DifferentialProfile::EnvelopeBounded,
                equivalent: false,
                mismatch: None,
                baseline_target: "aura".to_string(),
                candidate_target: "telltale".to_string(),
                scenario: "lane".to_string(),
            },
            semantic_summary: crate::telltale_parity::TelltaleParitySemanticSummaryV1 {
                relation: TelltaleParitySemanticRelationV1::SafetyVisibleDivergence,
                execution_regime_match: true,
                theorem_profile_match: false,
                scheduler_profile_match: false,
                normalized_observability_match: true,
            },
            upstream: TelltaleUpstreamReportV1 {
                baseline_run: sample_run_summary(TheoremSchedulerProfile::CanonicalExact),
                telltale_run: sample_run_summary(TheoremSchedulerProfile::ThreadedEnvelope),
                baseline_decision_report: Some(DecisionReport {
                    kind: DecisionKind::TheoremEligibility,
                    outcome: DecisionOutcome::Certified(DecisionCertificate::TheoremEligibility {
                        eligibility: TheoremEligibility::Exact,
                    }),
                }),
                telltale_decision_report: Some(DecisionReport {
                    kind: DecisionKind::TheoremEligibility,
                    outcome: DecisionOutcome::Counterexample(
                        DecisionCounterexample::TheoremEligibility {
                            cause: TheoremEligibilityCounterexample::Ineligible {
                                reason: "envelope mismatch".to_string(),
                            },
                        },
                    ),
                }),
                baseline_sweep_manifest: None,
                telltale_sweep_manifest: None,
                comparison: TelltaleUpstreamComparisonV1 {
                    execution_regime_match: true,
                    theorem_profile_match: false,
                    scheduler_profile_match: false,
                    normalized_observability_match: true,
                    decision_report_match: Some(false),
                    sweep_run_count_match: None,
                },
            },
        };

        let counterexample = counterexample_from_control_plane_report(
            TelltaleControlPlaneLane::ReplyBlockAccountability,
            &report,
        )
        .expect("counterexample");

        assert_eq!(
            counterexample.decision_kind,
            Some(DecisionKind::TheoremEligibility)
        );
        assert!(!counterexample.schedule_noise_only);
        assert!(counterexample.lane.contains("reply_block_accountability"));
    }

    #[test]
    fn suite_catalog_tournaments_produce_semantic_regressions() {
        let baseline = run_suite_catalog(
            AuraExperimentSuiteCatalog::AdaptivePrivacyCore,
            AuraPolicyPreset::ProvisionalLegacy,
        )
        .expect("baseline suite");
        let candidate = run_suite_catalog(
            AuraExperimentSuiteCatalog::AdaptivePrivacyCore,
            AuraPolicyPreset::PhaseSixTuned,
        )
        .expect("candidate suite");

        let report = compare_suite_catalogs(
            AuraExperimentSuiteCatalog::AdaptivePrivacyCore,
            AuraPolicyPreset::ProvisionalLegacy,
            &baseline,
            AuraPolicyPreset::PhaseSixTuned,
            &candidate,
            0,
        );

        assert!(!report.semantic_regressions.is_empty());
        let dir = tempdir().expect("tempdir");
        write_experiment_artifact(&dir.path().join("suite-tournament.json"), &report)
            .expect("write suite report");
    }

    #[test]
    fn suite_catalog_manifests_are_stable_under_replay() {
        let first = run_suite_catalog(
            AuraExperimentSuiteCatalog::ControlPlaneCore,
            AuraPolicyPreset::PhaseSixTuned,
        )
        .expect("first suite");
        let second = run_suite_catalog(
            AuraExperimentSuiteCatalog::ControlPlaneCore,
            AuraPolicyPreset::PhaseSixTuned,
        )
        .expect("second suite");

        assert_eq!(first.manifest, second.manifest);
    }
}
