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

macro_rules! declared_candidates_fn {
    ($fn_name:ident, $capability:ty) => {
        pub fn $fn_name() -> &'static [$capability] {
            <$capability>::declared_names()
        }
    };
}

declared_candidates_fn!(
    evaluation_candidates_for_auth_guard,
    AuthenticationCapability
);
declared_candidates_fn!(
    evaluation_candidates_for_guardian_auth_protocol,
    GuardianAuthCapability
);
declared_candidates_fn!(
    evaluation_candidates_for_recovery_authorization,
    RecoveryAuthorizationCapability
);
declared_candidates_fn!(evaluation_candidates_for_dkd_protocol, DkdCapability);
