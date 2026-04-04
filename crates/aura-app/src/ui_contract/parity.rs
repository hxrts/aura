use serde::{Deserialize, Serialize};

use super::{
    classify_settings_section_item_id, ConfirmationState, ControlId, ListId, OperationState,
    RuntimeEventKind, ScreenId, SettingsSectionSurfaceId, ToastKind, UiSnapshot,
};

type ParityListItemSignature = (String, bool, ConfirmationState);
type ParityListSignature = (ListId, Vec<ParityListItemSignature>);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiParityMismatch {
    pub field: &'static str,
    pub web: String,
    pub tui: String,
}

fn parity_relevant_lists(screen: ScreenId) -> &'static [ListId] {
    match screen {
        ScreenId::Onboarding => &[],
        ScreenId::Neighborhood => &[
            ListId::Navigation,
            ListId::Homes,
            ListId::NeighborhoodMembers,
        ],
        ScreenId::Chat => &[ListId::Navigation, ListId::Channels],
        ScreenId::Contacts => &[ListId::Navigation, ListId::Contacts],
        ScreenId::Notifications => &[ListId::Navigation, ListId::Notifications],
        ScreenId::Settings => &[ListId::Navigation, ListId::SettingsSections],
    }
}

fn parity_list_signature(snapshot: &UiSnapshot) -> Vec<ParityListSignature> {
    let relevant_lists = parity_relevant_lists(snapshot.screen);
    let mut lists = snapshot
        .lists
        .iter()
        .filter(|list| relevant_lists.contains(&list.id))
        .map(|list| {
            let mut items = list
                .items
                .iter()
                .filter(|item| {
                    !(snapshot.screen == ScreenId::Settings
                        && list.id == ListId::SettingsSections
                        && !matches!(
                            classify_settings_section_item_id(&item.id),
                            Some(SettingsSectionSurfaceId::Shared(_))
                        ))
                })
                .map(|item| (item.id.clone(), item.selected, item.confirmation))
                .collect::<Vec<_>>();
            items.sort_by(|left, right| {
                left.0
                    .cmp(&right.0)
                    .then_with(|| left.1.cmp(&right.1))
                    .then_with(|| format!("{:?}", left.2).cmp(&format!("{:?}", right.2)))
            });
            (list.id, items)
        })
        .collect::<Vec<_>>();
    lists.sort_by_key(|(list_id, _)| list_id.dom_segment());
    lists
}

fn parity_selection_signature(snapshot: &UiSnapshot) -> Vec<(ListId, String)> {
    let relevant_lists = parity_relevant_lists(snapshot.screen);
    let mut selections = snapshot
        .selections
        .iter()
        .filter(|selection| relevant_lists.contains(&selection.list))
        .filter(|selection| {
            !(snapshot.screen == ScreenId::Settings
                && selection.list == ListId::SettingsSections
                && !matches!(
                    classify_settings_section_item_id(&selection.item_id),
                    Some(SettingsSectionSurfaceId::Shared(_))
                ))
        })
        .map(|selection| (selection.list, selection.item_id.clone()))
        .collect::<Vec<_>>();
    selections.sort_by(|left, right| {
        left.0
            .dom_segment()
            .cmp(right.0.dom_segment())
            .then_with(|| left.1.cmp(&right.1))
    });
    selections
}

fn parity_operation_signature(snapshot: &UiSnapshot) -> Vec<(String, OperationState)> {
    let mut operations = snapshot
        .operations
        .iter()
        .map(|operation| (operation.id.0.clone(), operation.state))
        .collect::<Vec<_>>();
    operations.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| format!("{:?}", left.1).cmp(&format!("{:?}", right.1)))
    });
    operations
}

fn parity_message_signature(snapshot: &UiSnapshot) -> Vec<String> {
    let mut messages = snapshot
        .messages
        .iter()
        .map(|message| message.content.clone())
        .collect::<Vec<_>>();
    messages.sort();
    messages
}

fn parity_toast_signature(snapshot: &UiSnapshot) -> Vec<(ToastKind, String)> {
    let mut toasts = snapshot
        .toasts
        .iter()
        .map(|toast| (toast.kind, toast.message.clone()))
        .collect::<Vec<_>>();
    toasts.sort_by(|left, right| {
        format!("{:?}", left.0)
            .cmp(&format!("{:?}", right.0))
            .then_with(|| left.1.cmp(&right.1))
    });
    toasts
}

