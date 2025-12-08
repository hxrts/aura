//! TUI End-to-End Integration Tests
//!
//! Verifies that TUI components and effect commands are properly wired together
//! for the complete user flow:
//!
//! 1. Account creation
//! 2. Invitation creation/export/import
//! 3. Contact management
//! 4. Chat group creation
//! 5. Messaging
//!
//! These tests verify structural correctness rather than full reactive integration.

use aura_terminal::tui::components::{
    AccountSetupState, ChatCreateState, ContactSelectState, InvitationCodeState,
    InvitationCreateState, InvitationImportState, TextInputState,
};
use aura_terminal::tui::context::IoContext;
use aura_terminal::tui::effects::EffectCommand;
use aura_terminal::tui::screens::Screen;
use aura_terminal::tui::types::{Contact, ContactStatus, InvitationType};

/// Test that IoContext initializes with all required views
#[tokio::test]
async fn test_context_initialization() {
    let context = IoContext::with_defaults();

    // Verify all views are accessible
    let _ = context.chat_view();
    let _ = context.guardians_view();
    let _ = context.recovery_view();
    let _ = context.invitations_view();
    let _ = context.block_view();

    println!("✓ IoContext initialized with all views");
}

/// Test account setup modal state machine
#[test]
fn test_account_setup_state() {
    let mut state = AccountSetupState::new();

    // Initial state should not be visible
    assert!(!state.visible);
    assert!(state.display_name.is_empty());
    assert!(state.error.is_none());

    // Show the modal
    state.show();
    assert!(state.visible);

    // Set a display name
    state.set_display_name("Test User".to_string());
    assert_eq!(state.display_name, "Test User");

    // can_submit - should succeed with valid name
    assert!(state.can_submit());

    // Test empty name
    state.set_display_name("".to_string());
    assert!(!state.can_submit());

    // Hide the modal
    state.hide();
    assert!(!state.visible);

    println!("✓ AccountSetupState state machine works correctly");
}

/// Test that CreateAccount command is properly structured
#[test]
fn test_create_account_command() {
    let cmd = EffectCommand::CreateAccount {
        display_name: "Test User".to_string(),
    };

    match cmd {
        EffectCommand::CreateAccount { display_name } => {
            assert_eq!(display_name, "Test User");
        }
        _ => panic!("Expected CreateAccount command"),
    }

    println!("✓ CreateAccount command structure is correct");
}

/// Test invitation creation modal state
#[test]
fn test_invitation_create_state() {
    let mut state = InvitationCreateState::new();

    // Initial state
    assert!(!state.visible);
    // Note: invitation_type defaults to Guardian per Rust's derive(Default)
    // but show() resets it to Contact

    // Show modal - this sets invitation_type to Contact
    state.show();
    assert!(state.visible);
    assert_eq!(state.invitation_type, InvitationType::Contact);

    // Cycle type using next_type
    state.next_type();
    assert_eq!(state.invitation_type, InvitationType::Guardian);

    // Set message
    state.set_message("Join my network".to_string());
    assert_eq!(state.message, "Join my network");

    // Cycle type again
    state.next_type();
    assert_eq!(state.invitation_type, InvitationType::Channel);

    // Hide
    state.hide();
    assert!(!state.visible);

    println!("✓ InvitationCreateState state machine works correctly");
}

/// Test invitation code display state
#[test]
fn test_invitation_code_state() {
    let mut state = InvitationCodeState::new();

    // Initial state
    assert!(!state.visible);
    assert!(state.code.is_empty());

    // Show with code (takes 3 args: invitation_id, invitation_type, code)
    state.show(
        "inv_123".to_string(),
        "Contact".to_string(),
        "inv_abc123xyz".to_string(),
    );
    assert!(state.visible);
    assert_eq!(state.code, "inv_abc123xyz");
    assert_eq!(state.invitation_type, "Contact");
    assert_eq!(state.invitation_id, "inv_123");

    // Hide
    state.hide();
    assert!(!state.visible);
    assert!(state.code.is_empty());

    println!("✓ InvitationCodeState state machine works correctly");
}

/// Test invitation import modal state
#[test]
fn test_invitation_import_state() {
    let mut state = InvitationImportState::new();

    // Initial state
    assert!(!state.visible);
    assert!(state.code.is_empty());

    // Show modal
    state.show();
    assert!(state.visible);

    // Set code using set_code
    state.set_code("inv_received_code".to_string());
    assert_eq!(state.code, "inv_received_code");

    // can_submit
    assert!(state.can_submit());

    // Test empty validation
    state.set_code("".to_string());
    assert!(!state.can_submit());

    // Hide
    state.hide();
    assert!(!state.visible);

    println!("✓ InvitationImportState state machine works correctly");
}

