//! Quint -> Telltale bridge export helpers.
//!
//! This module translates parsed Quint JSON IR into the bridge interchange schema.

use crate::bridge_format::{
    BridgeBundleV1, SessionEdgeV1, SessionNodeKindV1, SessionNodeV1, SessionTypeInterchangeV1,
};
use aura_core::{hash, AuraError};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, BTreeSet};

/// Lightweight module summary extracted from Quint JSON IR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuintModuleSummary {
    /// Module name.
    pub name: String,
    /// Definition names declared by module.
    pub definitions: Vec<String>,
    /// Assumption count from IR.
    pub assumptions: u64,
}

/// Bridge export failures.
#[derive(Debug, thiserror::Error)]
pub enum BridgeExportError {
    /// JSON IR shape is invalid.
    #[error("invalid Quint JSON IR: {message}")]
    InvalidIr {
        /// Validation details.
        message: String,
    },
    /// Bundle fails deterministic bridge validation.
    #[error("bridge bundle validation failed: {message}")]
    InvalidBundle {
        /// Validation details.
        message: String,
    },
    /// Internal serialization failure.
    #[error("bridge export serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
    /// Internal Aura error.
    #[error(transparent)]
    Aura(#[from] AuraError),
}

/// Parse Quint module summaries from JSON IR.
///
/// # Errors
///
/// Returns [`BridgeExportError::InvalidIr`] when `modules` is missing or malformed.
pub fn parse_quint_modules(ir: &JsonValue) -> Result<Vec<QuintModuleSummary>, BridgeExportError> {
    let Some(modules) = ir.get("modules").and_then(JsonValue::as_array) else {
        return Err(BridgeExportError::InvalidIr {
            message: "expected top-level 'modules' array".to_string(),
        });
    };

    modules
        .iter()
        .map(|module| {
            let name = module
                .get("name")
                .and_then(JsonValue::as_str)
                .unwrap_or("unknown")
                .to_string();
            let definitions = module
                .get("definitions")
                .and_then(JsonValue::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.get("name").and_then(JsonValue::as_str))
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let assumptions = module
                .get("assumptions")
                .and_then(JsonValue::as_array)
                .map_or(0_u64, |items| {
                    u64::try_from(items.len()).unwrap_or(u64::MAX)
                });

            Ok(QuintModuleSummary {
                name,
                definitions,
                assumptions,
            })
        })
        .collect()
}

/// Export parsed Quint JSON IR into session interchange payloads.
///
/// # Errors
///
/// Returns [`BridgeExportError`] when parsing or validation fails.
pub fn export_quint_to_telltale_bundle(
    ir: &JsonValue,
    source_label: &str,
) -> Result<BridgeBundleV1, BridgeExportError> {
    let modules = parse_quint_modules(ir)?;
    let mut bundle = BridgeBundleV1::default();
    bundle
        .metadata
        .insert("source".to_string(), source_label.to_string());

    for module in modules {
        // Deterministic synthetic role extraction:
        // - explicit role_<name> definition prefixes become roles
        // - otherwise fallback to one module role
        let mut roles: Vec<String> = module
            .definitions
            .iter()
            .filter_map(|definition| definition.strip_prefix("role_").map(ToString::to_string))
            .collect();
        if roles.is_empty() {
            roles.push(format!("{}_role", module.name));
        }
        roles.sort();
        roles.dedup();

        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        let mut previous_node_id: Option<String> = None;
        let steps = if module.definitions.is_empty() {
            vec!["end".to_string()]
        } else {
            module.definitions.clone()
        };

        for (index, definition) in steps.iter().enumerate() {
            let node_id = format!("{}_n{}", module.name, index);
            let role = roles[index % roles.len()].clone();
            let kind = if index + 1 == steps.len() {
                SessionNodeKindV1::End
            } else {
                SessionNodeKindV1::Send
            };
            nodes.push(SessionNodeV1 {
                id: node_id.clone(),
                role,
                kind,
                label: Some(definition.clone()),
                annotations: BTreeMap::new(),
            });
            if let Some(previous) = previous_node_id {
                edges.push(SessionEdgeV1 {
                    from: previous,
                    to: node_id.clone(),
                    guard: None,
                    message: Some(definition.clone()),
                });
            }
            previous_node_id = Some(node_id);
        }

        let session_id = hex::encode(hash::hash(module.name.as_bytes()));
        bundle.session_types.push(SessionTypeInterchangeV1 {
            session_id,
            protocol: module.name.clone(),
            roles,
            nodes,
            edges,
            source: BTreeMap::from([("quint_module".to_string(), module.name)]),
        });
    }

    validate_export_bundle(&bundle)?;
    Ok(bundle)
}

/// Validate exported bundle translation invariants.
///
/// # Errors
///
/// Returns [`BridgeExportError::InvalidBundle`] when structural checks fail.
pub fn validate_export_bundle(bundle: &BridgeBundleV1) -> Result<(), BridgeExportError> {
    if bundle.session_types.is_empty() {
        return Err(BridgeExportError::InvalidBundle {
            message: "bundle must contain at least one session type".to_string(),
        });
    }

    for session in &bundle.session_types {
        if session.roles.is_empty() {
            return Err(BridgeExportError::InvalidBundle {
                message: format!("session {} has no roles", session.protocol),
            });
        }
        let node_ids = session
            .nodes
            .iter()
            .map(|node| node.id.clone())
            .collect::<BTreeSet<_>>();
        if node_ids.len() != session.nodes.len() {
            return Err(BridgeExportError::InvalidBundle {
                message: format!("session {} has duplicate node IDs", session.protocol),
            });
        }
        for edge in &session.edges {
            if !node_ids.contains(&edge.from) || !node_ids.contains(&edge.to) {
                return Err(BridgeExportError::InvalidBundle {
                    message: format!(
                        "session {} edge {} -> {} references unknown node",
                        session.protocol, edge.from, edge.to
                    ),
                });
            }
        }
    }

    Ok(())
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn sample_ir() -> JsonValue {
        serde_json::json!({
            "modules": [
                {
                    "name": "consensus_bridge",
                    "definitions": [
                        {"name": "role_coordinator"},
                        {"name": "role_witness"},
                        {"name": "send_vote"},
                        {"name": "recv_commit"}
                    ],
                    "assumptions": [{"name": "auth_channels"}]
                }
            ]
        })
    }

    #[test]
    fn parses_module_summaries() {
        let modules = parse_quint_modules(&sample_ir()).expect("parse modules");
        assert_eq!(modules.len(), 1);
        assert_eq!(modules[0].name, "consensus_bridge");
        assert_eq!(modules[0].assumptions, 1);
    }

    #[test]
    fn exports_bundle_with_valid_structure() {
        let bundle =
            export_quint_to_telltale_bundle(&sample_ir(), "verification/quint/consensus.qnt")
                .expect("export");
        assert_eq!(bundle.session_types.len(), 1);
        assert!(
            !bundle.session_types[0].roles.is_empty(),
            "roles should be inferred"
        );
        validate_export_bundle(&bundle).expect("bundle should validate");
    }

    #[test]
    fn exports_at_least_three_modules() {
        let ir = serde_json::json!({
            "modules": [
                {"name": "m1", "definitions": [{"name": "role_a"}, {"name": "s1"}], "assumptions": []},
                {"name": "m2", "definitions": [{"name": "role_b"}, {"name": "s2"}], "assumptions": []},
                {"name": "m3", "definitions": [{"name": "role_c"}, {"name": "s3"}], "assumptions": []}
            ]
        });

        let bundle = export_quint_to_telltale_bundle(&ir, "verification/quint/bridge_suite.qnt")
            .expect("export three modules");
        assert!(
            bundle.session_types.len() >= 3,
            "expected at least three exported session types"
        );
    }
}
