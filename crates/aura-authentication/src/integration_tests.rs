//! Integration tests for authentication and authorization
//!
//! These tests verify that authentication and authorization work together correctly.

#[cfg(test)]
mod tests {
    use crate::{AuthenticationContext, EventAuthorization, ThresholdConfig, ThresholdSig};
    use aura_crypto::Effects;
    use aura_types::{AccountId, DeviceId, DeviceIdExt, GuardianId};

    #[test]
    fn test_device_authentication_flow() {
        let effects = Effects::test();
        let device_id = DeviceId::new_with_effects(&effects);
        let account_id = AccountId::new();

        // Generate keys
        let signing_key = aura_crypto::generate_ed25519_key();
        let verifying_key = aura_crypto::ed25519_verifying_key(&signing_key);

        // Create authentication context
        let mut auth_context = AuthenticationContext::new();
        auth_context.add_device_key(device_id, verifying_key.clone());

        // Create event authorization
        let message = b"test event message";
        let signature = aura_crypto::ed25519_sign(&signing_key, message);

        let authorization = EventAuthorization::DeviceCertificate {
            device_id,
            signature,
        };

        // Validate event authorization
        let result = crate::event_validation::EventValidator::validate_event_authorization(
            &authorization,
            message,
            &auth_context,
            &account_id,
        );

        assert!(result.is_ok());
    }

    #[test]
    fn test_guardian_authentication_flow() {
        let effects = Effects::test();
        let guardian_id = GuardianId::new();
        let account_id = AccountId::new();

        // Generate guardian keys
        let signing_key = aura_crypto::generate_ed25519_key();
        let verifying_key = aura_crypto::ed25519_verifying_key(&signing_key);

        // Create authentication context
        let mut auth_context = AuthenticationContext::new();
        auth_context.add_guardian_key(guardian_id, verifying_key.clone());

        // Create event authorization
        let message = b"test guardian event";
        let signature = aura_crypto::ed25519_sign(&signing_key, message);

        let authorization = EventAuthorization::GuardianSignature {
            guardian_id,
            signature,
        };

        // Validate event authorization
        let result = crate::event_validation::EventValidator::validate_event_authorization(
            &authorization,
            message,
            &auth_context,
            &account_id,
        );

        assert!(result.is_ok());
    }

    #[test]
    fn test_threshold_authentication_flow() {
        let effects = Effects::test();
        let account_id = AccountId::new();

        // Generate group keys for threshold
        let group_signing_key = aura_crypto::generate_ed25519_key();
        let group_verifying_key = aura_crypto::ed25519_verifying_key(&group_signing_key);

        // Create authentication context
        let mut auth_context = AuthenticationContext::new();
        auth_context.add_group_key(account_id, group_verifying_key.clone());
        auth_context.add_threshold_config(
            account_id,
            ThresholdConfig {
                threshold: 1, // Use 1 for Ed25519 compatibility
                participants: vec![
                    DeviceId::new_with_effects(&effects),
                    DeviceId::new_with_effects(&effects),
                    DeviceId::new_with_effects(&effects),
                ],
            },
        );

        // Create threshold signature
        let message = b"test threshold event";
        let signature = aura_crypto::ed25519_sign(&group_signing_key, message);

        let threshold_sig = ThresholdSig {
            signature,
            signers: vec![0], // One signer for Ed25519 compatibility
            signature_shares: vec![vec![1, 2, 3]], // One mock share
        };

        let authorization = EventAuthorization::ThresholdSignature(threshold_sig);

        // Validate event authorization
        let result = crate::event_validation::EventValidator::validate_event_authorization(
            &authorization,
            message,
            &auth_context,
            &account_id,
        );

        assert!(result.is_ok());
    }

    #[test]
    fn test_invalid_authentication_fails() {
        let effects = Effects::test();
        let device_id = DeviceId::new_with_effects(&effects);
        let account_id = AccountId::new();

        // Generate different keys (wrong key for verification)
        let signing_key = aura_crypto::generate_ed25519_key();
        let wrong_verifying_key =
            aura_crypto::ed25519_verifying_key(&aura_crypto::generate_ed25519_key());

        // Create authentication context with wrong key
        let mut auth_context = AuthenticationContext::new();
        auth_context.add_device_key(device_id, wrong_verifying_key);

        // Create event authorization
        let message = b"test event message";
        let signature = aura_crypto::ed25519_sign(&signing_key, message);

        let authorization = EventAuthorization::DeviceCertificate {
            device_id,
            signature,
        };

        // Validate event authorization should fail
        let result = crate::event_validation::EventValidator::validate_event_authorization(
            &authorization,
            message,
            &auth_context,
            &account_id,
        );

        assert!(result.is_err());
    }
}
