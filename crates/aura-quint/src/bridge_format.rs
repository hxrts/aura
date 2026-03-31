//! Telltale bridge interchange schema.
//!
//! These types provide a stable, versioned format for:
//! - Quint session model export into Telltale-compatible choreography graphs
//! - Telltale/Lean property import into Quint invariant suites
//! - Proof-certificate exchange across verification pipelines

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;

/// Schema version for bridge bundles.
pub const AURA_TELLTALE_BRIDGE_SCHEMA_V1: &str = "aura.telltale-bridge.v1";

/// Bridge bundle containing session, property, and certificate payloads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BridgeBundleV1 {
    /// Schema version identifier.
    pub schema_version: String,
    /// Session type interchange payloads.
    #[serde(default)]
    pub session_types: Vec<SessionTypeInterchangeV1>,
    /// Property interchange payloads.
    #[serde(default)]
    pub properties: Vec<PropertyInterchangeV1>,
    /// Proof certificate payloads.
    #[serde(default)]
    pub certificates: Vec<ProofCertificateV1>,
    /// Optional metadata labels.
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

impl Default for BridgeBundleV1 {
    fn default() -> Self {
        Self {
            schema_version: AURA_TELLTALE_BRIDGE_SCHEMA_V1.to_string(),
            session_types: Vec::new(),
            properties: Vec::new(),
            certificates: Vec::new(),
            metadata: BTreeMap::new(),
        }
    }
}

/// Session type interchange payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionTypeInterchangeV1 {
    /// Stable session graph identifier.
    pub session_id: String,
    /// Human-friendly protocol/choreography name.
    pub protocol: String,
    /// Participating roles.
    pub roles: Vec<String>,
    /// Graph nodes.
    pub nodes: Vec<SessionNodeV1>,
    /// Graph edges.
    pub edges: Vec<SessionEdgeV1>,
    /// Optional source metadata.
    #[serde(default)]
    pub source: BTreeMap<String, String>,
}

/// Session node kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionNodeKindV1 {
    /// Send action.
    Send,
    /// Receive action.
    Recv,
    /// Branch/choice action.
    Choose,
    /// Merge/join action.
    Merge,
    /// Terminal action.
    End,
}

/// Session graph node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionNodeV1 {
    /// Stable node id.
    pub id: String,
    /// Owning role.
    pub role: String,
    /// Node kind.
    pub kind: SessionNodeKindV1,
    /// Optional operation/message label.
    pub label: Option<String>,
    /// Optional effect annotation payload.
    #[serde(default)]
    pub annotations: BTreeMap<String, JsonValue>,
}

/// Session graph edge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionEdgeV1 {
    /// Source node id.
    pub from: String,
    /// Target node id.
    pub to: String,
    /// Optional guard/capability requirement.
    pub guard: Option<String>,
    /// Optional message label.
    pub message: Option<String>,
}

/// Property class for interchange.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PropertyClassV1 {
    /// State-safety invariant.
    Safety,
    /// Eventual progress/liveness claim.
    Liveness,
    /// Termination/step-bound claim.
    Termination,
    /// Byzantine safety claim.
    ByzantineSafety,
    /// Native/WASM conformance claim.
    Conformance,
}

/// Property interchange payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropertyInterchangeV1 {
    /// Stable property id.
    pub id: String,
    /// Property class.
    pub class: PropertyClassV1,
    /// Source backend (`quint`, `telltale`, `lean`).
    pub source_backend: String,
    /// Symbolic source expression (e.g. Quint invariant name).
    pub source_expr: String,
    /// Optional translated target expression.
    pub target_expr: Option<String>,
    /// Optional assumptions required for validity.
    #[serde(default)]
    pub assumptions: Vec<String>,
    /// Optional metadata.
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

/// Proof backend for certificates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProofBackendV1 {
    /// Lean proof artifact.
    Lean,
    /// Quint model-checking artifact.
    Quint,
    /// Telltale theorem-pack artifact.
    Telltale,
}

/// Proof certificate payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProofCertificateV1 {
    /// Stable certificate id.
    pub certificate_id: String,
    /// Backend that produced this certificate.
    pub backend: ProofBackendV1,
    /// Referenced property id.
    pub property_id: String,
    /// Hash of normalized statement.
    pub statement_digest_hex: String,
    /// Hash of proof/counterexample artifact.
    pub artifact_digest_hex: String,
    /// Verification outcome.
    pub verified: bool,
    /// Optional timestamp in milliseconds.
    pub verified_at_ms: Option<u64>,
    /// Optional tool/runtime version info.
    #[serde(default)]
    pub toolchain: BTreeMap<String, String>,
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn bridge_bundle_roundtrip_json() {
        let bundle = BridgeBundleV1 {
            session_types: vec![SessionTypeInterchangeV1 {
                session_id: "sid-1".to_string(),
                protocol: "recovery".to_string(),
                roles: vec!["account".to_string(), "guardian".to_string()],
                nodes: vec![SessionNodeV1 {
                    id: "n0".to_string(),
                    role: "account".to_string(),
                    kind: SessionNodeKindV1::Send,
                    label: Some("request".to_string()),
                    annotations: BTreeMap::new(),
                }],
                edges: vec![SessionEdgeV1 {
                    from: "n0".to_string(),
                    to: "n1".to_string(),
                    guard: Some("cap.recovery.grant".to_string()),
                    message: Some("request".to_string()),
                }],
                source: BTreeMap::new(),
            }],
            properties: vec![PropertyInterchangeV1 {
                id: "p1".to_string(),
                class: PropertyClassV1::Safety,
                source_backend: "quint".to_string(),
                source_expr: "no_double_commit".to_string(),
                target_expr: Some("ByzSafe".to_string()),
                assumptions: vec!["authentic_channels".to_string()],
                metadata: BTreeMap::new(),
            }],
            certificates: vec![ProofCertificateV1 {
                certificate_id: "cert-1".to_string(),
                backend: ProofBackendV1::Lean,
                property_id: "p1".to_string(),
                statement_digest_hex: "aa".repeat(32),
                artifact_digest_hex: "bb".repeat(32),
                verified: true,
                verified_at_ms: Some(1_700_000_000_000),
                toolchain: BTreeMap::from([("lean".to_string(), "4.8.0".to_string())]),
            }],
            ..BridgeBundleV1::default()
        };

        let payload = serde_json::to_vec(&bundle).expect("serialize bundle");
        let decoded: BridgeBundleV1 = serde_json::from_slice(&payload).expect("deserialize bundle");
        assert_eq!(decoded, bundle);
        assert_eq!(decoded.schema_version, AURA_TELLTALE_BRIDGE_SCHEMA_V1);
    }
}
