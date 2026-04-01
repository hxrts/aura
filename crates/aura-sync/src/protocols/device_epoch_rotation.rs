//! Device-scoped epoch rotation choreography surface.
//!
//! This protocol covers the device-specific share distribution / acceptance /
//! commit handshake used by enrollment and removal ceremonies once the local
//! initiator has already prepared the pending epoch.

use aura_core::types::identifiers::CeremonyId;
use aura_core::{AttestedOp, AuthorityId, DeviceId};
use aura_macros::choreography;
use serde::{Deserialize, Serialize};

/// The initiating ceremony type for one device-scoped epoch rotation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceEpochRotationKind {
    Rotation,
    Enrollment,
    Removal,
}

/// Proposal sent from the initiating device to one participant device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceEpochProposal {
    pub ceremony_id: CeremonyId,
    pub kind: DeviceEpochRotationKind,
    pub subject_authority: AuthorityId,
    pub pending_epoch: u64,
    pub initiator_device_id: DeviceId,
    pub participant_device_id: DeviceId,
    pub key_package: Vec<u8>,
    pub threshold_config: Vec<u8>,
    pub public_key_package: Vec<u8>,
}

/// Acceptance issued by one participant device after locally staging the share.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceEpochAcceptance {
    pub ceremony_id: CeremonyId,
    pub acceptor_device_id: DeviceId,
}

/// Commit sent by the initiator once the ceremony threshold is satisfied.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceEpochCommit {
    pub ceremony_id: CeremonyId,
    pub new_epoch: u64,
    pub attested_leaf_op: Option<AttestedOp>,
}

choreography!(include_str!("src/protocols/device_epoch_rotation.tell"));
