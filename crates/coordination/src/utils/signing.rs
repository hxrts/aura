// Event signing utilities for orchestrators
//
// This module provides helpers for signing events with device keys.
// All events must be signed before being appended to the ledger.

use ed25519_dalek::{Signature, SigningKey, Signer};
use aura_journal::{Event, LedgerError};

/// Event signer - holds device signing key
pub struct EventSigner {
    pub signing_key: SigningKey,
}

impl EventSigner {
    /// Create a new event signer with a signing key
    pub fn new(signing_key: SigningKey) -> Self {
        EventSigner { signing_key }
    }
    
    /// Sign an event
    ///
    /// Computes the event hash and signs it with the device key
    pub fn sign_event(&self, event: &Event) -> Result<Signature, LedgerError> {
        let event_hash = event.hash()?;
        Ok(self.signing_key.sign(&event_hash))
    }
    
    /// Get the public key for this signer
    pub fn public_key(&self) -> ed25519_dalek::VerifyingKey {
        self.signing_key.verifying_key()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::Verifier;

    #[test]
    fn test_event_signing() {
        let _effects = aura_crypto::Effects::test();
        // Use deterministic key bytes instead of random
        let signing_key = SigningKey::from_bytes(&[1u8; 32]);
        let signer = EventSigner::new(signing_key);
        
        // Create a dummy event
        let event = aura_journal::Event::new(
            aura_journal::AccountId::new(),
            0,
            None,
            0,
            aura_journal::EventType::EpochTick(aura_journal::EpochTickEvent {
                new_epoch: 1,
                evidence_hash: [0u8; 32],
            }),
            aura_journal::EventAuthorization::DeviceCertificate {
                device_id: aura_journal::DeviceId(uuid::Uuid::from_bytes([1u8; 16])),
                signature: Signature::from_bytes(&[0u8; 64]),
            },
        );
        
        // Sign it
        let signature = signer.sign_event(&event).unwrap();
        
        // Verify signature
        let event_hash = event.hash().unwrap();
        assert!(signer.public_key().verify(&event_hash, &signature).is_ok());
    }
}

