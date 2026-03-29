use super::*;
use crate::tui::channel_selection::{
    authoritative_committed_selection, CommittedChannelSelection, SharedCommittedChannelSelection,
};

pub(super) fn resolve_committed_selected_channel_id(
    state: &TuiState,
    shared_channels: &[Channel],
) -> Option<CommittedChannelSelection> {
    shared_channels
        .get(state.chat.selected_channel)
        .map(authoritative_committed_selection)
}

pub(super) fn handle_channel_selection_change(
    current: &TuiState,
    new_state: &TuiState,
    shared_channels: &Arc<parking_lot::RwLock<Vec<Channel>>>,
    selected_channel_id: &SharedCommittedChannelSelection,
) {
    let idx = new_state.chat.selected_channel;

    let channels = shared_channels.read().clone();
    let next_selected = channels.get(idx).map(authoritative_committed_selection);
    let current_selected = selected_channel_id.read().clone();

    if new_state.chat.selected_channel == current.chat.selected_channel
        && next_selected == current_selected
    {
        return;
    }

    {
        let mut guard = selected_channel_id.write();
        *guard = next_selected;
    }
}

#[cfg(test)]
mod tests {
    use super::{handle_channel_selection_change, resolve_committed_selected_channel_id};
    use crate::tui::channel_selection::CommittedChannelSelection;
    use crate::tui::state::TuiState;
    use crate::tui::types::Channel;
    use std::path::Path;
    use std::sync::Arc;

