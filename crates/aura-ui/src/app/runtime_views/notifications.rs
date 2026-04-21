use crate::model::{NotificationSelectionId, UiController};
use aura_app::ui::signals::{CONTACTS_SIGNAL, ERROR_SIGNAL, INVITATIONS_SIGNAL, RECOVERY_SIGNAL};
use aura_app::ui::types::{
    AppError, ContactRelationshipState, ContactsState, InvitationsState, RecoveryState,
};
use aura_app::ui_contract::{
    AmpTransitionPolicySnapshot, AmpTransitionState, InvitationFactKind, OperationState,
    RuntimeEventSnapshot, RuntimeFact,
};
use aura_app::views::{truncate_id_for_display, EffectiveName};
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
    AmpRaiseEmergencyAlarm,
    AmpApproveQuarantine,
    AmpApproveCryptoshred,
    AmpViewConflictEvidence,
    AmpViewFinalizationStatus,
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
    contact.effective_name()
}

fn display_contact_name_for_authority(contacts: &ContactsState, authority_id: &str) -> String {
    contacts
        .all_contacts()
        .find(|contact| contact.id.to_string() == authority_id)
        .map(display_contact_name)
        .unwrap_or_else(|| truncate_id_for_display(authority_id))
}

fn amp_transition_policy_label(policy: Option<AmpTransitionPolicySnapshot>) -> &'static str {
    match policy {
        Some(AmpTransitionPolicySnapshot::Normal) => "normal transition",
        Some(AmpTransitionPolicySnapshot::Additive) => "additive transition",
        Some(AmpTransitionPolicySnapshot::Subtractive) => "subtractive transition",
        Some(AmpTransitionPolicySnapshot::EmergencyQuarantine) => "emergency quarantine",
        Some(AmpTransitionPolicySnapshot::EmergencyCryptoshred) => "emergency cryptoshred",
        None => "transition",
    }
}

fn amp_transition_state_label(state: AmpTransitionState) -> &'static str {
    match state {
        AmpTransitionState::Observed => "Observed proposal",
        AmpTransitionState::A2Live => "A2 live successor",
        AmpTransitionState::A2Conflict => "A2 conflict",
        AmpTransitionState::A3Finalized => "A3 finalized",
        AmpTransitionState::A3Conflict => "A3 conflict",
        AmpTransitionState::Aborted => "Aborted",
        AmpTransitionState::Superseded => "Superseded",
    }
}

