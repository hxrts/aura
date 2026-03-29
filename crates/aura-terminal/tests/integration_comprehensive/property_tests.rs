use super::*;

proptest! {
    #[test]
    fn prop_tab_cycle(start in 1u8..=5) {
        let mut tui = TestTui::new();
        tui.send_char(char::from_digit(start as u32, 10).unwrap());
        let initial = tui.screen();

        for _ in 0..5 {
            tui.send_tab();
        }

        prop_assert_eq!(tui.screen(), initial);
    }

    #[test]
    fn prop_screen_nav_works(key in screen_key_strategy()) {
        let mut tui = TestTui::new();
        tui.send_tab();
        tui.send_tab();
        tui.send_char(key);

        let expected = match key {
            '1' => Screen::Neighborhood,
            '2' => Screen::Chat,
            '3' => Screen::Contacts,
            '4' => Screen::Notifications,
            '5' => Screen::Settings,
            _ => unreachable!(),
        };

        prop_assert_eq!(tui.screen(), expected);
    }

    #[test]
    fn prop_modal_homes_nav(nav_keys in prop::collection::vec(screen_key_strategy(), 1..10)) {
        let mut tui = TestTui::new();
        let screen_before = tui.screen();

        tui.send_char('?');
        prop_assert!(tui.has_modal());

        for key in nav_keys {
            tui.send_char(key);
        }

        prop_assert_eq!(tui.screen(), screen_before);
        prop_assert!(tui.has_modal());
    }

    #[test]
    fn prop_escape_exits_insert(chars in prop::collection::vec(any::<char>().prop_filter("printable", |c| c.is_ascii_graphic()), 0..50)) {
        let mut tui = TestTui::new();

        if tui.screen() == Screen::Neighborhood {
            tui.state.neighborhood.home_count = 1;
            tui.send_enter();
        }
        tui.send_char('i');
        prop_assert!(tui.is_insert_mode());

        for c in chars {
            tui.send_char(c);
        }

        tui.send_escape();
        prop_assert!(!tui.is_insert_mode());
    }

    #[test]
    fn prop_no_panics(events in prop::collection::vec(terminal_event_strategy(), 0..200)) {
        let mut tui = TestTui::new();

        for event in events {
            tui.send(event);
        }

        let _ = tui.screen();
        let _ = tui.is_insert_mode();
        let _ = tui.has_modal();
    }

    #[test]
    fn prop_deterministic(events in prop::collection::vec(terminal_event_strategy(), 1..50)) {
        let mut tui1 = TestTui::new();
        let mut tui2 = TestTui::new();

        for event in &events {
            tui1.send(event.clone());
            tui2.send(event.clone());
        }

        prop_assert_eq!(tui1.screen(), tui2.screen());
        prop_assert_eq!(tui1.is_insert_mode(), tui2.is_insert_mode());
        prop_assert_eq!(tui1.has_modal(), tui2.has_modal());
        prop_assert_eq!(tui1.state.terminal_size, tui2.state.terminal_size);
    }

    #[test]
    fn prop_indices_non_negative(k_presses in 0usize..100) {
        let mut tui = TestTui::new();

        for screen in ['3', '4', '5'] {
            tui.send_char(screen);
            for _ in 0..k_presses {
                tui.send_char('k');
            }
        }
    }

    #[test]
    fn prop_insert_mode_screens(screen in screen_key_strategy()) {
        let mut tui = TestTui::new();
        tui.send_char(screen);
        tui.send_char('i');

        let should_be_insert = matches!(tui.screen(), Screen::Chat);
        prop_assert_eq!(tui.is_insert_mode(), should_be_insert);
    }

    #[test]
    fn prop_help_from_anywhere(screen in screen_key_strategy()) {
        let mut tui = TestTui::new();
        tui.send_char(screen);
        tui.send_char('?');

        prop_assert!(tui.has_modal());
        let is_help_modal = tui.current_modal().is_some_and(|modal| match modal {
            QueuedModal::Help { .. } => true,
            _ => false,
        });
        prop_assert!(is_help_modal);
    }

    #[test]
    fn prop_resize_updates(width in 10u16..500, height in 10u16..200) {
        let mut tui = TestTui::new();
        tui.send(events::resize(width, height));

        prop_assert_eq!(tui.state.terminal_size, (width, height));
    }
}
