//! Cross-validation harness for Quint vs Lean/Telltale bridge artifacts.

use crate::bridge_format::{BridgeBundleV1, ProofBackendV1};
use std::collections::BTreeMap;

/// Result of one Quint model-checking evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuintCheckResult {
    /// Property id.
    pub property_id: String,
    /// Whether Quint accepted the property.
    pub holds: bool,
}

/// Discrepancy between Quint result and proof certificate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrossValidationDiscrepancy {
    /// Property id with mismatch.
    pub property_id: String,
    /// Quint model-checking outcome.
    pub quint_holds: bool,
    /// Proof certificate outcome.
    pub certificate_holds: bool,
    /// Certificate backend for mismatch context.
    pub backend: ProofBackendV1,
}

/// Cross-validation report across all imported properties.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrossValidationReport {
    /// Number of properties checked in Quint.
    pub properties_checked: usize,
    /// Number of certificates compared.
    pub certificates_compared: usize,
    /// Mismatch entries.
    pub discrepancies: Vec<CrossValidationDiscrepancy>,
}

impl CrossValidationReport {
    /// True when Quint and certificate outcomes agree for all compared properties.
    #[must_use]
    pub fn is_consistent(&self) -> bool {
        self.discrepancies.is_empty()
    }
}

/// Adapter for running Quint checks in cross-validation.
pub trait QuintModelCheckExecutor {
    /// Run Quint model checking for one property id and expression.
    fn check(&mut self, property_id: &str, expression: &str) -> Result<bool, String>;
}

/// Deterministic in-memory executor for tests and CI dry-runs.
#[derive(Debug, Default, Clone)]
pub struct StaticQuintExecutor {
    outcomes: BTreeMap<String, bool>,
}

impl StaticQuintExecutor {
    /// Build an executor from known outcomes.
    #[must_use]
    pub fn new(outcomes: BTreeMap<String, bool>) -> Self {
        Self { outcomes }
    }
}

impl QuintModelCheckExecutor for StaticQuintExecutor {
    fn check(&mut self, property_id: &str, _expression: &str) -> Result<bool, String> {
        Ok(*self.outcomes.get(property_id).unwrap_or(&true))
    }
}

/// Run cross-validation by:
/// 1. running Quint checks for bridge properties
/// 2. comparing Quint outcomes against proof certificates
/// 3. producing a discrepancy report
///
/// # Errors
///
/// Returns an error when Quint execution fails for any property.
pub fn run_cross_validation<E: QuintModelCheckExecutor>(
    bundle: &BridgeBundleV1,
    executor: &mut E,
) -> Result<CrossValidationReport, String> {
    let mut quint_results = BTreeMap::new();
    for property in &bundle.properties {
        let expression = property
            .target_expr
            .as_deref()
            .unwrap_or(property.source_expr.as_str());
        let holds = executor
            .check(&property.id, expression)
            .map_err(|err| format!("quint check failed for {}: {err}", property.id))?;
        quint_results.insert(property.id.clone(), holds);
    }

    let mut discrepancies = Vec::new();
    let mut compared = 0usize;
    for certificate in &bundle.certificates {
        let Some(quint_holds) = quint_results.get(&certificate.property_id).copied() else {
            continue;
        };
        compared = compared.saturating_add(1);
        if quint_holds != certificate.verified {
            discrepancies.push(CrossValidationDiscrepancy {
                property_id: certificate.property_id.clone(),
                quint_holds,
                certificate_holds: certificate.verified,
                backend: certificate.backend,
            });
        }
    }

    Ok(CrossValidationReport {
        properties_checked: quint_results.len(),
        certificates_compared: compared,
        discrepancies,
    })
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::bridge_format::{
        BridgeBundleV1, ProofBackendV1, ProofCertificateV1, PropertyClassV1, PropertyInterchangeV1,
    };

    fn sample_bundle() -> BridgeBundleV1 {
        let mut bundle = BridgeBundleV1::default();
        bundle.properties.push(PropertyInterchangeV1 {
            id: "p1".to_string(),
            class: PropertyClassV1::Safety,
            source_backend: "telltale".to_string(),
            source_expr: "coherent".to_string(),
            target_expr: Some("coherent".to_string()),
            assumptions: vec![],
            metadata: BTreeMap::default(),
        });
        bundle.certificates.push(ProofCertificateV1 {
            certificate_id: "c1".to_string(),
            backend: ProofBackendV1::Lean,
            property_id: "p1".to_string(),
            statement_digest_hex: "11".repeat(32),
            artifact_digest_hex: "22".repeat(32),
            verified: true,
            verified_at_ms: Some(123),
            toolchain: BTreeMap::default(),
        });
        bundle
    }

    #[test]
    fn cross_validation_passes_when_outcomes_match() {
        let bundle = sample_bundle();
        let mut executor = StaticQuintExecutor::new(BTreeMap::from([("p1".to_string(), true)]));
        let report = run_cross_validation(&bundle, &mut executor).expect("run");
        assert!(report.is_consistent());
        assert_eq!(report.properties_checked, 1);
        assert_eq!(report.certificates_compared, 1);
    }

    #[test]
    fn cross_validation_reports_discrepancies() {
        let bundle = sample_bundle();
        let mut executor = StaticQuintExecutor::new(BTreeMap::from([("p1".to_string(), false)]));
        let report = run_cross_validation(&bundle, &mut executor).expect("run");
        assert_eq!(report.discrepancies.len(), 1);
        assert!(!report.is_consistent());
    }
}
