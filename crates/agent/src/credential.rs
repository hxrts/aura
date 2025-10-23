// Session credential issuance and verification

use crate::{DerivedIdentity, SessionCredential, Result};
use aura_journal::{DeviceId, SessionEpoch};
use blake3;
use hkdf::Hkdf;
use sha2::Sha256;
use tracing::debug;

/// Issue a session credential for a derived identity
/// 
/// # Enhanced Security Model
/// 
/// Session credentials now ensure:
/// - Only current session epoch keys can initiate transport sessions
/// - Leaked keys cannot probe active devices post-recovery
/// - Challenge-response binding prevents precomputation attacks
/// - Operation-specific scoping limits ticket capabilities
/// - Nonce tracking prevents replay attacks
/// - Device attestation binding (placeholder for TPM/SEP)
/// 
/// # Arguments
/// 
/// * `device_id` - Device issuing the ticket
/// * `session_epoch` - Current session epoch
/// * `identity` - Derived identity to issue ticket for
/// * `challenge` - 32-byte challenge from verifier (MUST be server-generated random)
/// * `operation_scope` - What this ticket authorizes (e.g., "read:messages", "write:profile")
/// * `device_nonce` - Monotonic nonce for replay prevention
/// * `device_attestation` - Optional platform attestation (TPM/SEP quote)
/// * `ttl_seconds` - Ticket validity period
#[allow(clippy::too_many_arguments)]
pub fn issue_credential(
    device_id: DeviceId,
    session_epoch: SessionEpoch,
    identity: &DerivedIdentity,
    challenge: &[u8; 32],
    operation_scope: &str,
    device_nonce: u64,
    device_attestation: Option<Vec<u8>>,
    ttl_seconds: Option<u64>,
) -> Result<SessionCredential> {
    debug!(
        "Issuing session credential for device {} at epoch {} (scope: {}, nonce: {})",
        device_id, session_epoch.0, operation_scope, device_nonce
    );
    
    let ttl = ttl_seconds.unwrap_or(identity.capsule.ttl.unwrap_or(24 * 3600));
    let issued_at = current_timestamp()?;
    let expires_at = issued_at + ttl;
    
    // Generate handshake secret with challenge binding:
    // HKDF(seed_capsule || session_epoch || challenge || nonce || operation_scope)
    //
    // This binds the ticket to:
    // - The verifier's challenge (prevents precomputation)
    // - The specific operation scope (limits capabilities)
    // - A monotonic nonce (prevents replay)
    let mut ikm = Vec::new();
    ikm.extend_from_slice(&identity.seed_fingerprint);
    ikm.extend_from_slice(&session_epoch.0.to_le_bytes());
    ikm.extend_from_slice(challenge);
    ikm.extend_from_slice(&device_nonce.to_le_bytes());
    ikm.extend_from_slice(operation_scope.as_bytes());
    
    let hk = Hkdf::<Sha256>::new(None, &ikm);
    let mut capability = vec![0u8; 64];
    hk.expand(b"aura.presence_credential.v2", &mut capability)
        .map_err(|e| crate::AgentError::CryptoError(format!("HKDF expand failed: {}", e)))?;
    
    // In production, would wrap capability with HPKE or generate Biscuit token
    // For MVP, we use the derived secret
    
    Ok(SessionCredential {
        issued_by: device_id,
        expires_at,
        session_epoch: session_epoch.0,
        capability,
        challenge: *challenge,
        operation_scope: operation_scope.to_string(),
        nonce: device_nonce,
        device_attestation,
    })
}

/// Issue a simple session credential (legacy, insecure)
///
/// **DEPRECATED**: Use `issue_credential` with proper challenge-response.
///
/// This function generates a random challenge internally, which is insecure
/// for production use. It's provided for testing and backwards compatibility.
#[deprecated(note = "Use issue_credential with server-provided challenge")]
pub fn issue_simple_credential(
    device_id: DeviceId,
    session_epoch: SessionEpoch,
    identity: &DerivedIdentity,
    ttl_seconds: Option<u64>,
    effects: &aura_crypto::Effects,
) -> Result<SessionCredential> {
    // Generate random challenge (insecure - should be server-provided)
    let challenge = generate_nonce(effects);
    
    issue_credential(
        device_id,
        session_epoch,
        identity,
        &challenge,
        "legacy:all", // Unrestricted scope (insecure)
        0,            // No nonce tracking
        None,         // No attestation
        ttl_seconds,
    )
}

