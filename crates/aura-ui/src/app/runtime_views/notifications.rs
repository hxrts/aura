use crate::model::{NotificationSelectionId, UiController};
use aura_app::ui::signals::{
    CONTACTS_SIGNAL, ERROR_SIGNAL, INVITATIONS_SIGNAL, RECOVERY_SIGNAL,
};
use aura_app::ui::types::{
    AppError, ContactRelationshipState, ContactsState, InvitationsState, RecoveryState,
};
use aura_core::effects::reactive::ReactiveEffects;
use std::sync::Arc;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::app) struct NotificationRuntimeItem {
    pub(in crate::app) id: String,
    pub(in crate::app) kind_label: String,
    pub(in crate::app) title: String,
    pub(in crate::app) subtitle: String,
    pub(in crate::app) detail: String,
    pub(in crate::app) timestamp: u64,
    pub(in crate::app) action: NotificationRuntimeAction,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::app) enum NotificationRuntimeAction {
    #[default]
    None,
    ReceivedInvitation,
    PendingChannelInvitation,
    SentInvitation,
    RecoveryApproval,
    FriendRequest,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::app) struct NotificationsRuntimeView {
    pub(in crate::app) loaded: bool,
    pub(in crate::app) items: Vec<NotificationRuntimeItem>,
}