fn parity_runtime_event_signature(snapshot: &UiSnapshot) -> Vec<(RuntimeEventKind, String)> {
    let mut events = snapshot
        .runtime_events
        .iter()
        .map(|event| (event.kind(), event.key()))
        .collect::<Vec<_>>();
    events.sort_by(|left, right| {
        format!("{:?}", left.0)
            .cmp(&format!("{:?}", right.0))
            .then_with(|| left.1.cmp(&right.1))
    });
    events
}

#[must_use]
pub fn compare_ui_snapshots_for_parity(
    web: &UiSnapshot,
    tui: &UiSnapshot,
) -> Vec<UiParityMismatch> {
    let mut mismatches = Vec::new();

    if web.screen != tui.screen {
        mismatches.push(UiParityMismatch {
            field: "screen",
            web: format!("{:?}", web.screen),
            tui: format!("{:?}", tui.screen),
        });
    }
    if web.readiness != tui.readiness {
        mismatches.push(UiParityMismatch {
            field: "readiness",
            web: format!("{:?}", web.readiness),
            tui: format!("{:?}", tui.readiness),
        });
    }
    if web.open_modal != tui.open_modal {
        mismatches.push(UiParityMismatch {
            field: "open_modal",
            web: format!("{:?}", web.open_modal),
            tui: format!("{:?}", tui.open_modal),
        });
    }
    if web.focused_control != tui.focused_control {
        mismatches.push(UiParityMismatch {
            field: "focused_control",
            web: format!("{:?}", web.focused_control),
            tui: format!("{:?}", tui.focused_control),
        });
    }

    let web_selections = parity_selection_signature(web);
    let tui_selections = parity_selection_signature(tui);
    if web_selections != tui_selections {
        mismatches.push(UiParityMismatch {
            field: "selections",
            web: format!("{web_selections:?}"),
            tui: format!("{tui_selections:?}"),
        });
    }

    let web_lists = parity_list_signature(web);
    let tui_lists = parity_list_signature(tui);
    if web_lists != tui_lists {
        mismatches.push(UiParityMismatch {
            field: "lists",
            web: format!("{web_lists:?}"),
            tui: format!("{tui_lists:?}"),
        });
    }

    let web_operations = parity_operation_signature(web);
    let tui_operations = parity_operation_signature(tui);
    if web_operations != tui_operations {
        mismatches.push(UiParityMismatch {
            field: "operations",
            web: format!("{web_operations:?}"),
            tui: format!("{tui_operations:?}"),
        });
    }

    let web_messages = parity_message_signature(web);
    let tui_messages = parity_message_signature(tui);
    if web_messages != tui_messages {
        mismatches.push(UiParityMismatch {
            field: "messages",
            web: format!("{web_messages:?}"),
            tui: format!("{tui_messages:?}"),
        });
    }

    let web_toasts = parity_toast_signature(web);
    let tui_toasts = parity_toast_signature(tui);
    if web_toasts != tui_toasts {
        mismatches.push(UiParityMismatch {
            field: "toasts",
            web: format!("{web_toasts:?}"),
            tui: format!("{tui_toasts:?}"),
        });
    }

    let web_runtime_events = parity_runtime_event_signature(web);
    let tui_runtime_events = parity_runtime_event_signature(tui);
    if web_runtime_events != tui_runtime_events {
        mismatches.push(UiParityMismatch {
            field: "runtime_events",
            web: format!("{web_runtime_events:?}"),
            tui: format!("{tui_runtime_events:?}"),
        });
    }

    mismatches
}

fn parity_mismatch_is_covered_by_exception(
    web: &UiSnapshot,
    tui: &UiSnapshot,
    mismatch: &UiParityMismatch,
) -> bool {
    matches!(
        (
            mismatch.field,
            web.screen,
            tui.screen,
            web.focused_control,
            tui.focused_control
        ),
        (
            "focused_control",
            ScreenId::Settings,
            ScreenId::Settings,
            Some(ControlId::SettingsToggleThemeButton),
            _
        ) | (
            "focused_control",
            ScreenId::Settings,
            ScreenId::Settings,
            _,
            Some(ControlId::SettingsToggleThemeButton)
        )
    )
}

#[must_use]
pub fn uncovered_ui_parity_mismatches(web: &UiSnapshot, tui: &UiSnapshot) -> Vec<UiParityMismatch> {
    compare_ui_snapshots_for_parity(web, tui)
        .into_iter()
        .filter(|mismatch| !parity_mismatch_is_covered_by_exception(web, tui, mismatch))
        .collect()
}
