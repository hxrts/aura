#![allow(missing_docs)]

use aura_macros::capability_family;

#[capability_family(namespace = "session")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionCoordinationCapability {
    #[capability("request")]
    Request,
    #[capability("invite_participants")]
    InviteParticipants,
    #[capability("respond")]
    Respond,
    #[capability("create")]
    Create,
    #[capability("notify_participants")]
    NotifyParticipants,
    #[capability("reject_creation")]
    RejectCreation,
    #[capability("notify_participants_failure")]
    NotifyParticipantsFailure,
}

pub fn evaluation_candidates_for_session_coordination_protocol(
) -> &'static [SessionCoordinationCapability] {
    SessionCoordinationCapability::declared_names()
}