fn build_notifications_runtime_view(
    invitations: InvitationsState,
    recovery: RecoveryState,
    contacts: ContactsState,
    error: Option<AppError>,
) -> NotificationsRuntimeView {
    let mut items = Vec::new();

    for invitation in invitations.all_pending() {
        if !matches!(
            invitation.status,
            aura_app::ui::types::InvitationStatus::Pending
        ) {
            continue;
        }

        let (kind_label, title, subtitle, detail, action) =
            match (invitation.direction, invitation.invitation_type) {
                (
                    aura_app::ui::types::InvitationDirection::Received,
                    aura_app::ui::types::InvitationType::Guardian,
                ) => (
                    "Guardian Request",
                    format!("Guardian Request from {}", invitation.from_name),
                    invitation
                        .message
                        .clone()
                        .unwrap_or_else(|| "Pending response".to_string()),
                    invitation
                        .home_name
                        .clone()
                        .unwrap_or_else(|| invitation.from_id.to_string()),
                    NotificationRuntimeAction::ReceivedInvitation,
                ),
                (
                    aura_app::ui::types::InvitationDirection::Received,
                    aura_app::ui::types::InvitationType::Chat,
                ) => (
                    "Contact Request",
                    format!("Contact Request from {}", invitation.from_name),
                    invitation
                        .message
                        .clone()
                        .unwrap_or_else(|| "Pending response".to_string()),
                    invitation
                        .home_name
                        .clone()
                        .unwrap_or_else(|| invitation.from_id.to_string()),
                    NotificationRuntimeAction::ReceivedInvitation,
                ),
                (
                    aura_app::ui::types::InvitationDirection::Received,
                    aura_app::ui::types::InvitationType::Home,
                ) => (
                    "Home Invite",
                    format!("Home Invite from {}", invitation.from_name),
                    invitation
                        .message
                        .clone()
                        .unwrap_or_else(|| "Pending response".to_string()),
                    invitation
                        .home_name
                        .clone()
                        .unwrap_or_else(|| invitation.from_id.to_string()),
                    NotificationRuntimeAction::PendingChannelInvitation,
                ),
                (
                    aura_app::ui::types::InvitationDirection::Sent,
                    aura_app::ui::types::InvitationType::Guardian,
                ) => (
                    "Sent Guardian Invite",
                    format!(
                        "Guardian invite to {}",
                        invitation
                            .to_name
                            .clone()
                            .unwrap_or_else(|| "unknown recipient".to_string())
                    ),
                    invitation
                        .message
                        .clone()
                        .unwrap_or_else(|| "Waiting for recipient".to_string()),
                    invitation.home_name.clone().unwrap_or_else(|| {
                        invitation
                            .to_id
                            .map(|id| id.to_string())
                            .unwrap_or_else(|| "unknown recipient".to_string())
                    }),
                    NotificationRuntimeAction::SentInvitation,
                ),
                (
                    aura_app::ui::types::InvitationDirection::Sent,
                    aura_app::ui::types::InvitationType::Chat,
                ) => (
                    "Sent Contact Invite",
                    format!(
                        "Contact invite to {}",
                        invitation
                            .to_name
                            .clone()
                            .unwrap_or_else(|| "unknown recipient".to_string())
                    ),
                    invitation
                        .message
                        .clone()
                        .unwrap_or_else(|| "Waiting for recipient".to_string()),
                    invitation.home_name.clone().unwrap_or_else(|| {
                        invitation
                            .to_id
                            .map(|id| id.to_string())
                            .unwrap_or_else(|| "unknown recipient".to_string())
                    }),
                    NotificationRuntimeAction::SentInvitation,
                ),
                (
                    aura_app::ui::types::InvitationDirection::Sent,
                    aura_app::ui::types::InvitationType::Home,
                ) => (
                    "Sent Home Invite",
                    format!(
                        "Home invite to {}",
                        invitation
                            .to_name
                            .clone()
                            .unwrap_or_else(|| "unknown recipient".to_string())
                    ),
                    invitation
                        .message
                        .clone()
                        .unwrap_or_else(|| "Waiting for recipient".to_string()),
                    invitation.home_name.clone().unwrap_or_else(|| {
                        invitation
                            .to_id
                            .map(|id| id.to_string())
                            .unwrap_or_else(|| "unknown recipient".to_string())
                    }),
                    NotificationRuntimeAction::SentInvitation,
                ),
            };
        items.push(NotificationRuntimeItem {
            id: invitation.id.clone(),
            kind_label: kind_label.to_string(),
            title,
            subtitle,
            detail,
            timestamp: invitation.created_at,
            action,
        });
    }

    for request in recovery.pending_requests() {
        items.push(NotificationRuntimeItem {
            id: request.id.to_string(),
            kind_label: "Recovery Approval".to_string(),
            title: "Recovery approval requested".to_string(),
            subtitle: format!(
                "{}/{} approvals",
                request.approvals_received, request.approvals_required
            ),
            detail: request.account_id.to_string(),
            timestamp: request.initiated_at,
            action: NotificationRuntimeAction::RecoveryApproval,
        });
    }

    for contact in contacts.all_contacts() {
        if contact.relationship_state != ContactRelationshipState::PendingInbound {
            continue;
        }
        let name = if !contact.nickname.trim().is_empty() {
            contact.nickname.clone()
        } else if let Some(suggestion) = contact
            .nickname_suggestion
            .as_ref()
            .filter(|v| !v.trim().is_empty())
        {
            suggestion.clone()
        } else {
            contact.id.to_string().chars().take(8).collect()
        };
        items.push(NotificationRuntimeItem {
            id: contact.id.to_string(),
            kind_label: "Friend Request".to_string(),
            title: format!("Friend request from {name}"),
            subtitle: "Accept or decline".to_string(),
            detail: contact.id.to_string(),
            timestamp: contact.last_interaction.unwrap_or(0),
            action: NotificationRuntimeAction::FriendRequest,
        });
    }

    if let Some(error) = error {
        items.push(NotificationRuntimeItem {
            id: "runtime-error".to_string(),
            kind_label: "Runtime Error".to_string(),
            title: "Latest runtime error".to_string(),
            subtitle: error.to_string(),
            detail: "Check browser console and runtime logs for context.".to_string(),
            timestamp: 0,
            action: NotificationRuntimeAction::None,
        });
    }

    items.sort_by(|left, right| right.timestamp.cmp(&left.timestamp));
    NotificationsRuntimeView {
        loaded: true,
        items,
    }
}

pub(in crate::app) async fn load_notifications_runtime_view(
    controller: Arc<UiController>,
) -> NotificationsRuntimeView {
    let invitations = {
        let core = controller.app_core().read().await;
        core.read(&*INVITATIONS_SIGNAL).await.unwrap_or_default()
    };
    let recovery = {
        let core = controller.app_core().read().await;
        core.read(&*RECOVERY_SIGNAL).await.unwrap_or_default()
    };
    let contacts = {
        let core = controller.app_core().read().await;
        core.read(&*CONTACTS_SIGNAL).await.unwrap_or_default()
    };
    let error = {
        let core = controller.app_core().read().await;
        core.read(&*ERROR_SIGNAL).await.unwrap_or_default()
    };
    let runtime = build_notifications_runtime_view(invitations, recovery, contacts, error);
    controller.publish_runtime_notifications_projection(
        runtime
            .items
            .iter()
            .map(|item| (NotificationSelectionId(item.id.clone()), item.title.clone()))
            .collect(),
        Vec::new(),
    );
    runtime
}
