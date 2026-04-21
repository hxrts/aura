//! Telltale-native adaptive-privacy control-plane choreographies.
//!
//! These protocols cover only protocol-critical control-plane execution:
//! anonymous path establishment and typed reply-block witness/accountability
//! flows. Bootstrap and stale-node re-entry remain runtime-local until they
//! become true multi-party admission/evidence protocols instead of local hint
//! lookup and cache refresh.

#![allow(clippy::unused_unit)]

use aura_core::service::{
    AccountabilityReplyBlock, EstablishedPath, EstablishedPathRef, Route, ServiceFamily,
};
use aura_core::types::identifiers::{AuthorityId, ContextId};
use aura_macros::tell;
use aura_mpst::CompositionManifest;
use serde::{Deserialize, Serialize};

/// Exact adaptive-privacy control-plane protocols owned by Telltale.
pub const ADAPTIVE_PRIVACY_CONTROL_PROTOCOLS: &[&str] = &[
    "AnonymousPathEstablishProtocol",
    "MoveReceiptReplyBlockProtocol",
    "HoldDepositReplyBlockProtocol",
    "HoldRetrievalReplyBlockProtocol",
    "HoldAuditReplyBlockProtocol",
];

/// One surviving non-Telltale control-path exception.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlPathException {
    /// Human-readable surface name.
    pub surface: &'static str,
    /// Current owner responsible for the exception.
    pub owner: &'static str,
    /// Why this path still remains outside the Telltale execution boundary.
    pub rationale: &'static str,
    /// Concrete condition that must be met before removal.
    pub removal_condition: &'static str,
}

/// Explicit adaptive-privacy control-path exceptions that remain runtime-local.
pub const CONTROL_PATH_EXCEPTIONS: &[ControlPathException] = &[
    ControlPathException {
        surface: "bootstrap_reentry",
        owner: "aura-agent::runtime::services::{bootstrap_broker,rendezvous_manager}",
        rationale:
            "bootstrap and stale-node re-entry are still local descriptor/hint lookup rather than a canonical multi-party admission or evidence protocol",
        removal_condition:
            "move to Telltale only once bootstrap/re-entry carries typed multi-party admission, evidence, and transition ownership instead of runtime-local cache refresh",
    },
    ControlPathException {
        surface: "host_local_anonymous_path_execution",
        owner: "aura-agent::runtime::services::path_manager",
        rationale:
            "the runtime service still owns encrypted setup-object construction, replay suppression, and path reuse while the new protocol artifacts are admission-ready but not yet the canonical executor",
        removal_condition:
            "delete the host-local establish state machine once the VM-host bridge executes AnonymousPathEstablishProtocol as the canonical runtime path",
    },
    ControlPathException {
        surface: "host_local_reply_block_execution",
        owner: "aura-agent::runtime::services::hold_manager",
        rationale:
            "the runtime service still owns reply-block issuance, witness verification, and verified-only local budget application while the new accountability choreographies define the canonical protocol envelope shape",
        removal_condition:
            "delete the parallel local reply-block lifecycle state machines once the reply-block choreographies become the canonical runtime execution path",
    },
];

/// Failure classification for anonymous-path establish control flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnonymousPathEstablishFailureMode {
    Timeout,
    Cancelled,
    ReplayRejected,
    StaleOwner,
}

/// Request to open one anonymous-path establish control session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnonymousPathEstablishRequest {
    pub scope: ContextId,
    pub destination: AuthorityId,
    pub route: Route,
    pub owner_id: String,
    pub requested_at_ms: u64,
}

/// Success evidence for one established anonymous path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnonymousPathEstablishSucceeded {
    pub session_id: u64,
    pub path: EstablishedPath,
    pub owner_id: String,
    pub owner_generation: u64,
    pub readiness_witness_id: u64,
    pub ready_at_ms: u64,
}

/// Failure evidence for one anonymous-path establish attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnonymousPathEstablishFailed {
    pub session_id: u64,
    pub mode: AnonymousPathEstablishFailureMode,
    pub owner_id: String,
    pub owner_generation: Option<u64>,
    pub witness_id: Option<u64>,
    pub observed_at_ms: u64,
    pub reason: String,
}

/// Shared witness-submission payload for reply-block accountability protocols.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplyBlockWitnessSubmission {
    pub scope: ContextId,
    pub family: ServiceFamily,
    pub kind: ReplyBlockWitnessKind,
    pub reply_block: AccountabilityReplyBlock,
    pub reply_path: EstablishedPathRef,
    pub providers: Vec<AuthorityId>,
    pub command_scope: [u8; 32],
    pub selector: Option<[u8; 32]>,
    pub observed_at_ms: u64,
    pub success: bool,
}

/// Canonical witness kind for reply-block accountability control flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplyBlockWitnessKind {
    MoveReceipt,
    HoldDeposit,
    HoldRetrieval,
    HoldAudit,
}

/// Success evidence returned after witness verification/admission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplyBlockWitnessAccepted {
    pub family: ServiceFamily,
    pub kind: ReplyBlockWitnessKind,
    pub providers: Vec<AuthorityId>,
    pub observed_at_ms: u64,
    pub success: bool,
    pub outstanding_hold_delta: i32,
}

/// Failure evidence returned after witness rejection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplyBlockWitnessRejected {
    pub family: ServiceFamily,
    pub kind: ReplyBlockWitnessKind,
    pub observed_at_ms: u64,
    pub reason: String,
}

/// Witness submission for move-receipt accountability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoveReceiptWitnessSubmission {
    pub inner: ReplyBlockWitnessSubmission,
}

/// Accepted move-receipt witness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoveReceiptWitnessAccepted {
    pub inner: ReplyBlockWitnessAccepted,
}

