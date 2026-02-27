//! Conformance artifact replay and verification helpers.

use aura_core::{AuraConformanceArtifactV1, ConformanceValidationError};
use std::fs;
use std::path::Path;

/// Replay verification report for one conformance artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConformanceReplayReport {
    /// Number of surfaces with verified digests.
    pub surfaces_verified: usize,
    /// Number of per-surface step hash vectors verified.
    pub step_hash_sets_verified: usize,
    /// Whether full run digest was verified.
    pub run_digest_verified: bool,
}

/// Errors returned by conformance artifact replay checks.
#[derive(Debug, thiserror::Error)]
pub enum ConformanceReplayError {
    /// IO read failure.
    #[error("failed to read conformance artifact at {path}: {source}")]
    Io {
        /// Path that failed.
        path: String,
        /// Underlying IO error.
        source: std::io::Error,
    },
    /// JSON decode failure.
    #[error("failed to decode conformance artifact JSON: {source}")]
    Decode {
        /// Underlying decode error.
        source: serde_json::Error,
    },
    /// Schema/required-surface validation failure.
    #[error("invalid conformance artifact: {source}")]
    Validation {
        /// Underlying validation error.
        source: ConformanceValidationError,
    },
    /// Digest mismatch between stored and recomputed artifact.
    #[error("conformance digest mismatch: {field}")]
    DigestMismatch {
        /// Field name that mismatched.
        field: String,
    },
}

/// Load conformance artifact from JSON bytes.
///
/// # Errors
///
/// Returns [`ConformanceReplayError::Decode`] for invalid JSON payloads.
pub fn load_conformance_artifact_bytes(
    payload: &[u8],
) -> Result<AuraConformanceArtifactV1, ConformanceReplayError> {
    AuraConformanceArtifactV1::from_json_slice(payload)
        .map_err(|source| ConformanceReplayError::Decode { source })
}

/// Load conformance artifact from file path.
///
/// # Errors
///
/// Returns IO/decode errors when reading or decoding fails.
pub fn load_conformance_artifact_file(
    path: impl AsRef<Path>,
) -> Result<AuraConformanceArtifactV1, ConformanceReplayError> {
    let path_ref = path.as_ref();
    let payload = fs::read(path_ref).map_err(|source| ConformanceReplayError::Io {
        path: path_ref.display().to_string(),
        source,
    })?;
    load_conformance_artifact_bytes(&payload)
}

/// Verify one conformance artifact by recomputing hashes and required surfaces.
///
/// # Errors
///
/// Returns validation or digest mismatch errors.
pub fn replay_conformance_artifact(
    artifact: &AuraConformanceArtifactV1,
) -> Result<ConformanceReplayReport, ConformanceReplayError> {
    artifact
        .validate_required_surfaces()
        .map_err(|source| ConformanceReplayError::Validation { source })?;

    let mut recomputed = artifact.clone();
    recomputed
        .recompute_digests()
        .map_err(|source| ConformanceReplayError::Decode { source })?;

    let step_hash_sets_verified = if artifact.step_hashes.is_empty() {
        0
    } else {
        if artifact.step_hashes != recomputed.step_hashes {
            return Err(ConformanceReplayError::DigestMismatch {
                field: "step_hashes".to_string(),
            });
        }
        artifact.step_hashes.len()
    };

    let mut surfaces_verified = 0usize;
    for (surface_name, surface) in &artifact.surfaces {
        if let Some(expected_digest) = &surface.digest_hex {
            let Some(recomputed_surface) = recomputed.surfaces.get(surface_name) else {
                return Err(ConformanceReplayError::DigestMismatch {
                    field: "surface_digest".to_string(),
                });
            };
            if recomputed_surface.digest_hex.as_ref() != Some(expected_digest) {
                return Err(ConformanceReplayError::DigestMismatch {
                    field: "surface_digest".to_string(),
                });
            }
            surfaces_verified = surfaces_verified.saturating_add(1);
        }
    }

    let run_digest_verified = if artifact.run_digest_hex.is_some() {
        if artifact.run_digest_hex != recomputed.run_digest_hex {
            return Err(ConformanceReplayError::DigestMismatch {
                field: "run_digest".to_string(),
            });
        }
        true
    } else {
        false
    };

    Ok(ConformanceReplayReport {
        surfaces_verified,
        step_hash_sets_verified,
        run_digest_verified,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{
        AuraConformanceRunMetadataV1, AuraConformanceSurfaceV1, ConformanceSurfaceName,
    };

    #[test]
    fn replay_verification_passes_for_self_consistent_artifact() {
        let mut artifact = AuraConformanceArtifactV1::new(AuraConformanceRunMetadataV1 {
            target: "native".to_string(),
            profile: "native_coop".to_string(),
            scenario: "replay_pass".to_string(),
            seed: Some(1),
            commit: None,
            async_host_transcript_entries: None,
            async_host_transcript_digest_hex: None,
        });
        for surface in ConformanceSurfaceName::REQUIRED {
            artifact.insert_surface(
                surface,
                AuraConformanceSurfaceV1::new(vec![serde_json::json!({"surface": "ok"})], None),
            );
        }
        artifact
            .recompute_digests()
            .expect("digest recompute should succeed");

        let report = replay_conformance_artifact(&artifact).expect("artifact should verify");
        assert_eq!(report.surfaces_verified, 3);
        assert_eq!(report.step_hash_sets_verified, 3);
        assert!(report.run_digest_verified);
    }

    #[test]
    fn replay_verification_fails_on_digest_mismatch() {
        let mut artifact = AuraConformanceArtifactV1::new(AuraConformanceRunMetadataV1 {
            target: "native".to_string(),
            profile: "native_coop".to_string(),
            scenario: "replay_fail".to_string(),
            seed: Some(2),
            commit: None,
            async_host_transcript_entries: None,
            async_host_transcript_digest_hex: None,
        });
        for surface in ConformanceSurfaceName::REQUIRED {
            artifact.insert_surface(
                surface,
                AuraConformanceSurfaceV1::new(vec![serde_json::json!({"surface": "ok"})], None),
            );
        }
        artifact
            .recompute_digests()
            .expect("digest recompute should succeed");
        artifact.run_digest_hex.replace("bad_digest".to_string());

        let err = replay_conformance_artifact(&artifact).expect_err("must fail on bad digest");
        assert!(matches!(err, ConformanceReplayError::DigestMismatch { .. }));
    }
}
