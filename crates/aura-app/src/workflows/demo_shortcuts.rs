//! Shared deterministic demo shortcuts used by frontend-specific developer demo UX.

use base64::Engine;

use crate::workflows::{demo_config::DEMO_SEED_2024, ids};
use aura_core::types::identifiers::AuthorityId;
use serde::{Deserialize, Serialize};

pub const DUAL_FRONTEND_DEMO_WEB_SURFACE: &str = "web";
/// Canonical tablet device label for the browser-side developer demo flow.
pub const DUAL_FRONTEND_DEMO_WEB_TABLET_NAME: &str = "Tablet";

const ALICE_NAME: &str = "Alice";
const CAROL_NAME: &str = "Carol";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct DemoContactInviteCodePayload {
    version: u8,
    invitation_id: String,
    sender_id: String,
    invitation_type: DemoInvitationType,
    expires_at: Option<String>,
    message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
enum DemoInvitationType {
    Contact { nickname: String },
}

/// Deterministic browser-demo contact codes plus the addressed Tablet invitee authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DualFrontendWebDemoShortcuts {
    /// Alice's browser-specific demo contact code.
    pub alice_invite_code: String,
    /// Carol's browser-specific demo contact code.
    pub carol_invite_code: String,
    /// Deterministic invitee authority used for the browser demo Tablet device.
    pub tablet_invitee_authority_id: AuthorityId,
}

/// Build the deterministic Alice/Carol/Tablet shortcut set for `just demo` web sessions.
pub fn dual_frontend_web_demo_shortcuts() -> DualFrontendWebDemoShortcuts {
    DualFrontendWebDemoShortcuts {
        alice_invite_code: generate_demo_contact_invite_code(
            ALICE_NAME,
            DEMO_SEED_2024,
            DUAL_FRONTEND_DEMO_WEB_SURFACE,
        ),
        carol_invite_code: generate_demo_contact_invite_code(
            CAROL_NAME,
            DEMO_SEED_2024 + 1,
            DUAL_FRONTEND_DEMO_WEB_SURFACE,
        ),
        tablet_invitee_authority_id: ids::authority_id(&format!(
            "demo:{}:{}:authority",
            DEMO_SEED_2024 + 3,
            DUAL_FRONTEND_DEMO_WEB_TABLET_NAME
        )),
    }
}

pub fn generate_demo_contact_invite_code(name: &str, authority_seed: u64, variant: &str) -> String {
    let sender_id = ids::authority_id(&format!("demo:{authority_seed}:{name}:authority"));
    let invitation_id = ids::uuid(&format!(
        "demo:{authority_seed}:{name}:invitation:{variant}"
    ));

    let invitation_data = DemoContactInviteCodePayload {
        version: 1,
        invitation_id: invitation_id.to_string(),
        sender_id: sender_id.uuid().to_string(),
        invitation_type: DemoInvitationType::Contact {
            nickname: name.to_string(),
        },
        expires_at: None,
        message: format!("Contact invitation from {name} (demo:{variant})"),
    };

    let json_str = match serde_json::to_string(&invitation_data) {
        Ok(json) => json,
        Err(error) => panic!("demo invite serialization should not fail: {error}"),
    };
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json_str.as_bytes());
    format!("aura:v1:{b64}")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn decode_invitation(code: &str) -> DemoContactInviteCodePayload {
        let encoded = code
            .split(':')
            .nth(2)
            .expect("demo invite code includes encoded payload");
        let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(encoded.as_bytes())
            .expect("demo invite code payload decodes");
        serde_json::from_slice(&decoded).expect("demo invite code payload is valid json")
    }

    #[test]
    fn web_demo_shortcuts_keep_real_demo_authorities_but_use_distinct_codes() {
        let web = dual_frontend_web_demo_shortcuts();
        let alice_default = generate_demo_contact_invite_code(ALICE_NAME, DEMO_SEED_2024, "tui");
        let carol_default =
            generate_demo_contact_invite_code(CAROL_NAME, DEMO_SEED_2024 + 1, "tui");

        assert_ne!(web.alice_invite_code, alice_default);
        assert_ne!(web.carol_invite_code, carol_default);

        let parsed_web_alice = decode_invitation(&web.alice_invite_code);
        let parsed_default_alice = decode_invitation(&alice_default);
        let parsed_web_carol = decode_invitation(&web.carol_invite_code);
        let parsed_default_carol = decode_invitation(&carol_default);

        assert_eq!(parsed_web_alice.sender_id, parsed_default_alice.sender_id);
        assert_eq!(parsed_web_carol.sender_id, parsed_default_carol.sender_id);
    }

    #[test]
    fn web_demo_shortcuts_tablet_authority_is_deterministic() {
        let first = dual_frontend_web_demo_shortcuts();
        let second = dual_frontend_web_demo_shortcuts();

        assert_eq!(
            first.tablet_invitee_authority_id,
            second.tablet_invitee_authority_id
        );
    }
}
