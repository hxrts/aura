#![allow(missing_docs)]

use aura_macros::capability_family;

#[capability_family(namespace = "auth")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AuthenticationCapability {
    #[capability("request")]
    Request,
    #[capability("submit_proof")]
    SubmitProof,
    #[capability("verify")]
    Verify,
    #[capability("create_session")]
    CreateSession,
}

#[capability_family(namespace = "auth")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GuardianAuthCapability {
    #[capability("guardian:request_approval")]
    RequestApproval,
    #[capability("guardian:coordinate")]
    Coordinate,
    #[capability("guardian:submit_proof")]
    SubmitProof,
    #[capability("guardian:verify")]
    Verify,
}

#[capability_family(namespace = "recovery")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecoveryAuthorizationCapability {
    #[capability("initiate")]
    Initiate,
    #[capability("approve")]
    Approve,
}

#[capability_family(namespace = "dkd")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DkdCapability {
    #[capability("initiate")]
    Initiate,
    #[capability("commit")]
    Commit,
    #[capability("reveal")]
    Reveal,
    #[capability("finalize")]
    Finalize,
}

pub fn evaluation_candidates_for_auth_guard() -> &'static [AuthenticationCapability] {
    AuthenticationCapability::declared_names()
}

pub fn evaluation_candidates_for_guardian_auth_protocol() -> &'static [GuardianAuthCapability] {
    GuardianAuthCapability::declared_names()
}

pub fn evaluation_candidates_for_recovery_authorization(
) -> &'static [RecoveryAuthorizationCapability] {
    RecoveryAuthorizationCapability::declared_names()
}

pub fn evaluation_candidates_for_dkd_protocol() -> &'static [DkdCapability] {
    DkdCapability::declared_names()
}
