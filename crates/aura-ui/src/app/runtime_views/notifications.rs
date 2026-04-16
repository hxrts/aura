use crate::model::{NotificationSelectionId, UiController};
use aura_app::ui::signals::{CONTACTS_SIGNAL, ERROR_SIGNAL, INVITATIONS_SIGNAL, RECOVERY_SIGNAL};
use aura_app::ui::types::{
    AppError, ContactRelationshipState, ContactsState, InvitationsState, RecoveryState,
};
use aura_app::ui_contract::{
    InvitationFactKind, OperationState, RuntimeEventSnapshot, RuntimeFact,
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
    runtime_events: &[RuntimeEventSnapshot],
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

    for (idx, event) in runtime_events.iter().enumerate() {
        let Some(item) = runtime_event_notification(
            event,
            &contacts,
            runtime_event_timestamp(runtime_events.len(), idx),
        ) else {
            continue;
        };
        items.push(item);
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

fn runtime_event_timestamp(total_events: usize, index: usize) -> u64 {
    u64::MAX.saturating_sub(total_events.saturating_sub(index) as u64)
}

fn display_contact_name(contact: &aura_app::ui::types::Contact) -> String {
    if !contact.nickname.trim().is_empty() {
        return contact.nickname.clone();
    }
    if let Some(suggestion) = contact
        .nickname_suggestion
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        return suggestion.clone();
    }
    contact.id.to_string().chars().take(8).collect()
}

fn display_contact_name_for_authority(contacts: &ContactsState, authority_id: &str) -> String {
    contacts
        .all_contacts()
        .find(|contact| contact.id.to_string() == authority_id)
        .map(display_contact_name)
        .unwrap_or_else(|| authority_id.chars().take(8).collect())
}

fn runtime_event_notification(
    event: &RuntimeEventSnapshot,
    contacts: &ContactsState,
    timestamp: u64,
) -> Option<NotificationRuntimeItem> {
    match &event.fact {
        RuntimeFact::InvitationAccepted {
            invitation_kind: InvitationFactKind::Contact,
            authority_id: Some(authority_id),
            operation_state: Some(OperationState::Succeeded),
        } => {
            let name = display_contact_name_for_authority(contacts, authority_id);
            Some(NotificationRuntimeItem {
                id: format!("contact-accepted:{authority_id}"),
                kind_label: "Contact Invite Accepted".to_string(),
                title: format!("{name} is now a contact"),
                subtitle: "Contact link ready".to_string(),
                detail: authority_id.clone(),
                timestamp,
                action: NotificationRuntimeAction::None,
            })
        }
        RuntimeFact::GuardianInvitationAccepted {
            authority_id,
            guardian_name,
        } => {
            let name = guardian_name
                .clone()
                .filter(|value| !value.trim().is_empty())
                .or_else(|| {
                    authority_id
                        .as_deref()
                        .map(|value| display_contact_name_for_authority(contacts, value))
                })
                .unwrap_or_else(|| "Guardian".to_string());
            Some(NotificationRuntimeItem {
                id: format!(
                    "guardian-accepted:{}",
                    authority_id.as_deref().unwrap_or("*")
                ),
                kind_label: "Guardian Invite Accepted".to_string(),
                title: format!("{name} is now a guardian"),
                subtitle: "Guardian link ready".to_string(),
                detail: authority_id.clone().unwrap_or_else(|| name.clone()),
                timestamp,
                action: NotificationRuntimeAction::None,
            })
        }
        RuntimeFact::DeviceEnrollmentAccepted {
            device_id,
            device_name,
            device_count,
        } => {
            let name = device_name
                .clone()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| {
                    device_id
                        .as_deref()
                        .map(|value| value.chars().take(8).collect())
                        .unwrap_or_else(|| "Device".to_string())
                });
            Some(NotificationRuntimeItem {
                id: format!("device-accepted:{}", device_id.as_deref().unwrap_or("*")),
                kind_label: "Device Invite Accepted".to_string(),
                title: format!("{name} joined this account"),
                subtitle: device_count
                    .map(|count| format!("{count} registered devices"))
                    .unwrap_or_else(|| "Device enrollment completed".to_string()),
                detail: device_id.clone().unwrap_or_else(|| name.clone()),
                timestamp,
                action: NotificationRuntimeAction::None,
            })
        }
        _ => None,
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
    let runtime_events = controller
        .ui_model()
        .map(|model| model.runtime_events)
        .unwrap_or_default();
    let runtime =
        build_notifications_runtime_view(invitations, recovery, contacts, error, &runtime_events);
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

#[cfg(test)]
mod tests {
    use super::*;
    use aura_app::ui::contract::RuntimeEventId;
    use aura_app::ui::types::Contact;
    use aura_app::views::ReadReceiptPolicy;
    use aura_core::types::identifiers::AuthorityId;

    #[test]
    fn build_notifications_runtime_view_surfaces_contact_acceptance_events() {
        let alice = AuthorityId::new_from_entropy([1u8; 32]);
        let contacts = ContactsState::from_contacts([Contact {
            id: alice,
            nickname: "Alice".to_string(),
            nickname_suggestion: None,
            is_guardian: false,
            is_member: false,
            last_interaction: None,
            is_online: true,
            read_receipt_policy: ReadReceiptPolicy::default(),
            relationship_state: ContactRelationshipState::Contact,
            invitation_code: None,
        }]);
        let runtime = build_notifications_runtime_view(
            InvitationsState::default(),
            RecoveryState::default(),
            contacts,
            None,
            &[RuntimeEventSnapshot {
                id: RuntimeEventId("runtime-event-1".to_string()),
                fact: RuntimeFact::InvitationAccepted {
                    invitation_kind: InvitationFactKind::Contact,
                    authority_id: Some(alice.to_string()),
                    operation_state: Some(OperationState::Succeeded),
                },
            }],
        );

        assert_eq!(runtime.items.len(), 1);
        assert_eq!(runtime.items[0].id, format!("contact-accepted:{alice}"));
        assert_eq!(runtime.items[0].kind_label, "Contact Invite Accepted");
        assert_eq!(runtime.items[0].title, "Alice is now a contact");
    }

    #[test]
    fn build_notifications_runtime_view_surfaces_guardian_acceptance_events() {
        let runtime = build_notifications_runtime_view(
            InvitationsState::default(),
            RecoveryState::default(),
            ContactsState::default(),
            None,
            &[RuntimeEventSnapshot {
                id: RuntimeEventId("runtime-event-2".to_string()),
                fact: RuntimeFact::GuardianInvitationAccepted {
                    authority_id: Some("guardian-1".to_string()),
                    guardian_name: Some("Alice".to_string()),
                },
            }],
        );

        assert_eq!(runtime.items.len(), 1);
        assert_eq!(runtime.items[0].id, "guardian-accepted:guardian-1");
        assert_eq!(runtime.items[0].kind_label, "Guardian Invite Accepted");
        assert_eq!(runtime.items[0].title, "Alice is now a guardian");
    }

    #[test]
    fn build_notifications_runtime_view_surfaces_device_acceptance_events() {
        let runtime = build_notifications_runtime_view(
            InvitationsState::default(),
            RecoveryState::default(),
            ContactsState::default(),
            None,
            &[RuntimeEventSnapshot {
                id: RuntimeEventId("runtime-event-3".to_string()),
                fact: RuntimeFact::DeviceEnrollmentAccepted {
                    device_id: Some("device-1".to_string()),
                    device_name: Some("Laptop".to_string()),
                    device_count: Some(2),
                },
            }],
        );

        assert_eq!(runtime.items.len(), 1);
        assert_eq!(runtime.items[0].id, "device-accepted:device-1");
        assert_eq!(runtime.items[0].kind_label, "Device Invite Accepted");
        assert_eq!(runtime.items[0].title, "Laptop joined this account");
    }
}