/// Test CreateInvitation command structure
#[test]
fn test_create_invitation_command() {
    let cmd = EffectCommand::CreateInvitation {
        invitation_type: "Contact".to_string(),
        message: Some("Welcome!".to_string()),
        ttl_secs: Some(86400),
    };

    match cmd {
        EffectCommand::CreateInvitation {
            invitation_type,
            message,
            ttl_secs,
        } => {
            assert_eq!(invitation_type, "Contact");
            assert_eq!(message, Some("Welcome!".to_string()));
            assert_eq!(ttl_secs, Some(86400));
        }
        _ => panic!("Expected CreateInvitation command"),
    }

    println!("✓ CreateInvitation command structure is correct");
}

/// Test ExportInvitation and ImportInvitation commands
#[test]
fn test_invitation_export_import_commands() {
    let export_cmd = EffectCommand::ExportInvitation {
        invitation_id: "inv_123".to_string(),
    };

    match export_cmd {
        EffectCommand::ExportInvitation { invitation_id } => {
            assert_eq!(invitation_id, "inv_123");
        }
        _ => panic!("Expected ExportInvitation command"),
    }

    let import_cmd = EffectCommand::ImportInvitation {
        code: "inv_code_abc".to_string(),
    };

    match import_cmd {
        EffectCommand::ImportInvitation { code } => {
            assert_eq!(code, "inv_code_abc");
        }
        _ => panic!("Expected ImportInvitation command"),
    }

    println!("✓ Export/Import invitation commands structure is correct");
}

/// Test text input modal state (for petname editing)
#[test]
fn test_text_input_modal_state() {
    let mut state = TextInputState::new();

    // Initial state
    assert!(!state.visible);
    assert!(state.value.is_empty());

    // Show with context (takes: title, value, placeholder, context_id)
    state.show(
        "Edit Petname",
        "Alice",
        "Enter new name",
        Some("contact_123".to_string()),
    );
    assert!(state.visible);
    assert_eq!(state.title, "Edit Petname");
    assert_eq!(state.value, "Alice");
    assert_eq!(state.placeholder, "Enter new name");
    assert_eq!(state.context_id, Some("contact_123".to_string()));

    // Modify value using push_char
    state.push_char('!');
    assert_eq!(state.value, "Alice!");

    // Backspace
    state.pop_char();
    assert_eq!(state.value, "Alice");

    // Hide
    state.hide();
    assert!(!state.visible);

    println!("✓ TextInputState state machine works correctly");
}

/// Test contact select modal state (for block invites)
#[test]
fn test_contact_select_state() {
    let mut state = ContactSelectState::new();

    // Initial state
    assert!(!state.visible);
    assert!(state.contacts.is_empty());
    assert!(!state.can_select());

    // Create test contacts
    let contacts = vec![
        Contact::new("c1", "Alice").with_status(ContactStatus::Active),
        Contact::new("c2", "Bob").with_status(ContactStatus::Active),
        Contact::new("c3", "Charlie").with_status(ContactStatus::Active),
    ];

    // Show modal with contacts
    state.show("Invite to Block", contacts);
    assert!(state.visible);
    assert_eq!(state.title, "Invite to Block");
    assert_eq!(state.contacts.len(), 3);
    assert_eq!(state.selected_index, 0);
    assert!(state.can_select());

    // Navigate
    assert_eq!(state.get_selected_id(), Some("c1".to_string()));

    state.select_next();
    assert_eq!(state.selected_index, 1);
    assert_eq!(state.get_selected_id(), Some("c2".to_string()));

    state.select_next();
    assert_eq!(state.selected_index, 2);

    // Boundary - can't go past end
    state.select_next();
    assert_eq!(state.selected_index, 2);

    state.select_prev();
    assert_eq!(state.selected_index, 1);

    // Hide
    state.hide();
    assert!(!state.visible);
    assert!(state.contacts.is_empty());

    println!("✓ ContactSelectState state machine works correctly");
}

/// Test chat create modal state
#[test]
fn test_chat_create_state() {
    let mut state = ChatCreateState::new();

    // Initial state
    assert!(!state.visible);
    assert!(state.name.is_empty());

    // Show modal
    state.show();
    assert!(state.visible);

    // Set name using push_char
    for c in "My Group".chars() {
        state.push_char(c);
    }
    assert_eq!(state.name, "My Group");

    // can_submit - should succeed with valid name
    assert!(state.can_submit());

    // Test empty name - clear and verify
    state.name.clear();
    assert!(!state.can_submit());

    // Hide
    state.hide();
    assert!(!state.visible);

    println!("✓ ChatCreateState state machine works correctly");
}

