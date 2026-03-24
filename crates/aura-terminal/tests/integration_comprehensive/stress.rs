use super::*;

#[test]
fn test_rapid_screen_switching() {
    let mut tui = TestTui::new();

    for i in 0..10000 {
        let key = char::from_digit(((i % 5) + 1) as u32, 10).unwrap();
        tui.send_char(key);
    }

    assert!(matches!(
        tui.screen(),
        Screen::Neighborhood
            | Screen::Chat
            | Screen::Contacts
            | Screen::Notifications
            | Screen::Settings
    ));
}

#[test]
fn test_rapid_insert_mode_toggle() {
    let mut tui = TestTui::new();
    tui.go_to_screen(Screen::Chat);

    for _ in 0..10000 {
        tui.send_char('i');
        tui.send_escape();
    }

    assert!(!tui.is_insert_mode());
}

#[test]
fn test_very_long_input() {
    let mut tui = TestTui::new();
    tui.go_to_screen(Screen::Chat);
    tui.send_char('i');

    let long_text = "a".repeat(100_000);
    for c in long_text.chars() {
        tui.send_char(c);
    }

    assert_eq!(tui.state.chat.input_buffer.len(), 100_000);
}

#[test]
fn test_many_modal_opens_closes() {
    let mut tui = TestTui::new();

    for _ in 0..1000 {
        tui.send_char('?');
        tui.send_escape();
    }

    assert!(!tui.has_modal());
}

#[test]
fn test_mixed_rapid_operations() {
    let mut tui = TestTui::new();

    for i in 0..1000 {
        match i % 10 {
            0 => tui.send_char('i'),
            1 => tui.send_escape(),
            2 => tui.send_char('j'),
            3 => tui.send_char('k'),
            4 => tui.send_tab(),
            5 => tui.send_char('?'),
            6 => tui.send_escape(),
            7 => tui.send(events::resize(80 + (i % 100) as u16, 24)),
            8 => tui.send_char((b'1' + (i % 5) as u8) as char),
            _ => tui.send_char('h'),
        }
    }
}
