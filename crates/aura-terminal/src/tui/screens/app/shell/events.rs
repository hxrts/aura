use super::*;
use crate::tui::channel_selection::{
    authoritative_channel_binding, CommittedChannelSelection, SharedCommittedChannelSelection,
};
use aura_app::ui_contract::ChannelBindingWitness;

pub(super) fn resolve_committed_selected_channel_id(
    state: &TuiState,
    shared_channels: &[Channel],
) -> Option<CommittedChannelSelection> {
    shared_channels
        .get(state.chat.selected_channel)
        .map(|channel| CommittedChannelSelection::new(channel.id.clone()))
}

pub(super) fn handle_channel_selection_change(
    current: &TuiState,
    new_state: &TuiState,
    shared_channels: &Arc<parking_lot::RwLock<Vec<Channel>>>,
    selected_channel_id: &SharedCommittedChannelSelection,
    selected_channel_binding: &Arc<parking_lot::RwLock<Option<ChannelBindingWitness>>>,
) {
    let idx = new_state.chat.selected_channel;

    let channels = shared_channels.read().clone();
    let next_selected = channels
        .get(idx)
        .map(|channel| CommittedChannelSelection::new(channel.id.clone()));
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
    {
        let mut guard = selected_channel_binding.write();
        *guard = channels.get(idx).map(authoritative_channel_binding);
    }
}

#[cfg(test)]
mod tests {
    use super::{handle_channel_selection_change, resolve_committed_selected_channel_id};
    use crate::tui::channel_selection::CommittedChannelSelection;
    use crate::tui::state::TuiState;
    use crate::tui::types::Channel;
    use aura_app::ui_contract::ChannelBindingWitness;
    use std::path::Path;
    use std::sync::Arc;

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
        let selected_channel_id = Arc::new(parking_lot::RwLock::new(None));
        let selected_channel_binding = Arc::new(parking_lot::RwLock::new(Some(
            ChannelBindingWitness::new("channel-1", Some("ctx-123".to_string())),
        )));

        handle_channel_selection_change(
            &current,
            &next,
            &channels,
            &selected_channel_id,
            &selected_channel_binding,
        );

        assert_eq!(
            *selected_channel_id.read(),
            Some(CommittedChannelSelection::new("channel-1"))
        );
        assert_eq!(
            *selected_channel_binding.read(),
            Some(ChannelBindingWitness::new("channel-1", None))
        );
    }

    #[test]
    fn send_dispatch_does_not_background_retry_selection() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let shell_path = repo_root.join("crates/aura-terminal/src/tui/screens/app/shell.rs");
        let shell_source = std::fs::read_to_string(&shell_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", shell_path.display()));
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
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let shell_path = repo_root.join("crates/aura-terminal/src/tui/screens/app/shell.rs");
        let shell_source = std::fs::read_to_string(&shell_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", shell_path.display()));
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
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let shell_path = repo_root.join("crates/aura-terminal/src/tui/screens/app/shell.rs");
        let shell_source = std::fs::read_to_string(&shell_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", shell_path.display()));

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
        let parsed_start = source
            .find(
                "let parsed = match aura_app::ui::workflows::strong_command::ParsedCommand::parse(",
            )
            .unwrap_or_else(|| panic!("missing slash-command parse path"));
        let join_callback_start = source[parsed_start..]
            .find("fn make_join_channel(")
            .map(|offset| parsed_start + offset)
            .unwrap_or_else(|| panic!("missing join-channel callback factory"));
        let slash_command_branch = &source[parsed_start..join_callback_start];

        assert!(!slash_command_branch.contains("joined_channel_name"));
        assert!(!slash_command_branch.contains("get_chat_state("));
        assert!(!slash_command_branch.contains("candidate.name.eq_ignore_ascii_case"));
        assert!(!slash_command_branch.contains("UiUpdate::ChannelSelected("));
    }

    #[test]
    fn invite_to_channel_dispatch_clears_readiness_without_local_lifecycle_authorship() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let shell_path = repo_root.join("crates/aura-terminal/src/tui/screens/app/shell.rs");
        let shell_source = std::fs::read_to_string(&shell_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", shell_path.display()));

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
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let shell_path = repo_root.join("crates/aura-terminal/src/tui/screens/app/shell.rs");
        let shell_source = std::fs::read_to_string(&shell_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", shell_path.display()));

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
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let shell_path = repo_root.join("crates/aura-terminal/src/tui/screens/app/shell.rs");
        let shell_source = std::fs::read_to_string(&shell_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", shell_path.display()));
        let helper_path = repo_root.join("crates/aura-terminal/src/tui/key_rotation.rs");
        let helper_source = std::fs::read_to_string(&helper_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", helper_path.display()));

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
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let shell_path = repo_root.join("crates/aura-terminal/src/tui/screens/app/shell.rs");
        let shell_source = std::fs::read_to_string(&shell_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", shell_path.display()));

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