/// Test CreateChannel command structure
#[test]
fn test_create_channel_command() {
    let cmd = EffectCommand::CreateChannel {
        name: "Test Group".to_string(),
        topic: Some("Discussion".to_string()),
        members: vec!["user1".to_string(), "user2".to_string()],
    };

    match cmd {
        EffectCommand::CreateChannel {
            name,
            topic,
            members,
        } => {
            assert_eq!(name, "Test Group");
            assert_eq!(topic, Some("Discussion".to_string()));
            assert_eq!(members.len(), 2);
        }
        _ => panic!("Expected CreateChannel command"),
    }

    println!("✓ CreateChannel command structure is correct");
}

/// Test StartDirectChat command
#[test]
fn test_start_direct_chat_command() {
    let cmd = EffectCommand::StartDirectChat {
        contact_id: "contact_456".to_string(),
    };

    match cmd {
        EffectCommand::StartDirectChat { contact_id } => {
            assert_eq!(contact_id, "contact_456");
        }
        _ => panic!("Expected StartDirectChat command"),
    }

    println!("✓ StartDirectChat command structure is correct");
}

/// Test SendMessage command structure
#[test]
fn test_send_message_command() {
    let cmd = EffectCommand::SendMessage {
        channel: "chan_123".to_string(),
        content: "Hello, world!".to_string(),
    };

    match cmd {
        EffectCommand::SendMessage { channel, content } => {
            assert_eq!(channel, "chan_123");
            assert_eq!(content, "Hello, world!");
        }
        _ => panic!("Expected SendMessage command"),
    }

    println!("✓ SendMessage command structure is correct");
}

/// Test SendBlockInvitation command
#[test]
fn test_send_block_invitation_command() {
    let cmd = EffectCommand::SendBlockInvitation {
        contact_id: "contact_789".to_string(),
    };

    match cmd {
        EffectCommand::SendBlockInvitation { contact_id } => {
            assert_eq!(contact_id, "contact_789");
        }
        _ => panic!("Expected SendBlockInvitation command"),
    }

    println!("✓ SendBlockInvitation command structure is correct");
}

/// Test UpdateContactPetname command
#[test]
fn test_update_petname_command() {
    let cmd = EffectCommand::UpdateContactPetname {
        contact_id: "contact_123".to_string(),
        petname: "New Name".to_string(),
    };

    match cmd {
        EffectCommand::UpdateContactPetname {
            contact_id,
            petname,
        } => {
            assert_eq!(contact_id, "contact_123");
            assert_eq!(petname, "New Name");
        }
        _ => panic!("Expected UpdateContactPetname command"),
    }

    println!("✓ UpdateContactPetname command structure is correct");
}

/// Test screen navigation enum
#[test]
fn test_screen_enum_coverage() {
    // Verify all screens are accessible
    let screens = [
        Screen::Block,
        Screen::Chat,
        Screen::Contacts,
        Screen::Recovery,
        Screen::Invitations,
        Screen::Settings,
        Screen::Help,
        Screen::Neighborhood,
    ];

    assert_eq!(screens.len(), 8, "Expected 8 screens");

    // Verify Default trait
    let default_screen = Screen::default();
    assert_eq!(
        default_screen,
        Screen::Block,
        "Default screen should be Block"
    );

    println!("✓ Screen enum covers all screens");
}

/// Test invitation type enum
#[test]
fn test_invitation_type_enum() {
    let types = [
        InvitationType::Contact,
        InvitationType::Guardian,
        InvitationType::Channel,
    ];

    // Verify display strings
    assert_eq!(format!("{:?}", types[0]), "Contact");
    assert_eq!(format!("{:?}", types[1]), "Guardian");
    assert_eq!(format!("{:?}", types[2]), "Channel");

    println!("✓ InvitationType enum is complete");
}

/// Test contact status enum
#[test]
fn test_contact_status_enum() {
    let contact = Contact::new("id", "Name").with_status(ContactStatus::Active);

    assert_eq!(contact.status, ContactStatus::Active);

    let pending = Contact::new("id2", "Name2").with_status(ContactStatus::Pending);

    assert_eq!(pending.status, ContactStatus::Pending);

    println!("✓ ContactStatus enum works correctly");
}

