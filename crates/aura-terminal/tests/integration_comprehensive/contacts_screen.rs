use super::*;

#[test]
fn test_contacts_list_navigation() {
    let mut tui = TestTui::new();
    tui.go_to_screen(Screen::Contacts);
    tui.state.contacts.contact_count = 10;

    let initial = tui.state.contacts.selected_index;
    tui.send_char('j');
    assert_eq!(tui.state.contacts.selected_index, initial + 1);

    tui.send_char('k');
    assert_eq!(tui.state.contacts.selected_index, initial);
}

#[test]
fn test_contacts_edit_nickname() {
    let mut tui = TestTui::new();
    tui.go_to_screen(Screen::Contacts);

    tui.send_char('e');
    tui.assert_dispatch(|d| matches!(d, DispatchCommand::OpenContactNicknameModal));

    use aura_terminal::tui::state::NicknameModalState;
    tui.state.modal_queue.enqueue(QueuedModal::ContactsNickname(
        NicknameModalState::for_contact("contact-123", ""),
    ));
    tui.assert_has_modal();

    tui.type_text("Alice");
    tui.clear_commands();
    tui.send_enter();

    tui.assert_dispatch(
        |d| matches!(d, DispatchCommand::UpdateNickname { nickname, .. } if nickname == "Alice"),
    );
    assert!(!tui.has_modal());
}

#[test]
fn test_contacts_open_guardian_setup_modal() {
    let mut tui = TestTui::new();
    tui.go_to_screen(Screen::Contacts);
    tui.clear_commands();
    tui.send_char('g');

    tui.assert_dispatch(|d| matches!(d, DispatchCommand::OpenGuardianSetup));
}

#[test]
fn test_contacts_start_chat() {
    let mut tui = TestTui::new();
    tui.go_to_screen(Screen::Contacts);
    tui.clear_commands();
    tui.send_char('c');

    tui.assert_dispatch(|d| matches!(d, DispatchCommand::StartChat { .. }));
}

#[test]
fn test_contacts_navigation_saturates_at_zero() {
    let mut tui = TestTui::new();
    tui.go_to_screen(Screen::Contacts);

    for _ in 0..50 {
        tui.send_char('k');
    }
    assert_eq!(tui.state.contacts.selected_index, 0);
}
