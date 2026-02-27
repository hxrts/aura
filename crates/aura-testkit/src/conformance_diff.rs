//! Law-based conformance diff engine for `AuraConformanceArtifactV1`.

use aura_core::{
    envelope_law_class, AuraConformanceArtifactV1, AuraEnvelopeLawClass, ConformanceSurfaceName,
};
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// Effect envelope law registry used by diffing.
#[derive(Debug, Clone, Default)]
pub struct EnvelopeLawRegistry {
    classes: BTreeMap<String, AuraEnvelopeLawClass>,
}

impl EnvelopeLawRegistry {
    /// Create a registry from Aura's canonical envelope classification table.
    #[must_use]
    pub fn from_aura_registry() -> Self {
        let mut out = Self::default();
        for (kind, class) in aura_core::AURA_EFFECT_ENVELOPE_CLASSIFICATIONS {
            out.classes.insert((*kind).to_string(), *class);
        }
        out
    }

    /// Resolve law class for one effect kind.
    #[must_use]
    pub fn class_for(&self, effect_kind: &str) -> Option<AuraEnvelopeLawClass> {
        self.classes
            .get(effect_kind)
            .copied()
            .or_else(|| envelope_law_class(effect_kind))
    }
}

/// First mismatch payload for diff reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConformanceMismatch {
    /// Surface where mismatch occurred.
    pub surface: ConformanceSurfaceName,
    /// Optional step index where mismatch is first observed.
    pub step_index: Option<usize>,
    /// Optional law context.
    pub law: Option<AuraEnvelopeLawClass>,
    /// Human-readable mismatch detail.
    pub detail: String,
}

/// Diff result for one artifact comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConformanceDiffReport {
    /// True when all compared surfaces are equivalent under declared laws.
    pub equivalent: bool,
    /// First mismatch (if any).
    pub first_mismatch: Option<ConformanceMismatch>,
}

impl ConformanceDiffReport {
    fn equivalent() -> Self {
        Self {
            equivalent: true,
            first_mismatch: None,
        }
    }

    fn mismatch(mismatch: ConformanceMismatch) -> Self {
        Self {
            equivalent: false,
            first_mismatch: Some(mismatch),
        }
    }
}

/// Compare two conformance artifacts using per-surface law-aware comparers.
#[must_use]
pub fn compare_artifacts(
    baseline: &AuraConformanceArtifactV1,
    candidate: &AuraConformanceArtifactV1,
    registry: &EnvelopeLawRegistry,
) -> ConformanceDiffReport {
    if let Some(mismatch) = compare_surface_strict(
        baseline,
        candidate,
        ConformanceSurfaceName::Observable,
        None,
    ) {
        return ConformanceDiffReport::mismatch(mismatch);
    }

    if let Some(mismatch) = compare_scheduler_surface(baseline, candidate) {
        return ConformanceDiffReport::mismatch(mismatch);
    }

    if let Some(mismatch) = compare_effect_surface(baseline, candidate, registry) {
        return ConformanceDiffReport::mismatch(mismatch);
    }

    ConformanceDiffReport::equivalent()
}

fn compare_surface_strict(
    baseline: &AuraConformanceArtifactV1,
    candidate: &AuraConformanceArtifactV1,
    surface: ConformanceSurfaceName,
    law: Option<AuraEnvelopeLawClass>,
) -> Option<ConformanceMismatch> {
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
        return None;
    }

    let max_len = baseline_entries.len().max(candidate_entries.len());
    for idx in 0..max_len {
        if baseline_entries.get(idx) != candidate_entries.get(idx) {
            return Some(ConformanceMismatch {
                surface,
                step_index: Some(idx),
                law,
                detail: "strict surface mismatch".to_string(),
            });
        }
    }

    Some(ConformanceMismatch {
        surface,
        step_index: None,
        law,
        detail: "strict surface mismatch".to_string(),
    })
}

fn compare_scheduler_surface(
    baseline: &AuraConformanceArtifactV1,
    candidate: &AuraConformanceArtifactV1,
) -> Option<ConformanceMismatch> {
    let surface = ConformanceSurfaceName::SchedulerStep;
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
        return None;
    }

    let baseline_norm = normalized_multiset(baseline_entries);
    let candidate_norm = normalized_multiset(candidate_entries);
    if baseline_norm == candidate_norm {
        return None;
    }

    Some(ConformanceMismatch {
        surface,
        step_index: None,
        law: Some(AuraEnvelopeLawClass::Commutative),
        detail: "scheduler entries differ after normalized multiset comparison".to_string(),
    })
}

