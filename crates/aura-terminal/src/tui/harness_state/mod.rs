//! Structured TUI state export for harness observation.

mod commands;
mod snapshot;
mod socket;

pub(crate) use commands::apply_harness_command;
pub use commands::TuiSemanticInputs;
pub use snapshot::{maybe_export_ui_snapshot, publish_loading_ui_snapshot};
pub(crate) use socket::{
    accept_harness_command_submission, clear_harness_command_sender,
    complete_pending_semantic_submission, ensure_harness_command_listener,
    fail_pending_semantic_submission, register_harness_command_sender,
    reject_harness_command_submission, track_pending_semantic_submission, PendingSemanticValueKind,
};

#[cfg(test)]
mod tests {
    use super::commands::{apply_harness_command, TuiSemanticInputs};
    use super::snapshot::authoritative_ui_snapshot;
    use super::socket::{
        accept_harness_command_submission, clear_harness_command_sender,
        complete_pending_semantic_submission, forward_test_harness_commands_from_listener,
        register_harness_command_sender, track_pending_semantic_submission,
        PendingSemanticValueKind,
    };
    use crate::tui::screens::Screen;
    use crate::tui::state::modal_queue::QueuedModal;
    use crate::tui::state::views::{AccountSetupModalState, DeviceEnrollmentCeremonyModalState};
    use crate::tui::state::DispatchCommand;
    use crate::tui::state::InvitationKind;
    use crate::tui::tasks::UiTaskOwner;
    use crate::tui::types::{Channel as TuiChannel, Device as TuiDevice, SettingsSection};
    use crate::tui::updates::{harness_command_channel, HarnessCommandSubmission};
    use crate::tui::{TuiCommand, TuiState};
    use aura_app::ui::contract::{
        ControlId, HarnessUiCommand, HarnessUiCommandReceipt, ListId, OperationId, OperationState,
        ScreenId, UiReadiness,
    };
    use aura_app::ui::types::StateSnapshot;
    use aura_app::ui_contract::RuntimeFact;
    use aura_core::effects::PhysicalTimeEffects;
    use aura_core::{
        execute_with_timeout_budget, TimeoutBudget, TimeoutExecutionProfile, TimeoutRunError,
    };
    use aura_effects::time::PhysicalTimeHandler;
    use std::os::unix::net::UnixListener as StdUnixListener;
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::OnceLock;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{UnixListener, UnixStream};
    use tokio::sync::Mutex;

    static TEST_SOCKET_COUNTER: AtomicU64 = AtomicU64::new(0);
    static HARNESS_BRIDGE_TEST_GUARD: OnceLock<Mutex<()>> = OnceLock::new();

    fn test_socket_path(label: &str) -> std::path::PathBuf {
        let suffix = TEST_SOCKET_COUNTER.fetch_add(1, Ordering::Relaxed);
        let compact_label = label
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .take(6)
            .collect::<String>();
        let temp_root = if std::path::Path::new("/tmp").is_dir() {
            std::path::PathBuf::from("/tmp")
        } else {
            std::env::temp_dir()
        };
        temp_root.join(format!(
            "atui-{}-{}-{suffix}.sock",
            compact_label,
            std::process::id()
        ))
    }