/// Rejected move-receipt witness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoveReceiptWitnessRejected {
    pub inner: ReplyBlockWitnessRejected,
}

/// Witness submission for hold-deposit accountability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoldDepositWitnessSubmission {
    pub inner: ReplyBlockWitnessSubmission,
}

/// Accepted hold-deposit witness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoldDepositWitnessAccepted {
    pub inner: ReplyBlockWitnessAccepted,
}

/// Rejected hold-deposit witness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoldDepositWitnessRejected {
    pub inner: ReplyBlockWitnessRejected,
}

/// Witness submission for hold-retrieval accountability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoldRetrievalWitnessSubmission {
    pub inner: ReplyBlockWitnessSubmission,
}

/// Accepted hold-retrieval witness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoldRetrievalWitnessAccepted {
    pub inner: ReplyBlockWitnessAccepted,
}

/// Rejected hold-retrieval witness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoldRetrievalWitnessRejected {
    pub inner: ReplyBlockWitnessRejected,
}

/// Witness submission for hold-audit accountability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoldAuditWitnessSubmission {
    pub inner: ReplyBlockWitnessSubmission,
}

/// Accepted hold-audit witness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoldAuditWitnessAccepted {
    pub inner: ReplyBlockWitnessAccepted,
}

/// Rejected hold-audit witness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoldAuditWitnessRejected {
    pub inner: ReplyBlockWitnessRejected,
}

/// Anonymous-path establish control-plane choreography.
pub mod anonymous_path_establish {
    #![allow(unused_imports)]
    use super::*;

    tell!(include_str!(
        "src/adaptive_privacy_control/anonymous_path_establish.tell"
    ));
}

/// Move-receipt reply-block accountability choreography.
pub mod move_receipt_reply_block {
    #![allow(unused_imports)]
    use super::*;

    tell!(include_str!(
        "src/adaptive_privacy_control/move_receipt_reply_block.tell"
    ));
}

/// Hold-deposit reply-block accountability choreography.
pub mod hold_deposit_reply_block {
    #![allow(unused_imports)]
    use super::*;

    tell!(include_str!(
        "src/adaptive_privacy_control/hold_deposit_reply_block.tell"
    ));
}

/// Hold-retrieval reply-block accountability choreography.
pub mod hold_retrieval_reply_block {
    #![allow(unused_imports)]
    use super::*;

    tell!(include_str!(
        "src/adaptive_privacy_control/hold_retrieval_reply_block.tell"
    ));
}

/// Hold-audit reply-block accountability choreography.
pub mod hold_audit_reply_block {
    #![allow(unused_imports)]
    use super::*;

    tell!(include_str!(
        "src/adaptive_privacy_control/hold_audit_reply_block.tell"
    ));
}

/// Return all adaptive-privacy control-plane manifests.
#[must_use]
pub fn control_plane_manifests() -> Vec<CompositionManifest> {
    vec![
        anonymous_path_establish::telltale_session_types_anonymous_path_establish::vm_artifacts::composition_manifest(),
        move_receipt_reply_block::telltale_session_types_move_receipt_reply_block::vm_artifacts::composition_manifest(),
        hold_deposit_reply_block::telltale_session_types_hold_deposit_reply_block::vm_artifacts::composition_manifest(),
        hold_retrieval_reply_block::telltale_session_types_hold_retrieval_reply_block::vm_artifacts::composition_manifest(),
        hold_audit_reply_block::telltale_session_types_hold_audit_reply_block::vm_artifacts::composition_manifest(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anonymous_path_manifest_is_theorem_pack_free() {
        let manifest =
            anonymous_path_establish::telltale_session_types_anonymous_path_establish::vm_artifacts::composition_manifest();
        assert!(manifest.theorem_packs.is_empty());
        assert!(manifest.required_theorem_packs.is_empty());
        assert_eq!(manifest.role_names, vec!["Owner", "PathManager"]);
    }

    #[test]
    fn reply_block_manifests_are_theorem_pack_free() {
        for manifest in control_plane_manifests().into_iter().skip(1) {
            assert!(
                manifest.theorem_packs.is_empty(),
                "reply-block control protocol should remain theorem-pack-free until a dedicated runtime admission surface exists: {}",
                manifest.protocol_name
            );
            assert!(
                manifest.required_theorem_packs.is_empty(),
                "reply-block control protocol should remain theorem-pack-free until a dedicated runtime admission surface exists: {}",
                manifest.protocol_name
            );
        }
    }

    #[test]
    fn adaptive_privacy_control_inventory_covers_expected_protocols() {
        let manifest_names = control_plane_manifests()
            .into_iter()
            .map(|manifest| manifest.protocol_name)
            .collect::<Vec<_>>();
        assert_eq!(
            manifest_names.len(),
            ADAPTIVE_PRIVACY_CONTROL_PROTOCOLS.len()
        );
        for protocol_name in ADAPTIVE_PRIVACY_CONTROL_PROTOCOLS {
            assert!(
                manifest_names.iter().any(|name| name == protocol_name),
                "missing adaptive-privacy control protocol manifest: {protocol_name}"
            );
        }
    }

    #[test]
    fn adaptive_privacy_control_inventory_keeps_bootstrap_runtime_local() {
        let manifest_ids = control_plane_manifests()
            .into_iter()
            .map(|manifest| manifest.protocol_id)
            .collect::<Vec<_>>();
        for manifest_id in manifest_ids {
            assert!(
                !manifest_id.contains("bootstrap") && !manifest_id.contains("reentry"),
                "bootstrap/re-entry must remain runtime-local until it becomes a true multi-party control protocol: {manifest_id}"
            );
        }
        assert!(CONTROL_PATH_EXCEPTIONS
            .iter()
            .any(|exception| exception.surface == "bootstrap_reentry"));
    }
}
