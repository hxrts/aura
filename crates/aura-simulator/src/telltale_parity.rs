//! Telltale VM parity boundary for aura-simulator.
//!
//! This module defines a crate-local integration boundary.
//! It avoids direct VM execution coupling in the default simulator path.
//! Callers provide conformance artifacts and select a comparison profile.

use crate::differential_tester::{DifferentialProfile, DifferentialReport, DifferentialTester};
use aura_core::{AuraConformanceArtifactV1, ConformanceSurfaceName};
use aura_testkit::load_conformance_artifact_file;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Schema identifier for simulator parity reports.
pub const AURA_TELLTALE_PARITY_REPORT_SCHEMA_V1: &str = "aura.telltale-parity.report.v1";

/// Canonical mapping row from telltale VM event families to Aura surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TelltaleSurfaceMappingV1 {
    /// Telltale event family label.
    pub telltale_event_kind: &'static str,
    /// Aura artifact surface label.
    pub aura_surface: &'static str,
    /// Normalization rule before differential comparison.
    pub normalization: &'static str,
}

/// Canonical surface mapping used by all telltale parity lanes.
pub const TELLTALE_SURFACE_MAPPINGS_V1: &[TelltaleSurfaceMappingV1] = &[
    TelltaleSurfaceMappingV1 {
        telltale_event_kind: "observable",
        aura_surface: "observable",
        normalization: "identity",
    },
    TelltaleSurfaceMappingV1 {
        telltale_event_kind: "scheduler_step",
        aura_surface: "scheduler_step",
        normalization: "tick_normalized",
    },
    TelltaleSurfaceMappingV1 {
        telltale_event_kind: "effect",
        aura_surface: "effect",
        normalization: "effect_envelope_classification",
    },
];

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
}

/// Stable simulator parity artifact payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

    let report = TelltaleParityReportV1 {
        schema_version: AURA_TELLTALE_PARITY_REPORT_SCHEMA_V1.to_string(),
        lane: "aura-simulator:telltale-parity".to_string(),
        comparison_classification: match differential.profile {
            DifferentialProfile::Strict => "strict".to_string(),
            DifferentialProfile::EnvelopeBounded => "envelope_bounded".to_string(),
        },
        first_mismatch_surface: differential.mismatch.as_ref().and_then(|m| m.surface),
        first_mismatch_step_index: differential.mismatch.as_ref().and_then(|m| m.step_index),
        differential,
    };

    write_parity_report(&input.output_report_path, &report)?;
    Ok(report)
}

/// Validate that an artifact satisfies required canonical surfaces.
pub fn validate_telltale_mapping_surfaces(
    artifact: &AuraConformanceArtifactV1,
) -> Result<(), ConformanceSurfaceName> {
    for required in ConformanceSurfaceName::REQUIRED {
        if artifact.surfaces.contains_key(&required) {
            continue;
        }
        return Err(required);
    }
    Ok(())
}

fn load_artifact(path: &Path) -> Result<AuraConformanceArtifactV1, TelltaleParityError> {
    load_conformance_artifact_file(path).map_err(|error| TelltaleParityError::LoadArtifact {
        path: path.display().to_string(),
        message: error.to_string(),
    })
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
    use std::path::PathBuf;
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

    #[test]
    fn boundary_uses_profile_from_input() {
        let baseline = artifact("aura", &["a", "b"]);
        let telltale_candidate = artifact("telltale_vm", &["b", "a"]);
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
    fn canonical_surface_mapping_matches_required_surfaces() {
        let mapped = TELLTALE_SURFACE_MAPPINGS_V1
            .iter()
            .map(|row| row.aura_surface)
            .collect::<std::collections::BTreeSet<_>>();
        assert!(mapped.contains("observable"));
        assert!(mapped.contains("scheduler_step"));
        assert!(mapped.contains("effect"));
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
        let candidate = artifact("telltale_vm", &["b", "a"]);
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
        })
        .expect("run lane");
        assert_eq!(report.schema_version, AURA_TELLTALE_PARITY_REPORT_SCHEMA_V1);
        assert!(report.differential.equivalent);

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
        let candidate = artifact("telltale_vm", &["b", "a"]);
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
        let candidate = artifact("telltale_vm", &["b", "a"]);
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
        })
        .expect("run lane");
        assert!(!report.differential.equivalent);
        assert_eq!(report.comparison_classification, "strict");
        assert!(report.first_mismatch_surface.is_some());
        assert!(report.first_mismatch_step_index.is_some());
    }
}
