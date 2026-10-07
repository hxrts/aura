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

/// The channel a send (or slash command) targets. A committed selection is
/// authoritative: when it is not currently listed the send has no target,
/// rather than being redirected to whatever channel now sits at the old
/// index (Task 120: a transient projection redirected a home send to Note to
/// Self). The index is consulted only when nothing is committed.
pub(super) fn resolve_send_target_channel(
    committed: Option<CommittedChannelSelection>,
    state: &TuiState,
    shared_channels: &[Channel],
) -> Option<CommittedChannelSelection> {
    match committed {
        Some(selection) => (shared_channels.is_empty()
            || shared_channels
                .iter()
                .any(|channel| channel.id == selection.channel_id()))
        .then_some(selection),
        None => resolve_committed_selected_channel_id(state, shared_channels),
    }
}

pub(super) fn handle_channel_selection_change(
    current: &TuiState,
    new_state: &TuiState,
    shared_channels: &Arc<parking_lot::RwLock<Vec<Channel>>>,
    selected_channel_id: &SharedCommittedChannelSelection,
) {
    let idx = new_state.chat.selected_channel;
    // Only a user-driven index change moves a committed selection. With the
    // index unchanged, a reordered or transiently shrunk projection must not
    // retarget (or clear) the selection the user committed to.
    if idx == current.chat.selected_channel && selected_channel_id.read().is_some() {
        return;
    }
    let next_selected = shared_channels
        .read()
        .get(idx)
        .map(authoritative_committed_selection);
    if next_selected.is_some() {
        *selected_channel_id.write() = next_selected;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        handle_channel_selection_change, resolve_committed_selected_channel_id,
        resolve_send_target_channel,
    };
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

    /// Task 120 (run 165): with BarbHome committed at index 1, the chat
    /// projection transiently listed only Note to Self; a key event then
    /// retargeted the selection by index and the next home send went to
    /// Note to Self. The committed selection must survive, and a send must
    /// never be redirected to another channel.
    #[test]
    fn transient_projection_does_not_retarget_committed_send_channel() {
        let mut state = TuiState::new();
        state.chat.selected_channel = 1;
        let home = CommittedChannelSelection::new("channel-home");
        let selected = Arc::new(parking_lot::RwLock::new(Some(home.clone())));
        let transient = Arc::new(parking_lot::RwLock::new(vec![Channel::new(
            "channel-note",
            "Note to Self",
        )]));

        handle_channel_selection_change(&state, &state, &transient, &selected);
        assert_eq!(*selected.read(), Some(home.clone()));
        assert_eq!(
            resolve_send_target_channel(selected.read().clone(), &state, &transient.read()),
            None,
            "an unlisted committed channel has no send target"
        );

        let listed = vec![
            Channel::new("channel-note", "Note to Self"),
            Channel::new("channel-home", "BarbHome"),
        ];
        state.chat.selected_channel = 0;
        assert_eq!(
            resolve_send_target_channel(selected.read().clone(), &state, &listed),
            Some(home)
        );
    }

    /// Task 126: a pre-settlement "sending" row is never resolved when the
    /// send is refused, so the send callback inserts nothing; the chat shows
    /// only the app projection's committed (or failed) message.
    #[test]
    fn send_callback_inserts_no_unsettled_pending_row() {
        let chat_factory =
            read_repo_source("crates/aura-terminal/src/tui/callbacks/factories/chat.rs");
        let send_start = chat_factory
            .find("fn make_send_owned")
            .expect("send callback factory");
        let send_end = chat_factory[send_start..]
            .find("fn make_retry_message")
            .map(|offset| send_start + offset)
            .expect("retry callback factory");
        let send_factory = &chat_factory[send_start..send_end];
        assert!(!send_factory.contains("UiUpdate::Message"));
        assert!(!send_factory.contains("Message::sending"));
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
        assert!(send_branch.contains("Select a channel before sending a message"));
        // Owners are allocated only after the channel resolves (work/8.md task 14).
        let owner_alloc = send_branch
            .find("submit_workflow_handoff_operation")
            .expect("send branch allocates an owner");
        let channel_resolve = send_branch
            .find("resolve_send_target_channel")
            .expect("send branch resolves the channel");
        assert!(channel_resolve < owner_alloc);
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

        assert!(source.contains("ui::workflows::slash_commands::prepare("));
        assert!(source.contains("ui::workflows::strong_command::execute_planned("));
        assert!(source.contains("let report ="));
        assert!(source.contains(".and_then(|metadata| metadata.semantic_operation.clone())"));
        assert!(source.contains("submit_local_terminal_operation("));
        assert!(!source.contains("ui::workflows::slash_commands::prepare_and_execute("));
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

    /// Task 127: the messages pane, retry and send target read the selected
    /// channel's messages from the shared all-channel projection at read
    /// time, so moving the selection switches the visible messages with no
    /// chat-state update in between.
    #[test]
    fn messages_pane_follows_selection_without_chat_update() {
        use crate::tui::channel_selection::selected_channel_messages;
        use crate::tui::types::Message;

        let channels = Arc::new(parking_lot::RwLock::new(vec![
            Channel::new("channel-1", "General"),
            Channel::new("channel-2", "Ops"),
        ]));
        // Written once; never refreshed during the test.
        let shared_messages = vec![
            Message::new("m1", "alice", "general hello").with_channel("channel-1"),
            Message::new("m2", "bob", "ops one").with_channel("channel-2"),
            Message::new("m3", "bob", "ops two").with_channel("channel-2"),
        ];
        let selected = Arc::new(parking_lot::RwLock::new(None));
        let pane = |selected: &crate::tui::channel_selection::SharedCommittedChannelSelection| {
            selected_channel_messages(
                &shared_messages,
                selected
                    .read()
                    .as_ref()
                    .map(CommittedChannelSelection::channel_id),
            )
            .into_iter()
            .map(|message| message.id)
            .collect::<Vec<_>>()
        };

        let mut previous = TuiState::new();
        for (idx, expected_channel, expected_ids) in [
            (0, "channel-1", vec!["m1"]),
            (1, "channel-2", vec!["m2", "m3"]),
            (0, "channel-1", vec!["m1"]),
        ] {
            let mut next = previous.clone();
            next.chat.selected_channel = idx;
            handle_channel_selection_change(&previous, &next, &channels, &selected);
            assert_eq!(pane(&selected), expected_ids);
            assert_eq!(
                resolve_send_target_channel(selected.read().clone(), &next, &channels.read())
                    .map(|selection| selection.channel_id().to_string()),
                Some(expected_channel.to_string())
            );
            previous = next;
        }
    }
}
