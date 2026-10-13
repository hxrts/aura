use super::*;
pub use aura_invitation::shareable::{
    ShareableInvitation, ShareableInvitationError, ShareableInvitationSenderProof,
    ShareableInvitationTransportMetadata,
};

fn default_imported_invitation_status() -> InvitationStatus {
    InvitationStatus::Pending
}

fn default_imported_sender_trust() -> ImportedSenderTrust {
    ImportedSenderTrust::SelfCertified
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) enum ImportedSenderTrust {
    SelfCertified,
    /// An independently retained active authority key matches the exact
    /// epoch and group key in the verified quorum code.
    TrustedThresholdGroupKey {
        epoch: u64,
    },
    /// A previously confirmed invitation establishes continuity of the key
    /// used for that invitation; it does not prove device membership.
    #[serde(alias = "TrustedDevice")]
    ConfirmedInvitationKey {
        device_id: DeviceId,
        key_epoch: Option<u64>,
    },
    /// The claimed device key matches a trusted registry entry, but that
    /// registry alone does not bind the device to the claimed authority.
    UnboundDeviceKeyMatch {
        device_id: DeviceId,
        key_epoch: Option<u64>,
    },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct StoredImportedInvitation {
    #[serde(flatten)]
    pub(super) shareable: ShareableInvitation,
    #[serde(default = "default_imported_invitation_status")]
    pub(super) status: InvitationStatus,
    #[serde(default)]
    pub(super) created_at: u64,
    #[serde(default = "default_imported_sender_trust")]
    pub(super) sender_trust: ImportedSenderTrust,
    /// Key that signed the imported code's sender proof; the inviter's
    /// response to our acceptance must verify against it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) sender_proof_key: Option<Vec<u8>>,
    /// Native quorum epoch of a v3 sender proof. It is retained only after
    /// the code signature and known-sender trust checks have passed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) sender_proof_epoch: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) sender_proof_device: Option<DeviceId>,
    /// Digest of the acceptance we sent and await the inviter's response to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) pending_acceptance_digest: Option<[u8; 32]>,
}

impl StoredImportedInvitation {
    pub(super) fn pending(
        shareable: ShareableInvitation,
        created_at: u64,
        sender_trust: ImportedSenderTrust,
    ) -> Self {
        Self {
            shareable,
            status: InvitationStatus::Pending,
            created_at,
            sender_trust,
            sender_proof_key: None,
            sender_proof_epoch: None,
            sender_proof_device: None,
            pending_acceptance_digest: None,
        }
    }
}

impl std::ops::Deref for StoredImportedInvitation {
    type Target = ShareableInvitation;

    fn deref(&self) -> &Self::Target {
        &self.shareable
    }
}
