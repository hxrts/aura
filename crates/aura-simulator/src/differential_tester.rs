//! Differential trace/artifact comparison utilities for replay conformance.

use std::path::Path;

use aura_core::{AuraConformanceArtifactV1, ConformanceSurfaceName};
use aura_testkit::{
    compare_artifacts, load_conformance_artifact_file, ConformanceMismatch, EnvelopeLawRegistry,
};
use serde::{Deserialize, Serialize};

/// Comparison profile for differential testing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DifferentialProfile {
    /// Byte-identical equality on all required surfaces.
    Strict,
    /// Envelope-bounded comparison using Aura law classes.
    #[default]
    EnvelopeBounded,
}

impl DifferentialProfile {
    /// Parse from CLI/config text.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "strict" => Some(Self::Strict),
            "envelope_bounded" | "envelope-bounded" | "envelope" => Some(Self::EnvelopeBounded),
            _ => None,
        }
    }
}

/// Structured differential mismatch payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DifferentialMismatch {
    /// Surface where mismatch occurred.
    pub surface: Option<ConformanceSurfaceName>,
    /// Optional step index of first mismatch.
    pub step_index: Option<usize>,
    /// Human-readable mismatch detail.
    pub detail: String,
}

/// Report emitted after one differential comparison.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DifferentialReport {
    /// Active comparison profile.
    pub profile: DifferentialProfile,
    /// True when runs are equivalent under the selected profile.
    pub equivalent: bool,
    /// First mismatch when not equivalent.
    pub mismatch: Option<DifferentialMismatch>,
    /// Baseline target label (metadata).
    pub baseline_target: String,
    /// Candidate target label (metadata).
    pub candidate_target: String,
    /// Scenario identifier (metadata).
    pub scenario: String,
}

/// Differential-testing errors.
#[derive(Debug, thiserror::Error)]
pub enum DifferentialTesterError {
    /// Failed loading a conformance artifact file.
    #[error("failed loading conformance artifact: {message}")]
    LoadArtifact {
        /// Human-readable load failure.
        message: String,
    },
    /// Candidate is outside the admitted differential envelope.
    #[error("differential envelope violation: {detail}")]
    EnvelopeViolation {
        /// Mismatch summary.
        detail: String,
    },
}

/// Law-aware differential tester used by simulator/CI replay checks.
#[derive(Debug, Clone)]
pub struct DifferentialTester {
    profile: DifferentialProfile,
    registry: EnvelopeLawRegistry,
}

impl DifferentialTester {
    /// Build a tester with one comparison profile.
    #[must_use]
    pub fn new(profile: DifferentialProfile) -> Self {
        Self {
            profile,
            registry: EnvelopeLawRegistry::from_aura_registry(),
        }
    }

    /// Active comparison profile.
    #[must_use]
    pub fn profile(&self) -> DifferentialProfile {
        self.profile
    }

    /// Compare baseline/candidate conformance artifacts.
    #[must_use]
    pub fn compare(
        &self,
        baseline: &AuraConformanceArtifactV1,
        candidate: &AuraConformanceArtifactV1,
    ) -> DifferentialReport {
        let mismatch = match self.profile {
            DifferentialProfile::EnvelopeBounded => {
                let report = compare_artifacts(baseline, candidate, &self.registry);
                report.first_mismatch.map(map_mismatch)
            }
            DifferentialProfile::Strict => strict_mismatch(baseline, candidate),
        };

        DifferentialReport {
            profile: self.profile,
            equivalent: mismatch.is_none(),
            mismatch,
            baseline_target: baseline.metadata.target.clone(),
            candidate_target: candidate.metadata.target.clone(),
            scenario: baseline.metadata.scenario.clone(),
        }
    }

    /// Compare artifacts loaded from disk.
    ///
    /// # Errors
    ///
    /// Returns load errors for malformed/missing files.
    pub fn compare_files(
        &self,
        baseline_path: impl AsRef<Path>,
        candidate_path: impl AsRef<Path>,
    ) -> Result<DifferentialReport, DifferentialTesterError> {
        let baseline = load_conformance_artifact_file(baseline_path).map_err(|error| {
            DifferentialTesterError::LoadArtifact {
                message: error.to_string(),
            }
        })?;
        let candidate = load_conformance_artifact_file(candidate_path).map_err(|error| {
            DifferentialTesterError::LoadArtifact {
                message: error.to_string(),
            }
        })?;
        Ok(self.compare(&baseline, &candidate))
    }

