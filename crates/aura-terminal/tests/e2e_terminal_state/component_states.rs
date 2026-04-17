use assert_matches::assert_matches;

use aura_terminal::tui::components::{AccountSetupState, ContactSelectState, TextInputState};
use aura_terminal::tui::effects::EffectCommand;
use aura_terminal::tui::screens::Screen;
use aura_terminal::tui::screens::{ChatCreateState, InvitationCodeState, InvitationImportState};
use aura_terminal::tui::types::{Contact, ContactStatus};

#[test]
fn test_account_setup_state_machine() {
    let mut state = AccountSetupState::new();
    assert!(!state.visible);
    assert!(state.nickname_suggestion.is_empty());
    assert!(state.error.is_none());

    state.show();
    assert!(state.visible);
    state.set_nickname_suggestion("Bob".to_string());
    assert_eq!(state.nickname_suggestion, "Bob");
    assert!(state.can_submit());

    state.set_nickname_suggestion("".to_string());
    assert!(!state.can_submit());
    state.hide();
    assert!(!state.visible);
}

#[test]
fn test_contact_select_state_machine() {
    let mut state = ContactSelectState::new();
    assert!(!state.visible);
    assert!(state.contacts.is_empty());
    assert!(!state.can_select());

    let contacts = vec![
        Contact::new("alice", "Alice").with_status(ContactStatus::Active),
        Contact::new("bob", "Bob").with_status(ContactStatus::Active),
        Contact::new("carol", "Carol").with_status(ContactStatus::Active),
    ];

    state.show("Select Guardian", contacts);
    assert!(state.visible);
    assert_eq!(state.contacts.len(), 3);
    assert_eq!(state.selected_index, 0);
    assert!(state.can_select());
    assert_eq!(state.get_selected_id(), Some("alice".to_string()));

    state.select_next();
    assert_eq!(state.selected_index, 1);
    assert_eq!(state.get_selected_id(), Some("bob".to_string()));

    state.select_next();
    assert_eq!(state.selected_index, 2);
    state.select_next();
    assert_eq!(state.selected_index, 2);

    state.select_prev();
    assert_eq!(state.selected_index, 1);
    state.hide();
    assert!(!state.visible);
    assert!(state.contacts.is_empty());
}

#[test]
fn test_screen_enum() {
    let screens = Screen::all();
    assert_eq!(screens.len(), 5);
    assert_eq!(Screen::Neighborhood.key_number(), 1);
    assert_eq!(Screen::Chat.key_number(), 2);
    assert_eq!(Screen::Contacts.key_number(), 3);
    assert_eq!(Screen::Notifications.key_number(), 4);
    assert_eq!(Screen::Settings.key_number(), 5);
    assert_eq!(Screen::from_key(1), Some(Screen::Neighborhood));
    assert_eq!(Screen::from_key(2), Some(Screen::Chat));
    assert_eq!(Screen::from_key(3), Some(Screen::Contacts));
    assert_eq!(Screen::from_key(4), Some(Screen::Notifications));
    assert_eq!(Screen::from_key(5), Some(Screen::Settings));
    assert_eq!(Screen::from_key(6), None);
    assert_eq!(Screen::from_key(0), None);
    assert_eq!(Screen::Neighborhood.next(), Screen::Chat);
    assert_eq!(Screen::Chat.prev(), Screen::Neighborhood);
    assert_eq!(Screen::Settings.next(), Screen::Neighborhood);
    assert_eq!(Screen::default(), Screen::Neighborhood);
}

#[test]
fn test_effect_commands() {
    let cmd = EffectCommand::CreateAccount {
        nickname_suggestion: "Bob".to_string(),
    };
    assert_matches!(cmd, EffectCommand::CreateAccount { nickname_suggestion } if nickname_suggestion == "Bob");

    let cmd = EffectCommand::SendMessage {
        channel: "general".to_string(),
        content: "Hello!".to_string(),
    };
    assert_matches!(cmd, EffectCommand::SendMessage { channel, content } if channel == "general" && content == "Hello!");

    let cmd = EffectCommand::CreateInvitation {
        receiver_id: Some(aura_core::AuthorityId::new_from_entropy([21u8; 32])),
        invitation_type: "Guardian".to_string(),
        nickname: Some("Guardian Ops".to_string()),
        message: Some("Be my guardian".to_string()),
        ttl_secs: Some(3600),
        operation_instance_id: None,
    };
    assert_matches!(
        cmd,
        EffectCommand::CreateInvitation {
            receiver_id,
            invitation_type,
            nickname,
            message,
            ttl_secs,
            operation_instance_id,
        }
            if receiver_id == Some(aura_core::AuthorityId::new_from_entropy([21u8; 32]))
                && invitation_type == "Guardian"
                && nickname == Some("Guardian Ops".to_string())
                && message == Some("Be my guardian".to_string())
                && ttl_secs == Some(3600)
                && operation_instance_id.is_none()
    );
}

#[test]
fn test_chat_create_state_machine() {
    let mut state = ChatCreateState::new();
    assert!(!state.visible);
    assert!(state.name.is_empty());
    state.show();
    assert!(state.visible);
    for c in "Test Channel".chars() {
        state.push_char(c);
    }
    assert_eq!(state.name, "Test Channel");
    assert!(state.can_submit());
    state.name.clear();
    assert!(!state.can_submit());
    state.hide();
    assert!(!state.visible);
}

#[test]
fn test_invitation_code_state_machine() {
    let mut state = InvitationCodeState::new();
    assert!(!state.visible);
    assert!(state.code.is_empty());
    state.show(
        "inv_123".to_string(),
        "Guardian".to_string(),
        "aura://invite/xyz".to_string(),
    );
    assert!(state.visible);
    assert_eq!(state.invitation_id, "inv_123");
    assert_eq!(state.invitation_type, "Guardian");
    assert_eq!(state.code, "aura://invite/xyz");
    state.hide();
    assert!(!state.visible);
    assert!(state.code.is_empty());
}

#[test]
fn test_invitation_import_state_machine() {
    let mut state = InvitationImportState::new();
    assert!(!state.visible);
    assert!(state.code.is_empty());
    state.show();
    assert!(state.visible);
    assert!(!state.can_submit());
    state.set_code("aura://invite/abc123".to_string());
    assert!(state.can_submit());
    state.set_code("".to_string());
    assert!(!state.can_submit());
    state.hide();
    assert!(!state.visible);
}

#[test]
fn test_text_input_state_machine() {
    let mut state = TextInputState::new();
    assert!(!state.visible);
    assert!(state.value.is_empty());
    state.show(
        "Edit Nickname",
        "Alice",
        "Enter name",
        Some("contact_alice".to_string()),
    );
    assert!(state.visible);
    assert_eq!(state.title, "Edit Nickname");
    assert_eq!(state.value, "Alice");
    assert_eq!(state.placeholder, "Enter name");
    assert_eq!(state.context_id, Some("contact_alice".to_string()));
    state.push_char('!');
    assert_eq!(state.value, "Alice!");
    state.pop_char();
    assert_eq!(state.value, "Alice");
    state.hide();
    assert!(!state.visible);
}
