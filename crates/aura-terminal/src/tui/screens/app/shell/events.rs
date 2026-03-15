use super::*;

pub(super) fn resolve_committed_selected_channel_id(
    state: &TuiState,
    shared_channels: &[Channel],
) -> Option<String> {
    shared_channels
        .get(state.chat.selected_channel)
        .map(|channel| channel.id.clone())
}

pub(super) fn handle_channel_selection_change(
    current: &TuiState,
    new_state: &TuiState,
    shared_channels: &Arc<std::sync::RwLock<Vec<Channel>>>,
    selected_channel_id: &Arc<std::sync::RwLock<Option<String>>>,
    selected_channel_binding: &Arc<std::sync::RwLock<Option<SelectedChannelBinding>>>,
) {
    let idx = new_state.chat.selected_channel;

    let channels = match shared_channels.read() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };
    let next_selected = channels.get(idx).map(|channel| channel.id.clone());
    let current_selected = selected_channel_id
        .read()
        .ok()
        .and_then(|guard| guard.clone());

    if new_state.chat.selected_channel == current.chat.selected_channel
        && next_selected == current_selected
    {
        return;
    }

    if let Ok(mut guard) = selected_channel_id.write() {
        *guard = next_selected;
    }
    if let Ok(mut guard) = selected_channel_binding.write() {
        let previous = guard.clone();
        *guard = channels.get(idx).map(|channel| {
            SelectedChannelBinding::merged_from_channel(channel, previous.as_ref())
        });
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_committed_selected_channel_id;
    use crate::tui::state_machine::TuiState;
    use crate::tui::types::Channel;
    use std::path::Path;

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
            Some("channel-2".to_string())
        );

        state.chat.selected_channel = 4;
        assert_eq!(
            resolve_committed_selected_channel_id(&state, &channels),
            None
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
}
