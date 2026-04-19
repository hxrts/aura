#![allow(missing_docs)]
#![allow(dead_code)]
// Typed proof surfaces are referenced through proc-macro contracts and
// target-specific workflow/test paths that strict dead-code analysis does not
// model consistently.

use super::super::semantic_postcondition_proof_capability;
use aura_core::{ChannelId, SemanticOwnerPostcondition, SemanticSuccessProof};

macro_rules! semantic_success_proof {
    ($vis:vis struct $name:ident => $postcondition:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq)]
        $vis struct $name;

        impl SemanticSuccessProof for $name {
            fn declared_postcondition(&self) -> SemanticOwnerPostcondition {
                SemanticOwnerPostcondition::new($postcondition)
            }
        }
    };
    ($vis:vis struct $name:ident { $field:ident : $ty:ty } => $postcondition:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq)]
        $vis struct $name {
            $field: $ty,
        }

        impl SemanticSuccessProof for $name {
            fn declared_postcondition(&self) -> SemanticOwnerPostcondition {
                SemanticOwnerPostcondition::new($postcondition)
            }
        }
    };
}

semantic_success_proof!(
    pub(in crate::workflows) struct ChannelMembershipReadyProof {
        channel_id: ChannelId
    } => "channel_membership_ready"
);
semantic_success_proof!(
    pub(in crate::workflows) struct HomeCreatedProof {
        home_id: ChannelId
    } => "home_created"
);
semantic_success_proof!(
    pub(in crate::workflows) struct AccountCreatedProof => "account_created"
);
semantic_success_proof!(
    pub(in crate::workflows) struct ChannelInvitationCreatedProof {
        invitation_id: aura_core::InvitationId
    } => "channel_invitation_created"
);
semantic_success_proof!(
    pub(in crate::workflows) struct InvitationCreatedProof {
        invitation_id: aura_core::InvitationId
    } => "invitation_created"
);
semantic_success_proof!(
    pub(in crate::workflows) struct InvitationExportedProof {
        invitation_id: aura_core::InvitationId
    } => "invitation_exported"
);
semantic_success_proof!(
    pub(in crate::workflows) struct InvitationAcceptedOrMaterializedProof {
        invitation_id: aura_core::InvitationId
    } => "invitation_accepted_or_materialized"
);
semantic_success_proof!(
    pub(in crate::workflows) struct PendingInvitationConsumedProof {
        invitation_id: aura_core::InvitationId
    } => "pending_invitation_consumed"
);
semantic_success_proof!(
    pub(in crate::workflows) struct InvitationDeclinedProof {
        invitation_id: aura_core::InvitationId
    } => "invitation_declined"
);
semantic_success_proof!(
    pub(in crate::workflows) struct InvitationRevokedProof {
        invitation_id: aura_core::InvitationId
    } => "invitation_revoked"
);
semantic_success_proof!(
    pub(in crate::workflows) struct DeviceEnrollmentStartedProof {
        ceremony_id: aura_core::CeremonyId
    } => "device_enrollment_started"
);
semantic_success_proof!(
    pub(in crate::workflows) struct DeviceEnrollmentImportedProof {
        invitation_id: aura_core::InvitationId
    } => "device_enrollment_imported"
);
semantic_success_proof!(
    pub(in crate::workflows) struct MessageCommittedProof {
        message_id: String
    } => "message_committed"
);

