//! Shared deterministic demo invitation helpers.
//!
//! These helpers are used by both the development-only demo surfaces and the
//! terminal test support modules, so they must stay available without the
//! `development` feature.

use base64::Engine;

use crate::ids;

/// Generate a deterministic contact invite code for a demo agent.
///
/// The code format matches `ShareableInvitation` from aura-agent:
/// `aura:v1:<base64-encoded-json>`.
///
/// This generates CONTACT invitations (not Guardian).
/// Guardian requests are sent in-band after someone is a contact.
pub fn generate_demo_contact_invite_code(name: &str, seed: u64) -> String {
    let sender_id = ids::authority_id(&format!("demo:{seed}:{name}:authority"));
    let invitation_id = ids::uuid(&format!("demo:{seed}:{name}:invitation"));

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
        "message": format!("Contact invitation from {name} (demo)")
    });

    let json_str = serde_json::to_string(&invitation_data).unwrap_or_default();
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json_str.as_bytes());
    format!("aura:v1:{b64}")
}
