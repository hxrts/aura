//! Ceremony and bootstrap bridge types.

use aura_core::threshold::{AgreementMode, ParticipantIdentity};
use aura_core::types::identifiers::{AuthorityId, CeremonyId};
use aura_core::types::Epoch;
use aura_core::DeviceId;

/// High-level ceremony kind exposed across the runtime bridge boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CeremonyKind {
    /// Guardian threshold key rotation ceremony for an account authority.
    GuardianRotation,
    /// Device threshold key rotation ceremony (multifactor authority).
    DeviceRotation,
    /// Device enrollment ceremony (account authority membership change + rotation).
    DeviceEnrollment,
    /// Device removal ceremony (account authority membership change + rotation).
    DeviceRemoval,
    /// Guardian-based recovery ceremony.
    Recovery,
    /// Invitation ceremony (contact/guardian/channel).
    Invitation,
    /// Rendezvous secure-channel ceremony.
    RendezvousSecureChannel,
    /// OTA hard-fork activation ceremony.
    OtaActivation,
}

/// Stable reason a runtime-owned ceremony reached a failed terminal state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CeremonyFailureReason {
    Rejected,
    Cancelled,
    TimedOut,
    ChoreographyFailed,
    RuntimeFailed,
    Superseded,
}

/// Terminal result established by the runtime ceremony owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CeremonyTerminalOutcome {
    Committed,
    Failed(CeremonyFailureReason),
}

/// Result of starting a device enrollment ceremony.
#[derive(Debug, Clone)]
pub struct DeviceEnrollmentStart {
    /// Ceremony identifier for status polling / cancellation.
    pub ceremony_id: CeremonyId,
    /// Shareable enrollment code (e.g. QR/copy-paste) to import on the new device.
    pub enrollment_code: String,
    /// Pending epoch created during prepare.
    pub pending_epoch: Epoch,
    /// Device id being enrolled.
    pub device_id: DeviceId,
    /// Separate confirmation transfer; absence is explicit unsupported runtime.
    pub manifest_transfer: Option<EnrollmentManifestTransferCodes>,
}

/// Observed identifiers for one still-owned issuer preparation. This DTO grants
/// no signature, generation, time-window or runtime admission authority.
#[derive(Debug, Clone)]
pub struct PreparedDeviceEnrollmentSigning {
    /// Identifier of the still-owned prepared issuer; this field grants no resume authority.
    pub ceremony_id: CeremonyId,
    /// Canonical public three-domain intent for explicit participant review and transfer.
    pub signing_intent_code: String,
    /// Original user-transferred setup proof required by participant admission.
    pub setup_transfer_code: String,
}

#[derive(Debug, Clone)]
pub struct EnrollmentManifestTransferCodes {
    pub manifest_code: String,
    /// The user transfers this separately from the enrollment payload.
    pub initiator_verifier_code: String,
}

/// Status of a key-rotation / membership-change ceremony.
#[derive(Debug, Clone)]
pub struct KeyRotationCeremonyStatus {
    /// Ceremony identifier.
    pub ceremony_id: CeremonyId,
    /// What kind of ceremony this is.
    pub kind: CeremonyKind,
    /// Number of participants who have accepted.
    pub accepted_count: u16,
    /// Total number of required participants.
    pub total_count: u16,
    /// Threshold required for completion.
    pub threshold: u16,
    /// Whether the ceremony is complete.
    pub is_complete: bool,
    /// Whether the ceremony has failed.
    pub has_failed: bool,
    /// List of participants who have accepted.
    pub accepted_participants: Vec<ParticipantIdentity>,
    /// Optional error message if failed.
    pub error_message: Option<String>,
    /// Pending epoch for key rotation (if applicable).
    pub pending_epoch: Option<Epoch>,
    /// Agreement mode (A1/A2/A3).
    pub agreement_mode: AgreementMode,
    /// Whether reversion is still possible.
    pub reversion_risk: bool,
}

/// Status of a guardian ceremony.
#[derive(Debug, Clone)]
pub struct CeremonyStatus {
    /// Ceremony identifier.
    pub ceremony_id: CeremonyId,
    /// Number of guardians who have accepted.
    pub accepted_count: u16,
    /// Total number of guardians.
    pub total_count: u16,
    /// Threshold required for completion.
    pub threshold: u16,
    /// Whether the ceremony is complete.
    pub is_complete: bool,
    /// Whether the ceremony has failed.
    pub has_failed: bool,
    /// List of guardian IDs who have accepted.
    pub accepted_guardians: Vec<AuthorityId>,
    /// Optional error message if failed.
    pub error_message: Option<String>,
    /// Pending epoch for key rotation.
    pub pending_epoch: Option<Epoch>,
    /// Agreement mode (A1/A2/A3).
    pub agreement_mode: AgreementMode,
    /// Whether reversion is still possible.
    pub reversion_risk: bool,
}

/// Discovery origin for a bootstrap candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootstrapCandidateOrigin {
    /// Candidate discovered through native LAN discovery.
    Lan,
    /// Candidate discovered through a localhost bootstrap broker.
    LocalBroker,
    /// Candidate discovered through a LAN-visible bootstrap broker.
    LanBroker,
}

/// Information about a bootstrap candidate available for enrollment.
#[derive(Debug, Clone)]
pub struct BootstrapCandidateInfo {
    /// Authority ID of the discovered candidate.
    pub authority_id: AuthorityId,
    /// Discovery origin for this candidate.
    pub origin: BootstrapCandidateOrigin,
    /// Best currently known address for the candidate bootstrap path.
    pub address: String,
    /// When this candidate was discovered (ms since epoch).
    pub discovered_at_ms: u64,
    /// Nickname suggestion if available from the descriptor.
    pub nickname_suggestion: Option<String>,
}
