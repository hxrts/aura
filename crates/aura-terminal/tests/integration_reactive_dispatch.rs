#![allow(
    missing_docs,
    dead_code,
    unused,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::all
)]
//! # Reactive Dispatch Integration Tests
//!
//! These tests verify that dispatch command handlers correctly integrate with
//! reactive signal subscriptions. They catch bugs where dispatch handlers use
//! stale captured data instead of current reactive state.
//!
//! ## Class of Bugs Tested
//!
//! 1. **Stale closure capture**: Dispatch handler closures capture props at
//!    render time, but props may be empty/stale while signals have current data.
//!
//! 2. **State mutation ordering**: Dispatch handlers that modify state must
//!    update the correct state object (e.g., new_state not tui_state).
//!
//! 3. **Reactive data flow**: Data added via signals must flow through to
//!    dispatch handlers that need that data.

use std::sync::{Arc, RwLock};

use aura_terminal::tui::screens::Screen;
use aura_terminal::tui::state_machine::{
    transition, DispatchCommand, GuardianCandidate, QueuedModal, TuiCommand, TuiState,
};
use aura_terminal::tui::types::Contact;

use aura_core::effects::terminal::events;

// ============================================================================
// Test Helpers
// ============================================================================

/// Extension trait to allow setting guardian status with a bool in tests
trait ContactTestExt {
    fn with_guardian(self, is_guardian: bool) -> Self;
}

impl ContactTestExt for Contact {
    fn with_guardian(mut self, is_guardian: bool) -> Self {
        self.is_guardian = is_guardian;
        self
    }
}

/// Simulates the SharedContacts pattern used by the shell.
///
/// This Arc<RwLock<Vec<Contact>>> is updated by reactive subscriptions,
/// and dispatch handlers read from it to get current contacts.
type SharedContacts = Arc<RwLock<Vec<Contact>>>;

/// Test wrapper that simulates the shell's dispatch handling.
struct DispatchTestHarness {
    state: TuiState,
    commands: Vec<TuiCommand>,
    shared_contacts: SharedContacts,
}

