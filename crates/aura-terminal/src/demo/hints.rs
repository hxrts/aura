//! # Demo Hints
//!
//! Provides contextual hints and pre-generated invite codes for demo mode.
//!
//! The hints guide users through the demo flow by showing relevant information
//! for each screen (e.g., Alice's invite code on the Invitations screen).

use aura_core::identifiers::AuthorityId;
use base64::Engine;
use uuid::Uuid;

/// Demo hints that can be displayed in the TUI
#[derive(Debug, Clone, Default)]
pub struct DemoHints {
    /// Alice's invite code for guardian setup
    pub alice_invite_code: String,
    /// Charlie's invite code for guardian setup
    pub charlie_invite_code: String,
    /// Alice's display name
    pub alice_name: String,
    /// Charlie's display name
    pub charlie_name: String,
    /// Current contextual hint message
    pub current_hint: Option<String>,
}

impl DemoHints {
    /// Create demo hints with deterministic invite codes based on seed
    ///
    /// Both the authority IDs and invitation IDs are derived deterministically
    /// from the seed, ensuring reproducible demo behavior.
    pub fn new(seed: u64) -> Self {
        let alice_code = generate_invite_code("alice", seed);
        let charlie_code = generate_invite_code("charlie", seed);

        Self {
            alice_invite_code: alice_code,
            charlie_invite_code: charlie_code,
            alice_name: "Alice".to_string(),
            charlie_name: "Charlie".to_string(),
            current_hint: None,
        }
    }

    /// Get the hint for the invitations screen
    pub fn invitations_hint(&self) -> String {
        format!(
            "Demo: Import Alice's code to add her as guardian: {}",
            self.alice_invite_code
        )
    }

    /// Get the hint for the recovery screen
    pub fn recovery_hint(&self) -> String {
        "Demo: Press 'r' to start recovery. Alice and Charlie will auto-approve.".to_string()
    }

    /// Get the hint for the contacts screen
    pub fn contacts_hint(&self) -> String {
        format!(
            "Demo: Alice and Charlie are your guardians. Alice: {}, Charlie: {}",
            &self.alice_invite_code[..20.min(self.alice_invite_code.len())],
            &self.charlie_invite_code[..20.min(self.charlie_invite_code.len())]
        )
    }

    /// Get a general demo mode indicator
    pub fn demo_indicator(&self) -> String {
        "DEMO MODE - Alice and Charlie are simulated guardians".to_string()
    }
}

/// Generate a deterministic invite code for a demo agent
///
/// The code format matches `ShareableInvitation` from aura-agent:
/// `aura:v1:<base64-encoded-json>`
///
/// This ensures the codes can be imported via the standard invitation system.
fn generate_invite_code(name: &str, seed: u64) -> String {
    // Create deterministic authority ID matching the simulator's derivation
    let authority_entropy =
        aura_core::hash::hash(format!("demo:{}:{}:authority", seed, name).as_bytes());
    let sender_id = AuthorityId::new_from_entropy(authority_entropy);

    // Create deterministic invitation ID from seed and name
    let invitation_id_entropy =
        aura_core::hash::hash(format!("demo:{}:{}:invitation", seed, name).as_bytes());
    let invitation_id = Uuid::from_bytes(invitation_id_entropy[..16].try_into().unwrap());

    // Create ShareableInvitation-compatible structure
    // Note: invitation_type uses the aura-invitation InvitationType::Guardian format
    let invitation_data = serde_json::json!({
        "version": 1,
        "invitation_id": invitation_id.to_string(),
        "sender_id": sender_id.to_string(),
        "invitation_type": {
            "Guardian": {
                "subject_authority": sender_id.to_string()
            }
        },
        "expires_at": null,
        "message": format!("Guardian invitation from {} (demo)", name)
    });

    // Encode as base64 with aura:v1: prefix
    let json_str = serde_json::to_string(&invitation_data).unwrap_or_default();
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json_str.as_bytes());
    format!("aura:v1:{}", b64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_demo_hints_creation() {
        let hints = DemoHints::new(2024);
        assert!(!hints.alice_invite_code.is_empty());
        assert!(!hints.charlie_invite_code.is_empty());
        assert_eq!(hints.alice_name, "Alice");
        assert_eq!(hints.charlie_name, "Charlie");
    }

    #[test]
    fn test_invite_code_deterministic() {
        let hints1 = DemoHints::new(2024);
        let hints2 = DemoHints::new(2024);
        assert_eq!(hints1.alice_invite_code, hints2.alice_invite_code);
        assert_eq!(hints1.charlie_invite_code, hints2.charlie_invite_code);
    }

    #[test]
    fn test_invite_code_format() {
        let hints = DemoHints::new(2024);

        // Verify format is aura:v1:<base64>
        assert!(hints.alice_invite_code.starts_with("aura:v1:"));
        assert!(hints.charlie_invite_code.starts_with("aura:v1:"));

        // Extract and decode the base64 portion
        let parts: Vec<&str> = hints.alice_invite_code.split(':').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], "aura");
        assert_eq!(parts[1], "v1");

        let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(parts[2].as_bytes())
            .expect("Should be valid base64");
        let json_str = String::from_utf8(decoded).expect("Should be valid UTF-8");
        let data: serde_json::Value = serde_json::from_str(&json_str).expect("Should be valid JSON");

        // Verify ShareableInvitation structure
        assert_eq!(data["version"], 1);
        assert!(data.get("invitation_id").is_some());
        assert!(data.get("sender_id").is_some());
        assert!(data.get("invitation_type").is_some());
        assert!(data["invitation_type"].get("Guardian").is_some());
    }

    #[test]
    fn test_hints_messages() {
        let hints = DemoHints::new(2024);

        let inv_hint = hints.invitations_hint();
        assert!(inv_hint.contains("Alice"));
        assert!(inv_hint.contains(&hints.alice_invite_code));

        let recovery_hint = hints.recovery_hint();
        assert!(recovery_hint.contains("recovery"));
    }
}
