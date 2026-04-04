//! Telltale protocol-machine parity boundary for aura-simulator.
//!
//! This module defines a crate-local integration boundary.
//! It avoids direct protocol-machine execution coupling in the default simulator path.
//! Callers provide conformance artifacts, while supported file/control-plane
//! lanes require upstream Telltale 11 run sidecars for theorem-facing context.

use crate::differential_tester::{DifferentialProfile, DifferentialReport, DifferentialTester};
use aura_core::{AuraConformanceArtifactV1, ConformanceSurfaceName};
use aura_testkit::load_conformance_artifact_file;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use telltale_simulator::analysis::NormalizedObservability;
use telltale_simulator::decision::DecisionReport as TelltaleDecisionReport;
use telltale_simulator::environment::EnvironmentTrace;
use telltale_simulator::fault::AssumptionDiagnostic;
use telltale_simulator::reconfiguration::ReconfigurationSummary;
use telltale_simulator::runner::{SchedulerProfileSummary, TheoremProgressSummary};
use telltale_simulator::scenario::{
    ExecutionRegime, ResolvedExecutionBackend, ResolvedTheoremProfile,
};
use telltale_simulator::sweep::SweepManifest;

/// Schema identifier for simulator parity reports.
pub const AURA_TELLTALE_PARITY_REPORT_SCHEMA_V1: &str = "aura.telltale-parity.report.v1";
/// Environment variable that overrides the upstream simulator runner command.
pub const AURA_TELLTALE_SIMULATOR_RUNNER_ENV: &str = "AURA_TELLTALE_SIMULATOR_RUNNER";

/// Aura-owned command surface for invoking the upstream Telltale simulator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelltaleSimulatorCommandV1 {
    /// Executable to invoke.
    pub program: PathBuf,
    /// Static extra arguments inserted before the run request arguments.
    pub extra_args: Vec<String>,
}

impl TelltaleSimulatorCommandV1 {
    /// Resolve the default command from Aura configuration.
    #[must_use]
    pub fn from_environment() -> Self {
        let program = std::env::var_os(AURA_TELLTALE_SIMULATOR_RUNNER_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("telltale-simulator-run"));
        Self {
            program,
            extra_args: Vec::new(),
        }
    }

    /// Build argv for one upstream simulator run request.
    #[must_use]
    pub fn args_for_run(&self, request: &TelltaleSimulatorRunRequest) -> Vec<String> {
        let mut args = self.extra_args.clone();
        args.push("--config".to_string());
        args.push(request.config_path.display().to_string());
        args.push("--output".to_string());
        args.push(request.output_path.display().to_string());
        if request.pretty {
            args.push("--pretty".to_string());
        }
        args
    }
}

impl Default for TelltaleSimulatorCommandV1 {
    fn default() -> Self {
        Self::from_environment()
    }
}

/// One upstream Telltale simulator run request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelltaleSimulatorRunRequest {
    /// Simulator config passed to `telltale-simulator-run`.
    pub config_path: PathBuf,
    /// Output JSON written by the upstream simulator runner.
    pub output_path: PathBuf,
    /// Whether the upstream runner should emit pretty JSON.
    pub pretty: bool,
}

/// Aura-owned parity lane that first invokes an upstream simulator run.
#[derive(Debug, Clone)]
pub struct TelltaleParityRunnerFileRun {
    /// File-based Aura parity comparison inputs.
    pub parity: TelltaleParityFileRun,
    /// Upstream Telltale simulator run request for the candidate lane.
    pub telltale_run: TelltaleSimulatorRunRequest,
    /// Optional explicit command override.
    pub runner: Option<TelltaleSimulatorCommandV1>,
}

/// Aura-owned control-plane parity lane that first invokes an upstream simulator run.
#[derive(Debug, Clone)]
pub struct TelltaleControlPlaneRunnerFileRun {
    /// File-based Aura control-plane parity comparison inputs.
    pub parity: TelltaleControlPlaneFileRun,
    /// Upstream Telltale simulator run request for the candidate lane.
    pub telltale_run: TelltaleSimulatorRunRequest,
    /// Optional explicit command override.
    pub runner: Option<TelltaleSimulatorCommandV1>,
}

/// Stable boundary input for parity checks.
#[derive(Debug, Clone)]
pub struct TelltaleParityInput {
    /// Baseline Aura artifact.
    pub baseline: AuraConformanceArtifactV1,
    /// Candidate artifact generated from a telltale-driven execution lane.
    pub telltale_candidate: AuraConformanceArtifactV1,
    /// Comparison profile.
    pub profile: DifferentialProfile,
}

/// File-based simulator parity lane input.
#[derive(Debug, Clone)]
pub struct TelltaleParityFileRun {
    /// Baseline Aura artifact path.
    pub baseline_path: PathBuf,
    /// Telltale-candidate artifact path.
    pub telltale_candidate_path: PathBuf,
    /// Output path for stable parity report JSON.
    pub output_report_path: PathBuf,
    /// Comparison profile.
    pub profile: DifferentialProfile,
    /// Upstream Telltale 11 sidecar paths that define the default semantic report surface.
    pub upstream: TelltaleUpstreamPathsV1,
}

/// Protocol-critical control-plane lane kinds that should use telltale parity
/// instead of Aura-local scenario lifecycle reimplementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TelltaleControlPlaneLane {
    AnonymousPathEstablish,
    ReplyBlockAccountability,
}

impl TelltaleControlPlaneLane {
    fn lane_suffix(self) -> &'static str {
        match self {
            Self::AnonymousPathEstablish => "anonymous_path_establish",
            Self::ReplyBlockAccountability => "reply_block_accountability",
        }
    }
}

/// File-based telltale run for one protocol-critical control-plane lifecycle.
#[derive(Debug, Clone)]
pub struct TelltaleControlPlaneFileRun {
    /// Which control-plane lifecycle this lane covers.
    pub control_plane_lane: TelltaleControlPlaneLane,
    /// Baseline Aura artifact path.
    pub baseline_path: PathBuf,
    /// Telltale-candidate artifact path.
    pub telltale_candidate_path: PathBuf,
    /// Output path for stable parity report JSON.
    pub output_report_path: PathBuf,
    /// Comparison profile.
    pub profile: DifferentialProfile,
    /// Upstream Telltale 11 sidecar paths that define the default semantic report surface.
    pub upstream: TelltaleUpstreamPathsV1,
}