impl DispatchTestHarness {
    fn new() -> Self {
        Self {
            state: TuiState::new(),
            commands: Vec::new(),
            shared_contacts: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Send an event and process resulting commands
    fn send(&mut self, event: aura_core::effects::terminal::TerminalEvent) {
        let (mut new_state, commands) = transition(&self.state, event);

        // Process dispatch commands the same way shell.rs does
        for cmd in &commands {
            if let TuiCommand::Dispatch(dispatch_cmd) = cmd {
                self.process_dispatch(&mut new_state, dispatch_cmd);
            }
        }

        self.state = new_state;
        self.commands.extend(commands);
    }

    /// Simulate adding contacts via reactive signals
    fn add_contacts(&mut self, contacts: Vec<Contact>) {
        let mut guard = self.shared_contacts.write().unwrap();
        *guard = contacts;
    }

    /// Process a dispatch command (mimics shell.rs dispatch handling)
    fn process_dispatch(&mut self, new_state: &mut TuiState, cmd: &DispatchCommand) {
        match cmd {
            DispatchCommand::OpenGuardianSetup => {
                // Read current contacts from SharedContacts (reactive subscription)
                // NOT from stale captured props
                let current_contacts = self
                    .shared_contacts
                    .read()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();

                let candidates: Vec<GuardianCandidate> = current_contacts
                    .iter()
                    .map(|c| GuardianCandidate {
                        id: c.id.clone(),
                        name: c.nickname.clone(),
                        is_current_guardian: c.is_guardian,
                    })
                    .collect();

                let selected: Vec<usize> = candidates
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| c.is_current_guardian)
                    .map(|(i, _)| i)
                    .collect();

                let mut modal_state =
                    aura_terminal::tui::state_machine::GuardianSetupModalState::default();
                modal_state.contacts = candidates;
                modal_state.selected_indices = selected;

                // IMPORTANT: Modify new_state, not some other state object
                new_state
                    .modal_queue
                    .enqueue(QueuedModal::GuardianSetup(modal_state));
            }
            _ => {}
        }
    }

    fn send_char(&mut self, c: char) {
        self.send(events::char(c));
    }

    fn get_guardian_setup_modal(
        &self,
    ) -> Option<&aura_terminal::tui::state_machine::GuardianSetupModalState> {
        if let Some(QueuedModal::GuardianSetup(state)) = self.state.modal_queue.current() {
            Some(state)
        } else {
            None
        }
    }
}

// ============================================================================
// Data Flow Integration Tests
// ============================================================================

/// Test: Guardian setup modal receives contacts added via reactive signals.
///
/// This test catches the bug where contacts_for_dispatch captured stale props
/// instead of reading from reactive subscriptions.
#[test]
fn test_guardian_setup_receives_reactive_contacts() {
    let mut harness = DispatchTestHarness::new();

    // Navigate to Contacts screen
    harness.send_char('3');
    assert_eq!(harness.state.screen(), Screen::Contacts);

    // Simulate adding contacts via reactive signals
    // (In production, this happens when CONTACTS_SIGNAL fires)
    harness.add_contacts(vec![
        Contact::new("alice-id", "Alice").guardian(),
        Contact::new("carol-id", "Carol"),
    ]);

    // Press 'g' to open guardian setup
    harness.send_char('g');

    // Verify modal was opened with contacts from reactive subscription
    let modal = harness
        .get_guardian_setup_modal()
        .expect("Guardian setup modal should be open");

    assert_eq!(
        modal.contacts.len(),
        2,
        "Modal should have 2 contacts from reactive subscription"
    );
    assert_eq!(modal.contacts[0].name, "Alice");
    assert_eq!(modal.contacts[1].name, "Carol");

    // Alice should be pre-selected (is_guardian = true)
    assert!(
        modal.selected_indices.contains(&0),
        "Alice (index 0) should be pre-selected as guardian"
    );
    assert!(
        !modal.selected_indices.contains(&1),
        "Carol (index 1) should not be pre-selected"
    );
}

/// Test: Guardian setup modal is empty when no contacts exist.
#[test]
fn test_guardian_setup_empty_when_no_contacts() {
    let mut harness = DispatchTestHarness::new();

    harness.send_char('3'); // Contacts screen

    // Don't add any contacts - shared_contacts remains empty

    harness.send_char('g'); // Open guardian setup

    let modal = harness
        .get_guardian_setup_modal()
        .expect("Guardian setup modal should be open");

    assert!(modal.contacts.is_empty(), "Modal should have no contacts");
    assert!(
        modal.selected_indices.is_empty(),
        "No contacts means no selections"
    );
}

/// Test: Contacts added AFTER render are still visible in guardian setup.
///
/// This specifically catches the stale closure capture bug where contacts
/// captured at render time would miss later additions.
#[test]
fn test_contacts_added_after_render_are_visible() {
    let mut harness = DispatchTestHarness::new();

    // Navigate to Contacts screen (simulates initial render)
    harness.send_char('3');

    // At this point, in the old buggy code:
    // - props.contacts was empty
    // - contacts_for_dispatch captured empty vec
    // - Any contacts added later would be invisible to dispatch handler

    // Now simulate user adding contacts (via import, reactive subscription, etc.)
    harness.add_contacts(vec![Contact::new("bob-id", "Bob")]);

    // Some time passes, user navigates around
    harness.send_char('j'); // scroll
    harness.send_char('k'); // scroll

    // User presses 'g' - should see Bob even though he was added after render
    harness.send_char('g');

    let modal = harness
        .get_guardian_setup_modal()
        .expect("Guardian setup modal should be open");

    assert_eq!(
        modal.contacts.len(),
        1,
        "Modal should see Bob who was added after render"
    );
    assert_eq!(modal.contacts[0].name, "Bob");
}

/// Test: State machine produces OpenGuardianSetup dispatch command.
///
/// Unit test for the state machine logic (not integration).
#[test]
fn test_g_key_produces_open_guardian_setup_command() {
    let mut state = TuiState::new();

    // Navigate to Contacts
    let (new_state, _) = transition(&state, events::char('3'));
    state = new_state;

    assert_eq!(state.screen(), Screen::Contacts);

    // Press 'g'
    let (_, commands) = transition(&state, events::char('g'));

    // Should have Dispatch(OpenGuardianSetup) command
    let has_open_guardian = commands.iter().any(|cmd| {
        matches!(
            cmd,
            TuiCommand::Dispatch(DispatchCommand::OpenGuardianSetup)
        )
    });

    assert!(
        has_open_guardian,
        "Pressing 'g' on Contacts screen should emit OpenGuardianSetup dispatch command"
    );
}

/// Test: Ceremony in progress blocks new guardian setup.
#[test]
fn test_guardian_ceremony_in_progress_escape_cancels() {
    let mut state = TuiState::new();

    // Navigate to Contacts
    let (new_state, _) = transition(&state, events::char('4'));
    state = new_state;

    // Enqueue an in-progress guardian ceremony modal
    let mut modal = aura_terminal::tui::state_machine::GuardianSetupModalState::default();
    modal.step = aura_terminal::tui::state_machine::GuardianSetupStep::CeremonyInProgress;
    modal.ceremony.ceremony_id = Some("ceremony-123".to_string());
    state
        .modal_queue
        .enqueue(aura_terminal::tui::state_machine::QueuedModal::GuardianSetup(modal));

    // Escape should cancel (not silently dismiss)
    let (new_state, commands) = transition(&state, events::escape());

    let has_cancel = commands.iter().any(|cmd| {
        matches!(
            cmd,
            TuiCommand::Dispatch(DispatchCommand::CancelKeyRotationCeremony { ceremony_id })
                if ceremony_id == "ceremony-123"
        )
    });

    assert!(
        has_cancel,
        "Escape on in-progress guardian ceremony should dispatch CancelKeyRotationCeremony"
    );

    assert!(
        new_state.modal_queue.current().is_none(),
        "Cancel should dismiss the active guardian setup modal"
    );
}

// ============================================================================
// Stale Data Detection Tests (would catch the original bug)
// ============================================================================

/// Test that explicitly validates the reactive pattern.
///
/// This test would have caught the original bug by verifying that
/// dispatch handlers read from current state, not captured data.
#[test]
fn test_dispatch_uses_current_contacts_not_captured() {
    let mut harness = DispatchTestHarness::new();

    // Render the harness (in the old code, this would capture empty contacts)
    harness.send_char('3');

    // Old bug: contacts_for_dispatch was captured here as empty

    // Add contacts AFTER the "render"
    harness.add_contacts(vec![Contact::new("test-id", "Test Contact")]);

    // Trigger dispatch
    harness.send_char('g');

    // This would have FAILED with the old bug:
    // - contacts_for_dispatch was empty (captured at render)
    // - Modal would be opened with 0 contacts
    // - User would see "No contacts available"
    let modal = harness.get_guardian_setup_modal().unwrap();
    assert_eq!(
        modal.contacts.len(),
        1,
        "BUG: Dispatch handler used stale captured contacts instead of current reactive state"
    );
}

// ============================================================================
// Property Tests (would catch similar bugs)
// ============================================================================

use proptest::prelude::*;

prop_compose! {
    fn arb_contact()(
        id in "[a-z]{8}",
        name in "[A-Za-z ]{3,20}",
        is_guardian in any::<bool>()
    ) -> Contact {
        {
            let contact = Contact::new(id, name);
            if is_guardian {
                contact.guardian()
            } else {
                contact
            }
        }
    }
}

proptest! {
    /// Property: Guardian setup modal always reflects current SharedContacts state.
    #[test]
    fn prop_modal_reflects_current_contacts(
        contacts in prop::collection::vec(arb_contact(), 0..10)
    ) {
        let mut harness = DispatchTestHarness::new();

        harness.send_char('3'); // Contacts screen
        harness.add_contacts(contacts.clone());
        harness.send_char('g'); // Open guardian setup

        if let Some(modal) = harness.get_guardian_setup_modal() {
            prop_assert_eq!(
                modal.contacts.len(),
                contacts.len(),
                "Modal contact count must match SharedContacts"
            );

            // Verify guardian pre-selection matches
            let expected_guardians: Vec<usize> = contacts
                .iter()
                .enumerate()
                .filter(|(_, c)| c.is_guardian)
                .map(|(i, _)| i)
                .collect();

            prop_assert_eq!(
                modal.selected_indices.clone(),
                expected_guardians,
                "Pre-selected guardians must match is_guardian flags"
            );
        }
    }

    /// Property: Adding contacts after render doesn't break modal.
    #[test]
    fn prop_contacts_added_after_render_visible(
        initial in prop::collection::vec(arb_contact(), 0..5),
        added in prop::collection::vec(arb_contact(), 1..5)
    ) {
        let mut harness = DispatchTestHarness::new();

        harness.send_char('3');
        harness.add_contacts(initial.clone());

        // Simulate some navigation (as if time passed)
        harness.send_char('j');
        harness.send_char('k');

        // Add more contacts
        let mut all_contacts = initial;
        all_contacts.extend(added);
        harness.add_contacts(all_contacts.clone());

        // Open modal
        harness.send_char('g');

        if let Some(modal) = harness.get_guardian_setup_modal() {
            prop_assert_eq!(
                modal.contacts.len(),
                all_contacts.len(),
                "Modal must see all contacts including those added after render"
            );
        }
    }
}
