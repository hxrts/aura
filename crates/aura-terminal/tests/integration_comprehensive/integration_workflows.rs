use super::*;

#[test]
fn test_complete_chat_workflow() {
    let mut tui = TestTui::new();
    tui.go_to_screen(Screen::Chat);
    tui.send_char('n');
    tui.type_text("announcements");
    tui.send_enter();
    tui.send_char('t');
    tui.type_text("Important announcements only");
    tui.send_enter();
    tui.send_char('i');
    tui.type_text("Hello everyone!");
    tui.clear_commands();
    tui.send_enter();

    tui.assert_dispatch(|d| matches!(d, DispatchCommand::SendChatMessage { .. }));
}

#[test]
fn test_recovery_guardian_setup() {
    let mut tui = TestTui::new();
    tui.go_to_screen(Screen::Contacts);

    tui.clear_commands();
    tui.send_char('g');
    tui.assert_dispatch(|d| matches!(d, DispatchCommand::OpenGuardianSetup));
    tui.state.modal_queue.enqueue(QueuedModal::GuardianSetup(
        aura_terminal::tui::state::GuardianSetupModalState::default(),
    ));
    assert!(tui.state.is_guardian_setup_modal_active());

    tui.state.modal_queue.dismiss();
    let contacts = vec![
        aura_terminal::tui::state::GuardianCandidate {
            id: aura_terminal::ids::authority_id("guardian-contact-1").to_string(),
            name: "Contact 1".to_string(),
            is_current_guardian: false,
        },
        aura_terminal::tui::state::GuardianCandidate {
            id: aura_terminal::ids::authority_id("guardian-contact-2").to_string(),
            name: "Contact 2".to_string(),
            is_current_guardian: false,
        },
    ];
    tui.state.modal_queue.enqueue(QueuedModal::GuardianSetup(
        aura_terminal::tui::state::GuardianSetupModalState::from_contacts_with_selection(
            contacts,
            vec![],
        ),
    ));

    tui.send_char(' ');
    tui.send_char('j');
    tui.send_char(' ');
    tui.send_enter();
    if let Some(QueuedModal::GuardianSetup(state)) = tui.state.modal_queue.current() {
        assert_eq!(
            state.step(),
            aura_terminal::tui::state::GuardianSetupStep::ChooseThreshold
        );
    } else {
        unreachable!("guardian setup modal missing after enter");
    }

    tui.clear_commands();
    tui.send_enter();
    tui.assert_dispatch(|d| matches!(d, DispatchCommand::StartGuardianCeremony { .. }));
}

#[test]
fn test_settings_complete_configuration() {
    let mut tui = TestTui::new();
    tui.go_to_screen(Screen::Settings);
    tui.send_enter();
    tui.type_text("NewName");
    tui.send_enter();
    tui.send_char('j');
    tui.send_enter();
    tui.send_up();
    tui.send_enter();
    tui.send_char('j');
    tui.send_char('a');
    tui.type_text("Tablet");
    tui.send_enter();
    tui.send_char('j');
    tui.send_char(' ');
}

#[test]
fn test_neighborhood_exploration() {
    let mut tui = TestTui::new();
    tui.go_to_screen(Screen::Neighborhood);
    tui.state.neighborhood.home_count = 1;

    tui.send_char('l');
    tui.send_char('l');
    tui.send_char('j');
    tui.send_char('j');

    tui.clear_commands();
    tui.send_enter();
    tui.assert_dispatch(|d| matches!(d, DispatchCommand::EnterHome { .. }));

    tui.clear_commands();
    tui.state.neighborhood.mode = aura_terminal::tui::state::NeighborhoodMode::Map;
    tui.send_char('g');
    tui.assert_dispatch(|d| matches!(d, DispatchCommand::GoHome));
}
