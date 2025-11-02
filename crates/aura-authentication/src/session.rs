//! Session ticket verification
//!
//! This module handles verifying session tickets that authorize temporary
//! operations within a protocol session.

use crate::{AuthenticationError, Result};
use aura_crypto::{Ed25519Signature, Ed25519VerifyingKey};
use uuid::Uuid;

/// Session ticket that authorizes operations within a session
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionTicket {
    pub session_id: Uuid,
    pub issuer_device_id: aura_types::DeviceId,
    pub issued_at: u64,  // Epoch timestamp
    pub expires_at: u64, // Epoch timestamp
    pub scope: SessionScope,
    pub nonce: [u8; 16],
}

/// Scope of operations a session ticket authorizes
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum SessionScope {
    /// Ticket authorizes DKD operations
    Dkd { app_id: String, context: String },
    /// Ticket authorizes recovery operations
    Recovery { recovery_id: Uuid },
    /// Ticket authorizes resharing operations
    Resharing { new_threshold: u16 },
    /// Ticket authorizes general protocol operations
    Protocol { protocol_type: String },
}

/// Verify that a session ticket is authentic and valid
///
/// This function proves that a session ticket was issued by a trusted device
/// and is still valid (not expired).
///
/// # Arguments
///
/// * `ticket` - The session ticket to verify
/// * `ticket_signature` - Signature over the ticket
/// * `issuer_public_key` - Public key of the device that issued the ticket
/// * `current_time` - Current timestamp for expiry checking
///
/// # Returns
///
/// `Ok(())` if the ticket is valid and authentic,
/// `Err(AuthenticationError)` otherwise.
pub fn verify_session_ticket(
    ticket: &SessionTicket,
    ticket_signature: &Ed25519Signature,
    issuer_public_key: &Ed25519VerifyingKey,
    current_time: u64,
) -> Result<()> {
    // Check if ticket has expired
    if current_time > ticket.expires_at {
        return Err(AuthenticationError::InvalidSessionTicket(format!(
            "Session ticket expired: current_time={}, expires_at={}",
            current_time, ticket.expires_at
        )));
    }

    // Check if ticket is not yet valid
    if current_time < ticket.issued_at {
        return Err(AuthenticationError::InvalidSessionTicket(format!(
            "Session ticket not yet valid: current_time={}, issued_at={}",
            current_time, ticket.issued_at
        )));
    }

    // Serialize the ticket for signature verification
    let ticket_bytes = serialize_session_ticket(ticket)?;

    // Verify the signature
    aura_crypto::ed25519_verify(issuer_public_key, &ticket_bytes, ticket_signature).map_err(
        |e| {
            AuthenticationError::InvalidSessionTicket(format!(
                "Session ticket signature verification failed: {}",
                e
            ))
        },
    )?;

    tracing::debug!(
        session_id = %ticket.session_id,
        issuer = %ticket.issuer_device_id,
        "Session ticket verified successfully"
    );

    Ok(())
}

/// Verify that a session ticket authorizes a specific operation
///
/// This function checks that a valid session ticket has the correct scope
/// to authorize a specific operation.
///
/// # Arguments
///
/// * `ticket` - The session ticket to check
/// * `required_scope` - The scope required for the operation
///
/// # Returns
///
/// `Ok(())` if the ticket authorizes the operation,
/// `Err(AuthenticationError)` otherwise.
pub fn verify_session_authorization(
    ticket: &SessionTicket,
    required_scope: &SessionScope,
) -> Result<()> {
    if !scope_matches(&ticket.scope, required_scope) {
        return Err(AuthenticationError::InvalidSessionTicket(format!(
            "Session ticket scope mismatch: ticket has {:?}, required {:?}",
            ticket.scope, required_scope
        )));
    }

    Ok(())
}

/// Check if a ticket scope matches the required scope
fn scope_matches(ticket_scope: &SessionScope, required_scope: &SessionScope) -> bool {
    match (ticket_scope, required_scope) {
        (
            SessionScope::Dkd {
                app_id: t_app,
                context: t_ctx,
            },
            SessionScope::Dkd {
                app_id: r_app,
                context: r_ctx,
            },
        ) => t_app == r_app && t_ctx == r_ctx,
        (
            SessionScope::Recovery { recovery_id: t_id },
            SessionScope::Recovery { recovery_id: r_id },
        ) => t_id == r_id,
        (
            SessionScope::Resharing {
                new_threshold: t_threshold,
            },
            SessionScope::Resharing {
                new_threshold: r_threshold,
            },
        ) => t_threshold == r_threshold,
        (
            SessionScope::Protocol {
                protocol_type: t_type,
            },
            SessionScope::Protocol {
                protocol_type: r_type,
            },
        ) => t_type == r_type,
        _ => false,
    }
}

