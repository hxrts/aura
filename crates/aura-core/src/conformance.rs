//! Conformance artifact and envelope classification types.
//!
//! These types are shared by native/wasm parity harnesses and CI conformance checks.

use crate::crypto::hash;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, BTreeSet};

/// Schema version for [`AuraConformanceArtifactV1`].
pub const AURA_CONFORMANCE_SCHEMA_VERSION: &str = "aura.conformance.v1";

fn default_schema_version() -> String {
    AURA_CONFORMANCE_SCHEMA_VERSION.to_string()
}

fn normalize_schema_version(raw: &str) -> String {
    if raw == "1" {
        AURA_CONFORMANCE_SCHEMA_VERSION.to_string()
    } else {
        raw.to_string()
    }
}

fn deserialize_schema_version<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum SchemaVersionValue {
        String(String),
        Integer(u64),
    }

    let parsed = SchemaVersionValue::deserialize(deserializer)?;
    Ok(match parsed {
        SchemaVersionValue::String(version) => normalize_schema_version(&version),
        SchemaVersionValue::Integer(version) => normalize_schema_version(&version.to_string()),
    })
}

/// Required conformance surface names.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum ConformanceSurfaceName {
    /// Protocol-visible outputs after normalization.
    #[default]
    Observable,
    /// Logical scheduler progression (`step`, `session`, `role`, transition shape).
    SchedulerStep,
    /// Effect envelope stream after normalization/canonicalization.
    Effect,
}

impl ConformanceSurfaceName {
    /// Every run must include all declared conformance surfaces.
    pub const REQUIRED: [Self; 3] = [Self::Observable, Self::SchedulerStep, Self::Effect];
}

/// Run metadata captured in conformance artifacts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AuraVmDeterminismProfileV1 {
    /// Stable policy selector reference used by the host runtime.
    pub policy_ref: String,
    /// Aura protocol class bound to this run.
    pub protocol_class: String,
    /// Aura-selected runtime execution mode for this run.
    #[serde(default)]
    pub runtime_mode: String,
    /// Declared scheduler envelope class for this run.
    #[serde(default)]
    pub scheduler_envelope_class: String,
    /// Declared upper bound for threaded wave width, when applicable.
    #[serde(default)]
    pub declared_wave_width_bound: Option<usize>,
    /// Telltale determinism mode identifier.
    pub determinism_mode: String,
    /// Telltale effect determinism tier identifier.
    pub effect_determinism_tier: String,
    /// Telltale communication replay mode identifier.
    pub communication_replay_mode: String,
}

/// Run metadata captured in conformance artifacts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AuraConformanceRunMetadataV1 {
    /// Runtime target (`native`, `wasm`, etc).
    pub target: String,
    /// Profile (`native_coop`, `native_threaded`, `wasm_coop`, etc).
    pub profile: String,
    /// Scenario/protocol identifier.
    pub scenario: String,
    /// Optional deterministic seed.
    pub seed: Option<u64>,
    /// Optional source commit hash.
    pub commit: Option<String>,
    /// Optional async host transcript entry count.
    #[serde(default)]
    pub async_host_transcript_entries: Option<usize>,
    /// Optional async host transcript digest.
    #[serde(default)]
    pub async_host_transcript_digest_hex: Option<String>,
    /// Optional VM determinism profile selected for this run.
    #[serde(default)]
    pub vm_determinism_profile: Option<AuraVmDeterminismProfileV1>,
}

/// Surface payload captured for one conformance dimension.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AuraConformanceSurfaceV1 {
    /// Canonicalized entries for this surface.
    pub entries: Vec<JsonValue>,
    /// Optional stable digest for this surface.
    pub digest_hex: Option<String>,
}

impl AuraConformanceSurfaceV1 {
    /// Build one surface payload.
    #[must_use]
    pub fn new(entries: Vec<JsonValue>, digest_hex: Option<String>) -> Self {
        Self {
            entries,
            digest_hex,
        }
    }
}