    fn read_repo_source(relative_path: &str) -> String {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let source_path = repo_root.join(relative_path);
        std::fs::read_to_string(&source_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", source_path.display()))
    }

    #[test]
    fn committed_channel_resolution_requires_authoritative_selection() {
        let mut state = TuiState::new();
        let channels = vec![
            Channel::new("channel-1", "General"),
            Channel::new("channel-2", "Ops"),
        ];

        state.chat.selected_channel = 1;
        assert_eq!(
            resolve_committed_selected_channel_id(&state, &channels),
            Some(CommittedChannelSelection::new("channel-2"))
        );

        state.chat.selected_channel = 4;
        assert_eq!(
            resolve_committed_selected_channel_id(&state, &channels),
            None
        );
    }

    #[test]
    fn selection_change_drops_non_authoritative_preserved_context() {
        let mut current = TuiState::new();
        current.chat.selected_channel = 3;

        let mut next = TuiState::new();
        next.chat.selected_channel = 0;

        let channels = Arc::new(parking_lot::RwLock::new(vec![Channel::new(
            "channel-1",
            "General",
        )]));
        let selected_channel_id = Arc::new(parking_lot::RwLock::new(Some(
            CommittedChannelSelection::from_binding(
                &aura_app::ui_contract::ChannelBindingWitness::new(
                    "channel-1",
                    Some("ctx-123".to_string()),
                ),
            ),
        )));

        handle_channel_selection_change(&current, &next, &channels, &selected_channel_id);

        assert_eq!(
            *selected_channel_id.read(),
            Some(CommittedChannelSelection::from_binding(
                &aura_app::ui_contract::ChannelBindingWitness::new("channel-1", None)
            ))
        );
    }

    #[test]
    fn send_dispatch_does_not_background_retry_selection() {
        let shell_source = read_repo_source(
            "crates/aura-terminal/src/tui/screens/app/shell/dispatch_command_handlers.rs",
        );
        let send_start = shell_source
            .find("DispatchCommand::SendChatMessage")
            .unwrap_or_else(|| panic!("missing SendChatMessage dispatch arm"));
        let retry_start = shell_source[send_start..]
            .find("DispatchCommand::RetryMessage")
            .map(|offset| send_start + offset)
            .unwrap_or_else(|| panic!("missing RetryMessage dispatch arm"));
        let send_branch = &shell_source[send_start..retry_start];

        assert!(!send_branch.contains("sending shortly"));
        assert!(!send_branch.contains("tokio::time::sleep"));
        assert!(!send_branch.contains("selected_channel_id_for_dispatch.read()"));
        assert!(!send_branch.contains("visible_message_channel_id"));
        assert!(send_branch.contains("No committed channel selected"));
    }

    #[test]
    fn start_chat_dispatch_does_not_optimistically_navigate() {
        let shell_source = read_repo_source(
            "crates/aura-terminal/src/tui/screens/app/shell/dispatch_command_handlers.rs",
        );
        let start_chat = shell_source
            .find("DispatchCommand::StartChat")
            .unwrap_or_else(|| panic!("missing StartChat dispatch arm"));
        let next_arm = shell_source[start_chat..]
            .find("DispatchCommand::InviteSelectedContactToChannel")
            .map(|offset| start_chat + offset)
            .unwrap_or_else(|| panic!("missing InviteSelectedContactToChannel dispatch arm"));
        let start_chat_branch = &shell_source[start_chat..next_arm];

        assert!(!start_chat_branch.contains("router.go_to(Screen::Chat)"));
    }

    #[test]
    fn invitation_dispatch_uses_product_callbacks_without_harness_shortcuts() {
        let shell_source = read_repo_source(
            "crates/aura-terminal/src/tui/screens/app/shell/dispatch_command_handlers.rs",
        );

        let create_start = shell_source
            .find("DispatchCommand::CreateInvitation")
            .unwrap_or_else(|| panic!("missing CreateInvitation dispatch arm"));
        let import_start = shell_source[create_start..]
            .find("DispatchCommand::ImportInvitation")
            .map(|offset| create_start + offset)
            .unwrap_or_else(|| panic!("missing ImportInvitation dispatch arm"));
        let export_start = shell_source[import_start..]
            .find("DispatchCommand::ExportInvitation")
            .map(|offset| import_start + offset)
            .unwrap_or_else(|| panic!("missing ExportInvitation dispatch arm"));
        let create_branch = &shell_source[create_start..import_start];
        let import_branch = &shell_source[import_start..export_start];

        assert!(!create_branch.contains("AURA_HARNESS_MODE"));
        assert!(!import_branch.contains("AURA_HARNESS_MODE"));
        assert!(!create_branch.contains("runtime.create_contact_invitation"));
        assert!(!create_branch.contains("runtime.export_invitation"));
        assert!(!import_branch.contains("runtime.import_invitation"));
        assert!(!import_branch.contains("runtime.accept_invitation"));
    }

    #[test]
    fn join_and_accept_callbacks_consume_binding_witnesses() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let chat_callbacks_path =
            repo_root.join("crates/aura-terminal/src/tui/callbacks/factories/chat.rs");
        let source = std::fs::read_to_string(&chat_callbacks_path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", chat_callbacks_path.display())
        });

