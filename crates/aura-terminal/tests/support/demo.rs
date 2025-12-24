//! Demo-specific test helpers.
//!
//! This module provides utilities for testing demo flows, including
//! deterministic invitation code generation that matches the demo system.
//!
//! # Invitation Code Generation
//!
//! Demo invitation codes must match the derivation in:
//! - `aura_terminal::demo::hints::generate_invite_code`
//! - `aura_terminal::demo::mod::SimulatedAgent::new_with_shared_transport`
//! - `aura_terminal::demo::mod::AgentFactory::create_demo_agents`
//!
//! Key rules:
//! - Uses `ids::authority_id()` for domain-separated derivation
//! - Alice uses `seed`, Carol uses `seed + 1`
//! - Creates Contact invitations (not Guardian)

use aura_terminal::ids;
use base64::Engine;

/// Default seed used in demo mode.
pub const DEFAULT_DEMO_SEED: u64 = 2024;

/// Demo agent names.
pub const ALICE: &str = "Alice";
pub const CAROL: &str = "Carol";

/// Generate a deterministic invite code for a demo agent.
///
/// This MUST match the derivation used by the demo system to ensure
/// invitation codes generated in tests are compatible with demo agents.
///
/// # Arguments
///
/// * `name` - The agent name (e.g., "Alice", "Carol")
/// * `seed` - The seed for deterministic ID generation
///
/// # Example
///
/// ```ignore
/// // Generate Alice's invite code with default demo seed
/// let alice_code = generate_demo_invite_code("Alice", DEFAULT_DEMO_SEED);
///
/// // Generate Carol's invite code (seed offset by 1)
/// let carol_code = generate_demo_invite_code("Carol", DEFAULT_DEMO_SEED + 1);
/// ```
pub fn generate_demo_invite_code(name: &str, seed: u64) -> String {
    // Create deterministic authority ID using the SAME derivation as SimulatedAgent
    // CRITICAL: Must use ids::authority_id() for domain separation
    let sender_id = ids::authority_id(&format!("demo:{}:{}:authority", seed, name));

    // Create deterministic invitation ID from seed and name
    let invitation_id = ids::uuid(&format!("demo:{}:{}:invitation", seed, name));

    // Create ShareableInvitation-compatible structure
    // Uses Contact type (not Guardian) - guardian requests are sent in-band
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

/// Generate Alice's invite code with the default demo seed.
pub fn alice_invite_code() -> String {
    generate_demo_invite_code(ALICE, DEFAULT_DEMO_SEED)
}

/// Generate Carol's invite code with the default demo seed.
pub fn carol_invite_code() -> String {
    generate_demo_invite_code(CAROL, DEFAULT_DEMO_SEED + 1)
}

/// Get Alice's authority ID for the default demo seed.
pub fn alice_authority_id() -> aura_core::identifiers::AuthorityId {
    ids::authority_id(&format!("demo:{}:{}:authority", DEFAULT_DEMO_SEED, ALICE))
}

/// Get Carol's authority ID for the default demo seed.
pub fn carol_authority_id() -> aura_core::identifiers::AuthorityId {
    ids::authority_id(&format!(
        "demo:{}:{}:authority",
        DEFAULT_DEMO_SEED + 1,
        CAROL
    ))
}

/// Generate a guardian invitation code for demo testing.
///
/// Unlike contact invitations, guardian invitations are typically
/// sent in-band during the guardian setup flow. This function is
/// useful for testing guardian invitation parsing.
pub fn generate_demo_guardian_invite_code(name: &str, seed: u64) -> String {
    let sender_id = ids::authority_id(&format!("demo:{}:{}:authority", seed, name));
    let invitation_id = ids::uuid(&format!("demo:{}:{}:guardian-invitation", seed, name));

    let invitation_data = serde_json::json!({
        "version": 1,
        "invitation_id": invitation_id.to_string(),
        "sender_id": sender_id.uuid().to_string(),
        "invitation_type": {
            "Guardian": {
                "nickname": name
            }
        },
        "expires_at": null,
        "message": format!("Guardian invitation from {} (demo)", name)
    });

    let json_str = serde_json::to_string(&invitation_data).unwrap_or_default();
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json_str.as_bytes());
    format!("aura:v1:{}", b64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invite_codes_are_deterministic() {
        let code1 = generate_demo_invite_code("Alice", 2024);
        let code2 = generate_demo_invite_code("Alice", 2024);
        assert_eq!(code1, code2, "Same inputs should produce same codes");
    }

    #[test]
    fn test_different_seeds_produce_different_codes() {
        let code1 = generate_demo_invite_code("Alice", 2024);
        let code2 = generate_demo_invite_code("Alice", 2025);
        assert_ne!(
            code1, code2,
            "Different seeds should produce different codes"
        );
    }

    #[test]
    fn test_invite_code_format() {
        let code = generate_demo_invite_code("Alice", 2024);
        assert!(code.starts_with("aura:v1:"), "Should have aura:v1: prefix");
    }
}