    /// Assert candidate run stays within admitted differential envelope.
    ///
    /// # Errors
    ///
    /// Returns [`DifferentialTesterError::EnvelopeViolation`] on mismatch.
    pub fn assert_envelope_satisfied(
        &self,
        baseline: &AuraConformanceArtifactV1,
        candidate: &AuraConformanceArtifactV1,
    ) -> Result<(), DifferentialTesterError> {
        let report = self.compare(baseline, candidate);
        if report.equivalent {
            return Ok(());
        }

        let detail = report
            .mismatch
            .as_ref()
            .map(|mismatch| mismatch.detail.clone())
            .unwrap_or_else(|| "unknown differential mismatch".to_string());
        Err(DifferentialTesterError::EnvelopeViolation { detail })
    }
}

fn strict_mismatch(
    baseline: &AuraConformanceArtifactV1,
    candidate: &AuraConformanceArtifactV1,
) -> Option<DifferentialMismatch> {
    for surface in ConformanceSurfaceName::REQUIRED {
        let baseline_entries = baseline
            .surfaces
            .get(&surface)
            .map(|payload| payload.entries.as_slice())
            .unwrap_or(&[]);
        let candidate_entries = candidate
            .surfaces
            .get(&surface)
            .map(|payload| payload.entries.as_slice())
            .unwrap_or(&[]);

        if baseline_entries == candidate_entries {
            continue;
        }

        let max_len = baseline_entries.len().max(candidate_entries.len());
        let first_idx =
            (0..max_len).find(|idx| baseline_entries.get(*idx) != candidate_entries.get(*idx));
        return Some(DifferentialMismatch {
            surface: Some(surface),
            step_index: first_idx,
            detail: "strict profile mismatch".to_string(),
        });
    }
    None
}

fn map_mismatch(mismatch: ConformanceMismatch) -> DifferentialMismatch {
    DifferentialMismatch {
        surface: Some(mismatch.surface),
        step_index: mismatch.step_index,
        detail: mismatch.detail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{
        AuraConformanceRunMetadataV1, AuraConformanceSurfaceV1, ConformanceSurfaceName,
    };

    fn artifact_with_effect_order(effect_suffixes: &[&str]) -> AuraConformanceArtifactV1 {
        let mut artifact = AuraConformanceArtifactV1::new(AuraConformanceRunMetadataV1 {
            target: "native".to_string(),
            profile: "native_coop".to_string(),
            scenario: "diff".to_string(),
            seed: Some(7),
            commit: None,
            async_host_transcript_entries: None,
            async_host_transcript_digest_hex: None,
            vm_determinism_profile: None,
        });
        artifact.insert_surface(
            ConformanceSurfaceName::Observable,
            AuraConformanceSurfaceV1::new(vec![serde_json::json!({"event": "ok"})], None),
        );
        artifact.insert_surface(
            ConformanceSurfaceName::SchedulerStep,
            AuraConformanceSurfaceV1::new(vec![serde_json::json!({"step": 0})], None),
        );
        let effects = effect_suffixes
            .iter()
            .map(|suffix| serde_json::json!({"effect_kind": "send_decision", "sid": suffix}))
            .collect::<Vec<_>>();
        artifact.insert_surface(
            ConformanceSurfaceName::Effect,
            AuraConformanceSurfaceV1::new(effects, None),
        );
        artifact
    }

    #[test]
    fn strict_profile_detects_reordering() {
        let baseline = artifact_with_effect_order(&["a", "b"]);
        let candidate = artifact_with_effect_order(&["b", "a"]);
        let tester = DifferentialTester::new(DifferentialProfile::Strict);
        let report = tester.compare(&baseline, &candidate);
        assert!(!report.equivalent);
        assert!(report.mismatch.is_some());
    }

    #[test]
    fn envelope_profile_allows_commutative_reordering() {
        let baseline = artifact_with_effect_order(&["a", "b"]);
        let candidate = artifact_with_effect_order(&["b", "a"]);
        let tester = DifferentialTester::new(DifferentialProfile::EnvelopeBounded);
        let report = tester.compare(&baseline, &candidate);
        assert!(report.equivalent, "commutative reordering should pass");
    }
}
