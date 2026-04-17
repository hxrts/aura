//! # Notifications Screen
//!
//! Aggregated notifications for invitations, recovery approvals, and MFA prompts.

use iocraft::prelude::*;

use aura_app::ui::signals::{CONTACTS_SIGNAL, INVITATIONS_SIGNAL, RECOVERY_SIGNAL};
use aura_app::ui::types::invitations::{InvitationDirection, InvitationStatus, InvitationType};
use aura_app::ui_contract::{InvitationFactKind, OperationState, RuntimeFact};

use crate::tui::components::{DetailPanel, KeyValue, ListPanel};
use crate::tui::hooks::{subscribe_signal_with_retry, AppCoreContext};
use crate::tui::layout::dim;
use crate::tui::props::NotificationsViewProps;
use crate::tui::theme::{Spacing, Theme};
use crate::tui::types::PendingRequest;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NotificationKind {
    ContactInvite,
    GuardianInvite,
    HomeInvite,
    RecoveryApproval,
    ContactInviteAccepted,
    GuardianInviteAccepted,
    DeviceInviteAccepted,
}

impl NotificationKind {
    fn icon(self) -> &'static str {
        match self {
            Self::ContactInvite => "@",
            Self::GuardianInvite => "◆",
            Self::HomeInvite => "■",
            Self::RecoveryApproval => "⊗",
            Self::ContactInviteAccepted => "✓",
            Self::GuardianInviteAccepted => "✓",
            Self::DeviceInviteAccepted => "✓",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::ContactInvite => "Contact request",
            Self::GuardianInvite => "Guardian request",
            Self::HomeInvite => "Home invite",
            Self::RecoveryApproval => "Recovery approval",
            Self::ContactInviteAccepted => "Contact invite accepted",
            Self::GuardianInviteAccepted => "Guardian invite accepted",
            Self::DeviceInviteAccepted => "Device invite accepted",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::ContactInvite => Theme::PRIMARY,
            Self::GuardianInvite => Theme::WARNING,
            Self::HomeInvite => Theme::TEXT,
            Self::RecoveryApproval => Theme::SUCCESS,
            Self::ContactInviteAccepted => Theme::SUCCESS,
            Self::GuardianInviteAccepted => Theme::SUCCESS,
            Self::DeviceInviteAccepted => Theme::SUCCESS,
        }
    }
}