impl AccountCreatedProof {
    pub(in crate::workflows) fn new() -> Self {
        Self
    }
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "semantic_postcondition_proof",
    family = "proof_issuer"
)]
#[aura_macros::authoritative_source(kind = "proof_issuer")]
pub(in crate::workflows) fn issue_home_created_proof(home_id: ChannelId) -> HomeCreatedProof {
    let _ = semantic_postcondition_proof_capability();
    HomeCreatedProof { home_id }
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "semantic_postcondition_proof",
    family = "proof_issuer"
)]
#[aura_macros::authoritative_source(kind = "proof_issuer")]
pub(in crate::workflows) fn issue_account_created_proof() -> AccountCreatedProof {
    let _ = semantic_postcondition_proof_capability();
    AccountCreatedProof::new()
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "semantic_postcondition_proof",
    family = "proof_issuer"
)]
#[aura_macros::authoritative_source(kind = "proof_issuer")]
pub(in crate::workflows) fn issue_channel_membership_ready_proof(
    channel_id: ChannelId,
) -> ChannelMembershipReadyProof {
    let _ = semantic_postcondition_proof_capability();
    ChannelMembershipReadyProof { channel_id }
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "semantic_postcondition_proof",
    family = "proof_issuer"
)]
#[aura_macros::authoritative_source(kind = "proof_issuer")]
pub(in crate::workflows) fn issue_invitation_created_proof(
    invitation_id: aura_core::InvitationId,
) -> InvitationCreatedProof {
    let _ = semantic_postcondition_proof_capability();
    InvitationCreatedProof { invitation_id }
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "semantic_postcondition_proof",
    family = "proof_issuer"
)]
#[aura_macros::authoritative_source(kind = "proof_issuer")]
pub(in crate::workflows) fn issue_invitation_exported_proof(
    invitation_id: aura_core::InvitationId,
) -> InvitationExportedProof {
    let _ = semantic_postcondition_proof_capability();
    InvitationExportedProof { invitation_id }
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "semantic_postcondition_proof",
    family = "proof_issuer"
)]
#[aura_macros::authoritative_source(kind = "proof_issuer")]
pub(in crate::workflows) fn issue_channel_invitation_created_proof(
    invitation_id: aura_core::InvitationId,
) -> ChannelInvitationCreatedProof {
    let _ = semantic_postcondition_proof_capability();
    ChannelInvitationCreatedProof { invitation_id }
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "semantic_postcondition_proof",
    family = "proof_issuer"
)]
#[aura_macros::authoritative_source(kind = "proof_issuer")]
pub(in crate::workflows) fn issue_invitation_accepted_or_materialized_proof(
    invitation_id: aura_core::InvitationId,
) -> InvitationAcceptedOrMaterializedProof {
    let _ = semantic_postcondition_proof_capability();
    InvitationAcceptedOrMaterializedProof { invitation_id }
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "semantic_postcondition_proof",
    family = "proof_issuer"
)]
#[aura_macros::authoritative_source(kind = "proof_issuer")]
pub(in crate::workflows) fn issue_pending_invitation_consumed_proof(
    invitation_id: aura_core::InvitationId,
) -> PendingInvitationConsumedProof {
    let _ = semantic_postcondition_proof_capability();
    PendingInvitationConsumedProof { invitation_id }
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "semantic_postcondition_proof",
    family = "proof_issuer"
)]
#[aura_macros::authoritative_source(kind = "proof_issuer")]
pub(in crate::workflows) fn issue_invitation_declined_proof(
    invitation_id: aura_core::InvitationId,
) -> InvitationDeclinedProof {
    let _ = semantic_postcondition_proof_capability();
    InvitationDeclinedProof { invitation_id }
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "semantic_postcondition_proof",
    family = "proof_issuer"
)]
#[aura_macros::authoritative_source(kind = "proof_issuer")]
pub(in crate::workflows) fn issue_invitation_revoked_proof(
    invitation_id: aura_core::InvitationId,
) -> InvitationRevokedProof {
    let _ = semantic_postcondition_proof_capability();
    InvitationRevokedProof { invitation_id }
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "semantic_postcondition_proof",
    family = "proof_issuer"
)]
#[aura_macros::authoritative_source(kind = "proof_issuer")]
pub(in crate::workflows) fn issue_device_enrollment_started_proof(
    ceremony_id: aura_core::CeremonyId,
) -> DeviceEnrollmentStartedProof {
    let _ = semantic_postcondition_proof_capability();
    DeviceEnrollmentStartedProof { ceremony_id }
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "semantic_postcondition_proof",
    family = "proof_issuer"
)]
#[aura_macros::authoritative_source(kind = "proof_issuer")]
pub(in crate::workflows) fn issue_message_committed_proof(
    message_id: impl Into<String>,
) -> MessageCommittedProof {
    let _ = semantic_postcondition_proof_capability();
    MessageCommittedProof {
        message_id: message_id.into(),
    }
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "semantic_postcondition_proof",
    family = "proof_issuer"
)]
#[aura_macros::authoritative_source(kind = "proof_issuer")]
pub(in crate::workflows) fn issue_device_enrollment_imported_proof(
    invitation_id: aura_core::InvitationId,
) -> DeviceEnrollmentImportedProof {
    let _ = semantic_postcondition_proof_capability();
    DeviceEnrollmentImportedProof { invitation_id }
}