    async fn lock_harness_bridge_test() -> tokio::sync::MutexGuard<'static, ()> {
        HARNESS_BRIDGE_TEST_GUARD
            .get_or_init(|| Mutex::new(()))
            .lock()
            .await
    }

    #[test]
    fn account_setup_maps_to_onboarding_state() {
        let mut state = TuiState::new();
        state.show_modal(QueuedModal::AccountSetup(AccountSetupModalState::default()));

        let app_snapshot = StateSnapshot::default();
        let snapshot = authoritative_ui_snapshot(
            &state,
            TuiSemanticInputs {
                app_snapshot: &app_snapshot,
                contacts: &[],
                settings_devices: &[],
                chat_channels: &[],
                chat_messages: &[],
            },
        );
        assert_eq!(snapshot.readiness, UiReadiness::Ready);
        assert_eq!(snapshot.focused_control, Some(ControlId::OnboardingRoot));
        assert_eq!(snapshot.open_modal, None);
    }

    #[test]
    fn navigation_list_marks_current_screen() {
        let mut state = TuiState::new();
        state.router.go_to(Screen::Contacts);

        let app_snapshot = StateSnapshot::default();
        let snapshot = authoritative_ui_snapshot(
            &state,
            TuiSemanticInputs {
                app_snapshot: &app_snapshot,
                contacts: &[],
                settings_devices: &[],
                chat_channels: &[],
                chat_messages: &[],
            },
        );
        let nav = snapshot
            .lists
            .iter()
            .find(|list| list.id == ListId::Navigation)
            .unwrap_or_else(|| panic!("navigation list should exist"));
        assert!(nav.items.iter().any(|item| item.selected));
    }

    #[test]
    fn device_enrollment_modal_exports_operation_state() {
        let mut state = TuiState::new();
        let mut modal = DeviceEnrollmentCeremonyModalState::started(
            "ceremony-1".to_string(),
            "Mobile".to_string(),
            "code-123".to_string(),
        );
        modal.update_from_status(
            1,
            2,
            2,
            false,
            false,
            None,
            None,
            aura_core::threshold::AgreementMode::CoordinatorSoftSafe,
            false,
        );
        state.set_authoritative_operation_state(
            OperationId::device_enrollment(),
            None,
            None,
            OperationState::Submitting,
        );
        state.show_modal(QueuedModal::SettingsDeviceEnrollment(modal));

        let app_snapshot = StateSnapshot::default();
        let snapshot = authoritative_ui_snapshot(
            &state,
            TuiSemanticInputs {
                app_snapshot: &app_snapshot,
                contacts: &[],
                settings_devices: &[],
                chat_channels: &[],
                chat_messages: &[],
            },
        );
        let operation_state = snapshot
            .operations
            .iter()
            .find(|operation| operation.id == OperationId::device_enrollment())
            .map(|operation| operation.state);

        assert_eq!(operation_state, Some(OperationState::Submitting));
    }

    #[test]
    fn semantic_snapshot_does_not_synthesize_placeholder_contact_ids() {
        let mut state = TuiState::new();
        state.contacts.contact_count = 3;

        let app_snapshot = StateSnapshot::default();
        let snapshot = authoritative_ui_snapshot(
            &state,
            TuiSemanticInputs {
                app_snapshot: &app_snapshot,
                contacts: &[],
                settings_devices: &[],
                chat_channels: &[],
                chat_messages: &[],
            },
        );

        let contacts = snapshot
            .lists
            .iter()
            .find(|list| list.id == ListId::Contacts)
            .map(|list| list.items.clone())
            .unwrap_or_default();

        assert!(contacts.is_empty());
        assert!(!snapshot
            .selections
            .iter()
            .any(|selection| selection.list == ListId::Contacts));
    }

    #[test]
    fn harness_command_navigation_applies_immediately() {
        let mut state = TuiState::new();
        let followup = apply_harness_command(
            &mut state,
            HarnessUiCommand::NavigateScreen {
                screen: ScreenId::Settings,
            },
            TuiSemanticInputs {
                app_snapshot: &StateSnapshot::default(),
                contacts: &[],
                settings_devices: &[],
                chat_channels: &[],
                chat_messages: &[],
            },
        )
        .unwrap_or_else(|error| panic!("navigation command should apply: {error}"));

        assert!(followup.is_empty());
        assert_eq!(state.screen(), Screen::Settings);
    }

    #[test]
    fn harness_command_open_settings_section_applies_immediately() {
        let mut state = TuiState::new();
        let followup = apply_harness_command(
            &mut state,
            HarnessUiCommand::OpenSettingsSection {
                section: aura_app::scenario_contract::SettingsSection::Devices,
            },
            TuiSemanticInputs {
                app_snapshot: &StateSnapshot::default(),
                contacts: &[],
                settings_devices: &[],
                chat_channels: &[],
                chat_messages: &[],
            },
        )
        .unwrap_or_else(|error| panic!("settings section command should apply: {error}"));

        assert!(followup.is_empty());
        assert_eq!(state.screen(), Screen::Settings);
        assert_eq!(state.settings.section, SettingsSection::Devices);
    }

    #[test]
    fn harness_command_dismiss_transient_closes_modal() {
        let mut state = TuiState::new();
        state.show_modal(QueuedModal::AccountSetup(AccountSetupModalState::default()));

        let followup = apply_harness_command(
            &mut state,
            HarnessUiCommand::DismissTransient,
            TuiSemanticInputs {
                app_snapshot: &StateSnapshot::default(),
                contacts: &[],
                settings_devices: &[],
                chat_channels: &[],
                chat_messages: &[],
            },
        )
        .unwrap_or_else(|error| panic!("dismiss transient command should apply: {error}"));

        assert!(followup.is_empty());
        assert!(state.modal_queue.current().is_none());
    }

    #[test]
    fn harness_command_remove_device_emits_dispatch_followup() {
        let mut state = TuiState::new();
        let devices = vec![
            TuiDevice::new("device:current", "Current").current(),
            TuiDevice::new("device:removable", "Backup"),
        ];
        let followup = apply_harness_command(
            &mut state,
            HarnessUiCommand::RemoveSelectedDevice { device_id: None },
            TuiSemanticInputs {
                app_snapshot: &StateSnapshot::default(),
                contacts: &[],
                settings_devices: &devices,
                chat_channels: &[],
                chat_messages: &[],
            },
        )
        .unwrap_or_else(|error| panic!("remove device command should apply: {error}"));

        assert_eq!(state.screen(), Screen::Settings);
        assert_eq!(state.settings.section, SettingsSection::Devices);
        assert!(matches!(
            followup.as_slice(),
            [TuiCommand::HarnessRemoveVisibleDevice { device_id: Some(device_id) }]
                if device_id == "device:removable"
        ));
    }

    #[test]
    fn harness_command_remove_device_falls_back_to_last_visible_device_when_flags_drift() {
        let mut state = TuiState::new();
        let devices = vec![
            TuiDevice::new("device:current", "Current").current(),
            TuiDevice::new("device:removable", "Backup").current(),
        ];
        let followup = apply_harness_command(
            &mut state,
            HarnessUiCommand::RemoveSelectedDevice { device_id: None },
            TuiSemanticInputs {
                app_snapshot: &StateSnapshot::default(),
                contacts: &[],
                settings_devices: &devices,
                chat_channels: &[],
                chat_messages: &[],
            },
        )
        .unwrap_or_else(|error| panic!("remove device fallback should apply: {error}"));

        assert!(matches!(
            followup.as_slice(),
            [TuiCommand::HarnessRemoveVisibleDevice { device_id: Some(device_id) }]
                if device_id == "device:removable"
        ));
    }

    #[test]
    fn harness_command_switch_authority_is_noop_for_current_authority() {
        let mut state = TuiState::new();
        state.authorities =
            vec![crate::tui::types::AuthorityInfo::new("authority:current", "Current").current()];

        let followup = apply_harness_command(
            &mut state,
            HarnessUiCommand::SwitchAuthority {
                authority_id: "authority:current".to_string(),
            },
            TuiSemanticInputs {
                app_snapshot: &StateSnapshot::default(),
                contacts: &[],
                settings_devices: &[],
                chat_channels: &[],
                chat_messages: &[],
            },
        )
        .unwrap_or_else(|error| panic!("switch authority no-op should apply: {error}"));

        assert!(followup.is_empty());
        assert_eq!(state.screen(), Screen::Settings);
        assert_eq!(state.settings.section, SettingsSection::Authority);
    }

    #[test]
    fn harness_command_create_account_emits_dispatch_followup() {
        let mut state = TuiState::new();
        let followup = apply_harness_command(
            &mut state,
            HarnessUiCommand::CreateAccount {
                account_name: "AliceUser".to_string(),
            },
            TuiSemanticInputs {
                app_snapshot: &StateSnapshot::default(),
                contacts: &[],
                settings_devices: &[],
                chat_channels: &[],
                chat_messages: &[],
            },
        )
        .unwrap_or_else(|error| panic!("create account command should apply: {error}"));

        assert!(matches!(
            followup.as_slice(),
            [TuiCommand::Dispatch(DispatchCommand::CreateAccount { name })] if name == "AliceUser"
        ));
    }

    #[test]
    fn harness_command_join_channel_emits_dispatch_followup() {
        let mut state = TuiState::new();
        let followup = apply_harness_command(
            &mut state,
            HarnessUiCommand::JoinChannel {
                channel_name: "shared-parity-lab".to_string(),
            },
            TuiSemanticInputs {
                app_snapshot: &StateSnapshot::default(),
                contacts: &[],
                settings_devices: &[],
                chat_channels: &[],
                chat_messages: &[],
            },
        )
        .unwrap_or_else(|error| panic!("join channel command should apply: {error}"));

        assert!(matches!(
            followup.as_slice(),
            [TuiCommand::Dispatch(DispatchCommand::JoinChannel { channel_name })]
                if channel_name == "shared-parity-lab"
        ));
    }

    #[test]
    fn harness_command_create_channel_emits_dispatch_followup() {
        let mut state = TuiState::new();
        let followup = apply_harness_command(
            &mut state,
            HarnessUiCommand::CreateChannel {
                channel_name: "shared-parity-lab".to_string(),
            },
            TuiSemanticInputs {
                app_snapshot: &StateSnapshot::default(),
                contacts: &[],
                settings_devices: &[],
                chat_channels: &[],
                chat_messages: &[],
            },
        )
        .unwrap_or_else(|error| panic!("create channel command should apply: {error}"));

        assert_eq!(state.screen(), Screen::Chat);
        assert!(matches!(
            followup.as_slice(),
            [TuiCommand::Dispatch(DispatchCommand::CreateChannel {
                name,
                topic: None,
                members,
                threshold_k,
            })] if name == "shared-parity-lab" && members.is_empty() && threshold_k.get() == 1
        ));
    }

    #[test]
    fn harness_command_start_device_enrollment_emits_add_device_followup() {
        let mut state = TuiState::new();
        let expected_invitee_authority_id = aura_core::AuthorityId::new_from_entropy([0x77; 32]);
        let followup = apply_harness_command(
            &mut state,
            HarnessUiCommand::StartDeviceEnrollment {
                device_name: "Mobile".to_string(),
                invitee_authority_id: expected_invitee_authority_id.to_string(),
            },
            TuiSemanticInputs {
                app_snapshot: &StateSnapshot::default(),
                contacts: &[],
                settings_devices: &[],
                chat_channels: &[],
                chat_messages: &[],
            },
        )
        .unwrap_or_else(|error| panic!("device enrollment command should apply: {error}"));

        assert_eq!(state.screen(), Screen::Settings);
        assert_eq!(state.settings.section, SettingsSection::Devices);
        assert!(matches!(
            followup.as_slice(),
            [TuiCommand::Dispatch(DispatchCommand::AddDevice {
                name,
                invitee_authority_id
            })] if name == "Mobile"
                && *invitee_authority_id == expected_invitee_authority_id
        ));
    }

    #[test]
    fn harness_command_import_device_enrollment_code_uses_onboarding_dispatch() {
        let mut state = TuiState::new();
        let followup = apply_harness_command(
            &mut state,
            HarnessUiCommand::ImportDeviceEnrollmentCode {
                code: "device-code".to_string(),
            },
            TuiSemanticInputs {
                app_snapshot: &StateSnapshot::default(),
                contacts: &[],
                settings_devices: &[],
                chat_channels: &[],
                chat_messages: &[],
            },
        )
        .unwrap_or_else(|error| panic!("device import command should apply: {error}"));

        assert!(matches!(
            followup.as_slice(),
            [TuiCommand::Dispatch(DispatchCommand::ImportDeviceEnrollmentDuringOnboarding {
                code
            })] if code == "device-code"
        ));
    }

    #[test]
    fn harness_command_create_contact_invitation_emits_dispatch_followup() {
        let mut state = TuiState::new();
        let authority_id = crate::ids::authority_id("harness-state:test-contact").to_string();
        let followup = apply_harness_command(
            &mut state,
            HarnessUiCommand::CreateContactInvitation {
                receiver_authority_id: authority_id.clone(),
            },
            TuiSemanticInputs {
                app_snapshot: &StateSnapshot::default(),
                contacts: &[],
                settings_devices: &[],
                chat_channels: &[],
                chat_messages: &[],
            },
        )
        .unwrap_or_else(|error| panic!("create invitation command should apply: {error}"));

        assert!(matches!(
            followup.as_slice(),
            [TuiCommand::Dispatch(DispatchCommand::CreateInvitation {
                receiver_id,
                invitation_type: InvitationKind::Contact,
                message: None,
                ttl_secs: None,
            })] if receiver_id.to_string() == authority_id
        ));
    }

    #[test]
    fn harness_command_invite_actor_to_channel_emits_dispatch_followup() {
        let mut state = TuiState::new();
        let authority_id = crate::ids::authority_id("harness-state:test-channel-invite");
        let channel_id = aura_core::ChannelId::from_bytes([7u8; 32]);
        let followup = apply_harness_command(
            &mut state,
            HarnessUiCommand::InviteActorToChannel {
                authority_id: authority_id.to_string(),
                channel_id: channel_id.to_string(),
            },
            TuiSemanticInputs {
                app_snapshot: &StateSnapshot::default(),
                contacts: &[],
                settings_devices: &[],
                chat_channels: &[],
                chat_messages: &[],
            },
        )
        .unwrap_or_else(|error| panic!("invite actor command should apply: {error}"));

        assert!(matches!(
            followup.as_slice(),
            [TuiCommand::Dispatch(DispatchCommand::InviteActorToChannel {
                authority_id: dispatched_id,
                channel_id: dispatched_channel_id,
            })] if dispatched_id == &authority_id && dispatched_channel_id == &channel_id.to_string()
        ));
    }

    #[test]
    fn harness_command_import_invitation_emits_dispatch_followup() {
        let mut state = TuiState::new();
        let followup = apply_harness_command(
            &mut state,
            HarnessUiCommand::ImportInvitation {
                code: "aura:v1:test".to_string(),
            },
            TuiSemanticInputs {
                app_snapshot: &StateSnapshot::default(),
                contacts: &[],
                settings_devices: &[],
                chat_channels: &[],
                chat_messages: &[],
            },
        )
        .unwrap_or_else(|error| panic!("import invitation command should apply: {error}"));

        assert!(matches!(
            followup.as_slice(),
            [TuiCommand::Dispatch(DispatchCommand::ImportInvitation { code })]
                if code == "aura:v1:test"
        ));
    }

    #[test]
    fn harness_command_navigation_publishes_newer_authoritative_projection() {
        let app_snapshot = StateSnapshot::default();

        let initial_state = TuiState::new();
        let initial_snapshot = authoritative_ui_snapshot(
            &initial_state,
            TuiSemanticInputs {
                app_snapshot: &app_snapshot,
                contacts: &[],
                settings_devices: &[],
                chat_channels: &[],
                chat_messages: &[],
            },
        );

        let mut updated_state = TuiState::new();
        apply_harness_command(
            &mut updated_state,
            HarnessUiCommand::NavigateScreen {
                screen: ScreenId::Settings,
            },
            TuiSemanticInputs {
                app_snapshot: &app_snapshot,
                contacts: &[],
                settings_devices: &[],
                chat_channels: &[],
                chat_messages: &[],
            },
        )
        .unwrap_or_else(|error| panic!("navigation command should apply: {error}"));
        let updated_snapshot = authoritative_ui_snapshot(
            &updated_state,
            TuiSemanticInputs {
                app_snapshot: &app_snapshot,
                contacts: &[],
                settings_devices: &[],
                chat_channels: &[],
                chat_messages: &[],
            },
        );

        assert_eq!(updated_snapshot.screen, ScreenId::Settings);
        assert!(
            updated_snapshot.revision.semantic_seq > initial_snapshot.revision.semantic_seq,
            "semantic command application must publish a newer authoritative projection"
        );
    }

    #[test]
    fn harness_command_select_home_uses_visible_home_ids() {
        let mut state = TuiState::new();
        let mut app_snapshot = StateSnapshot::default();
        let home_id = "channel:1111111111111111111111111111111111111111111111111111111111111111";
        app_snapshot.neighborhood.home_home_id = home_id
            .parse()
            .unwrap_or_else(|error| panic!("home id should parse: {error}"));

        let followup = apply_harness_command(
            &mut state,
            HarnessUiCommand::SelectHome {
                home_id: home_id.to_string(),
            },
            TuiSemanticInputs {
                app_snapshot: &app_snapshot,
                contacts: &[],
                settings_devices: &[],
                chat_channels: &[],
                chat_messages: &[],
            },
        )
        .unwrap_or_else(|error| panic!("home selection command should apply: {error}"));

        assert!(followup.is_empty());
        assert_eq!(state.screen(), Screen::Neighborhood);
        assert_eq!(state.neighborhood.selected_home, 0);
    }

    #[test]
    fn harness_command_select_channel_emits_dispatch_followup() {
        let app_snapshot = StateSnapshot::default();
        let channels = vec![
            TuiChannel::new("channel:note-to-self", "Note to Self"),
            TuiChannel::new("channel:shared", "Shared"),
        ];
        let mut state = TuiState::new();
        let followup = apply_harness_command(
            &mut state,
            HarnessUiCommand::SelectChannel {
                channel_id: "channel:shared".to_string(),
            },
            TuiSemanticInputs {
                app_snapshot: &app_snapshot,
                contacts: &[],
                settings_devices: &[],
                chat_channels: &channels,
                chat_messages: &[],
            },
        )
        .unwrap_or_else(|error| panic!("channel selection command should apply: {error}"));

        assert_eq!(state.screen(), Screen::Chat);
        assert!(matches!(
            followup.as_slice(),
            [TuiCommand::Dispatch(DispatchCommand::SelectChannel { channel_id })]
                if channel_id == "channel:shared"
        ));
    }

    #[test]
    fn harness_command_channel_selection_uses_visible_channel_ids() {
        let app_snapshot = StateSnapshot::default();
        let channels = vec![
            TuiChannel::new("channel:note-to-self", "Note to Self"),
            TuiChannel::new("channel:shared", "Shared"),
        ];
        let mut state = TuiState::new();
        let followup = apply_harness_command(
            &mut state,
            HarnessUiCommand::ActivateListItem {
                list_id: ListId::Channels,
                item_id: "channel:shared".to_string(),
            },
            TuiSemanticInputs {
                app_snapshot: &app_snapshot,
                contacts: &[],
                settings_devices: &[],
                chat_channels: &channels,
                chat_messages: &[],
            },
        )
        .unwrap_or_else(|error| panic!("channel selection command should apply: {error}"));

        assert!(followup.is_empty());
        assert_eq!(state.screen(), Screen::Chat);
        assert_eq!(state.chat.selected_channel, 1);
    }

    #[tokio::test]
    async fn harness_command_bridge_acknowledges_submission_and_emits_update() {
        let _guard = lock_harness_bridge_test().await;
        let socket_path = test_socket_path("command-bridge");
        let _ = std::fs::remove_file(&socket_path);
        let listener = StdUnixListener::bind(&socket_path)
            .unwrap_or_else(|error| panic!("failed to bind {}: {error}", socket_path.display()));
        listener
            .set_nonblocking(true)
            .unwrap_or_else(|error| panic!("failed to configure nonblocking listener: {error}"));
        let listener = UnixListener::from_std(listener)
            .unwrap_or_else(|error| panic!("failed to convert listener: {error}"));

        let (command_tx, mut command_rx) = harness_command_channel();
        register_harness_command_sender(command_tx)
            .await
            .unwrap_or_else(|error| panic!("failed to register harness command sender: {error}"));
        let bridge_tasks = UiTaskOwner::new();
        bridge_tasks.spawn(async move {
            forward_test_harness_commands_from_listener(listener).await;
        });

        let apply_task = async move {
            let time = PhysicalTimeHandler::new();
            let started_at = time
                .physical_time()
                .await
                .unwrap_or_else(|error| panic!("failed to read physical time: {error}"));
            let timeout = TimeoutExecutionProfile::simulation_test()
                .scale_duration(Duration::from_secs(1))
                .unwrap_or_else(|error| panic!("failed to scale test timeout: {error}"));
            let budget = TimeoutBudget::from_start_and_timeout(&started_at, timeout)
                .unwrap_or_else(|error| panic!("failed to build test timeout budget: {error}"));
            let observed_submission = match execute_with_timeout_budget(&time, &budget, || async {
                Ok::<_, std::convert::Infallible>(command_rx.recv().await)
            })
            .await
            {
                Ok(Some(submission)) => submission,
                Ok(None) => panic!("harness command channel closed unexpectedly"),
                Err(TimeoutRunError::Timeout(error)) => {
                    panic!("timed out waiting for harness command submission: {error}")
                }
                Err(TimeoutRunError::Operation(error)) => match error {},
            };
            match observed_submission {
                HarnessCommandSubmission {
                    submission_id,
                    command:
                        HarnessUiCommand::NavigateScreen {
                            screen: ScreenId::Settings,
                        },
                } => {
                    accept_harness_command_submission(
                        submission_id,
                        None::<aura_app::ui_contract::HarnessUiOperationHandle>,
                        None::<aura_app::scenario_contract::SemanticCommandValue>,
                    )
                    .await
                    .unwrap_or_else(|error| {
                        panic!("failed to accept harness command submission: {error}")
                    });
                }
                other => panic!("unexpected harness command submission: {other:?}"),
            }
        };

        let client_task = async {
            let mut stream = UnixStream::connect(&socket_path)
                .await
                .unwrap_or_else(|error| {
                    panic!("failed to connect {}: {error}", socket_path.display())
                });
            let command = HarnessUiCommand::NavigateScreen {
                screen: ScreenId::Settings,
            };
            let payload = serde_json::to_vec(&command)
                .unwrap_or_else(|error| panic!("failed to encode harness command: {error}"));
            stream
                .write_all(&payload)
                .await
                .unwrap_or_else(|error| panic!("failed to write harness command: {error}"));
            stream.shutdown().await.unwrap_or_else(|error| {
                panic!("failed to half-close harness command stream: {error}")
            });
            let mut receipt_payload = Vec::new();
            stream
                .read_to_end(&mut receipt_payload)
                .await
                .unwrap_or_else(|error| panic!("failed to read harness command receipt: {error}"));
            let receipt = serde_json::from_slice::<HarnessUiCommandReceipt>(&receipt_payload)
                .unwrap_or_else(|error| {
                    panic!("failed to decode harness command receipt: {error}")
                });
            assert_eq!(receipt, HarnessUiCommandReceipt::Accepted { value: None });
        };

        let (_, ()) = tokio::join!(apply_task, client_task);
        bridge_tasks.shutdown();

        clear_harness_command_sender()
            .await
            .unwrap_or_else(|error| panic!("failed to clear harness command sender: {error}"));
        let _ = std::fs::remove_file(&socket_path);
    }

    #[tokio::test]
    async fn harness_command_bridge_tracks_pending_contact_invitation_value() {
        let _guard = lock_harness_bridge_test().await;
        let socket_path = test_socket_path("pending-contact-invitation");
        let _ = std::fs::remove_file(&socket_path);
        let listener = StdUnixListener::bind(&socket_path)
            .unwrap_or_else(|error| panic!("failed to bind {}: {error}", socket_path.display()));
        listener
            .set_nonblocking(true)
            .unwrap_or_else(|error| panic!("failed to configure nonblocking listener: {error}"));
        let listener = UnixListener::from_std(listener)
            .unwrap_or_else(|error| panic!("failed to convert listener: {error}"));

        let (command_tx, mut command_rx) = harness_command_channel();
        register_harness_command_sender(command_tx)
            .await
            .unwrap_or_else(|error| panic!("failed to register harness command sender: {error}"));
        let bridge_tasks = UiTaskOwner::new();
        bridge_tasks.spawn(async move {
            forward_test_harness_commands_from_listener(listener).await;
        });

        let apply_task = async move {
            let observed_submission = command_rx
                .recv()
                .await
                .unwrap_or_else(|| panic!("harness command channel closed unexpectedly"));
            match observed_submission {
                HarnessCommandSubmission {
                    submission_id,
                    command:
                        HarnessUiCommand::CreateContactInvitation {
                            receiver_authority_id,
                        },
                } => {
                    assert_eq!(receiver_authority_id, "authority:test-peer");
                    let operation = aura_app::ui_contract::HarnessUiOperationHandle::new(
                        OperationId::invitation_create(),
                        aura_app::ui_contract::OperationInstanceId(
                            "test-contact-invitation-instance".to_string(),
                        ),
                    );
                    track_pending_semantic_submission(
                        submission_id,
                        operation.clone(),
                        PendingSemanticValueKind::ContactInvitationCode,
                    )
                    .await
                    .unwrap_or_else(|error| {
                        panic!("failed to track pending semantic submission: {error}")
                    });
                    complete_pending_semantic_submission(
                        operation.instance_id().clone(),
                        aura_app::scenario_contract::SemanticCommandValue::ContactInvitationCode {
                            code: "invite-code".to_string(),
                        },
                    )
                    .await
                    .unwrap_or_else(|error| {
                        panic!("failed to complete pending semantic submission: {error}")
                    });
                }
                other => panic!("unexpected harness command submission: {other:?}"),
            }
        };

        let client_task = async {
            let mut stream = UnixStream::connect(&socket_path)
                .await
                .unwrap_or_else(|error| {
                    panic!("failed to connect {}: {error}", socket_path.display())
                });
            let command = HarnessUiCommand::CreateContactInvitation {
                receiver_authority_id: "authority:test-peer".to_string(),
            };
            let payload = serde_json::to_vec(&command)
                .unwrap_or_else(|error| panic!("failed to encode harness command: {error}"));
            stream
                .write_all(&payload)
                .await
                .unwrap_or_else(|error| panic!("failed to write harness command: {error}"));
            stream.shutdown().await.unwrap_or_else(|error| {
                panic!("failed to half-close harness command stream: {error}")
            });
            let mut receipt_payload = Vec::new();
            stream
                .read_to_end(&mut receipt_payload)
                .await
                .unwrap_or_else(|error| panic!("failed to read harness command receipt: {error}"));
            let receipt = serde_json::from_slice::<HarnessUiCommandReceipt>(&receipt_payload)
                .unwrap_or_else(|error| {
                    panic!("failed to decode harness command receipt: {error}")
                });
            assert_eq!(
                receipt,
                HarnessUiCommandReceipt::AcceptedWithOperation {
                    operation: aura_app::ui_contract::HarnessUiOperationHandle::new(
                        OperationId::invitation_create(),
                        aura_app::ui_contract::OperationInstanceId(
                            "test-contact-invitation-instance".to_string(),
                        ),
                    ),
                    value: Some(
                        aura_app::scenario_contract::SemanticCommandValue::ContactInvitationCode {
                            code: "invite-code".to_string(),
                        },
                    ),
                }
            );
        };

        let (_, ()) = tokio::join!(apply_task, client_task);
        bridge_tasks.shutdown();

        clear_harness_command_sender()
            .await
            .unwrap_or_else(|error| panic!("failed to clear harness command sender: {error}"));
        let _ = std::fs::remove_file(&socket_path);
    }

    /// Helper: read all `.rs` source files from the harness_state module directory
    /// and concatenate only non-test production source.
    fn read_production_source() -> String {
        let module_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui/harness_state");
        let mut production_source = String::new();
        for entry in std::fs::read_dir(&module_dir)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", module_dir.display()))
        {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                continue;
            }
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
            let file_production = source.split("#[cfg(test)]").next().unwrap_or(&source);
            production_source.push_str(file_production);
            production_source.push('\n');
        }
        production_source
    }

    #[test]
    fn semantic_snapshot_exporter_does_not_depend_on_parity_override_caches() {
        let production_source = read_production_source();

        assert!(
            !production_source.contains("static CONTACTS_OVERRIDE")
                && !production_source.contains("static DEVICES_OVERRIDE")
                && !production_source.contains("static MESSAGES_OVERRIDE"),
            "parity-critical TUI exports may not depend on override caches"
        );
        assert!(
            !production_source.contains("pub fn publish_contacts_list_export")
                && !production_source.contains("pub fn publish_devices_list_export")
                && !production_source.contains("pub fn publish_messages_export"),
            "parity-critical TUI exports may not declare parity override helpers"
        );
    }

    #[test]
    fn semantic_snapshot_ready_state_is_projection_only() {
        let production_source = read_production_source();

        assert!(
            !production_source.contains("contacts_override_input")
                && !production_source.contains("contact_items.is_empty()")
                && !production_source.contains("if home_ids.is_empty()"),
            "ready-state TUI export must stay pure projection without reconstruction fallbacks"
        );
    }

    #[test]
    fn semantic_snapshot_exports_tui_owned_runtime_facts() {
        let mut state = TuiState::new();
        state.upsert_runtime_fact(RuntimeFact::InvitationCodeReady {
            receiver_authority_id: None,
            source_operation: OperationId::invitation_create(),
            code: Some("invite-code".to_string()),
        });

        let app_snapshot = StateSnapshot::default();
        let snapshot = authoritative_ui_snapshot(
            &state,
            TuiSemanticInputs {
                app_snapshot: &app_snapshot,
                contacts: &[],
                settings_devices: &[],
                chat_channels: &[],
                chat_messages: &[],
            },
        );

        assert!(snapshot.runtime_events.iter().any(|event| {
            matches!(
                &event.fact,
                RuntimeFact::InvitationCodeReady { source_operation, .. }
                    if *source_operation == OperationId::invitation_create()
            )
        }));
    }

    #[test]
    fn semantic_snapshot_exporter_does_not_infer_parity_runtime_events() {
        let production_source = read_production_source();

        for forbidden in [
            "RuntimeFact::ContactLinkReady",
            "RuntimeFact::PendingHomeInvitationReady",
            "RuntimeFact::ChannelMembershipReady",
            "RuntimeFact::RecipientPeersResolved",
            "RuntimeFact::MessageDeliveryReady",
            "runtime_events.push(RuntimeEventSnapshot",
        ] {
            assert!(
                !production_source.contains(forbidden),
                "parity-critical runtime facts must not be synthesized during TUI snapshot export: {forbidden}"
            );
        }
    }
}