/// Versioned conformance artifact used by native/wasm parity harnesses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuraConformanceArtifactV1 {
    /// Schema version identifier.
    #[serde(
        default = "default_schema_version",
        deserialize_with = "deserialize_schema_version"
    )]
    pub schema_version: String,
    /// Run metadata.
    pub metadata: AuraConformanceRunMetadataV1,
    /// Required conformance surfaces.
    pub surfaces: BTreeMap<ConformanceSurfaceName, AuraConformanceSurfaceV1>,
    /// Optional per-surface step hashes.
    #[serde(default)]
    pub step_hashes: BTreeMap<ConformanceSurfaceName, Vec<String>>,
    /// Optional full-run canonical digest.
    pub run_digest_hex: Option<String>,
}

impl AuraConformanceArtifactV1 {
    /// Create an empty artifact for one run.
    #[must_use]
    pub fn new(metadata: AuraConformanceRunMetadataV1) -> Self {
        Self {
            schema_version: default_schema_version(),
            metadata,
            surfaces: BTreeMap::new(),
            step_hashes: BTreeMap::new(),
            run_digest_hex: None,
        }
    }

    /// Insert/update one conformance surface payload.
    pub fn insert_surface(
        &mut self,
        surface: ConformanceSurfaceName,
        payload: AuraConformanceSurfaceV1,
    ) {
        self.surfaces.insert(surface, payload);
    }

    /// Return missing required surfaces.
    #[must_use]
    pub fn missing_required_surfaces(&self) -> Vec<ConformanceSurfaceName> {
        ConformanceSurfaceName::REQUIRED
            .iter()
            .copied()
            .filter(|surface| !self.surfaces.contains_key(surface))
            .collect()
    }

    /// Validate the artifact declares all required conformance surfaces.
    ///
    /// # Errors
    ///
    /// Returns [`ConformanceValidationError::MissingRequiredSurfaces`] when at
    /// least one required surface is not present.
    pub fn validate_required_surfaces(&self) -> Result<(), ConformanceValidationError> {
        let missing = self.missing_required_surfaces();
        if missing.is_empty() {
            Ok(())
        } else {
            Err(ConformanceValidationError::MissingRequiredSurfaces { missing })
        }
    }

    /// Serialize the artifact as canonical JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails.
    pub fn canonical_json(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    /// Deserialize the artifact from canonical JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if deserialization fails.
    pub fn from_json_slice(payload: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(payload)
    }

    /// Recompute per-step hashes, per-surface digests, and full-run digest.
    ///
    /// # Errors
    ///
    /// Returns an error if any entry fails canonical serialization.
    pub fn recompute_digests(&mut self) -> Result<(), serde_json::Error> {
        let surface_keys: Vec<_> = self.surfaces.keys().copied().collect();
        for surface_key in surface_keys {
            if let Some(surface) = self.surfaces.get_mut(&surface_key) {
                let step_hashes: Result<Vec<_>, _> = surface
                    .entries
                    .iter()
                    .map(stable_hash_hex_from_serializable)
                    .collect();
                self.step_hashes.insert(surface_key, step_hashes?);
                surface.digest_hex = Some(stable_hash_hex_from_serializable(&surface.entries)?);
            }
        }

        let run_digest_payload = serde_json::json!({
            "schema_version": self.schema_version,
            "metadata": self.metadata,
            "surfaces": self.surfaces,
            "step_hashes": self.step_hashes,
        });
        self.run_digest_hex = Some(stable_hash_hex_from_serializable(&run_digest_payload)?);
        Ok(())
    }
}

/// Conformance envelope law class used by differential comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuraEnvelopeLawClass {
    /// Byte-identical comparison required.
    Strict,
    /// Order-insensitive comparison allowed.
    Commutative,
    /// Reduced by a declared normalization law before comparison.
    Algebraic,
}

