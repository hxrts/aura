//! Dioxus-based web UI application root and screen components.
//!
//! Provides the main application shell, screen routing, keyboard handling,
//! and toast notifications for the Aura web interface.

mod modal;
mod runtime_views;
mod screens;
mod shell;
mod snapshot;

use crate::components::{
    AuthorityPickerItem, ButtonVariant, ModalInputView, ModalView, PillTone,
    UiAuthorityPickerModal, UiButton, UiCard, UiCardBody, UiCardFooter, UiDeviceEnrollmentModal,
    UiFooter, UiListButton, UiListItem, UiModal, UiPill,
};
use crate::dom_ids::RequiredDomId;
use crate::model::{
    AccessDepth, AccessOverrideLevel, ActiveModal, AddDeviceWizardStep, CapabilityTier,
    CreateChannelDetailsField, CreateChannelWizardStep, ModalState, NeighborhoodMemberSelectionKey,
    NeighborhoodMode, ScreenId, SettingsSection, ThresholdWizardStep, UiController, UiModel,
    DEFAULT_CAPABILITY_FULL, DEFAULT_CAPABILITY_LIMITED, DEFAULT_CAPABILITY_PARTIAL,
};
use crate::readiness_owner;
use crate::task_owner::spawn_ui;
use aura_app::ui::contract::{
    list_item_dom_id, ConfirmationState, ControlId, FieldId, ListId, ListItemSnapshot,
    ListSnapshot, MessageSnapshot, ModalId, OperationId, OperationInstanceId, OperationSnapshot,
    OperationState, SelectionSnapshot, UiSnapshot,
};
use aura_app::ui::signals::{
    NetworkStatus, AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL, CHAT_SIGNAL, CONTACTS_SIGNAL,
    DISCOVERED_PEERS_SIGNAL, ERROR_SIGNAL, HOMES_SIGNAL, INVITATIONS_SIGNAL, NEIGHBORHOOD_SIGNAL,
    NETWORK_STATUS_SIGNAL, RECOVERY_SIGNAL, SETTINGS_SIGNAL, TRANSPORT_PEERS_SIGNAL,
};
use aura_app::ui::types::{
    all_command_help, command_help, format_network_status_with_severity, parse_chat_command,
    AccessLevel, ChatCommand, InvitationBridgeType,
};
use aura_app::ui::workflows::ceremonies as ceremony_workflows;
use aura_app::ui::workflows::moderation as moderation_workflows;
use aura_app::ui::workflows::moderator as moderator_workflows;
use aura_app::ui::workflows::{
    access as access_workflows, contacts as contacts_workflows, context as context_workflows,
    invitation as invitation_workflows, messaging as messaging_workflows, query as query_workflows,
    recovery as recovery_workflows, settings as settings_workflows, time as time_workflows,
};
use aura_app::ui_contract::{bridged_operation_statuses, ChannelFactKey, RuntimeFact};
use aura_app::views::chat::NOTE_TO_SELF_CHANNEL_NAME;
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::hash::hash;
use aura_core::types::identifiers::{AuthorityId, CeremonyId};
use dioxus::dioxus_core::schedule_update;
use dioxus::events::KeyboardData;
use dioxus::prelude::*;
use dioxus_shadcn::components::empty::{Empty, EmptyDescription, EmptyHeader, EmptyTitle};
use dioxus_shadcn::components::scroll_area::{ScrollArea, ScrollAreaViewport};
use dioxus_shadcn::components::toast::{use_toast, ToastOptions, ToastPosition, ToastProvider};
use dioxus_shadcn::theme::{themes, use_theme, ColorScheme, ThemeProvider};
use runtime_views::*;
use screens::{
    nav_button_id, nav_tab_class, nav_tabs, neighborhood_member_selection_key,
    render_screen_content,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use modal::{active_modal_title, modal_accepts_text, modal_view};
pub use shell::rendering::AuraUiRoot;
use snapshot::runtime_semantic_snapshot;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::UiModel;
    use aura_app::ui::contract::UiReadiness;
    use std::path::Path;

    #[test]
    fn runtime_projection_loaders_do_not_synthesize_authoritative_readiness() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let contacts_path = repo_root.join("crates/aura-ui/src/app/runtime_views/contacts.rs");
        let contacts_branch = std::fs::read_to_string(&contacts_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", contacts_path.display()));
        let notifications_path =
            repo_root.join("crates/aura-ui/src/app/runtime_views/notifications.rs");
        let notifications_branch =
            std::fs::read_to_string(&notifications_path).unwrap_or_else(|error| {
                panic!("failed to read {}: {error}", notifications_path.display())
            });

        assert!(!contacts_branch.contains("RuntimeFact::ContactLinkReady"));
        assert!(!notifications_branch.contains("RuntimeFact::PendingHomeInvitationReady"));
    }

    #[test]
    fn add_device_ceremony_monitor_uses_upstream_bounded_monitor() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let submit_path = repo_root.join("crates/aura-ui/src/app/shell/modal_submit.rs");
        let source = std::fs::read_to_string(&submit_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", submit_path.display()));

        let helper_start = source
            .find("fn monitor_runtime_device_enrollment_ceremony")
            .unwrap_or_else(|| panic!("missing monitor_runtime_device_enrollment_ceremony"));
        let helper_end = source[helper_start..]
            .find("fn removable_device_for_modal")
            .map(|offset| helper_start + offset)
            .unwrap_or_else(|| panic!("missing removable_device_for_modal"));
        let helper = &source[helper_start..helper_end];

        assert!(helper.contains("monitor_key_rotation_ceremony_with_policy"));
        assert!(helper.contains("CeremonyLifecycleState::TimedOut"));
        assert!(!helper.contains("sleep_ms(&app_core_for_status, 1_000)"));
        assert!(!helper.contains("loop {"));
    }

    #[test]
    fn import_device_enrollment_uses_upstream_acceptance_without_local_prewarm_loop() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let submit_path = repo_root.join("crates/aura-ui/src/app/shell/modal_submit.rs");
        let source = std::fs::read_to_string(&submit_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", submit_path.display()));

        let branch_start = source
            .find("SimpleModalSubmitAction::ImportDeviceEnrollmentCode => {")
            .unwrap_or_else(|| panic!("missing ImportDeviceEnrollmentCode branch"));
        let branch_end = source[branch_start..]
            .find("SimpleModalSubmitAction::CreateInvitation => {")
            .map(|offset| branch_start + offset)
            .unwrap_or_else(|| panic!("missing CreateInvitation branch"));
        let branch = &source[branch_start..branch_end];

        assert!(branch.contains("accept_device_enrollment_invitation("));
        assert!(!branch.contains("ensure_runtime_peer_connectivity("));
        assert!(!branch.contains("converge_runtime(&runtime)"));
        assert!(!branch.contains("sleep_ms(&app_core, 250)"));
        assert!(!branch.contains("for _ in 0..8"));
    }

    #[test]
    fn add_device_confirm_refresh_uses_typed_ceremony_status_handle() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let submit_path = repo_root.join("crates/aura-ui/src/app/shell/modal_submit.rs");
        let source = std::fs::read_to_string(&submit_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", submit_path.display()));

        let branch_start = source
            .find("WizardModalSubmitAction::RefreshAddDeviceStatus => {")
            .unwrap_or_else(|| panic!("missing RefreshAddDeviceStatus branch"));
        let branch_end = source[branch_start..]
            .find("WizardModalSubmitAction::CreateChannel => {")
            .map(|offset| branch_start + offset)
            .unwrap_or_else(|| panic!("missing CreateChannel wizard branch"));
        let branch = &source[branch_start..branch_end];

        assert!(branch.contains("runtime_device_enrollment_status_handle()"));
        assert!(branch.contains("get_key_rotation_ceremony_status("));
        assert!(branch.contains("update_runtime_device_enrollment_status("));
        assert!(!branch.contains("sleep_ms(&app_core"));
        assert!(!branch.contains("loop {"));
    }

    #[test]
    fn add_device_confirm_display_is_driven_by_typed_status_fields() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let modal_path = repo_root.join("crates/aura-ui/src/app/modal.rs");
        let source = std::fs::read_to_string(&modal_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", modal_path.display()));

        let branch_start = source
            .find("AddDeviceWizardStep::Confirm => {")
            .unwrap_or_else(|| panic!("missing AddDeviceWizardStep::Confirm details branch"));
        let branch_end = source[branch_start..]
            .find("        ModalState::ImportDeviceEnrollmentCode => {")
            .map(|offset| branch_start + offset)
            .unwrap_or_else(|| panic!("missing ImportDeviceEnrollmentCode details branch"));
        let branch = &source[branch_start..branch_end];

        assert!(branch.contains("state.accepted_count"));
        assert!(branch.contains("state.total_count.max(1)"));
        assert!(branch.contains("state.threshold.max(1)"));
        assert!(branch.contains("if let Some(error) = &state.error_message"));
        assert!(branch.contains("else if state.has_failed"));
        assert!(branch.contains("else if state.is_complete"));
        assert!(!branch.contains("time_workflows::sleep_ms"));
    }

    #[test]
    fn chat_selection_paths_use_canonical_channel_ids_instead_of_name_selection() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let keyboard_path = repo_root.join("crates/aura-ui/src/keyboard/chat.rs");
        let keyboard_source = std::fs::read_to_string(&keyboard_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", keyboard_path.display()));

        let join_start = keyboard_source
            .find("aura_app::ui::types::ChatCommand::Join { channel } => {")
            .unwrap_or_else(|| panic!("missing ChatCommand::Join branch"));
        let join_end = keyboard_source[join_start..]
            .find("aura_app::ui::types::ChatCommand::Leave => {")
            .map(|offset| join_start + offset)
            .unwrap_or_else(|| panic!("missing ChatCommand::Leave branch"));
        let join_branch = &keyboard_source[join_start..join_end];

        assert!(join_branch.contains("let channel_id = ensure_named_channel("));
        assert!(join_branch.contains("model.select_channel_id(Some(&channel_id));"));
        assert!(!join_branch.contains("select_channel_by_name("));

        let neighborhood_path = repo_root.join("crates/aura-ui/src/app/screens/neighborhood.rs");
        let neighborhood_source =
            std::fs::read_to_string(&neighborhood_path).unwrap_or_else(|error| {
                panic!("failed to read {}: {error}", neighborhood_path.display())
            });

        let channels_start = neighborhood_source
            .find(
                "for (channel_id, channel_name, channel_topic, is_selected) in &display_channels {",
            )
            .unwrap_or_else(|| panic!("missing display_channels loop"));
        let channels_end = neighborhood_source[channels_start..]
            .find("UiListItem {")
            .map(|offset| channels_start + offset)
            .unwrap_or_else(|| panic!("missing channel list item"));
        let channels_branch = &neighborhood_source[channels_start..channels_end];

        assert!(channels_branch.contains("controller.select_channel_by_id(&channel_id);"));
        assert!(!channels_branch.contains("select_channel_by_name("));
    }

    #[test]
    fn runtime_semantic_snapshot_marks_active_home_row_selected_when_falling_back_to_runtime_home()
    {
        let model = UiModel::new("authority-test".to_string());
        let neighborhood_runtime = NeighborhoodRuntimeView {
            loaded: true,
            active_home_name: "Shared Home".to_string(),
            homes: vec![NeighborhoodRuntimeHome {
                id: "channel:home-1".to_string(),
                name: "Shared Home".to_string(),
                member_count: Some(1),
                can_enter: true,
                is_local: true,
            }],
            ..NeighborhoodRuntimeView::default()
        };

        let snapshot = runtime_semantic_snapshot(
            &model,
            &neighborhood_runtime,
            &ChatRuntimeView::default(),
            &ContactsRuntimeView::default(),
            &SettingsRuntimeView::default(),
            &NotificationsRuntimeView::default(),
        );

        snapshot
            .validate_invariants()
            .expect("runtime snapshot should export matching home selection");
        let homes = snapshot
            .lists
            .iter()
            .find(|list| list.id == ListId::Homes)
            .expect("homes list should be exported");
        assert!(
            homes
                .items
                .iter()
                .any(|item| item.id == "channel:home-1" && item.selected),
            "active runtime home fallback must mark the matching row selected"
        );
        assert_eq!(
            snapshot.selected_item_id(ListId::Homes),
            Some("channel:home-1")
        );
    }

    #[test]
    fn runtime_semantic_snapshot_keeps_current_device_distinct_from_selection() {
        let model = UiModel::new("authority-test".to_string());
        let settings_runtime = SettingsRuntimeView {
            loaded: true,
            devices: vec![SettingsRuntimeDevice {
                id: "device-1".to_string(),
                name: "Current Device".to_string(),
                is_current: true,
            }],
            ..SettingsRuntimeView::default()
        };

        let snapshot = runtime_semantic_snapshot(
            &model,
            &NeighborhoodRuntimeView::default(),
            &ChatRuntimeView::default(),
            &ContactsRuntimeView::default(),
            &settings_runtime,
            &NotificationsRuntimeView::default(),
        );

        snapshot
            .validate_invariants()
            .expect("current device marker must not fabricate list selection");
        let devices = snapshot
            .lists
            .iter()
            .find(|list| list.id == ListId::Devices)
            .expect("devices list should be exported");
        assert_eq!(snapshot.selected_item_id(ListId::Devices), None);
        assert_eq!(devices.items.len(), 1);
        assert!(devices.items[0].is_current);
        assert!(!devices.items[0].selected);
    }

    #[test]
    fn runtime_semantic_snapshot_keeps_settings_loading_until_authoritative_projection_arrives() {
        let mut model = UiModel::new("authority-test".to_string());
        model.account_ready = true;
        model.set_screen(ScreenId::Settings);

        let loading_snapshot = runtime_semantic_snapshot(
            &model,
            &NeighborhoodRuntimeView::default(),
            &ChatRuntimeView::default(),
            &ContactsRuntimeView::default(),
            &SettingsRuntimeView::default(),
            &NotificationsRuntimeView::default(),
        );
        assert_eq!(loading_snapshot.readiness, UiReadiness::Loading);

        let ready_snapshot = runtime_semantic_snapshot(
            &model,
            &NeighborhoodRuntimeView::default(),
            &ChatRuntimeView::default(),
            &ContactsRuntimeView::default(),
            &SettingsRuntimeView {
                loaded: true,
                authority_id: "authority-test".to_string(),
                devices: vec![SettingsRuntimeDevice {
                    id: "device-1".to_string(),
                    name: "Current Device".to_string(),
                    is_current: true,
                }],
                authorities: vec![SettingsRuntimeAuthority {
                    id: AuthorityId::new_from_entropy([7_u8; 32]),
                    label: "Authority".to_string(),
                    is_current: true,
                }],
                ..SettingsRuntimeView::default()
            },
            &NotificationsRuntimeView::default(),
        );
        assert_eq!(ready_snapshot.readiness, UiReadiness::Ready);
    }

    #[test]
    fn runtime_semantic_snapshot_keeps_neighborhood_loading_until_home_projection_arrives() {
        let mut model = UiModel::new("authority-test".to_string());
        model.account_ready = true;
        model.set_screen(ScreenId::Neighborhood);

        let loading_snapshot = runtime_semantic_snapshot(
            &model,
            &NeighborhoodRuntimeView {
                loaded: true,
                ..NeighborhoodRuntimeView::default()
            },
            &ChatRuntimeView::default(),
            &ContactsRuntimeView::default(),
            &SettingsRuntimeView::default(),
            &NotificationsRuntimeView::default(),
        );
        assert_eq!(loading_snapshot.readiness, UiReadiness::Loading);

        let ready_snapshot = runtime_semantic_snapshot(
            &model,
            &NeighborhoodRuntimeView {
                loaded: true,
                active_home_id: "home-1".to_string(),
                ..NeighborhoodRuntimeView::default()
            },
            &ChatRuntimeView::default(),
            &ContactsRuntimeView::default(),
            &SettingsRuntimeView::default(),
            &NotificationsRuntimeView::default(),
        );
        assert_eq!(ready_snapshot.readiness, UiReadiness::Ready);
    }
}