/// Serialize a session ticket for signature verification
fn serialize_session_ticket(ticket: &SessionTicket) -> Result<Vec<u8>> {
    serde_json::to_vec(ticket).map_err(|e| {
        AuthenticationError::InvalidSessionTicket(format!(
            "Failed to serialize session ticket: {}",
            e
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_crypto::Effects;
    use aura_types::DeviceIdExt;

    fn create_test_ticket(effects: &Effects) -> SessionTicket {
        SessionTicket {
            session_id: Uuid::new_v4(),
            issuer_device_id: aura_types::DeviceId::new_with_effects(effects),
            issued_at: 1000,
            expires_at: 2000,
            scope: SessionScope::Dkd {
                app_id: "test-app".to_string(),
                context: "test-context".to_string(),
            },
            nonce: [1u8; 16],
        }
    }

    #[test]
    fn test_verify_session_ticket_success() {
        let effects = Effects::test();
        let ticket = create_test_ticket(&effects);

        // Generate a key pair for testing
        let signing_key = aura_crypto::generate_ed25519_key();
        let verifying_key = aura_crypto::ed25519_verifying_key(&signing_key);

        // Sign the ticket
        let ticket_bytes = serialize_session_ticket(&ticket).unwrap();
        let signature = aura_crypto::ed25519_sign(&signing_key, &ticket_bytes);

        let current_time = 1500; // Between issued_at and expires_at

        let result = verify_session_ticket(&ticket, &signature, &verifying_key, current_time);

        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_session_ticket_expired() {
        let effects = Effects::test();
        let ticket = create_test_ticket(&effects);

        let signing_key = aura_crypto::generate_ed25519_key();
        let verifying_key = aura_crypto::ed25519_verifying_key(&signing_key);
        let ticket_bytes = serialize_session_ticket(&ticket).unwrap();
        let signature = aura_crypto::ed25519_sign(&signing_key, &ticket_bytes);

        let current_time = 3000; // After expires_at

        let result = verify_session_ticket(&ticket, &signature, &verifying_key, current_time);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AuthenticationError::InvalidSessionTicket(_)
        ));
    }

    #[test]
    fn test_verify_session_authorization_matching_scope() {
        let effects = Effects::test();
        let ticket = create_test_ticket(&effects);

        let required_scope = SessionScope::Dkd {
            app_id: "test-app".to_string(),
            context: "test-context".to_string(),
        };

        let result = verify_session_authorization(&ticket, &required_scope);
        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_session_authorization_mismatched_scope() {
        let effects = Effects::test();
        let ticket = create_test_ticket(&effects);

        let required_scope = SessionScope::Dkd {
            app_id: "different-app".to_string(),
            context: "test-context".to_string(),
        };

        let result = verify_session_authorization(&ticket, &required_scope);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AuthenticationError::InvalidSessionTicket(_)
        ));
    }

    #[test]
    fn test_scope_matches() {
        let dkd_scope1 = SessionScope::Dkd {
            app_id: "app1".to_string(),
            context: "ctx1".to_string(),
        };
        let dkd_scope2 = SessionScope::Dkd {
            app_id: "app1".to_string(),
            context: "ctx1".to_string(),
        };
        let dkd_scope3 = SessionScope::Dkd {
            app_id: "app2".to_string(),
            context: "ctx1".to_string(),
        };

        assert!(scope_matches(&dkd_scope1, &dkd_scope2));
        assert!(!scope_matches(&dkd_scope1, &dkd_scope3));

        let recovery_scope1 = SessionScope::Recovery {
            recovery_id: aura_protocol::test_utils::generate_test_uuid(),
        };
        let _recovery_scope2 = SessionScope::Recovery {
            recovery_id: aura_protocol::test_utils::generate_test_uuid(),
        };

        // Different recovery IDs should not match
        assert!(!scope_matches(&dkd_scope1, &recovery_scope1));
    }
}
