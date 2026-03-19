//! Authentication fact → view reduction pipeline integration tests.
//!
//! Exercises the public API as a consumer would: create facts, reduce them
//! through the view reducer, and query the resulting view state.

use aura_authentication::{AuthFact, AuthViewReducer};
use aura_core::types::identifiers::AuthorityId;
use aura_core::{ContextId, DeviceId};
use aura_signature::session::SessionScope;

fn test_authority() -> AuthorityId {
    AuthorityId::new_from_entropy([1u8; 32])
}

fn test_context() -> ContextId {
    ContextId::new_from_entropy([3u8; 32])
}

fn test_device() -> DeviceId {
    DeviceId::new_from_entropy([2u8; 32])
}

/// Full session lifecycle through the public API: issue → revoke.
/// The view must reflect each state transition correctly.
#[test]
fn session_lifecycle_through_view_reducer() {
    let authority = test_authority();
    let context = test_context();
    let reducer = AuthViewReducer::new();

    let issue_facts = vec![
        AuthFact::ChallengeGenerated {
            context_id: context,
            session_id: "session-1".to_string(),
            authority_id: authority,
            device_id: Some(test_device()),
            scope: SessionScope::Protocol {
                protocol_type: "auth".to_string(),
            },
            expires_at_ms: 2000,
            created_at_ms: 1000,
        },
        AuthFact::SessionIssued {
            context_id: context,
            session_id: "session-1".to_string(),
            authority_id: authority,
            device_id: Some(test_device()),
            scope: SessionScope::Protocol {
                protocol_type: "auth".to_string(),
            },
            issued_at_ms: 1000,
            expires_at_ms: 2000,
        },
    ];

    let view = reducer.reduce_all(&issue_facts);
    assert!(
        view.active_sessions.contains_key("session-1"),
        "Issued session must appear in active_sessions"
    );
    assert_eq!(view.active_sessions["session-1"].authority_id, authority);

    // Revoke and verify removal
    let mut all_facts = issue_facts;
    all_facts.push(AuthFact::SessionRevoked {
        context_id: context,
        session_id: "session-1".to_string(),
        revoked_by: authority,
        reason: "user requested".to_string(),
        revoked_at_ms: 1500,
    });

    let revoked_view = reducer.reduce_all(&all_facts);
    assert!(
        !revoked_view.active_sessions.contains_key("session-1"),
        "Revoked session must not appear in active_sessions"
    );
}

/// Operation categories are consistent with the A/B/C classification.
#[test]
fn operation_categories_are_consistent() {
    assert_eq!(
        aura_authentication::operation_category("auth:challenge"),
        Some("A")
    );
    assert_eq!(
        aura_authentication::operation_category("auth:guardian-approval"),
        Some("C")
    );
    assert_eq!(
        aura_authentication::operation_category("auth:recovery-complete"),
        Some("C")
    );
}