/// Explicit effect-envelope registry for parity checks.
///
/// Keep this list aligned with effect kinds emitted by the admitted Telltale runtime.
pub const AURA_EFFECT_ENVELOPE_CLASSIFICATIONS: &[(&str, AuraEnvelopeLawClass)] = &[
    ("send_decision", AuraEnvelopeLawClass::Commutative),
    ("handle_recv", AuraEnvelopeLawClass::Strict),
    ("handle_choose", AuraEnvelopeLawClass::Strict),
    ("invoke_step", AuraEnvelopeLawClass::Commutative),
    ("handle_acquire", AuraEnvelopeLawClass::Strict),
    ("handle_release", AuraEnvelopeLawClass::Strict),
    ("output_condition_hint", AuraEnvelopeLawClass::Strict),
    ("topology_event", AuraEnvelopeLawClass::Algebraic),
];

/// Validation errors for conformance artifacts and effect-envelope registries.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConformanceValidationError {
    /// Missing one or more required conformance surfaces.
    #[error("conformance artifact is missing required surfaces: {missing:?}")]
    MissingRequiredSurfaces {
        /// Missing surfaces.
        missing: Vec<ConformanceSurfaceName>,
    },
    /// Effect trace contained an unclassified envelope kind.
    #[error("unclassified effect envelope kinds: {kinds:?}")]
    UnclassifiedEnvelopeKinds {
        /// Unknown/unclassified effect kinds.
        kinds: Vec<String>,
    },
}

/// Lookup law class for one effect envelope kind.
#[must_use]
pub fn envelope_law_class(kind: &str) -> Option<AuraEnvelopeLawClass> {
    AURA_EFFECT_ENVELOPE_CLASSIFICATIONS
        .iter()
        .find_map(|(registered_kind, class)| {
            if *registered_kind == kind {
                Some(*class)
            } else {
                None
            }
        })
}

/// Ensure all effect kinds are explicitly classified.
///
/// # Errors
///
/// Returns [`ConformanceValidationError::UnclassifiedEnvelopeKinds`] when one or
/// more kinds are missing from [`AURA_EFFECT_ENVELOPE_CLASSIFICATIONS`].
pub fn assert_effect_kinds_classified<I, S>(
    effect_kinds: I,
) -> Result<(), ConformanceValidationError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut missing = BTreeSet::new();
    for kind in effect_kinds {
        let effect_kind = kind.as_ref();
        if envelope_law_class(effect_kind).is_none() {
            missing.insert(effect_kind.to_string());
        }
    }

    if missing.is_empty() {
        Ok(())
    } else {
        Err(ConformanceValidationError::UnclassifiedEnvelopeKinds {
            kinds: missing.into_iter().collect(),
        })
    }
}