fn compare_effect_surface(
    baseline: &AuraConformanceArtifactV1,
    candidate: &AuraConformanceArtifactV1,
    registry: &EnvelopeLawRegistry,
) -> Option<ConformanceMismatch> {
    let surface = ConformanceSurfaceName::Effect;
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

    for entry in baseline_entries.iter().chain(candidate_entries.iter()) {
        let Some(kind) = effect_kind(entry) else {
            return Some(ConformanceMismatch {
                surface,
                step_index: None,
                law: None,
                detail: "effect entry missing effect_kind".to_string(),
            });
        };
        if registry.class_for(kind).is_none() {
            return Some(ConformanceMismatch {
                surface,
                step_index: None,
                law: None,
                detail: format!("unclassified effect_kind: {kind}"),
            });
        }
    }

    // Strict subset: exact order/value equality required.
    let baseline_strict: Vec<_> = baseline_entries
        .iter()
        .filter(|entry| classify(entry, registry) == Some(AuraEnvelopeLawClass::Strict))
        .cloned()
        .collect();
    let candidate_strict: Vec<_> = candidate_entries
        .iter()
        .filter(|entry| classify(entry, registry) == Some(AuraEnvelopeLawClass::Strict))
        .cloned()
        .collect();
    if baseline_strict != candidate_strict {
        let max_len = baseline_strict.len().max(candidate_strict.len());
        let first_idx =
            (0..max_len).find(|idx| baseline_strict.get(*idx) != candidate_strict.get(*idx));
        return Some(ConformanceMismatch {
            surface,
            step_index: first_idx,
            law: Some(AuraEnvelopeLawClass::Strict),
            detail: "strict effect entries diverged".to_string(),
        });
    }

    // Commutative subset: order-insensitive multiset comparison.
    let baseline_comm = normalized_multiset(
        &baseline_entries
            .iter()
            .filter(|entry| classify(entry, registry) == Some(AuraEnvelopeLawClass::Commutative))
            .cloned()
            .collect::<Vec<_>>(),
    );
    let candidate_comm = normalized_multiset(
        &candidate_entries
            .iter()
            .filter(|entry| classify(entry, registry) == Some(AuraEnvelopeLawClass::Commutative))
            .cloned()
            .collect::<Vec<_>>(),
    );
    if baseline_comm != candidate_comm {
        return Some(ConformanceMismatch {
            surface,
            step_index: None,
            law: Some(AuraEnvelopeLawClass::Commutative),
            detail: "commutative effect entries diverged after multiset normalization".to_string(),
        });
    }

    // Algebraic subset: compare reduced normal forms (kind -> set(hash(entry))).
    let baseline_algebraic = algebraic_normal_form(baseline_entries, registry);
    let candidate_algebraic = algebraic_normal_form(candidate_entries, registry);
    if baseline_algebraic != candidate_algebraic {
        return Some(ConformanceMismatch {
            surface,
            step_index: None,
            law: Some(AuraEnvelopeLawClass::Algebraic),
            detail: "algebraic effect normal forms diverged".to_string(),
        });
    }

    None
}

fn classify(entry: &JsonValue, registry: &EnvelopeLawRegistry) -> Option<AuraEnvelopeLawClass> {
    effect_kind(entry).and_then(|kind| registry.class_for(kind))
}

fn effect_kind(entry: &JsonValue) -> Option<&str> {
    entry.get("effect_kind").and_then(JsonValue::as_str)
}

fn normalized_multiset(entries: &[JsonValue]) -> Vec<String> {
    let mut out: Vec<_> = entries
        .iter()
        .map(stable_hash_hex_json)
        .map(Result::unwrap_or_default)
        .collect();
    out.sort_unstable();
    out
}

fn algebraic_normal_form(
    entries: &[JsonValue],
    registry: &EnvelopeLawRegistry,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut by_kind: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for entry in entries {
        if classify(entry, registry) != Some(AuraEnvelopeLawClass::Algebraic) {
            continue;
        }
        if let Some(kind) = effect_kind(entry) {
            by_kind
                .entry(kind.to_string())
                .or_default()
                .insert(stable_hash_hex_json(entry).unwrap_or_default());
        }
    }
    by_kind
}

