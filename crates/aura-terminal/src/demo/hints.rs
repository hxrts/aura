//! # Demo Hints
//!
//! Provides contextual hints and pre-generated invite codes for demo mode.
//!
//! ## Contact vs Guardian Flow
//!
//! Text-based invite codes are for establishing CONTACTS only.
//! Guardian requests are sent IN-BAND to existing contacts:
//! 1. User imports Alice's contact invitation → Alice becomes a contact
//! 2. User goes to Recovery page → selects Alice → sends guardian request
//! 3. Alice sees the request on her Guardian page → accepts
//! 4. Alice becomes the user's guardian
//!
//! The hints guide users through the demo flow by showing relevant information
//! for each screen (e.g., Alice's invite code on the Invitations screen).

use crate::demo_invitation::generate_demo_contact_invite_code;

/// Demo hints that can be displayed in the TUI
#[derive(Debug, Clone, Default)]
pub struct DemoHints {
    /// Alice's invite code for adding her as a contact
    pub alice_invite_code: String,
    /// Carol's invite code for adding her as a contact
    pub carol_invite_code: String,
    /// Alice's display name
    pub alice_name: String,
    /// Carol's display name
    pub carol_name: String,
    /// Current contextual hint message
    pub current_hint: Option<String>,
}

impl DemoHints {
    /// Create demo hints with deterministic invite codes based on seed
    ///
    /// Both the authority IDs and invitation IDs are derived deterministically
    /// from the seed, ensuring reproducible demo behavior.
    pub fn new(seed: u64) -> Self {
        // IMPORTANT: Must match AgentFactory::create_demo_agents() in demo/mod.rs
        // - Names are Title case ("Alice", "Carol")
        // - Alice uses `seed`, Carol uses `seed + 1`
        let alice_code = generate_demo_contact_invite_code("Alice", seed);
        let carol_code = generate_demo_contact_invite_code("Carol", seed + 1);

        Self {
            alice_invite_code: alice_code,
            carol_invite_code: carol_code,
            alice_name: "Alice".to_string(),
            carol_name: "Carol".to_string(),
            current_hint: None,
        }
    }

    /// Get the hint for the invitations screen
    pub fn invitations_hint(&self) -> String {
        format!(
            "Demo: Import Alice's code to add her as a contact: {}",
            self.alice_invite_code
        )
    }

    /// Get the hint for the recovery screen
    pub fn recovery_hint(&self) -> String {
        "Demo: Press 'a' to add a guardian. Select Alice or Carol from contacts.".to_string()
    }

    /// Get the hint for the contacts screen
    pub fn contacts_hint(&self) -> String {
        format!(
            "Demo: Import codes on Invitations page (5). Alice: {}, Carol: {}",
            &self.alice_invite_code[..20.min(self.alice_invite_code.len())],
            &self.carol_invite_code[..20.min(self.carol_invite_code.len())]
        )
    }

    /// Get a general demo mode indicator
    pub fn demo_indicator(&self) -> String {
        "DEMO MODE - Alice and Carol are simulated contacts".to_string()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn test_demo_hints_creation() {
        let hints = DemoHints::new(2024);
        assert!(!hints.alice_invite_code.is_empty());
        assert!(!hints.carol_invite_code.is_empty());
        assert_eq!(hints.alice_name, "Alice");
        assert_eq!(hints.carol_name, "Carol");
    }

    #[test]
    fn test_invite_code_deterministic() {
        let hints1 = DemoHints::new(2024);
        let hints2 = DemoHints::new(2024);
        assert_eq!(hints1.alice_invite_code, hints2.alice_invite_code);
        assert_eq!(hints1.carol_invite_code, hints2.carol_invite_code);
    }

    #[test]
    fn test_hints_messages() {
        let hints = DemoHints::new(2024);

        let inv_hint = hints.invitations_hint();
        assert!(inv_hint.contains("Alice"));
        assert!(inv_hint.contains(&hints.alice_invite_code));

        let recovery_hint = hints.recovery_hint();
        // Hint should mention how to add guardians (press 'a')
        assert!(recovery_hint.contains("guardian"));
    }
}
