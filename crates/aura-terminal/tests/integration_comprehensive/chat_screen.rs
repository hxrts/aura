use super::*;

#[test]
fn test_chat_focus_navigation() {
    let mut tui = TestTui::new();
    tui.go_to_screen(Screen::Chat);

    assert_eq!(tui.state.chat.focus, ChatFocus::Channels);
    tui.send_char('l');
    assert_eq!(tui.state.chat.focus, ChatFocus::Messages);
    tui.send_char('h');
    assert_eq!(tui.state.chat.focus, ChatFocus::Channels);
}

#[test]
fn test_chat_channel_selection() {
    let mut tui = TestTui::new();
    tui.go_to_screen(Screen::Chat);
    tui.state.chat.channel_count = 10;
    tui.state.chat.message_count = 50;

    let initial = tui.state.chat.selected_channel;
    tui.send_char('j');
    assert_eq!(tui.state.chat.selected_channel, initial + 1);

    tui.send_char('k');
    assert_eq!(tui.state.chat.selected_channel, initial);
}

#[test]
fn test_chat_message_scroll() {
    let mut tui = TestTui::new();
    tui.go_to_screen(Screen::Chat);
    tui.state.chat.channel_count = 10;
    tui.state.chat.message_count = 50;

    tui.send_char('l');
    assert_eq!(tui.state.chat.focus, ChatFocus::Messages);

    let initial = tui.state.chat.message_scroll;
    tui.send_char('k');
    assert_eq!(tui.state.chat.message_scroll, initial + 1);

    tui.send_char('j');
    assert_eq!(tui.state.chat.message_scroll, initial);
}

#[test]
fn test_chat_insert_mode() {
    let mut tui = TestTui::new();
    tui.go_to_screen(Screen::Chat);

    tui.send_char('i');
    assert!(tui.is_insert_mode());
    assert_eq!(tui.state.chat.focus, ChatFocus::Input);

    tui.type_text("Hello, Chat!");
    tui.clear_commands();
    tui.send_enter();

    tui.assert_dispatch(
        |d| matches!(d, DispatchCommand::SendChatMessage { content, .. } if content == "Hello, Chat!"),
    );
}

#[test]
fn test_chat_create_channel_modal() {
    let mut tui = TestTui::new();
    tui.go_to_screen(Screen::Chat);

    tui.send_char('n');
    tui.assert_dispatch(|d| matches!(d, DispatchCommand::OpenChatCreateWizard));
    tui.state
        .modal_queue
        .enqueue(QueuedModal::ChatCreate(CreateChannelModalState::new()));
    tui.assert_has_modal();

    tui.type_text("general");
    assert_eq!(tui.state.chat_create_modal_state().unwrap().name, "general");

    tui.state.modal_queue.update_active(|modal| {
        if let QueuedModal::ChatCreate(ref mut state) = modal {
            state.step = CreateChannelStep::Threshold;
        }
    });

    tui.clear_commands();
    tui.send_enter();

    tui.assert_dispatch(
        |d| matches!(d, DispatchCommand::CreateChannel { name, .. } if name == "general"),
    );
    assert!(!tui.has_modal());
}

#[test]
fn test_chat_set_topic_modal() {
    let mut tui = TestTui::new();
    tui.go_to_screen(Screen::Chat);

    tui.send_char('t');
    tui.assert_dispatch(|d| matches!(d, DispatchCommand::OpenChatTopicModal));

    tui.state
        .modal_queue
        .enqueue(QueuedModal::ChatTopic(TopicModalState::for_channel(
            "ch-123", "",
        )));

    tui.assert_has_modal();
    assert!(tui.state.is_chat_topic_modal_active());

    tui.type_text("Welcome to the channel!");
    assert_eq!(
        tui.state.chat_topic_modal_state().unwrap().value,
        "Welcome to the channel!"
    );

    tui.clear_commands();
    tui.send_enter();

    tui.assert_dispatch(|d| {
        matches!(
            d,
            DispatchCommand::SetChannelTopic { channel_id, topic }
                if channel_id == "ch-123" && topic == "Welcome to the channel!"
        )
    });
}

#[test]
fn test_chat_channel_info_modal() {
    let mut tui = TestTui::new();
    tui.go_to_screen(Screen::Chat);

    tui.send_char('o');
    tui.assert_dispatch(|d| matches!(d, DispatchCommand::OpenChatInfoModal));

    tui.state
        .modal_queue
        .enqueue(QueuedModal::ChatInfo(ChannelInfoModalState::for_channel(
            "ch-123",
            "info-channel",
            None,
        )));

    tui.assert_has_modal();
    assert!(tui.state.is_chat_info_modal_active());
    tui.send_escape();
    assert!(!tui.state.is_chat_info_modal_active());
}

#[test]
fn test_chat_retry_message() {
    let mut tui = TestTui::new();
    tui.go_to_screen(Screen::Chat);
    tui.send_char('l');
    tui.clear_commands();
    tui.send_char('r');
    tui.assert_dispatch(|d| matches!(d, DispatchCommand::RetryMessage));
}
