#![allow(missing_docs)]

use aura_macros::capability_family;

#[capability_family(namespace = "invitation")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InvitationCapability {
    #[capability("send")]
    Send,
    #[capability("accept")]
    Accept,
    #[capability("decline")]
    Decline,
    #[capability("cancel")]
    Cancel,
    #[capability("guardian")]
    Guardian,
    #[capability("guardian:accept")]
    GuardianAccept,
    #[capability("channel")]
    Channel,
    #[capability("device:enroll")]
    DeviceEnroll,
    #[capability("device:accept")]
    DeviceAccept,
}

pub const INVITATION_GUARD_CANDIDATES: &[InvitationCapability] = &[
    InvitationCapability::Send,
    InvitationCapability::Accept,
    InvitationCapability::Decline,
    InvitationCapability::Cancel,
    InvitationCapability::Guardian,
    InvitationCapability::Channel,
    InvitationCapability::DeviceEnroll,
];

pub fn evaluation_candidates_for_invitation_guard() -> &'static [InvitationCapability] {
    INVITATION_GUARD_CANDIDATES
}

pub fn evaluation_candidates_for_invitation_protocol() -> &'static [InvitationCapability] {
    InvitationCapability::declared_names()
}
