//! Byzantine safety admission/attestation types.

use crate::effects::CapabilityKey;
use serde::{Deserialize, Serialize};

/// Schema version for Byzantine safety attestation payloads.
pub const BYZANTINE_ATTESTATION_SCHEMA_V1: &str = "aura.byzantine_attestation.v1";

/// Required runtime capability evidence for a protocol admission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ByzantineAdmissionRequirement {
    /// Stable protocol identifier (`aura.consensus`, etc.).
    pub protocol_id: String,
    /// Runtime capability keys required for admission.
    pub required_capabilities: Vec<CapabilityKey>,
}

impl ByzantineAdmissionRequirement {
    /// Construct a requirement set for one protocol.
    #[must_use]
    pub fn new(protocol_id: impl Into<String>, required_capabilities: Vec<CapabilityKey>) -> Self {
        Self {
            protocol_id: protocol_id.into(),
            required_capabilities,
        }
    }
}

/// One capability entry from a captured admission snapshot.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CapabilitySnapshotEntry {
    /// Runtime capability key.
    pub capability: CapabilityKey,
    /// Whether the capability was admitted/enabled.
    pub admitted: bool,
}

/// Runtime capability snapshot captured at admission time.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CapabilitySnapshot {
    /// Schema version for compatibility checks.
    pub schema_version: String,
    /// Optional source label (`vm`, `adapter`, simulator lane, etc.).
    pub source: String,
    /// Captured capability inventory.
    pub entries: Vec<CapabilitySnapshotEntry>,
}

impl CapabilitySnapshot {
    /// Construct a snapshot from capability inventory.
    #[must_use]
    pub fn from_inventory(
        source: impl Into<String>,
        inventory: impl IntoIterator<Item = (CapabilityKey, bool)>,
    ) -> Self {
        Self {
            schema_version: BYZANTINE_ATTESTATION_SCHEMA_V1.to_string(),
            source: source.into(),
            entries: inventory
                .into_iter()
                .map(|(capability, admitted)| CapabilitySnapshotEntry {
                    capability,
                    admitted,
                })
                .collect(),
        }
    }
}

/// Attestation recorded for Byzantine-safe protocol execution.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ByzantineSafetyAttestation {
    /// Schema version for attestation decoding.
    pub schema_version: String,
    /// Protocol identifier associated with this attestation.
    pub protocol_id: String,
    /// Required capabilities used for admission.
    pub required_capabilities: Vec<CapabilityKey>,
    /// Snapshot captured at admission.
    pub capability_snapshot: CapabilitySnapshot,
    /// Optional external theorem/runtime evidence references.
    pub evidence_refs: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_attestation_from_snapshot() {
        let required = vec![
            CapabilityKey::new("byzantine_envelope"),
            CapabilityKey::new("vmEnvelopeAdherence"),
        ];
        let snapshot = CapabilitySnapshot::from_inventory(
            "vm",
            required
                .iter()
                .cloned()
                .map(|capability| (capability, true))
                .collect::<Vec<_>>(),
        );
        let attestation = ByzantineSafetyAttestation::new(
            "aura.consensus",
            required.clone(),
            snapshot,
            vec!["evidence://runtime/1".to_string()],
        );

        assert_eq!(attestation.protocol_id, "aura.consensus");
        assert_eq!(attestation.required_capabilities, required);
        assert_eq!(attestation.evidence_refs.len(), 1);
        assert_eq!(attestation.schema_version, BYZANTINE_ATTESTATION_SCHEMA_V1);
    }
}

impl ByzantineSafetyAttestation {
    /// Construct a Byzantine safety attestation payload.
    #[must_use]
    pub fn new(
        protocol_id: impl Into<String>,
        required_capabilities: Vec<CapabilityKey>,
        capability_snapshot: CapabilitySnapshot,
        evidence_refs: Vec<String>,
    ) -> Self {
        Self {
            schema_version: BYZANTINE_ATTESTATION_SCHEMA_V1.to_string(),
            protocol_id: protocol_id.into(),
            required_capabilities,
            capability_snapshot,
            evidence_refs,
        }
    }
}
