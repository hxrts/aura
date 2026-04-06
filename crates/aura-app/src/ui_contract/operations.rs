//! Shared semantic operation state, facts, and typed workflow outcomes.

use super::{RuntimeEventKind, RuntimeFact};
use aura_core::{OwnerEpoch, PublicationSequence};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToastKind {
    Success,
    Info,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ToastId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiReadiness {
    Loading,
    Ready,
}

#[aura_macros::ownership_lifecycle(
    initial = "Idle",
    ordered = "Idle,Submitting",
    terminals = "Succeeded,Failed"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    Idle,
    Submitting,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmationState {
    PendingLocal,
    Confirmed,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OperationId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OperationInstanceId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RuntimeEventId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelFactKey {
    pub id: Option<String>,
    pub name: Option<String>,
}

impl ChannelFactKey {
    #[must_use]
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            id: None,
            name: Some(name.into()),
        }
    }

    #[must_use]
    pub fn identified(id: impl Into<String>) -> Self {
        Self {
            id: Some(id.into()),
            name: None,
        }
    }

    #[must_use]
    pub fn matches_needle(&self, needle: &str) -> bool {
        self.id.as_deref().is_some_and(|id| id.contains(needle))
            || self
                .name
                .as_deref()
                .is_some_and(|name| name.contains(needle))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvitationFactKind {
    Generic,
    Contact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticOperationKind {
    CreateAccount,
    CreateHome,
    CreateNeighborhood,
    CreateChannel,
    StartGuardianCeremony,
    StartMultifactorCeremony,
    CancelGuardianCeremony,
    CancelKeyRotationCeremony,
    StartDeviceEnrollment,
    RemoveDevice,
    ImportDeviceEnrollmentCode,
    CreateContactInvitation,
    SendFriendRequest,
    AcceptFriendRequest,
    DeclineFriendRequest,
    RevokeFriendship,
    CreateHomeInvitation,
    CreateGuardianInvitation,
    ExportInvitation,
    AcceptContactInvitation,
    DeclineInvitation,
    RevokeInvitation,
    InviteActorToChannel,
    AcceptPendingChannelInvitation,
    JoinChannel,
    SendChatMessage,
    RetryChatMessage,
    SetChannelTopic,
    SetChannelMode,
    CloseChannel,
    KickActor,
    BanActor,
    UnbanActor,
    MuteActor,
    UnmuteActor,
    PinMessage,
    UnpinMessage,
    UpdateContactNickname,
    StartDirectChat,
    UpdateNicknameSuggestion,
    UpdateMfaPolicy,
    UpdateThreshold,
    GrantModerator,
    RevokeModerator,
    AddHomeToNeighborhood,
    LinkHomeOneHopLink,
    MovePosition,
    RemoveContact,
    StartRecovery,
    SubmitGuardianApproval,
}

#[aura_macros::ownership_lifecycle(
    initial = "Submitted",
    ordered = "Submitted,WorkflowDispatched,AuthoritativeContextReady,ContactLinkReady,MembershipReady,RecipientResolutionReady,PeerChannelReady,DeliveryReady",
    terminals = "Succeeded,Failed,Cancelled"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticOperationPhase {
    Submitted,
    WorkflowDispatched,
    AuthoritativeContextReady,
    ContactLinkReady,
    MembershipReady,
    RecipientResolutionReady,
    PeerChannelReady,
    DeliveryReady,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticFailureDomain {
    Command,
    Invitation,
    ChannelContext,
    Transport,
    Delivery,
    Projection,
    Internal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticFailureCode {
    UnsupportedCommand,
    MissingAuthoritativeContext,
    ContactLinkDidNotConverge,
    ChannelBootstrapUnavailable,
    PeerChannelNotEstablished,
    DeliveryReadinessNotReached,
    OperationTimedOut,
    ShellDeclaredSuccessIllegally,
    InternalError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticOperationError {
    pub domain: SemanticFailureDomain,
    pub code: SemanticFailureCode,
    pub detail: Option<String>,
}

impl SemanticOperationError {
    #[must_use]
    pub fn new(domain: SemanticFailureDomain, code: SemanticFailureCode) -> Self {
        Self {
            domain,
            code,
            detail: None,
        }
    }

    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticOperationStatus {
    pub kind: SemanticOperationKind,
    pub phase: SemanticOperationPhase,
    pub error: Option<SemanticOperationError>,
}

impl SemanticOperationStatus {
    #[must_use]
    pub fn new(kind: SemanticOperationKind, phase: SemanticOperationPhase) -> Self {
        Self {
            kind,
            phase,
            error: None,
        }
    }

    #[must_use]
    pub fn failed(kind: SemanticOperationKind, error: SemanticOperationError) -> Self {
        Self {
            kind,
            phase: SemanticOperationPhase::Failed,
            error: Some(error),
        }
    }

    #[must_use]
    pub fn cancelled(kind: SemanticOperationKind) -> Self {
        Self {
            kind,
            phase: SemanticOperationPhase::Cancelled,
            error: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowTerminalStatus {
    pub causality: Option<SemanticOperationCausality>,
    pub status: SemanticOperationStatus,
}

#[derive(Debug)]
pub struct WorkflowTerminalOutcome<T> {
    pub result: Result<T, aura_core::AuraError>,
    pub terminal: Option<WorkflowTerminalStatus>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticOperationCausality {
    pub owner_epoch: OwnerEpoch,
    pub publication_sequence: PublicationSequence,
}

impl SemanticOperationCausality {
    #[must_use]
    pub const fn new(owner_epoch: OwnerEpoch, publication_sequence: PublicationSequence) -> Self {
        Self {
            owner_epoch,
            publication_sequence,
        }
    }

    #[must_use]
    pub fn is_older_than(self, other: Self) -> bool {
        (self.owner_epoch.value(), self.publication_sequence.value())
            < (
                other.owner_epoch.value(),
                other.publication_sequence.value(),
            )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthoritativeSemanticFactKind {
    OperationStatus,
    InvitationAccepted,
    ContactLinkReady,
    PendingHomeInvitationReady,
    ChannelMembershipReady,
    RecipientPeersResolved,
    PeerChannelReady,
    MessageCommitted,
    MessageDeliveryReady,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AuthoritativeSemanticFact {
    OperationStatus {
        operation_id: OperationId,
        instance_id: Option<OperationInstanceId>,
        causality: Option<SemanticOperationCausality>,
        status: SemanticOperationStatus,
    },
    InvitationAccepted {
        invitation_kind: InvitationFactKind,
        authority_id: Option<String>,
        operation_state: Option<OperationState>,
    },
    ContactLinkReady {
        authority_id: String,
        contact_count: u32,
    },
    PendingHomeInvitationReady,
    ChannelMembershipReady {
        channel: ChannelFactKey,
        member_count: u32,
    },
    RecipientPeersResolved {
        channel: ChannelFactKey,
        member_count: u32,
    },
    PeerChannelReady {
        channel: ChannelFactKey,
        peer_authority_id: String,
        context_id: Option<String>,
    },
    MessageCommitted {
        channel: ChannelFactKey,
        content: String,
    },
    MessageDeliveryReady {
        channel: ChannelFactKey,
        member_count: u32,
    },
}

impl AuthoritativeSemanticFact {
    #[must_use]
    pub fn kind(&self) -> AuthoritativeSemanticFactKind {
        match self {
            Self::OperationStatus { .. } => AuthoritativeSemanticFactKind::OperationStatus,
            Self::InvitationAccepted { .. } => AuthoritativeSemanticFactKind::InvitationAccepted,
            Self::ContactLinkReady { .. } => AuthoritativeSemanticFactKind::ContactLinkReady,
            Self::PendingHomeInvitationReady => {
                AuthoritativeSemanticFactKind::PendingHomeInvitationReady
            }
            Self::ChannelMembershipReady { .. } => {
                AuthoritativeSemanticFactKind::ChannelMembershipReady
            }
            Self::RecipientPeersResolved { .. } => {
                AuthoritativeSemanticFactKind::RecipientPeersResolved
            }
            Self::PeerChannelReady { .. } => AuthoritativeSemanticFactKind::PeerChannelReady,
            Self::MessageCommitted { .. } => AuthoritativeSemanticFactKind::MessageCommitted,
            Self::MessageDeliveryReady { .. } => {
                AuthoritativeSemanticFactKind::MessageDeliveryReady
            }
        }
    }

    #[must_use]
    pub fn key(&self) -> String {
        match self {
            Self::OperationStatus {
                operation_id,
                instance_id,
                ..
            } => format!(
                "operation_status:{}:{}",
                operation_id.0,
                instance_id
                    .as_ref()
                    .map(|value| value.0.as_str())
                    .unwrap_or("*")
            ),
            Self::InvitationAccepted {
                invitation_kind,
                authority_id,
                ..
            } => format!(
                "invitation_accepted:{invitation_kind:?}:{}",
                authority_id.as_deref().unwrap_or("*")
            ),
            Self::ContactLinkReady { authority_id, .. } => {
                format!("contact_link_ready:{authority_id}")
            }
            Self::PendingHomeInvitationReady => "pending_home_invitation_ready".to_string(),
            Self::ChannelMembershipReady { channel, .. } => format!(
                "channel_membership_ready:{}",
                channel
                    .name
                    .as_deref()
                    .or(channel.id.as_deref())
                    .unwrap_or("*")
            ),
            Self::RecipientPeersResolved { channel, .. } => format!(
                "recipient_peers_resolved:{}",
                channel
                    .name
                    .as_deref()
                    .or(channel.id.as_deref())
                    .unwrap_or("*")
            ),
            Self::PeerChannelReady {
                channel,
                peer_authority_id,
                ..
            } => format!(
                "peer_channel_ready:{}:{peer_authority_id}",
                channel
                    .name
                    .as_deref()
                    .or(channel.id.as_deref())
                    .unwrap_or("*")
            ),
            Self::MessageCommitted { channel, content } => format!(
                "message_committed:{}:{}",
                channel
                    .name
                    .as_deref()
                    .or(channel.id.as_deref())
                    .unwrap_or("*"),
                content
            ),
            Self::MessageDeliveryReady { channel, .. } => format!(
                "message_delivery_ready:{}",
                channel
                    .name
                    .as_deref()
                    .or(channel.id.as_deref())
                    .unwrap_or("*")
            ),
        }
    }

    #[must_use]
    pub fn runtime_fact_bridge(&self) -> Option<(RuntimeEventKind, RuntimeFact)> {
        match self {
            Self::InvitationAccepted {
                invitation_kind,
                authority_id,
                operation_state,
            } => Some((
                RuntimeEventKind::InvitationAccepted,
                RuntimeFact::InvitationAccepted {
                    invitation_kind: *invitation_kind,
                    authority_id: authority_id.clone(),
                    operation_state: *operation_state,
                },
            )),
            Self::ContactLinkReady {
                authority_id,
                contact_count,
            } => Some((
                RuntimeEventKind::ContactLinkReady,
                RuntimeFact::ContactLinkReady {
                    authority_id: Some(authority_id.clone()),
                    contact_count: Some(*contact_count as usize),
                },
            )),
            Self::PendingHomeInvitationReady => Some((
                RuntimeEventKind::PendingHomeInvitationReady,
                RuntimeFact::PendingHomeInvitationReady,
            )),
            Self::ChannelMembershipReady {
                channel,
                member_count,
            } => Some((
                RuntimeEventKind::ChannelMembershipReady,
                RuntimeFact::ChannelMembershipReady {
                    channel: channel.clone(),
                    member_count: Some(*member_count),
                },
            )),
            Self::RecipientPeersResolved {
                channel,
                member_count,
            } => Some((
                RuntimeEventKind::RecipientPeersResolved,
                RuntimeFact::RecipientPeersResolved {
                    channel: channel.clone(),
                    member_count: *member_count,
                },
            )),
            Self::MessageCommitted { channel, content } => Some((
                RuntimeEventKind::MessageCommitted,
                RuntimeFact::MessageCommitted {
                    channel: channel.clone(),
                    content: content.clone(),
                },
            )),
            Self::MessageDeliveryReady {
                channel,
                member_count,
            } => Some((
                RuntimeEventKind::MessageDeliveryReady,
                RuntimeFact::MessageDeliveryReady {
                    channel: channel.clone(),
                    member_count: *member_count,
                },
            )),
            Self::OperationStatus { .. } | Self::PeerChannelReady { .. } => None,
        }
    }

    #[must_use]
    pub fn operation_status_bridge(
        &self,
    ) -> Option<(
        OperationId,
        Option<OperationInstanceId>,
        Option<SemanticOperationCausality>,
        SemanticOperationStatus,
    )> {
        match self {
            Self::OperationStatus {
                operation_id,
                instance_id,
                causality,
                status,
            } => Some((
                operation_id.clone(),
                instance_id.clone(),
                *causality,
                status.clone(),
            )),
            _ => None,
        }
    }
}

#[must_use]
pub fn bridged_operation_statuses(
    facts: &[AuthoritativeSemanticFact],
) -> Vec<(
    OperationId,
    Option<OperationInstanceId>,
    Option<SemanticOperationCausality>,
    SemanticOperationStatus,
)> {
    let mut bridged = facts
        .iter()
        .filter_map(AuthoritativeSemanticFact::operation_status_bridge)
        .collect::<Vec<_>>();

    let contact_link_ready = facts
        .iter()
        .any(|fact| matches!(fact, AuthoritativeSemanticFact::ContactLinkReady { .. }));

    if contact_link_ready {
        for (operation_id, _instance_id, _causality, status) in &mut bridged {
            if *operation_id == OperationId::invitation_accept_contact()
                && status.kind == SemanticOperationKind::AcceptContactInvitation
                && !status.phase.is_terminal()
            {
                *status = SemanticOperationStatus::new(
                    SemanticOperationKind::AcceptContactInvitation,
                    SemanticOperationPhase::Succeeded,
                );
            }
        }
    }

    bridged
}

impl OperationId {
    #[must_use]
    pub fn account_create() -> Self {
        Self("account_create".to_string())
    }

    #[must_use]
    pub fn create_home() -> Self {
        Self("create_home".to_string())
    }

    #[must_use]
    pub fn create_channel() -> Self {
        Self("create_channel".to_string())
    }

    #[must_use]
    pub fn start_guardian_ceremony() -> Self {
        Self("start_guardian_ceremony".to_string())
    }

    #[must_use]
    pub fn start_multifactor_ceremony() -> Self {
        Self("start_multifactor_ceremony".to_string())
    }

    #[must_use]
    pub fn cancel_guardian_ceremony() -> Self {
        Self("cancel_guardian_ceremony".to_string())
    }

    #[must_use]
    pub fn cancel_key_rotation_ceremony() -> Self {
        Self("cancel_key_rotation_ceremony".to_string())
    }

    #[must_use]
    pub fn invitation_create() -> Self {
        Self("invitation_create".to_string())
    }

    #[must_use]
    pub fn home_invitation_create() -> Self {
        Self("home_invitation_create".to_string())
    }

    #[must_use]
    pub fn invitation_accept_contact() -> Self {
        Self("invitation_accept_contact".to_string())
    }

    #[must_use]
    pub fn invitation_accept_channel() -> Self {
        Self("invitation_accept_channel".to_string())
    }

    #[must_use]
    pub fn invitation_export() -> Self {
        Self("invitation_export".to_string())
    }

    #[must_use]
    pub fn device_enrollment() -> Self {
        Self("device_enrollment".to_string())
    }

    #[must_use]
    pub fn remove_device() -> Self {
        Self("remove_device".to_string())
    }

    #[must_use]
    pub fn send_message() -> Self {
        Self("send_message".to_string())
    }

    #[must_use]
    pub fn join_channel() -> Self {
        Self("join_channel".to_string())
    }

    #[must_use]
    pub fn invitation_decline() -> Self {
        Self("invitation_decline".to_string())
    }

    #[must_use]
    pub fn invitation_revoke() -> Self {
        Self("invitation_revoke".to_string())
    }

    #[must_use]
    pub fn remove_contact() -> Self {
        Self("remove_contact".to_string())
    }

    #[must_use]
    pub fn send_friend_request() -> Self {
        Self("send_friend_request".to_string())
    }

    #[must_use]
    pub fn accept_friend_request() -> Self {
        Self("accept_friend_request".to_string())
    }

    #[must_use]
    pub fn decline_friend_request() -> Self {
        Self("decline_friend_request".to_string())
    }

    #[must_use]
    pub fn revoke_friendship() -> Self {
        Self("revoke_friendship".to_string())
    }

    #[must_use]
    pub fn retry_message() -> Self {
        Self("retry_message".to_string())
    }

    #[must_use]
    pub fn set_channel_topic() -> Self {
        Self("set_channel_topic".to_string())
    }

    #[must_use]
    pub fn set_channel_mode() -> Self {
        Self("set_channel_mode".to_string())
    }

    #[must_use]
    pub fn close_channel() -> Self {
        Self("close_channel".to_string())
    }

    #[must_use]
    pub fn kick_actor() -> Self {
        Self("kick_actor".to_string())
    }

    #[must_use]
    pub fn ban_actor() -> Self {
        Self("ban_actor".to_string())
    }

    #[must_use]
    pub fn unban_actor() -> Self {
        Self("unban_actor".to_string())
    }

    #[must_use]
    pub fn mute_actor() -> Self {
        Self("mute_actor".to_string())
    }

    #[must_use]
    pub fn unmute_actor() -> Self {
        Self("unmute_actor".to_string())
    }

    #[must_use]
    pub fn pin_message() -> Self {
        Self("pin_message".to_string())
    }

    #[must_use]
    pub fn unpin_message() -> Self {
        Self("unpin_message".to_string())
    }

    #[must_use]
    pub fn update_contact_nickname() -> Self {
        Self("update_contact_nickname".to_string())
    }

    #[must_use]
    pub fn start_direct_chat() -> Self {
        Self("start_direct_chat".to_string())
    }

    #[must_use]
    pub fn update_nickname_suggestion() -> Self {
        Self("update_nickname_suggestion".to_string())
    }

    #[must_use]
    pub fn update_mfa_policy() -> Self {
        Self("update_mfa_policy".to_string())
    }

    #[must_use]
    pub fn update_threshold() -> Self {
        Self("update_threshold".to_string())
    }

    #[must_use]
    pub fn grant_moderator() -> Self {
        Self("grant_moderator".to_string())
    }

    #[must_use]
    pub fn revoke_moderator() -> Self {
        Self("revoke_moderator".to_string())
    }

    #[must_use]
    pub fn create_neighborhood() -> Self {
        Self("create_neighborhood".to_string())
    }

    #[must_use]
    pub fn add_home_to_neighborhood() -> Self {
        Self("add_home_to_neighborhood".to_string())
    }

    #[must_use]
    pub fn link_home_one_hop_link() -> Self {
        Self("link_home_one_hop_link".to_string())
    }

    #[must_use]
    pub fn move_position() -> Self {
        Self("move_position".to_string())
    }

    #[must_use]
    pub fn start_recovery() -> Self {
        Self("start_recovery".to_string())
    }

    #[must_use]
    pub fn submit_guardian_approval() -> Self {
        Self("submit_guardian_approval".to_string())
    }
}