/// Stable simulator parity artifact payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TelltaleParityReportV1 {
    /// Artifact schema version.
    pub schema_version: String,
    /// Source lane identifier.
    pub lane: String,
    /// Comparison classification (`strict` or `envelope_bounded`).
    pub comparison_classification: String,
    /// First mismatch surface, when present.
    pub first_mismatch_surface: Option<ConformanceSurfaceName>,
    /// First mismatch step index, when present.
    pub first_mismatch_step_index: Option<usize>,
    /// Differential comparison report.
    pub differential: DifferentialReport,
    /// High-level Aura overlay derived from the authoritative upstream context.
    pub semantic_summary: TelltaleParitySemanticSummaryV1,
    /// Authoritative upstream Telltale 11 simulator context attached to this lane.
    pub upstream: TelltaleUpstreamReportV1,
}

/// Optional file paths for upstream Telltale 11 sidecar artifacts.
#[derive(Debug, Clone, Default)]
pub struct TelltaleUpstreamPathsV1 {
    /// Optional baseline simulator run output JSON from `telltale-simulator-run`.
    pub baseline_run_output_path: Option<PathBuf>,
    /// Optional candidate simulator run output JSON from `telltale-simulator-run`.
    pub telltale_run_output_path: Option<PathBuf>,
    /// Optional baseline theorem-eligibility or other decision report JSON.
    pub baseline_decision_report_path: Option<PathBuf>,
    /// Optional candidate theorem-eligibility or other decision report JSON.
    pub telltale_decision_report_path: Option<PathBuf>,
    /// Optional baseline sweep manifest JSON.
    pub baseline_sweep_manifest_path: Option<PathBuf>,
    /// Optional candidate sweep manifest JSON.
    pub telltale_sweep_manifest_path: Option<PathBuf>,
}

/// Upstream Telltale 11 simulator context carried inside Aura parity reports.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TelltaleUpstreamReportV1 {
    /// Baseline-side Telltale 11 run summary.
    pub baseline_run: TelltaleRunSummaryV1,
    /// Candidate-side Telltale 11 run summary.
    pub telltale_run: TelltaleRunSummaryV1,
    /// Baseline-side theorem/decision report, when provided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_decision_report: Option<TelltaleDecisionReport>,
    /// Candidate-side theorem/decision report, when provided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telltale_decision_report: Option<TelltaleDecisionReport>,
    /// Baseline-side sweep manifest, when provided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_sweep_manifest: Option<SweepManifest>,
    /// Candidate-side sweep manifest, when provided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telltale_sweep_manifest: Option<SweepManifest>,
    /// Compact cross-run comparison for the supplied upstream sidecars.
    pub comparison: TelltaleUpstreamComparisonV1,
}

/// Compact summary of a Telltale 11 simulator run sidecar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TelltaleRunSummaryV1 {
    /// Proof-side execution regime classification.
    pub execution_regime: ExecutionRegime,
    /// Resolved execution backend.
    pub backend: ResolvedExecutionBackend,
    /// Resolved theorem/profile information for the run.
    pub theorem_profile: ResolvedTheoremProfile,
    /// Theorem-native progress summary for the run.
    pub theorem_progress: TheoremProgressSummary,
    /// Scheduler-facing theorem/native execution profile summary.
    pub scheduler_profile: SchedulerProfileSummary,
    /// Reconfiguration accounting summary.
    pub reconfiguration_summary: ReconfigurationSummary,
    /// Shared environment trace emitted by the simulator.
    pub environment_trace: EnvironmentTrace,
    /// Assumption diagnostics derived from the adversary/theorem layer.
    pub assumption_diagnostics: Vec<AssumptionDiagnostic>,
    /// Envelope-normalized observability classification.
    pub normalized_observability: NormalizedObservability,
    /// Resolved scheduler concurrency for the run.
    pub scheduler_concurrency: u64,
    /// Worker-thread count for the run.
    pub worker_threads: u64,
}

/// Cross-run summary for the attached upstream Telltale 11 sidecars.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelltaleUpstreamComparisonV1 {
    /// Whether execution regimes match.
    pub execution_regime_match: bool,
    /// Whether theorem profile resolution matches.
    pub theorem_profile_match: bool,
    /// Whether scheduler-profile classification matches.
    pub scheduler_profile_match: bool,
    /// Whether normalized observability classes match.
    pub normalized_observability_match: bool,
    /// Whether the supplied theorem/decision reports match exactly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_report_match: Option<bool>,
    /// Whether the supplied sweep manifests have the same run count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sweep_run_count_match: Option<bool>,
}

/// High-level semantic relation for one Telltale-backed Aura parity report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelltaleParitySemanticRelationV1 {
    /// Raw strict comparison and upstream semantic classifications agree.
    ExactMatch,
    /// Raw traces differ, but upstream normalization and theorem-facing
    /// classifications agree.
    EquivalentUnderNormalization,
    /// The runs diverge on safety-visible behavior.
    SafetyVisibleDivergence,
}

