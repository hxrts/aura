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
    AuthorityPickerItem, ButtonVariant, ModalInputView, ModalView, PillTone, SelectableItem,
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
use aura_app::ui::types::{format_network_status_with_severity, AccessLevel, InvitationBridgeType};
use aura_app::ui::workflows::ceremonies as ceremony_workflows;
use aura_app::ui::workflows::moderator as moderator_workflows;
use aura_app::ui::workflows::{
    access as access_workflows, contacts as contacts_workflows, context as context_workflows,
    invitation as invitation_workflows, messaging as messaging_workflows,
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
            .find("SimpleModalSubmitAction::AcceptContactInvitation")
            .unwrap_or_else(|| panic!("missing AcceptContactInvitation branch"));
        let branch_end = source[branch_start..]
            .find("SimpleModalSubmitAction::CreateInvitation => {")
            .map(|offset| branch_start + offset)
            .unwrap_or_else(|| panic!("missing CreateInvitation branch"));
        let branch = &source[branch_start..branch_end];

        assert!(branch.contains("handoff::accept_imported_invitation("));
        assert!(!branch.contains("refresh_settings_from_runtime("));
        assert!(!branch.contains("load_contacts_runtime_view("));
        assert!(!branch.contains("ensure_runtime_peer_connectivity("));
        assert!(!branch.contains("converge_runtime(&runtime)"));
        assert!(!branch.contains("sleep_ms(&app_core, 250)"));
        assert!(!branch.contains("for _ in 0..8"));
    }

    #[test]
    fn contacts_channel_invites_use_authoritative_binding_and_typed_workflow() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let contacts_path = repo_root.join("crates/aura-ui/src/app/screens/contacts.rs");
        let contacts_source = std::fs::read_to_string(&contacts_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", contacts_path.display()));

        assert!(contacts_source.contains("selected_authoritative_channel("));
        assert!(contacts_source.contains("handoff::invite_authority_to_channel("));
        assert!(!contacts_source.contains("invite_user_to_channel("));
    }

    #[test]
    fn notifications_home_invites_use_pending_channel_acceptance_workflow() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let notifications_view_path =
            repo_root.join("crates/aura-ui/src/app/runtime_views/notifications.rs");
        let notifications_view_source = std::fs::read_to_string(&notifications_view_path)
            .unwrap_or_else(|error| {
                panic!(
                    "failed to read {}: {error}",
                    notifications_view_path.display()
                )
            });
        let notifications_actions_path =
            repo_root.join("crates/aura-ui/src/app/screens/notification_actions.rs");
        let notifications_actions_source = std::fs::read_to_string(&notifications_actions_path)
            .unwrap_or_else(|error| {
                panic!(
                    "failed to read {}: {error}",
                    notifications_actions_path.display()
                )
            });

        assert!(notifications_view_source.contains("PendingChannelInvitation"));
        assert!(
            notifications_actions_source.contains("handoff::accept_pending_channel_invitation(")
        );
    }

    #[test]
    fn semantic_lifecycle_source_keeps_must_use_drop_guardrails() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let lifecycle_path = repo_root.join("crates/aura-ui/src/semantic_lifecycle.rs");
        let lifecycle_source = std::fs::read_to_string(&lifecycle_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", lifecycle_path.display()));

        assert!(lifecycle_source.contains("#[must_use]\npub struct UiLocalOperationOwner"));
        assert!(lifecycle_source.contains("#[must_use]\npub struct UiWorkflowHandoffOwner"));
        assert!(lifecycle_source.contains("LocalTerminalSubmission<UiSubmittedOperationPublisher>"));
        assert!(
            lifecycle_source.contains("WorkflowHandoffSubmission<UiSubmittedOperationPublisher>")
        );
        assert!(lifecycle_source
            .contains("CeremonyMonitorHandoffSubmission<UiSubmittedOperationPublisher>"));
        assert!(!lifecycle_source.contains("SubmittedOperation<UiSubmittedOperationPublisher>"));
        assert!(lifecycle_source.contains("self.release.run_workflow("));
        assert!(lifecycle_source.contains("dropped_owner_error(kind)"));
    }

    #[test]
    fn invitation_modal_paths_use_handoff_owner_and_typed_terminal_settlement() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let submit_path = repo_root.join("crates/aura-ui/src/app/shell/modal_submit.rs");
        let source = std::fs::read_to_string(&submit_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", submit_path.display()));

        let accept_start = source
            .find("SimpleModalSubmitAction::AcceptContactInvitation")
            .unwrap_or_else(|| panic!("missing AcceptContactInvitation branch"));
        let create_start = source[accept_start..]
            .find("SimpleModalSubmitAction::CreateInvitation => {")
            .map(|offset| accept_start + offset)
            .unwrap_or_else(|| panic!("missing CreateInvitation branch"));
        let accept_branch = &source[accept_start..create_start];
        let create_branch = &source[create_start..];

        assert!(accept_branch.contains("UiWorkflowHandoffOwner::submit("));
        assert!(accept_branch.contains("transfer"));
        assert!(accept_branch.contains(".run_workflow("));
        assert!(accept_branch.contains("handoff::accept_imported_invitation("));
        assert!(!accept_branch.contains("complete_runtime_invitation_operation("));

        assert!(create_branch.contains("UiWorkflowHandoffOwner::submit("));
        assert!(create_branch.contains(".run_workflow("));
        assert!(create_branch.contains("handoff::create_contact_invitation("));
        assert!(!create_branch.contains("complete_runtime_modal_operation_success("));
    }

    #[test]
    fn notifications_invitation_actions_use_typed_handoff_workflows() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let notifications_path =
            repo_root.join("crates/aura-ui/src/app/screens/notification_actions.rs");
        let source = std::fs::read_to_string(&notifications_path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", notifications_path.display())
        });

        assert!(source.contains("handoff::decline_invitation_by_id("));
        assert!(source.contains("handoff::cancel_invitation_by_id("));
        assert!(source.contains("handoff::export_invitation_by_id("));
        assert!(source.contains("UiWorkflowHandoffOwner::submit("));
        assert!(source.contains(".run_workflow("));
    }

    #[test]
    fn shell_subscriptions_use_component_scoped_cancellable_owner() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let subscriptions_path = repo_root.join("crates/aura-ui/src/app/shell/subscriptions.rs");
        let source = std::fs::read_to_string(&subscriptions_path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", subscriptions_path.display())
        });

        assert!(source.contains("use_hook(crate::task_owner::new_ui_task_owner)"));
        assert!(source.contains("subscription_task_owner.spawn_local_cancellable(async move {"));
        assert!(!source.contains("spawn_ui(async move {"));
    }

    #[test]
    fn runtime_chat_send_uses_handoff_owner_and_typed_workflow() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let actions_path = repo_root.join("crates/aura-ui/src/app/shell/actions.rs");
        let source = std::fs::read_to_string(&actions_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", actions_path.display()));

        assert!(source.contains("UiWorkflowHandoffOwner::submit("));
        assert!(source.contains("UiOperationTransferScope::SendChatMessage"));
        assert!(source.contains("handoff::send_chat_message("));
        assert!(source.contains("SendChatTarget::ChannelName("));
    }

    #[test]
    fn runtime_slash_commands_use_shared_typed_execution_and_owner_metadata() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let actions_path = repo_root.join("crates/aura-ui/src/app/shell/actions.rs");
        let source = std::fs::read_to_string(&actions_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", actions_path.display()));

        assert!(source.contains("slash_command_workflows::prepare_and_execute("));
        assert!(source.contains("let report ="));
        assert!(source.contains(".and_then(|metadata| metadata.semantic_operation.clone())"));
        assert!(source.contains("UiLocalOperationOwner::submit("));
        assert!(!source.contains("slash_command_workflows::prepare("));
        assert!(!source.contains("slash_command_workflows::execute("));
        assert!(!source.contains("parse_chat_command(&raw)"));
    }

    #[test]
    fn web_harness_exact_handoff_paths_use_shared_transfer_run_workflow() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let commands_path = repo_root.join("crates/aura-web/src/harness/commands.rs");
        let source = std::fs::read_to_string(&commands_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", commands_path.display()));

        assert!(source.contains("begin_exact_handoff_operation("));
        assert!(source.contains(".run_workflow("));
        assert!(source.contains("spawn_handoff_workflow_task("));
        assert!(!source.contains("apply_handed_off_terminal_status("));
        assert!(!source.contains("catch_unwind().await"));
    }

    #[test]
    fn home_channel_modal_paths_use_local_terminal_owner() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let submit_path = repo_root.join("crates/aura-ui/src/app/shell/modal_submit.rs");
        let source = std::fs::read_to_string(&submit_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", submit_path.display()));

        assert!(source.contains("OperationId::create_home()"));
        assert!(source.contains("SemanticOperationKind::CreateHome"));
        assert!(source.contains("OperationId::create_channel()"));
        assert!(source.contains("SemanticOperationKind::CreateChannel"));
        assert!(source.contains("OperationId::set_channel_topic()"));
        assert!(source.contains("SemanticOperationKind::SetChannelTopic"));
        assert!(source.contains("UiLocalOperationOwner::submit("));
    }

    #[test]
    fn ceremony_modal_paths_use_ceremony_owner() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let submit_path = repo_root.join("crates/aura-ui/src/app/shell/modal_submit.rs");
        let source = std::fs::read_to_string(&submit_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", submit_path.display()));

        assert!(source.contains("UiCeremonySubmissionOwner::submit("));
        assert!(source.contains("OperationId::start_guardian_ceremony()"));
        assert!(source.contains("SemanticOperationKind::StartGuardianCeremony"));
        assert!(source.contains("OperationId::start_multifactor_ceremony()"));
        assert!(source.contains("SemanticOperationKind::StartMultifactorCeremony"));
        assert!(source.contains("OperationId::cancel_key_rotation_ceremony()"));
        assert!(source.contains("SemanticOperationKind::CancelKeyRotationCeremony"));
        assert!(source.contains("monitor_runtime_key_rotation_ceremony("));
    }

    #[test]
    fn settings_contacts_recovery_and_device_modal_paths_use_owned_submission() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let submit_path = repo_root.join("crates/aura-ui/src/app/shell/modal_submit.rs");
        let source = std::fs::read_to_string(&submit_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", submit_path.display()));

        assert!(source.contains("OperationId::update_nickname_suggestion()"));
        assert!(source.contains("SemanticOperationKind::UpdateNicknameSuggestion"));
        assert!(source.contains("OperationId::update_contact_nickname()"));
        assert!(source.contains("SemanticOperationKind::UpdateContactNickname"));
        assert!(source.contains("OperationId::remove_contact()"));
        assert!(source.contains("SemanticOperationKind::RemoveContact"));
        assert!(source.contains("OperationId::start_recovery()"));
        assert!(source.contains("SemanticOperationKind::StartRecovery"));
        assert!(source.contains("OperationId::grant_moderator()"));
        assert!(source.contains("SemanticOperationKind::GrantModerator"));
        assert!(source.contains("OperationId::revoke_moderator()"));
        assert!(source.contains("SemanticOperationKind::RevokeModerator"));
        assert!(source.contains("OperationId::device_enrollment()"));
        assert!(source.contains("SemanticOperationKind::StartDeviceEnrollment"));
        assert!(source.contains("UiLocalOperationOwner::submit("));
        assert!(source.contains("UiCeremonySubmissionOwner::submit("));
    }

    #[test]
    fn contacts_notifications_and_shortcuts_use_owned_runtime_actions() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let contacts_path = repo_root.join("crates/aura-ui/src/app/screens/contacts.rs");
        let contacts_source = std::fs::read_to_string(&contacts_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", contacts_path.display()));
        let notifications_path =
            repo_root.join("crates/aura-ui/src/app/screens/notification_actions.rs");
        let notifications_source =
            std::fs::read_to_string(&notifications_path).unwrap_or_else(|error| {
                panic!("failed to read {}: {error}", notifications_path.display())
            });
        let actions_path = repo_root.join("crates/aura-ui/src/app/shell/actions.rs");
        let actions_source = std::fs::read_to_string(&actions_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", actions_path.display()));

        assert!(contacts_source.contains("OperationId::start_direct_chat()"));
        assert!(contacts_source.contains("SemanticOperationKind::StartDirectChat"));
        assert!(contacts_source.contains("UiLocalOperationOwner::submit("));

        assert!(notifications_source.contains("OperationId::submit_guardian_approval()"));
        assert!(notifications_source.contains("SemanticOperationKind::SubmitGuardianApproval"));
        assert!(notifications_source.contains("UiLocalOperationOwner::submit("));

        assert!(actions_source.contains("OperationId::create_neighborhood()"));
        assert!(actions_source.contains("SemanticOperationKind::CreateNeighborhood"));
        assert!(actions_source.contains("OperationId::add_home_to_neighborhood()"));
        assert!(actions_source.contains("SemanticOperationKind::AddHomeToNeighborhood"));
        assert!(actions_source.contains("OperationId::link_home_one_hop_link()"));
        assert!(actions_source.contains("SemanticOperationKind::LinkHomeOneHopLink"));
        assert!(actions_source.contains("UiLocalOperationOwner::submit("));
    }

    #[test]
    fn device_removal_and_move_position_use_owned_submission() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let submit_path = repo_root.join("crates/aura-ui/src/app/shell/modal_submit.rs");
        let submit_source = std::fs::read_to_string(&submit_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", submit_path.display()));
        let neighborhood_path = repo_root.join("crates/aura-ui/src/app/screens/neighborhood.rs");
        let neighborhood_source =
            std::fs::read_to_string(&neighborhood_path).unwrap_or_else(|error| {
                panic!("failed to read {}: {error}", neighborhood_path.display())
            });

        assert!(submit_source.contains("OperationId::remove_device()"));
        assert!(submit_source.contains("SemanticOperationKind::RemoveDevice"));
        assert!(submit_source.contains("UiCeremonySubmissionOwner::submit("));

        assert!(neighborhood_source.contains("OperationId::move_position()"));
        assert!(neighborhood_source.contains("SemanticOperationKind::MovePosition"));
        assert!(neighborhood_source.contains("UiLocalOperationOwner::submit("));
    }

    #[test]
    fn chat_and_contacts_expose_retry_close_and_add_guardian_paths() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let chat_path = repo_root.join("crates/aura-ui/src/app/screens/chat.rs");
        let chat_source = std::fs::read_to_string(&chat_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", chat_path.display()));
        let contacts_path = repo_root.join("crates/aura-ui/src/app/screens/contacts.rs");
        let contacts_source = std::fs::read_to_string(&contacts_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", contacts_path.display()));

        assert!(chat_source.contains("ControlId::ChatCloseChannelButton"));
        assert!(chat_source.contains("OperationId::close_channel()"));
        assert!(chat_source.contains("SemanticOperationKind::CloseChannel"));
        assert!(chat_source.contains("ControlId::ChatRetryMessageButton"));
        assert!(chat_source.contains("OperationId::retry_message()"));
        assert!(chat_source.contains("SemanticOperationKind::RetryChatMessage"));
        assert!(chat_source.contains("handoff::retry_chat_message("));

        assert!(contacts_source.contains("ControlId::ContactsAddGuardianButton"));
        assert!(contacts_source.contains("SemanticOperationKind::CreateGuardianInvitation"));
        assert!(contacts_source.contains("handoff::create_guardian_invitation("));
    }

    #[test]
    fn guardian_cancel_path_uses_typed_ceremony_owner() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let submit_path = repo_root.join("crates/aura-ui/src/app/shell/modal_submit.rs");
        let submit_source = std::fs::read_to_string(&submit_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", submit_path.display()));
        let runtime_events_path = repo_root.join("crates/aura-ui/src/model/runtime_events.rs");
        let runtime_events_source =
            std::fs::read_to_string(&runtime_events_path).unwrap_or_else(|error| {
                panic!("failed to read {}: {error}", runtime_events_path.display())
            });

        assert!(submit_source.contains("OperationId::cancel_guardian_ceremony()"));
        assert!(submit_source.contains("SemanticOperationKind::CancelGuardianCeremony"));
        assert!(submit_source.contains("cancel_key_rotation_ceremony_by_id("));
        assert!(submit_source.contains("UiCeremonySubmissionOwner::submit("));
        assert!(runtime_events_source.contains("set_runtime_guardian_ceremony_id("));
        assert!(runtime_events_source.contains("clear_runtime_guardian_ceremony_id("));
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
    fn runtime_semantic_snapshot_marks_active_home_row_selected_when_falling_back_to_runtime_home(
    ) -> Result<(), &'static str> {
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
            .map_err(|_| "runtime snapshot should export matching home selection")?;
        let homes = snapshot
            .lists
            .iter()
            .find(|list| list.id == ListId::Homes)
            .ok_or("homes list should be exported")?;
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
        Ok(())
    }

    #[test]
    fn runtime_semantic_snapshot_ignores_stale_local_home_selection_without_matching_runtime_row(
    ) -> Result<(), &'static str> {
        let mut model = UiModel::new("authority-test".to_string());
        model.select_home("home-sam's-home", "Sam's Home");
        let neighborhood_runtime = NeighborhoodRuntimeView {
            loaded: true,
            active_home_name: "Shared Home".to_string(),
            active_home_id: "channel:home-1".to_string(),
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
            .map_err(|_| "stale local home selection must not violate list-selection invariants")?;
        let homes = snapshot
            .lists
            .iter()
            .find(|list| list.id == ListId::Homes)
            .ok_or("homes list should be exported")?;
        assert!(
            homes
                .items
                .iter()
                .any(|item| item.id == "channel:home-1" && item.selected),
            "runtime active home should become the selected exported row when local selection is stale"
        );
        assert_eq!(
            snapshot.selected_item_id(ListId::Homes),
            Some("channel:home-1")
        );
        Ok(())
    }

    #[test]
    fn runtime_semantic_snapshot_keeps_current_device_distinct_from_selection(
    ) -> Result<(), &'static str> {
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
            .map_err(|_| "current device marker must not fabricate list selection")?;
        let devices = snapshot
            .lists
            .iter()
            .find(|list| list.id == ListId::Devices)
            .ok_or("devices list should be exported")?;
        assert_eq!(snapshot.selected_item_id(ListId::Devices), None);
        assert_eq!(devices.items.len(), 1);
        assert!(devices.items[0].is_current);
        assert!(!devices.items[0].selected);
        Ok(())
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

    #[test]
    fn neighborhood_screen_uses_automatic_full_access_and_no_enter_as_control() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let neighborhood_path = repo_root.join("crates/aura-ui/src/app/screens/neighborhood.rs");
        let source = std::fs::read_to_string(&neighborhood_path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", neighborhood_path.display())
        });

        assert!(source.contains("let depth = AccessDepth::Full;"));
        assert!(!source.contains("NeighborhoodEnterAsButton"));
        assert!(!source.contains("Enter As:"));
        assert!(!source.contains("send_action_keys(\"d\")"));
    }
}