fn stable_hash_hex_from_serializable<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    let bytes = serde_json::to_vec(value)?;
    let digest = hash::hash(&bytes);
    Ok(hex::encode(digest))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_validation_requires_all_surfaces() {
        let metadata = AuraConformanceRunMetadataV1 {
            target: "native".to_string(),
            profile: "native_coop".to_string(),
            scenario: "ping_pong".to_string(),
            seed: Some(42),
            commit: None,
            async_host_transcript_entries: None,
            async_host_transcript_digest_hex: None,
            vm_determinism_profile: None,
        };
        let mut artifact = AuraConformanceArtifactV1::new(metadata);
        artifact.insert_surface(
            ConformanceSurfaceName::Observable,
            AuraConformanceSurfaceV1::new(vec![], None),
        );
        artifact.insert_surface(
            ConformanceSurfaceName::Effect,
            AuraConformanceSurfaceV1::new(vec![], None),
        );

        let err = match artifact.validate_required_surfaces() {
            Ok(()) => panic!("missing scheduler_step surface must fail"),
            Err(err) => err,
        };
        assert!(matches!(
            err,
            ConformanceValidationError::MissingRequiredSurfaces { .. }
        ));
    }

    #[test]
    fn artifact_validation_passes_when_all_surfaces_declared() {
        let metadata = AuraConformanceRunMetadataV1 {
            target: "wasm".to_string(),
            profile: "wasm_coop".to_string(),
            scenario: "ping_pong".to_string(),
            seed: Some(7),
            commit: Some("deadbeef".to_string()),
            async_host_transcript_entries: None,
            async_host_transcript_digest_hex: None,
            vm_determinism_profile: None,
        };
        let mut artifact = AuraConformanceArtifactV1::new(metadata);
        for surface in ConformanceSurfaceName::REQUIRED {
            artifact.insert_surface(surface, AuraConformanceSurfaceV1::new(vec![], None));
        }
        if let Err(err) = artifact.validate_required_surfaces() {
            panic!("all required surfaces should pass: {err}");
        }
    }

    #[test]
    fn effect_kind_registry_rejects_unknown_kinds() {
        let err = match assert_effect_kinds_classified(["send_decision", "new_kind"]) {
            Ok(()) => panic!("unknown kinds must fail"),
            Err(err) => err,
        };
        assert!(matches!(
            err,
            ConformanceValidationError::UnclassifiedEnvelopeKinds { .. }
        ));
    }

    #[test]
    fn effect_kind_registry_covers_current_runtime_effect_kinds() {
        if let Err(err) = assert_effect_kinds_classified([
            "send_decision",
            "handle_recv",
            "handle_choose",
            "invoke_step",
            "handle_acquire",
            "handle_release",
            "output_condition_hint",
            "topology_event",
        ]) {
            panic!("current Telltale runtime effect kinds should be classified: {err}");
        }
    }

    #[test]
    fn artifact_digest_recompute_is_deterministic() {
        let metadata = AuraConformanceRunMetadataV1 {
            target: "native".to_string(),
            profile: "native_coop".to_string(),
            scenario: "digest_determinism".to_string(),
            seed: Some(99),
            commit: Some("abc123".to_string()),
            async_host_transcript_entries: None,
            async_host_transcript_digest_hex: None,
            vm_determinism_profile: None,
        };

        let mut first = AuraConformanceArtifactV1::new(metadata.clone());
        let mut second = AuraConformanceArtifactV1::new(metadata);
        for surface in ConformanceSurfaceName::REQUIRED {
            let payload = AuraConformanceSurfaceV1::new(
                vec![serde_json::json!({"surface": format!("{surface:?}")})],
                None,
            );
            first.insert_surface(surface, payload.clone());
            second.insert_surface(surface, payload);
        }

        if let Err(err) = first.recompute_digests() {
            panic!("first digest recompute should succeed: {err}");
        }
        if let Err(err) = second.recompute_digests() {
            panic!("second digest recompute should succeed: {err}");
        }

        assert_eq!(first.step_hashes, second.step_hashes);
        assert_eq!(first.run_digest_hex, second.run_digest_hex);
    }

    #[test]
    fn legacy_numeric_schema_version_deserializes() {
        let payload = serde_json::json!({
            "schema_version": 1,
            "metadata": {
                "target": "native",
                "profile": "native_coop",
                "scenario": "legacy",
                "seed": null,
                "commit": null,
                "async_host_transcript_entries": null,
                "async_host_transcript_digest_hex": null,
                "vm_determinism_profile": null
            },
            "surfaces": {},
            "step_hashes": {},
            "run_digest_hex": null
        });

        let decoded: AuraConformanceArtifactV1 = match serde_json::from_value(payload) {
            Ok(decoded) => decoded,
            Err(err) => panic!("legacy numeric schema should decode: {err}"),
        };
        assert_eq!(decoded.schema_version, AURA_CONFORMANCE_SCHEMA_VERSION);
    }

    #[test]
    fn legacy_string_schema_version_deserializes() {
        let payload = serde_json::json!({
            "schema_version": "1",
            "metadata": {
                "target": "native",
                "profile": "native_coop",
                "scenario": "legacy",
                "seed": null,
                "commit": null,
                "async_host_transcript_entries": null,
                "async_host_transcript_digest_hex": null,
                "vm_determinism_profile": null
            },
            "surfaces": {},
            "step_hashes": {},
            "run_digest_hex": null
        });

        let decoded: AuraConformanceArtifactV1 = match serde_json::from_value(payload) {
            Ok(decoded) => decoded,
            Err(err) => panic!("legacy string schema should decode: {err}"),
        };
        assert_eq!(decoded.schema_version, AURA_CONFORMANCE_SCHEMA_VERSION);
    }
}