/// Semantic summary for one Telltale-backed Aura parity report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelltaleParitySemanticSummaryV1 {
    /// High-level semantic relation.
    pub relation: TelltaleParitySemanticRelationV1,
    /// Whether execution regimes match across supplied upstream sidecars.
    pub execution_regime_match: bool,
    /// Whether theorem profiles match across supplied upstream sidecars.
    pub theorem_profile_match: bool,
    /// Whether scheduler profiles match across supplied upstream sidecars.
    pub scheduler_profile_match: bool,
    /// Whether normalized observability classes match across supplied upstream sidecars.
    pub normalized_observability_match: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TelltaleRunOutputSidecarV1 {
    stats: TelltaleRunStatsSidecarV1,
    analysis: TelltaleRunAnalysisSidecarV1,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TelltaleRunStatsSidecarV1 {
    execution_regime: ExecutionRegime,
    theorem_profile: ResolvedTheoremProfile,
    theorem_progress: TheoremProgressSummary,
    scheduler_profile: SchedulerProfileSummary,
    reconfiguration_summary: ReconfigurationSummary,
    environment_trace: EnvironmentTrace,
    assumption_diagnostics: Vec<AssumptionDiagnostic>,
    backend: ResolvedExecutionBackend,
    scheduler_concurrency: u64,
    worker_threads: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TelltaleRunAnalysisSidecarV1 {
    normalized_observability: NormalizedObservability,
}

/// Errors for file-based telltale parity lane.
#[derive(Debug, thiserror::Error)]
pub enum TelltaleParityError {
    /// Failed to load one input artifact.
    #[error("failed loading conformance artifact from {path}: {message}")]
    LoadArtifact { path: String, message: String },
    /// Failed to serialize parity report.
    #[error("failed serializing parity report: {message}")]
    Serialize { message: String },
    /// Failed writing parity report.
    #[error("failed writing parity report to {path}: {message}")]
    WriteReport { path: String, message: String },
    /// Supported parity lane omitted required upstream sidecars.
    #[error("supported lane {lane} is missing required upstream sidecars: {missing:?}")]
    MissingRequiredUpstreamSidecars { lane: String, missing: Vec<String> },
    /// Failed invoking the upstream simulator runner.
    #[error("failed invoking telltale simulator runner {program}: {message}")]
    RunSimulator { program: String, message: String },
    /// Upstream simulator runner exited unsuccessfully.
    #[error("telltale simulator runner {program} failed with status {status}: {stderr}")]
    RunnerFailed {
        program: String,
        status: String,
        stderr: String,
    },
}

/// Entry-point trait for telltale-backed parity checks.
pub trait TelltaleParityRunner {
    /// Compare one telltale candidate against one Aura baseline artifact.
    fn run_telltale_parity(&self, input: TelltaleParityInput) -> DifferentialReport;
}

impl TelltaleParityRunner for DifferentialTester {
    fn run_telltale_parity(&self, input: TelltaleParityInput) -> DifferentialReport {
        let tester = DifferentialTester::new(input.profile);
        tester.compare(&input.baseline, &input.telltale_candidate)
    }
}

/// Run telltale parity from file artifacts and emit one report artifact.
///
/// # Errors
///
/// Returns [`TelltaleParityError`] when loading or writing artifacts fails.
pub fn run_telltale_parity_file_lane(
    input: &TelltaleParityFileRun,
) -> Result<TelltaleParityReportV1, TelltaleParityError> {
    let baseline = load_artifact(&input.baseline_path)?;
    let candidate = load_artifact(&input.telltale_candidate_path)?;
    validate_telltale_mapping_surfaces(&baseline).map_err(|missing| {
        TelltaleParityError::LoadArtifact {
            path: input.baseline_path.display().to_string(),
            message: format!("missing required surface: {missing:?}"),
        }
    })?;
    validate_telltale_mapping_surfaces(&candidate).map_err(|missing| {
        TelltaleParityError::LoadArtifact {
            path: input.telltale_candidate_path.display().to_string(),
            message: format!("missing required surface: {missing:?}"),
        }
    })?;

    let tester = DifferentialTester::new(input.profile);
    let differential = tester.run_telltale_parity(TelltaleParityInput {
        baseline,
        telltale_candidate: candidate,
        profile: input.profile,
    });
    let upstream = load_upstream_context("aura-simulator:telltale-parity", &input.upstream)?;

    let report = TelltaleParityReportV1 {
        schema_version: AURA_TELLTALE_PARITY_REPORT_SCHEMA_V1.to_string(),
        lane: "aura-simulator:telltale-parity".to_string(),
        comparison_classification: match differential.profile {
            DifferentialProfile::Strict => "strict".to_string(),
            DifferentialProfile::EnvelopeBounded => "envelope_bounded".to_string(),
        },
        first_mismatch_surface: differential.mismatch.as_ref().and_then(|m| m.surface),
        first_mismatch_step_index: differential.mismatch.as_ref().and_then(|m| m.step_index),
        semantic_summary: semantic_summary_from_report(&differential, &upstream),
        differential,
        upstream,
    };

    write_parity_report(&input.output_report_path, &report)?;
    Ok(report)
}

/// Run an upstream Telltale simulator invocation and attach its sidecar to the
/// Aura parity lane automatically.
///
/// # Errors
///
/// Returns [`TelltaleParityError`] when the upstream runner, artifact loading,
/// or report writing fails.
pub fn run_telltale_parity_with_runner(
    input: &TelltaleParityRunnerFileRun,
) -> Result<TelltaleParityReportV1, TelltaleParityError> {
    execute_telltale_simulator_run(input.runner.as_ref(), &input.telltale_run)?;
    let mut parity = input.parity.clone();
    let mut upstream = parity.upstream.clone();
    upstream.telltale_run_output_path = Some(input.telltale_run.output_path.clone());
    parity.upstream = upstream;
    run_telltale_parity_file_lane(&parity)
}

/// Run a protocol-critical telltale parity lane for a named control-plane
/// lifecycle and emit one stable report artifact.
pub fn run_telltale_control_plane_file_lane(
    input: &TelltaleControlPlaneFileRun,
) -> Result<TelltaleParityReportV1, TelltaleParityError> {
    let mut report = run_telltale_parity_file_lane(&TelltaleParityFileRun {
        baseline_path: input.baseline_path.clone(),
        telltale_candidate_path: input.telltale_candidate_path.clone(),
        output_report_path: input.output_report_path.clone(),
        profile: input.profile,
        upstream: input.upstream.clone(),
    })?;
    report.lane = format!(
        "aura-simulator:telltale-control-plane:{}",
        input.control_plane_lane.lane_suffix()
    );
    write_parity_report(&input.output_report_path, &report)?;
    Ok(report)
}

/// Run an upstream Telltale simulator invocation and attach its sidecar to the
/// Aura control-plane parity lane automatically.
///
/// # Errors
///
/// Returns [`TelltaleParityError`] when the upstream runner, artifact loading,
/// or report writing fails.
pub fn run_telltale_control_plane_with_runner(
    input: &TelltaleControlPlaneRunnerFileRun,
) -> Result<TelltaleParityReportV1, TelltaleParityError> {
    execute_telltale_simulator_run(input.runner.as_ref(), &input.telltale_run)?;
    let mut parity = input.parity.clone();
    let mut upstream = parity.upstream.clone();
    upstream.telltale_run_output_path = Some(input.telltale_run.output_path.clone());
    parity.upstream = upstream;
    run_telltale_control_plane_file_lane(&parity)
}

/// Validate that an artifact satisfies required canonical surfaces.
pub fn validate_telltale_mapping_surfaces(
    artifact: &AuraConformanceArtifactV1,
) -> Result<(), ConformanceSurfaceName> {
    artifact
        .missing_required_surfaces()
        .into_iter()
        .next()
        .map_or(Ok(()), Err)
}

fn load_artifact(path: &Path) -> Result<AuraConformanceArtifactV1, TelltaleParityError> {
    load_conformance_artifact_file(path).map_err(|error| TelltaleParityError::LoadArtifact {
        path: path.display().to_string(),
        message: error.to_string(),
    })
}

fn execute_telltale_simulator_run(
    command: Option<&TelltaleSimulatorCommandV1>,
    request: &TelltaleSimulatorRunRequest,
) -> Result<(), TelltaleParityError> {
    let command = command.cloned().unwrap_or_default();
    let args = command.args_for_run(request);
    let output = Command::new(&command.program)
        .args(&args)
        .output()
        .map_err(|error| TelltaleParityError::RunSimulator {
            program: command.program.display().to_string(),
            message: error.to_string(),
        })?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stderr = if stderr.is_empty() {
        "no stderr captured".to_string()
    } else {
        stderr
    };
    Err(TelltaleParityError::RunnerFailed {
        program: command.program.display().to_string(),
        status: output.status.to_string(),
        stderr,
    })
}

fn load_json_file<T>(path: &Path) -> Result<T, TelltaleParityError>
where
    T: for<'de> Deserialize<'de>,
{
    let payload = std::fs::read(path).map_err(|error| TelltaleParityError::LoadArtifact {
        path: path.display().to_string(),
        message: error.to_string(),
    })?;
    serde_json::from_slice(&payload).map_err(|error| TelltaleParityError::LoadArtifact {
        path: path.display().to_string(),
        message: error.to_string(),
    })
}

fn load_run_summary(path: &Path) -> Result<TelltaleRunSummaryV1, TelltaleParityError> {
    let sidecar: TelltaleRunOutputSidecarV1 = load_json_file(path)?;
    Ok(TelltaleRunSummaryV1 {
        execution_regime: sidecar.stats.execution_regime,
        backend: sidecar.stats.backend,
        theorem_profile: sidecar.stats.theorem_profile,
        theorem_progress: sidecar.stats.theorem_progress,
        scheduler_profile: sidecar.stats.scheduler_profile,
        reconfiguration_summary: sidecar.stats.reconfiguration_summary,
        environment_trace: sidecar.stats.environment_trace,
        assumption_diagnostics: sidecar.stats.assumption_diagnostics,
        normalized_observability: sidecar.analysis.normalized_observability,
        scheduler_concurrency: sidecar.stats.scheduler_concurrency,
        worker_threads: sidecar.stats.worker_threads,
    })
}

fn load_upstream_context(
    lane: &str,
    upstream: &TelltaleUpstreamPathsV1,
) -> Result<TelltaleUpstreamReportV1, TelltaleParityError> {
    let missing = required_upstream_sidecars(upstream);
    if !missing.is_empty() {
        return Err(TelltaleParityError::MissingRequiredUpstreamSidecars {
            lane: lane.to_string(),
            missing,
        });
    }

    let baseline_run = load_run_summary(
        upstream
            .baseline_run_output_path
            .as_deref()
            .expect("required upstream baseline run path"),
    )?;
    let telltale_run = load_run_summary(
        upstream
            .telltale_run_output_path
            .as_deref()
            .expect("required upstream candidate run path"),
    )?;
    let baseline_decision_report = upstream
        .baseline_decision_report_path
        .as_deref()
        .map(load_json_file::<TelltaleDecisionReport>)
        .transpose()?;
    let telltale_decision_report = upstream
        .telltale_decision_report_path
        .as_deref()
        .map(load_json_file::<TelltaleDecisionReport>)
        .transpose()?;
    let baseline_sweep_manifest = upstream
        .baseline_sweep_manifest_path
        .as_deref()
        .map(load_json_file::<SweepManifest>)
        .transpose()?;
    let telltale_sweep_manifest = upstream
        .telltale_sweep_manifest_path
        .as_deref()
        .map(load_json_file::<SweepManifest>)
        .transpose()?;

    let comparison = TelltaleUpstreamComparisonV1 {
        execution_regime_match: baseline_run.execution_regime == telltale_run.execution_regime,
        theorem_profile_match: baseline_run.theorem_profile == telltale_run.theorem_profile,
        scheduler_profile_match: baseline_run.scheduler_profile == telltale_run.scheduler_profile,
        normalized_observability_match: baseline_run.normalized_observability
            == telltale_run.normalized_observability,
        decision_report_match: match (&baseline_decision_report, &telltale_decision_report) {
            (Some(lhs), Some(rhs)) => Some(lhs == rhs),
            _ => None,
        },
        sweep_run_count_match: match (&baseline_sweep_manifest, &telltale_sweep_manifest) {
            (Some(lhs), Some(rhs)) => Some(lhs.runs.len() == rhs.runs.len()),
            _ => None,
        },
    };

    Ok(TelltaleUpstreamReportV1 {
        baseline_run,
        telltale_run,
        baseline_decision_report,
        telltale_decision_report,
        baseline_sweep_manifest,
        telltale_sweep_manifest,
        comparison,
    })
}

fn required_upstream_sidecars(upstream: &TelltaleUpstreamPathsV1) -> Vec<String> {
    let mut missing = Vec::new();
    if upstream.baseline_run_output_path.is_none() {
        missing.push("baseline_run_output_path".to_string());
    }
    if upstream.telltale_run_output_path.is_none() {
        missing.push("telltale_run_output_path".to_string());
    }
    missing
}

fn semantic_summary_from_report(
    differential: &DifferentialReport,
    upstream: &TelltaleUpstreamReportV1,
) -> TelltaleParitySemanticSummaryV1 {
    let comparison = &upstream.comparison;
    let theorem_aligned = comparison.execution_regime_match
        && comparison.theorem_profile_match
        && comparison.scheduler_profile_match;
    let relation = if theorem_aligned && comparison.normalized_observability_match {
        if differential.profile == DifferentialProfile::Strict && differential.equivalent {
            TelltaleParitySemanticRelationV1::ExactMatch
        } else {
            TelltaleParitySemanticRelationV1::EquivalentUnderNormalization
        }
    } else {
        TelltaleParitySemanticRelationV1::SafetyVisibleDivergence
    };
    TelltaleParitySemanticSummaryV1 {
        relation,
        execution_regime_match: comparison.execution_regime_match,
        theorem_profile_match: comparison.theorem_profile_match,
        scheduler_profile_match: comparison.scheduler_profile_match,
        normalized_observability_match: comparison.normalized_observability_match,
    }
}

fn write_parity_report(
    path: &Path,
    report: &TelltaleParityReportV1,
) -> Result<(), TelltaleParityError> {
    let payload =
        serde_json::to_vec_pretty(report).map_err(|error| TelltaleParityError::Serialize {
            message: error.to_string(),
        })?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| TelltaleParityError::WriteReport {
            path: path.display().to_string(),
            message: error.to_string(),
        })?;
    }
    std::fs::write(path, payload).map_err(|error| TelltaleParityError::WriteReport {
        path: path.display().to_string(),
        message: error.to_string(),
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{
        AuraConformanceRunMetadataV1, AuraConformanceSurfaceV1, ConformanceSurfaceName,
    };
    use serde::Serialize;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use telltale_simulator::decision::{DecisionCertificate, DecisionKind, DecisionOutcome};
    use telltale_simulator::fault::AssumptionFailureClass;
    use telltale_simulator::runner::{
        CriticalCapacityPhase, CriticalCapacitySummary, SchedulerBoundMode,
        SchedulerEnvelopeStatus, SchedulerFairnessRequirement,
    };
    use telltale_simulator::scenario::{
        ResolvedTheoremProfile, TheoremAssumptionBundle, TheoremEligibility,
        TheoremEnvelopeProfile, TheoremSchedulerProfile,
    };
    use telltale_simulator::sweep::SweepManifestEntry;
    use tempfile::tempdir;

    fn artifact(target: &str, effect_suffixes: &[&str]) -> AuraConformanceArtifactV1 {
        let mut artifact = AuraConformanceArtifactV1::new(AuraConformanceRunMetadataV1 {
            target: target.to_string(),
            profile: "native_coop".to_string(),
            scenario: "telltale_boundary".to_string(),
            seed: Some(9),
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

    fn write_json_fixture<T>(path: &Path, value: &T)
    where
        T: Serialize,
    {
        fs::write(
            path,
            serde_json::to_vec_pretty(value).expect("serialize fixture"),
        )
        .expect("write fixture");
    }

    #[test]
    fn boundary_uses_profile_from_input() {
        let baseline = artifact("aura", &["a", "b"]);
        let telltale_candidate = artifact("telltale_machine", &["b", "a"]);
        let runner = DifferentialTester::new(DifferentialProfile::Strict);
        let strict_report = runner.run_telltale_parity(TelltaleParityInput {
            baseline: baseline.clone(),
            telltale_candidate: telltale_candidate.clone(),
            profile: DifferentialProfile::Strict,
        });
        assert!(!strict_report.equivalent);

        let envelope_report = runner.run_telltale_parity(TelltaleParityInput {
            baseline,
            telltale_candidate,
            profile: DifferentialProfile::EnvelopeBounded,
        });
        assert!(envelope_report.equivalent);
    }

    #[test]
    fn surface_validation_rejects_missing_required_surface() {
        let mut baseline = artifact("aura", &["a"]);
        baseline.surfaces.remove(&ConformanceSurfaceName::Effect);
        let missing = validate_telltale_mapping_surfaces(&baseline)
            .expect_err("missing effect surface should be rejected");
        assert_eq!(missing, ConformanceSurfaceName::Effect);
    }

    #[test]
    fn file_lane_writes_stable_report_artifact() {
        let dir = tempdir().expect("tempdir");
        let baseline_path = dir.path().join("baseline.json");
        let candidate_path = dir.path().join("candidate.json");
        let report_path = dir.path().join("report.json");

        let baseline = artifact("aura", &["a", "b"]);
        let candidate = artifact("telltale_machine", &["b", "a"]);
        std::fs::write(
            &baseline_path,
            serde_json::to_vec_pretty(&baseline).expect("serialize baseline"),
        )
        .expect("write baseline");
        std::fs::write(
            &candidate_path,
            serde_json::to_vec_pretty(&candidate).expect("serialize candidate"),
        )
        .expect("write candidate");

        let report = run_telltale_parity_file_lane(&TelltaleParityFileRun {
            baseline_path,
            telltale_candidate_path: candidate_path,
            output_report_path: report_path.clone(),
            profile: DifferentialProfile::EnvelopeBounded,
            upstream: write_required_upstream_paths(dir.path()),
        })
        .expect("run lane");
        assert_eq!(report.schema_version, AURA_TELLTALE_PARITY_REPORT_SCHEMA_V1);
        assert!(report.differential.equivalent);
        assert_eq!(
            report.semantic_summary.relation,
            TelltaleParitySemanticRelationV1::EquivalentUnderNormalization
        );

        let written: TelltaleParityReportV1 =
            serde_json::from_slice(&std::fs::read(report_path).expect("read report"))
                .expect("decode report");
        assert_eq!(
            written.schema_version,
            AURA_TELLTALE_PARITY_REPORT_SCHEMA_V1
        );
        assert_eq!(written.lane, "aura-simulator:telltale-parity");
        assert_eq!(written.comparison_classification, "envelope_bounded");
    }

    #[test]
    fn telltale_parity_report_generation_ci() {
        let artifact_path = std::env::var("AURA_TELLTALE_PARITY_ARTIFACT")
            .ok()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("artifacts/telltale-parity/report.json"));
        let work_dir = artifact_path
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("artifacts/telltale-parity"));

        std::fs::create_dir_all(&work_dir).expect("create output directory");
        let baseline_path = work_dir.join("baseline.json");
        let candidate_path = work_dir.join("telltale_candidate.json");
        let baseline = artifact("aura", &["a", "b"]);
        let candidate = artifact("telltale_machine", &["b", "a"]);
        std::fs::write(
            &baseline_path,
            serde_json::to_vec_pretty(&baseline).expect("serialize baseline"),
        )
        .expect("write baseline");
        std::fs::write(
            &candidate_path,
            serde_json::to_vec_pretty(&candidate).expect("serialize candidate"),
        )
        .expect("write candidate");

        let report = run_telltale_parity_file_lane(&TelltaleParityFileRun {
            baseline_path,
            telltale_candidate_path: candidate_path,
            output_report_path: artifact_path.clone(),
            profile: DifferentialProfile::EnvelopeBounded,
            upstream: write_required_upstream_paths(&work_dir),
        })
        .expect("run lane");
        assert!(report.differential.equivalent);
        assert_eq!(report.comparison_classification, "envelope_bounded");
        assert!(artifact_path.exists());
    }

    #[test]
    fn mismatch_fields_are_populated_for_strict_failures() {
        let dir = tempdir().expect("tempdir");
        let baseline_path = dir.path().join("baseline.json");
        let candidate_path = dir.path().join("candidate.json");
        let report_path = dir.path().join("report.json");

        let baseline = artifact("aura", &["a", "b"]);
        let candidate = artifact("telltale_machine", &["b", "a"]);
        std::fs::write(
            &baseline_path,
            serde_json::to_vec_pretty(&baseline).expect("serialize baseline"),
        )
        .expect("write baseline");
        std::fs::write(
            &candidate_path,
            serde_json::to_vec_pretty(&candidate).expect("serialize candidate"),
        )
        .expect("write candidate");

        let report = run_telltale_parity_file_lane(&TelltaleParityFileRun {
            baseline_path,
            telltale_candidate_path: candidate_path,
            output_report_path: report_path,
            profile: DifferentialProfile::Strict,
            upstream: write_required_upstream_paths(dir.path()),
        })
        .expect("run lane");
        assert!(!report.differential.equivalent);
        assert_eq!(report.comparison_classification, "strict");
        assert!(report.first_mismatch_surface.is_some());
        assert!(report.first_mismatch_step_index.is_some());
    }

    #[test]
    fn control_plane_lane_writes_profiled_report_name() {
        let dir = tempdir().expect("tempdir");
        let baseline_path = dir.path().join("baseline.json");
        let candidate_path = dir.path().join("candidate.json");
        let report_path = dir.path().join("control-plane-report.json");

        let baseline = artifact("aura", &["a", "b"]);
        let candidate = artifact("telltale_machine", &["b", "a"]);
        std::fs::write(
            &baseline_path,
            serde_json::to_vec_pretty(&baseline).expect("serialize baseline"),
        )
        .expect("write baseline");
        std::fs::write(
            &candidate_path,
            serde_json::to_vec_pretty(&candidate).expect("serialize candidate"),
        )
        .expect("write candidate");

        let report = run_telltale_control_plane_file_lane(&TelltaleControlPlaneFileRun {
            control_plane_lane: TelltaleControlPlaneLane::AnonymousPathEstablish,
            baseline_path,
            telltale_candidate_path: candidate_path,
            output_report_path: report_path.clone(),
            profile: DifferentialProfile::EnvelopeBounded,
            upstream: write_required_upstream_paths(dir.path()),
        })
        .expect("run control-plane lane");
        assert_eq!(
            report.lane,
            "aura-simulator:telltale-control-plane:anonymous_path_establish"
        );

        let written: TelltaleParityReportV1 =
            serde_json::from_slice(&std::fs::read(report_path).expect("read report"))
                .expect("decode report");
        assert_eq!(
            written.lane,
            "aura-simulator:telltale-control-plane:anonymous_path_establish"
        );
    }

    fn sample_run_summary() -> TelltaleRunOutputSidecarV1 {
        TelltaleRunOutputSidecarV1 {
            stats: TelltaleRunStatsSidecarV1 {
                execution_regime: ExecutionRegime::CanonicalExact,
                theorem_profile: ResolvedTheoremProfile {
                    scheduler_profile: TheoremSchedulerProfile::CanonicalExact,
                    envelope_profile: TheoremEnvelopeProfile::Exact,
                    assumption_bundle: TheoremAssumptionBundle::FaultFreeTransport,
                    eligibility: TheoremEligibility::Exact,
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
                    implementation_policy:
                        telltale_simulator::scenario::ResolvedSchedulerPolicy::Cooperative,
                    theorem_profile: TheoremSchedulerProfile::CanonicalExact,
                    productive_exactness: true,
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
                assumption_diagnostics: vec![AssumptionDiagnostic {
                    tick: 0,
                    class: AssumptionFailureClass::FairnessFailure,
                    adversary_id: None,
                    detail: "none".to_string(),
                }],
                backend: ResolvedExecutionBackend::Canonical,
                scheduler_concurrency: 1,
                worker_threads: 1,
            },
            analysis: TelltaleRunAnalysisSidecarV1 {
                normalized_observability: NormalizedObservability {
                    schema_version: 1,
                    raw_observable_event_count: 1,
                    raw_reconfiguration_count: 0,
                    normalized_event_class: vec!["sent:x".to_string()],
                    normalized_reconfiguration_class: Vec::new(),
                },
            },
        }
    }

    fn sample_decision_report() -> TelltaleDecisionReport {
        TelltaleDecisionReport {
            kind: DecisionKind::TheoremEligibility,
            outcome: DecisionOutcome::Certified(DecisionCertificate::TheoremEligibility {
                eligibility: TheoremEligibility::Exact,
            }),
        }
    }

    fn write_required_upstream_paths(dir: &Path) -> TelltaleUpstreamPathsV1 {
        let baseline_run_path = dir.join("baseline-run.json");
        let candidate_run_path = dir.join("candidate-run.json");
        let run_output = sample_run_summary();
        write_json_fixture(&baseline_run_path, &run_output);
        write_json_fixture(&candidate_run_path, &run_output);
        TelltaleUpstreamPathsV1 {
            baseline_run_output_path: Some(baseline_run_path),
            telltale_run_output_path: Some(candidate_run_path),
            baseline_decision_report_path: None,
            telltale_decision_report_path: None,
            baseline_sweep_manifest_path: None,
            telltale_sweep_manifest_path: None,
        }
    }

    fn write_runner_script(path: &Path, body: &str) {
        fs::write(path, body).expect("write script");
        let mut perms = fs::metadata(path).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).expect("chmod");
    }

    #[test]
    fn file_lane_embeds_upstream_telltale_sidecars() {
        let dir = tempdir().expect("tempdir");
        let baseline_path = dir.path().join("baseline.json");
        let candidate_path = dir.path().join("candidate.json");
        let report_path = dir.path().join("report.json");
        let baseline_run_path = dir.path().join("baseline-run.json");
        let candidate_run_path = dir.path().join("candidate-run.json");
        let baseline_decision_path = dir.path().join("baseline-decision.json");
        let candidate_decision_path = dir.path().join("candidate-decision.json");
        let baseline_sweep_path = dir.path().join("baseline-sweep.json");
        let candidate_sweep_path = dir.path().join("candidate-sweep.json");

        let baseline = artifact("aura", &["a", "b"]);
        let candidate = artifact("telltale_machine", &["b", "a"]);
        std::fs::write(
            &baseline_path,
            serde_json::to_vec_pretty(&baseline).expect("serialize baseline"),
        )
        .expect("write baseline");
        std::fs::write(
            &candidate_path,
            serde_json::to_vec_pretty(&candidate).expect("serialize candidate"),
        )
        .expect("write candidate");

        let run_output = sample_run_summary();
        std::fs::write(
            &baseline_run_path,
            serde_json::to_vec_pretty(&run_output).expect("serialize baseline run"),
        )
        .expect("write baseline run");
        std::fs::write(
            &candidate_run_path,
            serde_json::to_vec_pretty(&run_output).expect("serialize candidate run"),
        )
        .expect("write candidate run");

        let decision = sample_decision_report();
        std::fs::write(
            &baseline_decision_path,
            serde_json::to_vec_pretty(&decision).expect("serialize baseline decision"),
        )
        .expect("write baseline decision");
        std::fs::write(
            &candidate_decision_path,
            serde_json::to_vec_pretty(&decision).expect("serialize candidate decision"),
        )
        .expect("write candidate decision");

        let sweep = SweepManifest {
            parallelism: 1,
            runs: vec![SweepManifestEntry {
                input_index: 0,
                scenario_name: "lane".to_string(),
                bindings: Vec::new(),
                theorem_profile: None,
                scheduler_profile: None,
                theorem_eligibility: decision,
                capacity_report: None,
                result_error: None,
            }],
        };
        std::fs::write(
            &baseline_sweep_path,
            serde_json::to_vec_pretty(&sweep).expect("serialize baseline sweep"),
        )
        .expect("write baseline sweep");
        std::fs::write(
            &candidate_sweep_path,
            serde_json::to_vec_pretty(&sweep).expect("serialize candidate sweep"),
        )
        .expect("write candidate sweep");

        let report = run_telltale_parity_file_lane(&TelltaleParityFileRun {
            baseline_path,
            telltale_candidate_path: candidate_path,
            output_report_path: report_path,
            profile: DifferentialProfile::EnvelopeBounded,
            upstream: TelltaleUpstreamPathsV1 {
                baseline_run_output_path: Some(baseline_run_path),
                telltale_run_output_path: Some(candidate_run_path),
                baseline_decision_report_path: Some(baseline_decision_path),
                telltale_decision_report_path: Some(candidate_decision_path),
                baseline_sweep_manifest_path: Some(baseline_sweep_path),
                telltale_sweep_manifest_path: Some(candidate_sweep_path),
            },
        })
        .expect("run lane");

        let upstream = report.upstream;
        assert!(upstream.baseline_decision_report.is_some());
        assert!(upstream.telltale_decision_report.is_some());
        assert!(upstream.baseline_sweep_manifest.is_some());
        assert!(upstream.telltale_sweep_manifest.is_some());
        let comparison = upstream.comparison;
        assert!(comparison.execution_regime_match);
        assert!(comparison.theorem_profile_match);
        assert!(comparison.scheduler_profile_match);
        assert!(comparison.normalized_observability_match);
        assert_eq!(comparison.decision_report_match, Some(true));
        assert_eq!(comparison.sweep_run_count_match, Some(true));
        let semantic = report.semantic_summary;
        assert_eq!(
            semantic.relation,
            TelltaleParitySemanticRelationV1::EquivalentUnderNormalization
        );
        assert!(semantic.execution_regime_match);
        assert!(semantic.theorem_profile_match);
        assert!(semantic.scheduler_profile_match);
        assert!(semantic.normalized_observability_match);
    }

    #[test]
    fn runner_command_builds_expected_argv() {
        let command = TelltaleSimulatorCommandV1 {
            program: PathBuf::from("/tmp/telltale-simulator-run"),
            extra_args: vec!["--frozen".to_string()],
        };
        let request = TelltaleSimulatorRunRequest {
            config_path: PathBuf::from("/tmp/scenario.toml"),
            output_path: PathBuf::from("/tmp/run.json"),
            pretty: true,
        };
        assert_eq!(
            command.args_for_run(&request),
            vec![
                "--frozen".to_string(),
                "--config".to_string(),
                "/tmp/scenario.toml".to_string(),
                "--output".to_string(),
                "/tmp/run.json".to_string(),
                "--pretty".to_string(),
            ]
        );
    }

    #[test]
    fn parity_runner_surfaces_upstream_runner_failures() {
        let dir = tempdir().expect("tempdir");
        let script_path = dir.path().join("fail-runner.sh");
        write_runner_script(&script_path, "#!/bin/sh\necho fail-runner >&2\nexit 17\n");
        let error = execute_telltale_simulator_run(
            Some(&TelltaleSimulatorCommandV1 {
                program: script_path,
                extra_args: Vec::new(),
            }),
            &TelltaleSimulatorRunRequest {
                config_path: dir.path().join("scenario.toml"),
                output_path: dir.path().join("run.json"),
                pretty: true,
            },
        )
        .expect_err("runner should fail");
        match error {
            TelltaleParityError::RunnerFailed { status, stderr, .. } => {
                assert!(status.contains("17"));
                assert!(stderr.contains("fail-runner"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn parity_runner_invokes_upstream_and_attaches_sidecar() {
        let dir = tempdir().expect("tempdir");
        let baseline_path = dir.path().join("baseline.json");
        let candidate_path = dir.path().join("candidate.json");
        let report_path = dir.path().join("report.json");
        let run_output_path = dir.path().join("candidate-run.json");
        let fixture_path = dir.path().join("fixture.json");
        let arg_log_path = dir.path().join("runner-args.log");
        let script_path = dir.path().join("ok-runner.sh");

        let baseline = artifact("aura", &["a", "b"]);
        let candidate = artifact("telltale_machine", &["b", "a"]);
        fs::write(
            &baseline_path,
            serde_json::to_vec_pretty(&baseline).expect("serialize baseline"),
        )
        .expect("write baseline");
        fs::write(
            &candidate_path,
            serde_json::to_vec_pretty(&candidate).expect("serialize candidate"),
        )
        .expect("write candidate");
        fs::write(
            &fixture_path,
            serde_json::to_vec_pretty(&sample_run_summary()).expect("serialize fixture"),
        )
        .expect("write fixture");

        let script = format!(
            "#!/bin/sh\nset -eu\nlog=\"{}\"\nfixture=\"{}\"\noutput=\"\"\nconfig=\"\"\n: > \"$log\"\nwhile [ \"$#\" -gt 0 ]; do\n  case \"$1\" in\n    --config) config=\"$2\"; shift 2 ;;\n    --output) output=\"$2\"; shift 2 ;;\n    --pretty) echo --pretty >> \"$log\"; shift ;;\n    *) echo \"$1\" >> \"$log\"; shift ;;\n  esac\ndone\nprintf '%s\\n' \"$config\" >> \"$log\"\ncat \"$fixture\" > \"$output\"\n",
            arg_log_path.display(),
            fixture_path.display()
        );
        write_runner_script(&script_path, &script);

        let report = run_telltale_parity_with_runner(&TelltaleParityRunnerFileRun {
            parity: TelltaleParityFileRun {
                baseline_path,
                telltale_candidate_path: candidate_path,
                output_report_path: report_path,
                profile: DifferentialProfile::EnvelopeBounded,
                upstream: TelltaleUpstreamPathsV1 {
                    baseline_run_output_path: Some(fixture_path.clone()),
                    telltale_run_output_path: None,
                    baseline_decision_report_path: None,
                    telltale_decision_report_path: None,
                    baseline_sweep_manifest_path: None,
                    telltale_sweep_manifest_path: None,
                },
            },
            telltale_run: TelltaleSimulatorRunRequest {
                config_path: dir.path().join("scenario.toml"),
                output_path: run_output_path.clone(),
                pretty: true,
            },
            runner: Some(TelltaleSimulatorCommandV1 {
                program: script_path,
                extra_args: Vec::new(),
            }),
        })
        .expect("run parity with runner");

        let upstream = report.upstream;
        assert_eq!(
            upstream.baseline_run.normalized_observability,
            upstream.telltale_run.normalized_observability
        );
        assert_eq!(
            report.semantic_summary.relation,
            TelltaleParitySemanticRelationV1::EquivalentUnderNormalization
        );
        assert!(run_output_path.exists());
        let logged = fs::read_to_string(arg_log_path).expect("read args");
        assert!(logged.contains("--pretty"));
        assert!(logged.contains("scenario.toml"));
    }

    #[test]
    fn semantic_summary_reports_exact_match_for_strict_aligned_runs() {
        let dir = tempdir().expect("tempdir");
        let baseline_path = dir.path().join("baseline.json");
        let candidate_path = dir.path().join("candidate.json");
        let report_path = dir.path().join("report.json");
        let baseline_run_path = dir.path().join("baseline-run.json");
        let candidate_run_path = dir.path().join("candidate-run.json");

        let baseline = artifact("aura", &["a", "b"]);
        let candidate = artifact("telltale_machine", &["a", "b"]);
        fs::write(
            &baseline_path,
            serde_json::to_vec_pretty(&baseline).expect("serialize baseline"),
        )
        .expect("write baseline");
        fs::write(
            &candidate_path,
            serde_json::to_vec_pretty(&candidate).expect("serialize candidate"),
        )
        .expect("write candidate");
        let run_output = sample_run_summary();
        fs::write(
            &baseline_run_path,
            serde_json::to_vec_pretty(&run_output).expect("serialize baseline run"),
        )
        .expect("write baseline run");
        fs::write(
            &candidate_run_path,
            serde_json::to_vec_pretty(&run_output).expect("serialize candidate run"),
        )
        .expect("write candidate run");

        let report = run_telltale_parity_file_lane(&TelltaleParityFileRun {
            baseline_path,
            telltale_candidate_path: candidate_path,
            output_report_path: report_path,
            profile: DifferentialProfile::Strict,
            upstream: TelltaleUpstreamPathsV1 {
                baseline_run_output_path: Some(baseline_run_path),
                telltale_run_output_path: Some(candidate_run_path),
                baseline_decision_report_path: None,
                telltale_decision_report_path: None,
                baseline_sweep_manifest_path: None,
                telltale_sweep_manifest_path: None,
            },
        })
        .expect("run lane");

        let semantic = report.semantic_summary;
        assert_eq!(
            semantic.relation,
            TelltaleParitySemanticRelationV1::ExactMatch
        );
    }

    #[test]
    fn semantic_summary_reports_safety_visible_divergence_for_theorem_mismatch() {
        let dir = tempdir().expect("tempdir");
        let baseline_path = dir.path().join("baseline.json");
        let candidate_path = dir.path().join("candidate.json");
        let report_path = dir.path().join("report.json");
        let baseline_run_path = dir.path().join("baseline-run.json");
        let candidate_run_path = dir.path().join("candidate-run.json");

        let baseline = artifact("aura", &["a", "b"]);
        let candidate = artifact("telltale_machine", &["b", "a"]);
        fs::write(
            &baseline_path,
            serde_json::to_vec_pretty(&baseline).expect("serialize baseline"),
        )
        .expect("write baseline");
        fs::write(
            &candidate_path,
            serde_json::to_vec_pretty(&candidate).expect("serialize candidate"),
        )
        .expect("write candidate");

        let baseline_run = sample_run_summary();
        let mut candidate_run = sample_run_summary();
        candidate_run.stats.theorem_profile.scheduler_profile =
            TheoremSchedulerProfile::ThreadedEnvelope;
        candidate_run.stats.theorem_profile.eligibility = TheoremEligibility::EnvelopeBounded;
        fs::write(
            &baseline_run_path,
            serde_json::to_vec_pretty(&baseline_run).expect("serialize baseline run"),
        )
        .expect("write baseline run");
        fs::write(
            &candidate_run_path,
            serde_json::to_vec_pretty(&candidate_run).expect("serialize candidate run"),
        )
        .expect("write candidate run");

        let report = run_telltale_parity_file_lane(&TelltaleParityFileRun {
            baseline_path,
            telltale_candidate_path: candidate_path,
            output_report_path: report_path,
            profile: DifferentialProfile::EnvelopeBounded,
            upstream: TelltaleUpstreamPathsV1 {
                baseline_run_output_path: Some(baseline_run_path),
                telltale_run_output_path: Some(candidate_run_path),
                baseline_decision_report_path: None,
                telltale_decision_report_path: None,
                baseline_sweep_manifest_path: None,
                telltale_sweep_manifest_path: None,
            },
        })
        .expect("run lane");

        let semantic = report.semantic_summary;
        assert_eq!(
            semantic.relation,
            TelltaleParitySemanticRelationV1::SafetyVisibleDivergence
        );
        assert!(!semantic.theorem_profile_match);
    }

    #[test]
    fn supported_file_lane_rejects_missing_required_upstream_run_sidecars() {
        let dir = tempdir().expect("tempdir");
        let baseline_path = dir.path().join("baseline.json");
        let candidate_path = dir.path().join("candidate.json");
        let report_path = dir.path().join("report.json");

        write_json_fixture(&baseline_path, &artifact("aura", &["a", "b"]));
        write_json_fixture(&candidate_path, &artifact("telltale_machine", &["b", "a"]));

        let error = run_telltale_parity_file_lane(&TelltaleParityFileRun {
            baseline_path,
            telltale_candidate_path: candidate_path,
            output_report_path: report_path,
            profile: DifferentialProfile::EnvelopeBounded,
            upstream: TelltaleUpstreamPathsV1::default(),
        })
        .expect_err("missing run sidecars should fail");

        match error {
            TelltaleParityError::MissingRequiredUpstreamSidecars { missing, .. } => {
                assert!(missing
                    .iter()
                    .any(|field| field == "baseline_run_output_path"));
                assert!(missing
                    .iter()
                    .any(|field| field == "telltale_run_output_path"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