        assert!(source.contains("join_channel_by_name_with_binding_terminal_status"));
        assert!(source.contains("accept_pending_channel_invitation_with_binding_terminal_status"));
        assert!(source.contains("UiUpdate::ChannelSelected(binding)"));
        assert!(source.contains("UiUpdate::ChannelSelected(accepted.binding)"));
    }

    #[test]
    fn slash_join_does_not_repair_selection_by_channel_name() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let chat_callbacks_path =
            repo_root.join("crates/aura-terminal/src/tui/callbacks/factories/chat.rs");
        let source = std::fs::read_to_string(&chat_callbacks_path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", chat_callbacks_path.display())
        });
        let join_start = source
            .find("fn make_join_channel(")
            .unwrap_or_else(|| panic!("missing join-channel callback factory"));
        let join_end = source[join_start..]
            .find("fn make_list_participants(")
            .map(|offset| join_start + offset)
            .unwrap_or_else(|| panic!("missing list-participants callback factory"));
        let join_branch = &source[join_start..join_end];

        assert!(join_branch.contains("join_channel_by_name_with_binding_terminal_status"));
        assert!(join_branch.contains("UiUpdate::ChannelSelected(binding)"));
        assert!(!join_branch.contains("joined_channel_name"));
        assert!(!join_branch.contains("get_chat_state("));
        assert!(!join_branch.contains("candidate.name.eq_ignore_ascii_case"));
    }

    #[test]
    fn invite_to_channel_dispatch_clears_readiness_without_local_lifecycle_authorship() {
        let shell_source = read_repo_source(
            "crates/aura-terminal/src/tui/screens/app/shell/dispatch_command_handlers.rs",
        );

        let invite_start = shell_source
            .find("DispatchCommand::InviteActorToChannel {")
            .unwrap_or_else(|| panic!("missing InviteActorToChannel dispatch arm"));
        let next_arm = shell_source[invite_start..]
            .find("DispatchCommand::RemoveContact")
            .map(|offset| invite_start + offset)
            .unwrap_or_else(|| panic!("missing RemoveContact dispatch arm"));
        let invite_branch = &shell_source[invite_start..next_arm];

        assert!(invite_branch.contains("RuntimeEventKind::PendingHomeInvitationReady"));
        assert!(invite_branch.contains("(cb.contacts.on_invite_to_channel)("));
    }

    #[test]
    fn device_enrollment_completion_refresh_does_not_sleep() {
        let shell_source =
            read_repo_source("crates/aura-terminal/src/tui/screens/app/shell/update_handlers.rs");

        let status_start = shell_source
            .find("UiUpdate::KeyRotationCeremonyStatus {")
            .unwrap_or_else(|| panic!("missing KeyRotationCeremonyStatus update arm"));
        let next_arm = shell_source[status_start..]
            .find("UiUpdate::OperationFailed")
            .map(|offset| status_start + offset)
            .unwrap_or_else(|| panic!("missing OperationFailed update arm"));
        let status_branch = &shell_source[status_start..next_arm];

        assert!(status_branch.contains("refresh_settings_from_runtime(&app_core).await"));
        assert!(!status_branch.contains("tokio::time::sleep"));
        assert!(!status_branch.contains("Small delay to allow commitment tree update to propagate"));
    }

    #[test]
    fn ceremony_monitors_use_typed_lifecycle_outcomes() {
        let shell_source = read_repo_source(
            "crates/aura-terminal/src/tui/screens/app/shell/dispatch_command_handlers.rs",
        );
        let helper_source = read_repo_source("crates/aura-terminal/src/tui/key_rotation.rs");

        let guardian_start = shell_source
            .find("DispatchCommand::StartGuardianCeremony")
            .unwrap_or_else(|| panic!("missing StartGuardianCeremony dispatch arm"));
        let mfa_start = shell_source[guardian_start..]
            .find("DispatchCommand::StartMfaCeremony")
            .map(|offset| guardian_start + offset)
            .unwrap_or_else(|| panic!("missing StartMfaCeremony dispatch arm"));
        let cancel_start = shell_source[mfa_start..]
            .find("DispatchCommand::CancelGuardianCeremony")
            .map(|offset| mfa_start + offset)
            .unwrap_or_else(|| panic!("missing CancelGuardianCeremony dispatch arm"));
        let guardian_branch = &shell_source[guardian_start..mfa_start];
        let mfa_branch = &shell_source[mfa_start..cancel_start];

        assert!(guardian_branch.contains("monitor_key_rotation_ceremony_with_policy("));
        assert!(mfa_branch.contains("monitor_key_rotation_ceremony_with_policy("));
        assert!(!guardian_branch.contains("monitor_key_rotation_ceremony("));
        assert!(!mfa_branch.contains("monitor_key_rotation_ceremony("));
        assert!(guardian_branch.contains("CeremonyLifecycleState::TimedOut"));
        assert!(mfa_branch.contains("CeremonyLifecycleState::TimedOut"));
        assert!(guardian_branch.contains("key_rotation_lifecycle_toast("));
        assert!(mfa_branch.contains("key_rotation_lifecycle_toast("));
        assert!(shell_source.contains("use crate::tui::key_rotation::{"));
        assert!(helper_source.contains("CeremonyLifecycleState::FailedRollbackIncomplete"));
        assert!(
            helper_source.contains("rollback was incomplete; manual intervention may be required")
        );
    }

    #[test]
    fn ceremony_monitors_use_required_publication() {
        let shell_source = read_repo_source(
            "crates/aura-terminal/src/tui/screens/app/shell/dispatch_command_handlers.rs",
        );

        let guardian_start = shell_source
            .find("DispatchCommand::StartGuardianCeremony")
            .unwrap_or_else(|| panic!("missing StartGuardianCeremony dispatch arm"));
        let cancel_start = shell_source[guardian_start..]
            .find("DispatchCommand::CancelGuardianCeremony")
            .map(|offset| guardian_start + offset)
            .unwrap_or_else(|| panic!("missing CancelGuardianCeremony dispatch arm"));
        let guardian_branch = &shell_source[guardian_start..cancel_start];

        assert!(guardian_branch.contains("send_optional_ui_update_required("));
        assert!(guardian_branch.contains("spawn_ui_update("));
        assert!(guardian_branch.contains("UiUpdatePublication::RequiredUnordered"));
        assert!(!guardian_branch.contains("try_send("));
    }

    #[test]
    fn ceremony_dispatch_paths_use_ceremony_submission_owner() {
        let shell_source = read_repo_source(
            "crates/aura-terminal/src/tui/screens/app/shell/dispatch_command_handlers.rs",
        );
        let dispatch_source =
            read_repo_source("crates/aura-terminal/src/tui/screens/app/shell/dispatch.rs");

        assert!(dispatch_source.contains("submit_ceremony_operation("));
        assert!(shell_source.contains("OperationId::start_guardian_ceremony()"));
        assert!(shell_source.contains("SemanticOperationKind::StartGuardianCeremony"));
        assert!(shell_source.contains("OperationId::start_multifactor_ceremony()"));
        assert!(shell_source.contains("SemanticOperationKind::StartMultifactorCeremony"));
        assert!(shell_source.contains("OperationId::cancel_guardian_ceremony()"));
        assert!(shell_source.contains("SemanticOperationKind::CancelGuardianCeremony"));
        assert!(shell_source.contains("OperationId::cancel_key_rotation_ceremony()"));
        assert!(shell_source.contains("SemanticOperationKind::CancelKeyRotationCeremony"));
        assert!(shell_source.contains("operation.monitor_started().await"));
        assert!(shell_source.contains("operation.cancel().await"));
    }

    #[test]
    fn slash_command_dispatch_uses_shared_typed_execution_and_owner_metadata() {
        let source = read_repo_source("crates/aura-terminal/src/tui/callbacks/factories/chat.rs");

        assert!(source.contains("ui::workflows::slash_commands::prepare_and_execute("));
        assert!(source.contains("let report ="));
        assert!(source.contains(".and_then(|metadata| metadata.semantic_operation.clone())"));
        assert!(source.contains("submit_local_terminal_operation("));
        assert!(!source.contains("ui::workflows::slash_commands::prepare("));
        assert!(!source.contains("ui::workflows::slash_commands::execute("));
        assert!(!source.contains("parse_chat_command(trimmed)"));
    }

    #[test]
    fn terminal_semantic_lifecycle_delegates_to_shared_typed_submission_wrappers() {
        let source = read_repo_source("crates/aura-terminal/src/tui/semantic_lifecycle.rs");

        assert!(source.contains("LocalTerminalSubmission<TuiSubmittedOperationPublisher>"));
        assert!(source.contains("WorkflowHandoffSubmission<TuiSubmittedOperationPublisher>"));
        assert!(source.contains("CeremonyMonitorHandoffSubmission<TuiSubmittedOperationPublisher>"));
        assert!(!source.contains("SubmittedOperation<TuiSubmittedOperationPublisher>"));
        assert!(!source.contains("SemanticOperationOwner"));
    }

    #[test]
    fn remove_device_dispatch_uses_ceremony_submission_owner() {
        let handlers_source = read_repo_source(
            "crates/aura-terminal/src/tui/screens/app/shell/dispatch_command_handlers.rs",
        );
        let dispatch_source =
            read_repo_source("crates/aura-terminal/src/tui/screens/app/shell/dispatch.rs");
        let settings_source =
            read_repo_source("crates/aura-terminal/src/tui/callbacks/factories/settings.rs");

        assert!(handlers_source.contains("OperationId::remove_device()"));
        assert!(handlers_source.contains("SemanticOperationKind::RemoveDevice"));
        assert!(handlers_source.contains("submit_ceremony_operation("));
        assert!(dispatch_source.contains("OperationId::remove_device()"));
        assert!(dispatch_source.contains("SemanticOperationKind::RemoveDevice"));
        assert!(dispatch_source.contains("submit_ceremony_operation("));
        assert!(settings_source.contains("operation.monitor_started().await"));
    }

    #[test]
    fn authority_updates_use_required_publication() {
        let source = read_repo_source(
            "crates/aura-terminal/src/tui/screens/app/subscriptions/nav_status.rs",
        );

        assert!(source.contains("spawn_ui_update("));
        assert!(source.contains("UiUpdatePublication::RequiredUnordered"));
        assert!(!source.contains("try_send(UiUpdate::AuthoritiesUpdated"));
    }

    #[test]
    fn device_enrollment_monitor_uses_required_publication() {
        let source =
            read_repo_source("crates/aura-terminal/src/tui/callbacks/factories/settings.rs");

        let monitor_start = source
            .find("monitor_key_rotation_ceremony_with_policy(")
            .unwrap_or_else(|| panic!("missing monitor_key_rotation_ceremony_with_policy"));
        let monitor_branch = &source[monitor_start..];

        assert!(monitor_branch.contains("send_ui_update_required_blocking("));
        assert!(!monitor_branch.contains("send_ui_update_lossy("));
    }

    #[test]
    fn terminal_events_hook_precedes_render_short_circuits() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let shell_path = repo_root.join("crates/aura-terminal/src/tui/screens/app/shell.rs");
        let shell_source = std::fs::read_to_string(&shell_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", shell_path.display()));

        let hook_start = shell_source
            .find("hooks.use_terminal_events({")
            .unwrap_or_else(|| panic!("missing terminal events hook"));
        let exit_guard = shell_source
            .find("if render_should_exit {")
            .unwrap_or_else(|| panic!("missing render exit guard"));
        let snapshot_guard = shell_source
            .find("if render_short_circuit {")
            .unwrap_or_else(|| panic!("missing render snapshot guard"));

        assert!(
            hook_start < exit_guard,
            "terminal events hook must be registered before render exit short-circuits"
        );
        assert!(
            hook_start < snapshot_guard,
            "terminal events hook must be registered before render snapshot short-circuits"
        );
    }

    #[test]
    fn chat_state_update_clamps_stale_selected_channel_even_with_unresolved_committed_selection() {
        let shell_source =
            read_repo_source("crates/aura-terminal/src/tui/screens/app/shell/update_handlers.rs");

        let update_start = shell_source
            .find("UiUpdate::ChatStateUpdated {")
            .unwrap_or_else(|| panic!("missing ChatStateUpdated update arm"));
        let next_arm = shell_source[update_start..]
            .find("UiUpdate::TopicSet")
            .map(|offset| update_start + offset)
            .unwrap_or_else(|| panic!("missing TopicSet update arm"));
        let update_branch = &shell_source[update_start..next_arm];

        assert!(update_branch.contains("state.chat.selected_channel >= channel_count"));
        assert!(!update_branch.contains("committed_selection.is_none()\n                                    && state.chat.selected_channel >= channel_count"));
    }
}
