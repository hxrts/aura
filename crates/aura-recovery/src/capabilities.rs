#![allow(missing_docs)]

use aura_macros::capability_family;

#[capability_family(namespace = "recovery")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecoveryCapability {
    #[capability("initiate")]
    Initiate,
    #[capability("coordinate")]
    Coordinate,
    #[capability("approve")]
    Approve,
    #[capability("finalize")]
    Finalize,
    #[capability("cancel")]
    Cancel,
}

#[capability_family(namespace = "recovery")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GuardianSetupCapability {
    #[capability("guardian_setup:initiate")]
    Initiate,
    #[capability("guardian_setup:accept_invitation")]
    AcceptInvitation,
    #[capability("guardian_setup:verify_invitation")]
    VerifyInvitation,
    #[capability("guardian_setup:complete")]
    Complete,
}

#[capability_family(namespace = "recovery")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MembershipChangeCapability {
    #[capability("membership_change:initiate")]
    Initiate,
    #[capability("membership_change:vote")]
    Vote,
    #[capability("membership_change:verify_proposal")]
    VerifyProposal,
    #[capability("membership_change:complete")]
    Complete,
}

pub fn evaluation_candidates_for_recovery_protocol() -> &'static [RecoveryCapability] {
    RecoveryCapability::declared_names()
}

pub fn evaluation_candidates_for_guardian_setup_protocol() -> &'static [GuardianSetupCapability] {
    GuardianSetupCapability::declared_names()
}

pub fn evaluation_candidates_for_membership_change_protocol(
) -> &'static [MembershipChangeCapability] {
    MembershipChangeCapability::declared_names()
}