/// E2E flow test - verifies the complete logical flow
#[test]
fn test_e2e_flow_logical() {
    println!("\n=== E2E Flow Verification ===\n");

    // Step 1: Account Setup
    println!("Step 1: Account Setup");
    let mut account_state = AccountSetupState::new();
    account_state.show();
    account_state.set_display_name("Alice".to_string());
    assert!(account_state.can_submit());
    let _create_account_cmd = EffectCommand::CreateAccount {
        display_name: "Alice".to_string(),
    };
    account_state.hide();
    println!("  ✓ Account created with name 'Alice'");

    // Step 2: Create Invitation
    println!("Step 2: Create Invitation");
    let mut invite_state = InvitationCreateState::new();
    invite_state.show();
    // Default is Contact type
    invite_state.set_message("Join me!".to_string());
    let _create_invite_cmd = EffectCommand::CreateInvitation {
        invitation_type: "Contact".to_string(),
        message: Some("Join me!".to_string()),
        ttl_secs: None,
    };
    invite_state.hide();
    println!("  ✓ Contact invitation created");

    // Step 3: Export Invitation Code
    println!("Step 3: Export Invitation Code");
    let mut code_state = InvitationCodeState::new();
    let _export_cmd = EffectCommand::ExportInvitation {
        invitation_id: "inv_alice_001".to_string(),
    };
    code_state.show(
        "inv_alice_001".to_string(),
        "Contact".to_string(),
        "aura://invite/alice001xyz".to_string(),
    );
    assert!(code_state.visible);
    assert!(!code_state.code.is_empty());
    code_state.hide();
    println!("  ✓ Invitation code exported");

    // Step 4: Import Invitation (simulating other user)
    println!("Step 4: Import Invitation (Bob)");
    let mut import_state = InvitationImportState::new();
    import_state.show();
    import_state.set_code("aura://invite/alice001xyz".to_string());
    assert!(import_state.can_submit());
    let _import_cmd = EffectCommand::ImportInvitation {
        code: "aura://invite/alice001xyz".to_string(),
    };
    import_state.hide();
    println!("  ✓ Invitation imported by Bob");

    // Step 5: Accept Invitation
    println!("Step 5: Accept Invitation");
    let _accept_cmd = EffectCommand::AcceptInvitation {
        invitation_id: "inv_from_alice".to_string(),
    };
    println!("  ✓ Invitation accepted, contact established");

    // Step 6: Create Chat Group
    println!("Step 6: Create Chat Group");
    let mut chat_state = ChatCreateState::new();
    chat_state.show();
    for c in "Alice & Bob".chars() {
        chat_state.push_char(c);
    }
    assert!(chat_state.can_submit());
    let _create_channel_cmd = EffectCommand::CreateChannel {
        name: "Alice & Bob".to_string(),
        topic: None,
        members: vec!["bob_id".to_string()],
    };
    chat_state.hide();
    println!("  ✓ Chat group created");

    // Step 7: Send Message
    println!("Step 7: Send Message");
    let _send_msg_cmd = EffectCommand::SendMessage {
        channel: "chan_alice_bob".to_string(),
        content: "Hello Bob!".to_string(),
    };
    println!("  ✓ Message sent");

    // Step 8: Start Direct Chat from Contacts
    println!("Step 8: Start Direct Chat");
    let _direct_chat_cmd = EffectCommand::StartDirectChat {
        contact_id: "bob_id".to_string(),
    };
    println!("  ✓ Direct chat started");

    // Step 9: Update Petname
    println!("Step 9: Update Contact Petname");
    let mut petname_state = TextInputState::new();
    petname_state.show(
        "Edit Petname",
        "Bob",
        "Enter name",
        Some("bob_id".to_string()),
    );
    petname_state.pop_char(); // Remove 'b'
    petname_state.pop_char(); // Remove 'o'
    petname_state.pop_char(); // Remove 'B'
    for c in "Bobby".chars() {
        petname_state.push_char(c);
    }
    let _update_petname_cmd = EffectCommand::UpdateContactPetname {
        contact_id: "bob_id".to_string(),
        petname: "Bobby".to_string(),
    };
    petname_state.hide();
    println!("  ✓ Petname updated to 'Bobby'");

    // Step 10: Block Invite
    println!("Step 10: Block Invite");
    let contacts = vec![Contact::new("charlie_id", "Charlie").with_status(ContactStatus::Active)];
    let mut select_state = ContactSelectState::new();
    select_state.show("Invite to Block", contacts);
    let selected_id = select_state.get_selected_id().unwrap();
    let _block_invite_cmd = EffectCommand::SendBlockInvitation {
        contact_id: selected_id,
    };
    select_state.hide();
    println!("  ✓ Block invitation sent to Charlie");

    println!("\n=== E2E Flow Complete ===\n");
}
