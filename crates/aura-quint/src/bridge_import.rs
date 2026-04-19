//! Telltale/Lean -> Quint bridge import helpers.

use crate::bridge_format::{BridgeBundleV1, ProofCertificateV1, PropertyInterchangeV1};

/// Import errors for bridge ingestion.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BridgeImportError {
    /// Property payload missing required fields for Quint generation.
    #[error("invalid property payload for Quint generation: {message}")]
    InvalidProperty {
        /// Validation details.
        message: String,
    },
}

/// Parse bridge properties that are valid import candidates for Quint.
#[must_use]
pub fn parse_telltale_properties(bundle: &BridgeBundleV1) -> Vec<PropertyInterchangeV1> {
    bundle
        .properties
        .iter()
        .filter(|property| {
            matches!(
                property.source_backend.as_str(),
                "telltale" | "lean" | "quint"
            )
        })
        .cloned()
        .collect()
}

fn normalized_module_name(module_name: &str) -> String {
    module_name.replace(' ', "_")
}

fn normalized_property_ident(property_id: &str) -> String {
    property_id.replace(['-', '.'], "_")
}

fn import_expression(property: &PropertyInterchangeV1) -> Result<&str, BridgeImportError> {
    if property.id.trim().is_empty() {
        return Err(BridgeImportError::InvalidProperty {
            message: "property id must not be empty".to_string(),
        });
    }

    let expression = property
        .target_expr
        .as_deref()
        .unwrap_or(property.source_expr.as_str())
        .trim();
    if expression.is_empty() {
        return Err(BridgeImportError::InvalidProperty {
            message: format!("property {} has empty expression", property.id),
        });
    }

    Ok(expression)
}

/// Generate a Quint module containing imported invariants/properties.
///
/// # Errors
///
/// Returns [`BridgeImportError::InvalidProperty`] when required identifiers are missing.
pub fn generate_quint_invariant_module(
    module_name: &str,
    properties: &[PropertyInterchangeV1],
) -> Result<String, BridgeImportError> {
    let normalized_module = normalized_module_name(module_name);
    let mut out = format!("module {} {{\n", normalized_module);

    for property in properties {
        let expression = import_expression(property)?;
        let ident = normalized_property_ident(&property.id);
        out.push_str(&format!("  // imported from {}\n", property.source_backend));
        out.push_str(&format!("  val {} = {}\n", ident, expression));
    }

    out.push_str("}\n");
    Ok(out)
}

/// Convert proof certificates into Quint assertion comments/guards.
#[must_use]
pub fn map_certificates_to_quint_assertions(certificates: &[ProofCertificateV1]) -> Vec<String> {
    certificates
        .iter()
        .map(|certificate| {
            if certificate.verified {
                format!(
                    "// certificate {} verified by {:?} for {}",
                    certificate.certificate_id, certificate.backend, certificate.property_id
                )
            } else {
                format!(
                    "// certificate {} FAILED by {:?} for {}",
                    certificate.certificate_id, certificate.backend, certificate.property_id
                )
            }
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::bridge_format::{
        BridgeBundleV1, ProofBackendV1, ProofCertificateV1, PropertyClassV1, PropertyInterchangeV1,
    };

    #[test]
    fn parses_importable_properties() {
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
        bundle.properties.push(PropertyInterchangeV1 {
            id: "p2".to_string(),
            class: PropertyClassV1::Liveness,
            source_backend: "external".to_string(),
            source_expr: "ignored".to_string(),
            target_expr: None,
            assumptions: vec![],
            metadata: BTreeMap::default(),
        });

        let imported = parse_telltale_properties(&bundle);
        assert_eq!(imported.len(), 1);
        assert_eq!(imported[0].id, "p1");
    }

    #[test]
    fn generates_quint_module_from_imported_properties() {
        let properties = vec![PropertyInterchangeV1 {
            id: "byz.safe".to_string(),
            class: PropertyClassV1::ByzantineSafety,
            source_backend: "lean".to_string(),
            source_expr: "ByzSafe".to_string(),
            target_expr: Some("ByzSafe".to_string()),
            assumptions: vec![],
            metadata: BTreeMap::default(),
        }];

        let module = generate_quint_invariant_module("ImportedProofs", &properties)
            .expect("module generation");
        assert!(module.contains("module ImportedProofs"));
        assert!(module.contains("val byz_safe = ByzSafe"));
    }

    #[test]
    fn maps_certificates_to_assertions() {
        let assertions = map_certificates_to_quint_assertions(&[ProofCertificateV1 {
            certificate_id: "cert-1".to_string(),
            backend: ProofBackendV1::Lean,
            property_id: "p1".to_string(),
            statement_digest_hex: "11".repeat(32),
            artifact_digest_hex: "22".repeat(32),
            verified: true,
            verified_at_ms: Some(1234),
            toolchain: BTreeMap::default(),
        }]);

        assert_eq!(assertions.len(), 1);
        assert!(assertions[0].contains("verified"));
    }

    #[test]
    fn imports_at_least_three_properties() {
        let mut bundle = BridgeBundleV1::default();
        for idx in 0..3 {
            bundle.properties.push(PropertyInterchangeV1 {
                id: format!("p{idx}"),
                class: PropertyClassV1::Safety,
                source_backend: "telltale".to_string(),
                source_expr: "safe".to_string(),
                target_expr: Some("safe".to_string()),
                assumptions: vec![],
                metadata: BTreeMap::default(),
            });
        }

        let imported = parse_telltale_properties(&bundle);
        assert!(
            imported.len() >= 3,
            "expected at least three imported properties"
        );
    }
}
