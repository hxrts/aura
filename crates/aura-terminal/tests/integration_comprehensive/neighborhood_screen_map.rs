use super::*;

#[test]
fn test_neighborhood_insert_mode_entry() {
    let mut tui = TestTui::new();
    tui.assert_screen(Screen::Neighborhood);

    tui.send_enter();
    tui.send_char('i');
    assert!(!tui.is_insert_mode());
    assert_eq!(tui.state.neighborhood.detail_focus, DetailFocus::Channels);
}

#[test]
fn test_neighborhood_enter_does_not_send_message() {
    let mut tui = TestTui::new();
    tui.send_enter();
    tui.clear_commands();
    tui.send_enter();

    assert!(!tui.has_dispatch(|_| true));
}

#[test]
fn test_neighborhood_empty_message_not_sent() {
    let mut tui = TestTui::new();
    tui.send_enter();
    tui.clear_commands();
    tui.send_enter();

    assert!(!tui.has_dispatch(|_| true));
}

#[test]
fn test_neighborhood_member_navigation() {
    let mut tui = TestTui::new();
    tui.state.neighborhood.home_count = 1;
    tui.send_enter();
    tui.state.neighborhood.detail_focus = DetailFocus::Members;
    tui.state.neighborhood.member_count = 10;

    let initial = tui.state.neighborhood.selected_member;
    tui.send_char('j');
    assert_eq!(tui.state.neighborhood.selected_member, initial + 1);

    tui.send_char('k');
    assert_eq!(tui.state.neighborhood.selected_member, initial);
}

#[test]
fn test_neighborhood_backspace_in_detail_mode() {
    let mut tui = TestTui::new();
    tui.send_enter();
    tui.clear_commands();
    tui.send_backspace();
    tui.send_backspace();
    assert!(!tui.has_dispatch(|d| matches!(d, DispatchCommand::SendChatMessage { .. })));
}