fn amp_transition_action(
    state: AmpTransitionState,
    policy: Option<AmpTransitionPolicySnapshot>,
    has_conflict: bool,
) -> NotificationRuntimeAction {
    if has_conflict
        || matches!(
            state,
            AmpTransitionState::A2Conflict | AmpTransitionState::A3Conflict
        )
    {
        return NotificationRuntimeAction::AmpViewConflictEvidence;
    }
    match policy {
        Some(AmpTransitionPolicySnapshot::EmergencyCryptoshred) => {
            NotificationRuntimeAction::AmpApproveCryptoshred
        }
        Some(AmpTransitionPolicySnapshot::EmergencyQuarantine) => {
            NotificationRuntimeAction::AmpApproveQuarantine
        }
        _ if matches!(state, AmpTransitionState::Observed) => {
            NotificationRuntimeAction::AmpRaiseEmergencyAlarm
        }
        _ => NotificationRuntimeAction::AmpViewFinalizationStatus,
    }
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
        RuntimeFact::AmpChannelTransitionUpdated { transition } => {
            let channel = transition
                .channel
                .name
                .as_deref()
                .or(transition.channel.id.as_deref())
                .unwrap_or("channel");
            let state = amp_transition_state_label(transition.state);
            let policy = amp_transition_policy_label(transition.emergency_policy);
            let has_conflict = !transition.conflict_evidence.is_empty();
            let mut subtitle = format!(
                "{state}; stable epoch {}; {policy}",
                transition.stable_epoch
            );
            if transition.cryptoshred_active {
                subtitle.push_str("; pre-emergency readable state may be unavailable");
            }
            if !transition.suspect_authorities.is_empty() {
                subtitle.push_str("; suspect excluded");
            }

            Some(NotificationRuntimeItem {
                id: format!("amp-transition:{}", event.key()),
                kind_label: "AMP Transition".to_string(),
                title: format!("#{channel} transition: {state}"),
                subtitle,
                detail: transition
                    .live_transition_id
                    .clone()
                    .or_else(|| transition.finalized_transition_id.clone())
                    .or_else(|| transition.conflict_evidence.first().cloned())
                    .unwrap_or_else(|| channel.to_string()),
                timestamp,
                action: amp_transition_action(
                    transition.state,
                    transition.emergency_policy,
                    has_conflict,
                ),
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
                id: RuntimeEventId::synthetic("runtime-event-1"),
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
    fn build_notifications_runtime_view_uses_shared_contact_name_fallbacks() {
        let suggestion_only = AuthorityId::new_from_entropy([4u8; 32]);
        let fallback_only = AuthorityId::new_from_entropy([5u8; 32]);
        let fallback_name = truncate_id_for_display(&fallback_only.to_string());
        let runtime_event = |id: &str, fact: RuntimeFact| RuntimeEventSnapshot {
            id: RuntimeEventId::synthetic(id),
            fact,
        };
        let contacts = ContactsState::from_contacts([
            Contact {
                id: suggestion_only,
                nickname: String::new(),
                nickname_suggestion: Some("Suggested".to_string()),
                is_guardian: false,
                is_member: false,
                last_interaction: None,
                is_online: true,
                read_receipt_policy: ReadReceiptPolicy::default(),
                relationship_state: ContactRelationshipState::Contact,
                invitation_code: None,
            },
            Contact {
                id: fallback_only,
                nickname: String::new(),
                nickname_suggestion: None,
                is_guardian: false,
                is_member: false,
                last_interaction: None,
                is_online: false,
                read_receipt_policy: ReadReceiptPolicy::default(),
                relationship_state: ContactRelationshipState::Contact,
                invitation_code: None,
            },
        ]);
        let runtime = build_notifications_runtime_view(
            InvitationsState::default(),
            RecoveryState::default(),
            contacts,
            None,
            &[
                runtime_event(
                    "runtime-event-4",
                    RuntimeFact::InvitationAccepted {
                        invitation_kind: InvitationFactKind::Contact,
                        authority_id: Some(suggestion_only.to_string()),
                        operation_state: Some(OperationState::Succeeded),
                    },
                ),
                runtime_event(
                    "runtime-event-5",
                    RuntimeFact::InvitationAccepted {
                        invitation_kind: InvitationFactKind::Contact,
                        authority_id: Some(fallback_only.to_string()),
                        operation_state: Some(OperationState::Succeeded),
                    },
                ),
            ],
        );

        let suggestion_item = runtime
            .items
            .iter()
            .find(|item| item.id == format!("contact-accepted:{suggestion_only}"));
        let fallback_item = runtime
            .items
            .iter()
            .find(|item| item.id == format!("contact-accepted:{fallback_only}"));
        let expected_fallback_title = format!("{fallback_name} is now a contact");

        assert_eq!(
            suggestion_item.map(|item| item.title.as_str()),
            Some("Suggested is now a contact")
        );
        assert_eq!(
            fallback_item.map(|item| item.title.as_str()),
            Some(expected_fallback_title.as_str())
        );
    }

    #[test]
    fn build_notifications_runtime_view_surfaces_guardian_acceptance_events() {
        let runtime = build_notifications_runtime_view(
            InvitationsState::default(),
            RecoveryState::default(),
            ContactsState::default(),
            None,
            &[RuntimeEventSnapshot {
                id: RuntimeEventId::synthetic("runtime-event-2"),
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
                id: RuntimeEventId::synthetic("runtime-event-3"),
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

    #[test]
    fn build_notifications_runtime_view_surfaces_amp_transition_events() {
        let runtime = build_notifications_runtime_view(
            InvitationsState::default(),
            RecoveryState::default(),
            ContactsState::default(),
            None,
            &[RuntimeEventSnapshot {
                id: RuntimeEventId::synthetic("runtime-event-amp"),
                fact: RuntimeFact::AmpChannelTransitionUpdated {
                    transition: aura_app::ui_contract::AmpChannelTransitionSnapshot {
                        channel: aura_app::ui_contract::ChannelFactKey::identified("channel-a"),
                        stable_epoch: 4,
                        state: AmpTransitionState::A2Conflict,
                        live_transition_id: None,
                        finalized_transition_id: None,
                        conflict_evidence: vec!["evidence-a".to_string()],
                        emergency_policy: Some(AmpTransitionPolicySnapshot::EmergencyQuarantine),
                        suspect_authorities: vec!["authority-a".to_string()],
                        quarantine_epochs: vec![5],
                        prune_before_epochs: Vec::new(),
                        cryptoshred_active: false,
                        accusation_history: Vec::new(),
                    },
                },
            }],
        );

        assert_eq!(runtime.items.len(), 1);
        assert_eq!(runtime.items[0].kind_label, "AMP Transition");
        assert!(runtime.items[0].title.contains("A2 conflict"));
        assert_eq!(
            runtime.items[0].action,
            NotificationRuntimeAction::AmpViewConflictEvidence
        );
    }

    #[test]
    fn build_notifications_runtime_view_surfaces_amp_cryptoshred_confirmation() {
        let runtime = build_notifications_runtime_view(
            InvitationsState::default(),
            RecoveryState::default(),
            ContactsState::default(),
            None,
            &[RuntimeEventSnapshot {
                id: RuntimeEventId::synthetic("runtime-event-amp-cryptoshred"),
                fact: RuntimeFact::AmpChannelTransitionUpdated {
                    transition: aura_app::ui_contract::AmpChannelTransitionSnapshot {
                        channel: aura_app::ui_contract::ChannelFactKey::identified("channel-b"),
                        stable_epoch: 8,
                        state: AmpTransitionState::A2Live,
                        live_transition_id: Some("transition-b".to_string()),
                        finalized_transition_id: None,
                        conflict_evidence: Vec::new(),
                        emergency_policy: Some(AmpTransitionPolicySnapshot::EmergencyCryptoshred),
                        suspect_authorities: Vec::new(),
                        quarantine_epochs: Vec::new(),
                        prune_before_epochs: vec![7],
                        cryptoshred_active: true,
                        accusation_history: Vec::new(),
                    },
                },
            }],
        );

        assert_eq!(runtime.items.len(), 1);
        assert!(runtime.items[0]
            .subtitle
            .contains("pre-emergency readable state may be unavailable"));
        assert_eq!(
            runtime.items[0].action,
            NotificationRuntimeAction::AmpApproveCryptoshred
        );
    }
}
