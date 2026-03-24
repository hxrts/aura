use super::*;

#[test]
fn test_help_modal() {
    let mut tui = TestTui::new();
    tui.send_char('?');
    tui.assert_has_modal();
    tui.assert_modal(|modal| matches!(modal, QueuedModal::Help { .. }));

    tui.state.help.scroll_max = 50;
    let initial_scroll = tui.state.help.scroll;
    tui.send_char('j');
    assert_eq!(tui.state.help.scroll, initial_scroll + 1);

    tui.send_escape();
    assert!(!tui.has_modal());
}

#[test]
fn test_account_setup_modal() {
    let mut tui = TestTui::with_account_setup();
    tui.assert_has_modal();
    tui.assert_modal(|modal| matches!(modal, QueuedModal::AccountSetup(_)));

    tui.type_text("MyAccount");
    tui.clear_commands();
    tui.send_enter();

    tui.assert_dispatch(
        |d| matches!(d, DispatchCommand::CreateAccount { name } if name == "MyAccount"),
    );
}

#[test]
fn test_modal_homes_screen_navigation() {
    let mut tui = TestTui::new();
    tui.send_char('?');
    tui.assert_has_modal();

    let screen_before = tui.screen();
    for key in ['1', '2', '3', '4', '5', '6', '7'] {
        tui.send_char(key);
        assert_eq!(tui.screen(), screen_before);
        tui.assert_has_modal();
    }

    tui.send_tab();
    assert_eq!(tui.screen(), screen_before);
}

#[test]
fn test_modal_escape_always_closes() {
    let mut tui = TestTui::new();
    let modal_openers = vec![
        (Screen::Neighborhood, '?'),
        (Screen::Chat, 'n'),
        (Screen::Chat, 't'),
        (Screen::Contacts, 'e'),
    ];

    for (screen, key) in modal_openers {
        tui.go_to_screen(screen);
        tui.send_char(key);

        if tui.has_modal() {
            tui.send_escape();
            assert!(!tui.has_modal(), "Modal should close with Escape");
        }
    }
}

#[test]
fn test_text_input_modal_validation() {
    let mut tui = TestTui::new();
    tui.go_to_screen(Screen::Chat);
    tui.send_char('n');
    tui.assert_dispatch(|d| matches!(d, DispatchCommand::OpenChatCreateWizard));
    tui.state
        .modal_queue
        .enqueue(QueuedModal::ChatCreate(CreateChannelModalState::new()));
    tui.assert_has_modal();

    tui.clear_commands();
    tui.send_enter();
}

#[test]
fn test_guardian_select_modal() {
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

    if let Some(QueuedModal::GuardianSetup(state)) = tui.state.modal_queue.current() {
        assert_eq!(state.focused_index(), 0);
    }
    tui.state.modal_queue.update_active(|modal| {
        if let QueuedModal::GuardianSetup(state) = modal {
            state.move_focus_down();
        }
    });
    if let Some(QueuedModal::GuardianSetup(state)) = tui.state.modal_queue.current() {
        assert_eq!(state.focused_index(), 1);
    }
}