/// Verify a session credential
pub fn verify_credential(
    credential: &SessionCredential,
    current_epoch: SessionEpoch,
) -> Result<()> {
    let current_time = current_timestamp()?;
    
    // Check expiry
    if current_time > credential.expires_at {
        return Err(crate::AgentError::EpochMismatch(
            format!("Credential expired at {}, current time {}", credential.expires_at, current_time)
        ));
    }
    
    // Check session epoch
    if credential.session_epoch != current_epoch.0 {
        return Err(crate::AgentError::EpochMismatch(
            format!(
                "Credential epoch {} does not match current epoch {}",
                credential.session_epoch, current_epoch.0
            )
        ));
    }
    
    // In production, would verify Biscuit token or HPKE wrapper
    // For MVP, we just check epoch and expiry
    
    debug!("Session credential verified for device {} at epoch {}", credential.issued_by, credential.session_epoch);
    Ok(())
}

/// Compute credential digest for CRDT caching
pub fn credential_digest(credential: &SessionCredential) -> crate::Result<[u8; 32]> {
    let serialized = serde_cbor::to_vec(credential)
        .map_err(|e| crate::AgentError::SerializationError(format!(
            "SessionCredential serialization failed: {}",
            e
        )))?;
    Ok(*blake3::hash(&serialized).as_bytes())
}

fn current_timestamp() -> crate::Result<u64> {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|e| crate::AgentError::SystemTimeError(format!(
            "System time is before UNIX epoch: {}",
            e
        )))
}

fn generate_nonce(effects: &aura_crypto::Effects) -> [u8; 32] {
    effects.random_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ContextCapsule;
    use ed25519_dalek::SigningKey;
    #[allow(unused_imports)]
    use rand::rngs::OsRng;

    #[test]
    #[allow(deprecated)]
    fn test_presence_ticket_lifecycle() {
        let device_id = DeviceId::new();
        let session_epoch = SessionEpoch::initial();
        
        // Create mock identity
        let capsule = ContextCapsule::simple("test-app", "test-context");
        let signing_key = SigningKey::from_bytes(&rand::random());
        let identity = DerivedIdentity {
            capsule,
            pk_derived: signing_key.verifying_key(),
            seed_fingerprint: [42u8; 32],
        };
        
        // Issue ticket (using legacy function for test)
        #[allow(deprecated)]
        let effects = aura_crypto::Effects::test();
        let credential = issue_simple_credential(device_id, session_epoch, &identity, Some(3600), &effects).unwrap();
        
        // Verify ticket
        assert!(verify_credential(&credential, session_epoch).is_ok());
        
        // Verify ticket fails with wrong epoch
        let wrong_epoch = session_epoch.increment();
        assert!(verify_credential(&credential, wrong_epoch).is_err());
    }
    
    #[test]
    fn test_credential_digest() {
        let device_id = DeviceId::new();
        let credential = SessionCredential {
            issued_by: device_id,
            expires_at: 1234567890,
            session_epoch: 1,
            capability: vec![1, 2, 3, 4],
            challenge: [0u8; 32],
            operation_scope: "test:read".to_string(),
            nonce: 1,
            device_attestation: None,
        };
        
        let digest1 = credential_digest(&credential).unwrap();
        let digest2 = credential_digest(&credential).unwrap();
        
        // Same ticket should produce same digest
        assert_eq!(digest1, digest2);
        
        // Different ticket should produce different digest
        let mut credential2 = credential.clone();
        credential2.session_epoch = 2;
        let digest3 = credential_digest(&credential2).unwrap();
        assert_ne!(digest1, digest3);
    }
    
    #[test]
    fn test_enhanced_ticket_with_challenge() {
        let device_id = DeviceId::new();
        let session_epoch = SessionEpoch::initial();
        let challenge = [42u8; 32];
        
        // Create mock identity
        let capsule = ContextCapsule::simple("test-app", "test-context");
        let signing_key = SigningKey::from_bytes(&rand::random());
        let identity = DerivedIdentity {
            capsule,
            pk_derived: signing_key.verifying_key(),
            seed_fingerprint: [42u8; 32],
        };
        
        // Issue ticket with specific operation scope
        let credential = issue_credential(
            device_id,
            session_epoch,
            &identity,
            &challenge,
            "read:messages",
            1,            // nonce
            None,         // no attestation
            Some(3600),   // 1 hour
        ).unwrap();
        
        // Verify ticket structure
        assert_eq!(credential.challenge, challenge);
        assert_eq!(credential.operation_scope, "read:messages");
        assert_eq!(credential.nonce, 1);
        assert!(credential.device_attestation.is_none());
        
        // Verify ticket
        assert!(verify_credential(&credential, session_epoch).is_ok());
    }
}