#[derive(Clone, Debug)]
struct NotificationItem {
    id: String,
    title: String,
    subtitle: String,
    kind: NotificationKind,
    timestamp: u64,
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

fn runtime_event_timestamp(total_events: usize, index: usize) -> u64 {
    u64::MAX.saturating_sub(total_events.saturating_sub(index) as u64)
}

fn runtime_notification_item(
    fact: &RuntimeFact,
    timestamp: u64,
    contact_names: &HashMap<String, String>,
) -> Option<NotificationItem> {
    match fact {
        RuntimeFact::InvitationAccepted {
            invitation_kind: InvitationFactKind::Contact,
            authority_id: Some(authority_id),
            operation_state: Some(OperationState::Succeeded),
        } => {
            let name = contact_names
                .get(authority_id)
                .cloned()
                .unwrap_or_else(|| authority_id.chars().take(8).collect());
            Some(NotificationItem {
                id: format!("contact-accepted:{authority_id}"),
                title: format!("{name} is now a contact"),
                subtitle: "Contact link ready".to_string(),
                kind: NotificationKind::ContactInviteAccepted,
                timestamp,
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
                        .as_ref()
                        .and_then(|value| contact_names.get(value).cloned())
                })
                .unwrap_or_else(|| "Guardian".to_string());
            Some(NotificationItem {
                id: format!(
                    "guardian-accepted:{}",
                    authority_id.as_deref().unwrap_or("*")
                ),
                title: format!("{name} is now a guardian"),
                subtitle: "Guardian link ready".to_string(),
                kind: NotificationKind::GuardianInviteAccepted,
                timestamp,
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
            Some(NotificationItem {
                id: format!("device-accepted:{}", device_id.as_deref().unwrap_or("*")),
                title: format!("{name} joined this account"),
                subtitle: device_count
                    .map(|count| format!("{count} registered devices"))
                    .unwrap_or_else(|| "Device enrollment completed".to_string()),
                kind: NotificationKind::DeviceInviteAccepted,
                timestamp,
            })
        }
        _ => None,
    }
}

/// Props for NotificationsScreen
#[derive(Default, Props)]
pub struct NotificationsScreenProps {
    /// All view state extracted from TuiState via `extract_notifications_view_props()`.
    pub view: NotificationsViewProps,
}

#[component]
pub fn NotificationsScreen(
    props: &NotificationsScreenProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let app_ctx = hooks.use_context::<AppCoreContext>();

    let reactive_invites = hooks.use_state(Vec::new);
    let reactive_recovery = hooks.use_state(Vec::new);
    let reactive_contact_names = hooks.use_state(HashMap::<String, String>::new);

    hooks.use_future({
        let mut reactive_contact_names = reactive_contact_names.clone();
        let app_core = app_ctx.app_core.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*CONTACTS_SIGNAL, move |state| {
                let names = state
                    .all_contacts()
                    .map(|contact| (contact.id.to_string(), display_contact_name(contact)))
                    .collect::<HashMap<_, _>>();
                reactive_contact_names.set(names);
            })
            .await;
        }
    });

    // Invitations notifications
    hooks.use_future({
        let mut reactive_invites = reactive_invites.clone();
        let app_core = app_ctx.app_core.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*INVITATIONS_SIGNAL, move |state| {
                let mut items = Vec::new();
                for inv in state.all_pending() {
                    if inv.direction != InvitationDirection::Received
                        || inv.status != InvitationStatus::Pending
                    {
                        continue;
                    }

                    let (kind, title) = match inv.invitation_type {
                        InvitationType::Guardian => (
                            NotificationKind::GuardianInvite,
                            format!("Guardian request from {}", inv.from_name),
                        ),
                        InvitationType::Chat => (
                            NotificationKind::ContactInvite,
                            format!("Contact request from {}", inv.from_name),
                        ),
                        InvitationType::Home => (
                            NotificationKind::HomeInvite,
                            format!("Home invite from {}", inv.from_name),
                        ),
                    };

                    let subtitle = inv
                        .message
                        .clone()
                        .unwrap_or_else(|| "Pending response".to_string());

                    items.push(NotificationItem {
                        id: inv.id.clone(),
                        title,
                        subtitle,
                        kind,
                        timestamp: inv.created_at,
                    });
                }

                reactive_invites.set(items);
            })
            .await;
        }
    });

    // Recovery approval notifications
    hooks.use_future({
        let mut reactive_recovery = reactive_recovery.clone();
        let app_core = app_ctx.app_core.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*RECOVERY_SIGNAL, move |state| {
                let mut items = Vec::new();
                for req in state.pending_requests() {
                    let pending = PendingRequest::from(req);
                    let progress = format!(
                        "{}/{} approvals",
                        pending.approvals_received, pending.approvals_required
                    );
                    let account = if pending.account_name.is_empty() {
                        "Unknown account".to_string()
                    } else if pending.account_name.len() > 16 {
                        format!("{}…", &pending.account_name[..8])
                    } else {
                        pending.account_name.clone()
                    };

                    items.push(NotificationItem {
                        id: pending.id.clone(),
                        title: format!("Recovery approval for {account}"),
                        subtitle: progress,
                        kind: NotificationKind::RecoveryApproval,
                        timestamp: pending.initiated_at,
                    });
                }

                reactive_recovery.set(items);
            })
            .await;
        }
    });

    let mut notifications = reactive_invites.read().clone();
    notifications.extend(reactive_recovery.read().clone());
    let contact_names = reactive_contact_names.read().clone();
    notifications.extend(
        props
            .view
            .runtime_facts
            .iter()
            .enumerate()
            .filter_map(|(idx, fact)| {
                runtime_notification_item(
                    fact,
                    runtime_event_timestamp(props.view.runtime_facts.len(), idx),
                    &contact_names,
                )
            }),
    );
    notifications.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    // Filter out dismissed notifications.
    notifications.retain(|item| !props.view.dismissed_ids.contains(&item.id));

    // Write back the visible IDs so the keyboard handler can resolve
    // selected_index → concrete notification ID for dismissal.
    if let Ok(mut sink) = props.view.visible_ids_sink.lock() {
        *sink = notifications.iter().map(|item| item.id.clone()).collect();
    }

    let selected_index = props
        .view
        .selected_index
        .min(notifications.len().saturating_sub(1));
    let selected = notifications.get(selected_index);

    let list_items: Vec<AnyElement<'static>> = notifications
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            let is_selected = idx == selected_index;
            let bg = if is_selected {
                Theme::LIST_BG_SELECTED
            } else {
                Theme::LIST_BG_NORMAL
            };
            let text_color = if is_selected {
                Theme::LIST_TEXT_SELECTED
            } else {
                Theme::LIST_TEXT_NORMAL
            };

            element! {
                View(
                    key: item.id.clone(),
                    flex_direction: FlexDirection::Row,
                    background_color: bg,
                    padding_left: Spacing::XS,
                    padding_right: Spacing::XS,
                    gap: Spacing::XS,
                ) {
                    Text(content: item.kind.icon().to_string(), color: item.kind.color())
                    View(flex_direction: FlexDirection::Column, flex_grow: 1.0) {
                        Text(content: item.title.clone(), color: text_color)
                        Text(content: item.subtitle.clone(), color: Theme::TEXT_MUTED)
                    }
                    Text(content: item.kind.label().to_string(), color: Theme::TEXT_MUTED)
                }
            }
            .into_any()
        })
        .collect();

    let detail_content: Vec<AnyElement<'static>> = if let Some(item) = selected {
        vec![
            element! { KeyValue(label: "Type".to_string(), value: item.kind.label().to_string()) }
                .into_any(),
            element! { KeyValue(label: "Title".to_string(), value: item.title.clone()) }.into_any(),
            element! { KeyValue(label: "Details".to_string(), value: item.subtitle.clone()) }
                .into_any(),
        ]
    } else {
        Vec::new()
    };

    element! {
        View(
            flex_direction: FlexDirection::Column,
            width: dim::TOTAL_WIDTH,
            height: dim::MIDDLE_HEIGHT,
            overflow: Overflow::Hidden,
        ) {
            View(
                flex_direction: FlexDirection::Row,
                height: dim::MIDDLE_HEIGHT,
                gap: dim::TWO_PANEL_GAP,
                overflow: Overflow::Hidden,
            ) {
                View(
                    width: dim::TWO_PANEL_LEFT_WIDTH,
                    height: dim::MIDDLE_HEIGHT,
                ) {
                    ListPanel(
                        title: "Notifications".to_string(),
                        count: notifications.len(),
                        focused: false,
                        items: list_items,
                        empty_message: "No notifications".to_string(),
                    )
                }

                View(
                    width: dim::TWO_PANEL_RIGHT_WIDTH,
                    height: dim::MIDDLE_HEIGHT,
                ) {
                    DetailPanel(
                        title: "Details".to_string(),
                        focused: false,
                        content: detail_content,
                        empty_message: "Select a notification".to_string(),
                    )
                }
            }
        }
    }
}

/// Run the notifications screen (requires AppCoreContext for domain data)
pub async fn run_notifications_screen() -> std::io::Result<()> {
    element! {
        NotificationsScreen(
            view: NotificationsViewProps::default(),
        )
    }
    .fullscreen()
    .await
}