fn stable_hash_hex_json(value: &JsonValue) -> Result<String, serde_json::Error> {
    let payload = serde_json::to_vec(value)?;
    let mut hasher = Sha256::new();
    hasher.update(&payload);
    let digest = hasher.finalize();
    Ok(hex::encode(digest))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{AuraConformanceRunMetadataV1, AuraConformanceSurfaceV1};

    fn artifact(effect_entries: Vec<JsonValue>) -> AuraConformanceArtifactV1 {
        let mut artifact = AuraConformanceArtifactV1::new(AuraConformanceRunMetadataV1 {
            target: "native".to_string(),
            profile: "native_coop".to_string(),
            scenario: "diff".to_string(),
            seed: None,
            commit: None,
            async_host_transcript_entries: None,
            async_host_transcript_digest_hex: None,
        });
        artifact.insert_surface(
            ConformanceSurfaceName::Observable,
            AuraConformanceSurfaceV1::new(vec![serde_json::json!({"o": 1})], None),
        );
        artifact.insert_surface(
            ConformanceSurfaceName::SchedulerStep,
            AuraConformanceSurfaceV1::new(vec![serde_json::json!({"s": 1})], None),
        );
        artifact.insert_surface(
            ConformanceSurfaceName::Effect,
            AuraConformanceSurfaceV1::new(effect_entries, None),
        );
        artifact
    }

    #[test]
    fn strict_law_is_order_sensitive() {
        let registry = EnvelopeLawRegistry::from_aura_registry();
        let baseline = artifact(vec![
            serde_json::json!({"effect_kind": "handle_recv", "id": 1}),
            serde_json::json!({"effect_kind": "handle_recv", "id": 2}),
        ]);
        let candidate = artifact(vec![
            serde_json::json!({"effect_kind": "handle_recv", "id": 2}),
            serde_json::json!({"effect_kind": "handle_recv", "id": 1}),
        ]);

        let report = compare_artifacts(&baseline, &candidate, &registry);
        assert!(!report.equivalent);
        assert_eq!(
            report.first_mismatch.expect("mismatch").law,
            Some(AuraEnvelopeLawClass::Strict)
        );
    }

    #[test]
    fn commutative_law_is_order_insensitive() {
        let registry = EnvelopeLawRegistry::from_aura_registry();
        let baseline = artifact(vec![
            serde_json::json!({"effect_kind": "send_decision", "id": 1}),
            serde_json::json!({"effect_kind": "invoke_step", "id": 2}),
        ]);
        let candidate = artifact(vec![
            serde_json::json!({"effect_kind": "invoke_step", "id": 2}),
            serde_json::json!({"effect_kind": "send_decision", "id": 1}),
        ]);

        let report = compare_artifacts(&baseline, &candidate, &registry);
        assert!(report.equivalent);
    }

    #[test]
    fn algebraic_law_reducer_is_idempotent() {
        let registry = EnvelopeLawRegistry::from_aura_registry();
        let duplicated = vec![
            serde_json::json!({"effect_kind": "topology_event", "id": 1}),
            serde_json::json!({"effect_kind": "topology_event", "id": 1}),
        ];

        let first = algebraic_normal_form(&duplicated, &registry);
        let second = algebraic_normal_form(&duplicated, &registry);
        assert_eq!(first, second);
    }

    #[test]
    fn reports_first_mismatch_with_surface_and_law_context() {
        let registry = EnvelopeLawRegistry::from_aura_registry();
        let baseline = artifact(vec![
            serde_json::json!({"effect_kind": "handle_release", "x": 1}),
        ]);
        let candidate = artifact(vec![
            serde_json::json!({"effect_kind": "handle_release", "x": 2}),
        ]);

        let report = compare_artifacts(&baseline, &candidate, &registry);
        assert!(!report.equivalent);
        let mismatch = report.first_mismatch.expect("expected mismatch");
        assert_eq!(mismatch.surface, ConformanceSurfaceName::Effect);
        assert_eq!(mismatch.law, Some(AuraEnvelopeLawClass::Strict));
    }
}
