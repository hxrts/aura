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

use base64::Engine;

use crate::ids;

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
        let alice_code = generate_invite_code("Alice", seed);
        let carol_code = generate_invite_code("Carol", seed + 1);

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

/// Generate a deterministic invite code for a demo agent
///
/// The code format matches `ShareableInvitation` from aura-agent:
/// `aura:v1:<base64-encoded-json>`
///
/// This generates CONTACT invitations (not Guardian).
/// Guardian requests are sent in-band after someone is a contact.
fn generate_invite_code(name: &str, seed: u64) -> String {
    // Create deterministic authority ID matching the simulator's derivation
    // IMPORTANT: Must use ids::authority_id() to match SimulatedAgent derivation in demo/mod.rs
    let sender_id = ids::authority_id(&format!("demo:{}:{}:authority", seed, name));

    // Create deterministic invitation ID from seed and name
    let invitation_id = ids::uuid(&format!("demo:{}:{}:invitation", seed, name));

    // Create ShareableInvitation-compatible structure
    // Note: invitation_type uses the aura-invitation InvitationType::Contact format
    // Guardian requests are sent in-band AFTER adding as contact
    // IMPORTANT: Use sender_id.uuid() to get bare UUID for serde serialization
    // (sender_id.to_string() includes "authority-" prefix which breaks deserialization)
    let invitation_data = serde_json::json!({
        "version": 1,
        "invitation_id": invitation_id.to_string(),
        "sender_id": sender_id.uuid().to_string(),
        "invitation_type": {
            "Contact": {
                "nickname": name
            }
        },
        "expires_at": null,
        "message": format!("Contact invitation from {} (demo)", name)
    });

    // Encode as base64 with aura:v1: prefix
    let json_str = serde_json::to_string(&invitation_data).unwrap_or_default();
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json_str.as_bytes());
    format!("aura:v1:{}", b64)
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
    fn test_invite_code_format() {
        let hints = DemoHints::new(2024);

        // Verify format is aura:v1:<base64>
        assert!(hints.alice_invite_code.starts_with("aura:v1:"));
        assert!(hints.carol_invite_code.starts_with("aura:v1:"));

        // Extract and decode the base64 portion
        let parts: Vec<&str> = hints.alice_invite_code.split(':').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], "aura");
        assert_eq!(parts[1], "v1");

        let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(parts[2].as_bytes())
            .expect("Should be valid base64");
        let json_str = String::from_utf8(decoded).expect("Should be valid UTF-8");
        let data: serde_json::Value =
            serde_json::from_str(&json_str).expect("Should be valid JSON");

        // Verify ShareableInvitation structure
        assert_eq!(data["version"], 1);
        assert!(data.get("invitation_id").is_some());
        assert!(data.get("sender_id").is_some());
        assert!(data.get("invitation_type").is_some());
        // Should be Contact type, not Guardian
        assert!(data["invitation_type"].get("Contact").is_some());
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

    #[test]
    fn test_invite_code_parseable_by_shareable_invitation() {
        use aura_agent::handlers::ShareableInvitation;

        let hints = DemoHints::new(2024);

        // Verify Alice's code can be parsed
        let alice_parsed = ShareableInvitation::from_code(&hints.alice_invite_code)
            .expect("Alice's invitation code should be parseable");
        assert_eq!(alice_parsed.version, 1);
        assert!(!alice_parsed.invitation_id.is_empty());
        // Verify the invitation type is Contact (not Guardian)
        match alice_parsed.invitation_type {
            aura_invitation::InvitationType::Contact { nickname } => {
                assert_eq!(nickname, Some("Alice".to_string()));
            }
            _ => panic!(
                "Expected Contact invitation type, got {:?}",
                alice_parsed.invitation_type
            ),
        }

        // Verify Carol's code can be parsed
        let carol_parsed = ShareableInvitation::from_code(&hints.carol_invite_code)
            .expect("Carol's invitation code should be parseable");
        assert_eq!(carol_parsed.version, 1);
        assert!(!carol_parsed.invitation_id.is_empty());
        match carol_parsed.invitation_type {
            aura_invitation::InvitationType::Contact { nickname } => {
                assert_eq!(nickname, Some("Carol".to_string()));
            }
            _ => panic!(
                "Expected Contact invitation type, got {:?}",
                carol_parsed.invitation_type
            ),
        }

        // Verify different seeds produce different codes
        assert_ne!(
            alice_parsed.sender_id, carol_parsed.sender_id,
            "Alice and Carol should have different sender IDs"
        );
    }

    /// Verify that hints derive the SAME authority IDs as the simulator.
    ///
    /// This test ensures that when a user imports an invitation code from hints,
    /// the contact's AuthorityId matches what the SimulatedAgent uses internally.
    /// Without this, guardian bindings won't match because the signal_coordinator
    /// looks up contacts by authority_id.
    #[test]
    fn test_hints_authority_matches_simulator_derivation() {
        use aura_agent::handlers::ShareableInvitation;

        let seed = 2024u64;
        let hints = DemoHints::new(seed);

        // Parse Alice's invitation to get the sender_id used in hints
        let alice_parsed = ShareableInvitation::from_code(&hints.alice_invite_code)
            .expect("Alice's invitation code should be parseable");
        // sender_id is already AuthorityId (typed ID refactor)
        let hints_alice_authority = alice_parsed.sender_id;

        // Derive Alice's authority the SAME way SimulatedAgent does (demo/mod.rs line 269)
        let simulator_alice_authority =
            ids::authority_id(&format!("demo:{}:{}:authority", seed, "Alice"));

        assert_eq!(
            hints_alice_authority, simulator_alice_authority,
            "Hints and simulator must derive the same AuthorityId for Alice"
        );

        // Same check for Carol (uses seed + 1 like AgentFactory)
        let carol_parsed = ShareableInvitation::from_code(&hints.carol_invite_code)
            .expect("Carol's invitation code should be parseable");
        let hints_carol_authority = carol_parsed.sender_id;

        // Carol's seed is seed + 1 (see AgentFactory::create_demo_agents)
        let simulator_carol_authority =
            ids::authority_id(&format!("demo:{}:{}:authority", seed + 1, "Carol"));

        assert_eq!(
            hints_carol_authority, simulator_carol_authority,
            "Hints and simulator must derive the same AuthorityId for Carol"
        );
    }
}
