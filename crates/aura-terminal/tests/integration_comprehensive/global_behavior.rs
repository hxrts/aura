use super::*;

#[test]
fn test_quit_from_any_screen() {
    for screen in [
        Screen::Neighborhood,
        Screen::Chat,
        Screen::Contacts,
        Screen::Neighborhood,
        Screen::Settings,
        Screen::Notifications,
    ] {
        let mut tui = TestTui::new();
        tui.go_to_screen(screen);
        tui.send_char('q');

        assert!(tui.state.should_exit, "q should quit from {screen:?}");
        assert!(tui.has_exit());
    }
}

#[test]
fn test_quit_homeed_in_insert_mode() {
    let mut tui = TestTui::new();
    tui.go_to_screen(Screen::Chat);
    tui.send_char('i');
    assert!(tui.is_insert_mode());

    tui.send_char('q');
    assert!(!tui.state.should_exit);
    assert_eq!(tui.state.chat.input_buffer, "q");
}

#[test]
fn test_quit_homeed_in_modal() {
    let mut tui = TestTui::new();
    tui.send_char('?');
    tui.assert_has_modal();
    tui.send_char('q');
    assert!(!tui.state.should_exit);
}

#[test]
fn test_help_from_any_screen() {
    for screen in [
        Screen::Neighborhood,
        Screen::Chat,
        Screen::Contacts,
        Screen::Neighborhood,
        Screen::Settings,
        Screen::Notifications,
    ] {
        let mut tui = TestTui::new();
        tui.go_to_screen(screen);

        tui.send_char('?');
        tui.assert_has_modal();
        tui.assert_modal(|modal| matches!(modal, QueuedModal::Help { .. }));
        tui.send_escape();
    }
}

#[test]
fn test_resize_preserves_state() {
    let mut tui = TestTui::new();
    tui.go_to_screen(Screen::Chat);
    tui.send_char('i');
    tui.type_text("Hello");

    let screen_before = tui.screen();
    let buffer_before = tui.state.chat.input_buffer.clone();
    let insert_before = tui.is_insert_mode();

    tui.send(events::resize(120, 40));

    assert_eq!(tui.screen(), screen_before);
    assert_eq!(tui.state.chat.input_buffer, buffer_before);
    assert_eq!(tui.is_insert_mode(), insert_before);
    assert_eq!(tui.state.terminal_size, (120, 40));
}
