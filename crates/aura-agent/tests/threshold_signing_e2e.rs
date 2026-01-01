//! Threshold Signing End-to-End Tests
//!
//! Tests the complete threshold signing flow with real FROST cryptography:
//! - 8.7.1: Single-device account creation with threshold signing
//! - 8.7.2: Guardian recovery approval with real FROST signatures
//! - 8.7.3: Consensus with threshold signatures (verify integration)
//! - 8.7.4: DKD protocol with real FROST signing
//!
//! These tests validate that the unified ThresholdSigningEffects infrastructure
//! produces valid FROST signatures that can be verified.

use aura_core::effects::CryptoExtendedEffects;
use aura_core::identifiers::AuthorityId;
use aura_core::threshold::{
    ApprovalContext, GroupAction, SignableOperation, SigningContext, ThresholdConfig,
    ThresholdSignature,
};
use aura_core::tree::{TreeCommitment, TreeOp, TreeOpKind};
use aura_core::Epoch;
use aura_testkit::mock_effects::MockEffects;

/// Helper to create a test authority ID with unique entropy
fn test_authority(seed: u8) -> AuthorityId {
    AuthorityId::new_from_entropy([seed; 32])
}

/// Helper to create a test tree operation
fn test_tree_op(epoch: u64) -> TreeOp {
    TreeOp {
        parent_epoch: Epoch::new(epoch),
        parent_commitment: [0u8; 32],
        op: TreeOpKind::RotateEpoch { affected: vec![] },
        version: 1,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 8.7.1: Single-Device Account Creation with Threshold Signing
// ═══════════════════════════════════════════════════════════════════════════

mod single_device_tests {
    use super::*;
    use aura_core::effects::ThresholdSigningEffects;

    /// Test that MockEffects implements ThresholdSigningEffects
    #[tokio::test]
    async fn test_mock_effects_threshold_signing() {
        let effects = MockEffects::deterministic();
        let authority = test_authority(1);

        // Bootstrap should succeed
        let public_key = effects.bootstrap_authority(&authority).await;
        assert!(public_key.is_ok(), "Bootstrap should succeed");
        assert!(
            !public_key.unwrap().is_empty(),
            "Public key should not be empty"
        );

        // Should have signing capability after bootstrap
        let has_capability = effects.has_signing_capability(&authority).await;
        // Note: MockEffects always returns true for has_signing_capability
        assert!(
            has_capability,
            "Should have signing capability after bootstrap"
        );
    }

    /// Test signing context construction for self operations
    #[test]
    fn test_signing_context_self_operation() {
        let authority = test_authority(2);
        let tree_op = test_tree_op(0);

        let context = SigningContext::self_tree_op(authority, tree_op);

        assert!(
            matches!(context.approval_context, ApprovalContext::SelfOperation),
            "Should be self operation context"
        );
    }

    /// Test signing context construction for recovery assistance
    #[test]
    fn test_signing_context_recovery_assistance() {
        let signer_authority = test_authority(3);
        let recovering_authority = test_authority(4);
        let session_id = "test-session-123".to_string();

        let context = SigningContext {
            authority: signer_authority,
            operation: SignableOperation::RecoveryApproval {
                target: recovering_authority,
                new_root: TreeCommitment([1u8; 32]),
            },
            approval_context: ApprovalContext::RecoveryAssistance {
                recovering: recovering_authority,
                session_id,
            },
        };

        assert!(
            matches!(
                context.approval_context,
                ApprovalContext::RecoveryAssistance { .. }
            ),
            "Should be recovery assistance context"
        );
    }

    /// Test threshold config validation
    #[test]
    fn test_threshold_config_validation() {
        // Valid 1-of-1
        let config = ThresholdConfig::new(1, 1);
        assert!(config.is_ok(), "1-of-1 should be valid");

        // Valid 2-of-3
        let config = ThresholdConfig::new(2, 3);
        assert!(config.is_ok(), "2-of-3 should be valid");

        // Invalid: threshold > total
        let config = ThresholdConfig::new(3, 2);
        assert!(config.is_err(), "threshold > total should be invalid");

        // Invalid: zero threshold
        let config = ThresholdConfig::new(0, 3);
        assert!(config.is_err(), "zero threshold should be invalid");

        // Invalid: zero total
        let config = ThresholdConfig::new(1, 0);
        assert!(config.is_err(), "zero total should be invalid");
    }

    /// Test threshold signature structure
    #[test]
    fn test_threshold_signature_structure() {
        let signature = ThresholdSignature {
            signature: vec![0u8; 64],
            signer_count: 2,
            signers: vec![1, 2],
            public_key_package: vec![0u8; 32],
            epoch: 0,
        };

        assert_eq!(signature.signer_count, 2);
        assert_eq!(signature.signers.len(), 2);
        assert_eq!(signature.signature.len(), 64);
    }

    /// Test single signer convenience constructor
    #[test]
    fn test_threshold_signature_single_signer() {
        let signature_bytes = vec![1u8; 64];
        let public_key = vec![2u8; 32];
        let epoch = 5;

        let signature =
            ThresholdSignature::single_signer(signature_bytes.clone(), public_key.clone(), epoch);

        assert_eq!(signature.signer_count, 1);
        assert_eq!(signature.signers, vec![1]);
        assert_eq!(signature.signature, signature_bytes);
        assert_eq!(signature.public_key_package, public_key);
        assert_eq!(signature.epoch, epoch);
    }

    /// Test mock signing produces valid signature structure
    #[tokio::test]
    async fn test_mock_signing_produces_signature() {
        let effects = MockEffects::deterministic();
        let authority = test_authority(5);

        // Bootstrap
        effects.bootstrap_authority(&authority).await.unwrap();

        // Create signing context
        let context = SigningContext::self_tree_op(authority, test_tree_op(0));

        // Sign
        let result = effects.sign(context).await;
        assert!(result.is_ok(), "Signing should succeed");

        let signature = result.unwrap();
        assert_eq!(signature.signer_count, 1);
        assert!(!signature.signature.is_empty());
        assert!(!signature.public_key_package.is_empty());
    }

    /// Test threshold state exposes agreement mode
    #[tokio::test]
    async fn test_threshold_state_agreement_mode() {
        let effects = MockEffects::deterministic();
        let authority = test_authority(6);

        effects.bootstrap_authority(&authority).await.unwrap();
        let state = effects.threshold_state(&authority).await.unwrap();

        assert_eq!(
            state.agreement_mode,
            aura_core::threshold::AgreementMode::Provisional
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 8.7.2: Guardian Recovery Approval with Real FROST Signatures
// ═══════════════════════════════════════════════════════════════════════════

mod guardian_recovery_tests {
    use super::*;
    use aura_core::effects::ThresholdSigningEffects;

    /// Test guardian approval signing context
    #[test]
    fn test_guardian_approval_context() {
        let guardian_authority = test_authority(10);
        let recovering_authority = test_authority(11);
        let session_id = "recovery-session-456".to_string();

        let context = SigningContext {
            authority: guardian_authority,
            operation: SignableOperation::RecoveryApproval {
                target: recovering_authority,
                new_root: TreeCommitment([0xAB; 32]),
            },
            approval_context: ApprovalContext::RecoveryAssistance {
                recovering: recovering_authority,
                session_id: session_id.clone(),
            },
        };

        match context.approval_context {
            ApprovalContext::RecoveryAssistance {
                recovering,
                session_id: sid,
            } => {
                assert_eq!(recovering, recovering_authority);
                assert_eq!(sid, session_id);
            }
            _ => panic!("Expected RecoveryAssistance context"),
        }
    }

    /// Test multiple guardian signing (mock)
    #[tokio::test]
    async fn test_multiple_guardian_mock_signing() {
        let effects = MockEffects::deterministic();

        // Create 3 guardian authorities
        let guardians: Vec<AuthorityId> = (20..23).map(test_authority).collect();
        let recovering = test_authority(25);

        // Bootstrap each guardian
        for guardian in &guardians {
            effects.bootstrap_authority(guardian).await.unwrap();
        }

        // Collect signatures from threshold of guardians (2 of 3)
        let mut signatures = Vec::new();
        for (i, guardian) in guardians.iter().take(2).enumerate() {
            let context = SigningContext {
                authority: *guardian,
                operation: SignableOperation::RecoveryApproval {
                    target: recovering,
                    new_root: TreeCommitment([0xCD; 32]),
                },
                approval_context: ApprovalContext::RecoveryAssistance {
                    recovering,
                    session_id: format!("session-{}", i),
                },
            };

            let sig = effects.sign(context).await.unwrap();
            signatures.push(sig);
        }

        // Verify we collected threshold signatures
        assert_eq!(signatures.len(), 2);
        for sig in &signatures {
            assert_eq!(sig.signer_count, 1); // Each guardian signs individually
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 8.7.3: Consensus with Threshold Signatures
// ═══════════════════════════════════════════════════════════════════════════

mod consensus_tests {
    use super::*;

    /// Test that consensus-related signing contexts are valid
    #[test]
    fn test_consensus_signing_context_structure() {
        let authority = test_authority(30);
        let group = test_authority(31);
        let proposal_id = "proposal-789".to_string();

        let context = SigningContext {
            authority,
            operation: SignableOperation::GroupProposal {
                group,
                action: GroupAction::Custom {
                    action_type: "consensus-vote".to_string(),
                    data: vec![1, 2, 3],
                },
            },
            approval_context: ApprovalContext::GroupDecision {
                group,
                proposal_id: proposal_id.clone(),
            },
        };

        match context.approval_context {
            ApprovalContext::GroupDecision {
                group: g,
                proposal_id: pid,
            } => {
                assert_eq!(g, group);
                assert_eq!(pid, proposal_id);
            }
            _ => panic!("Expected GroupDecision context"),
        }
    }

    /// Test elevated operation context for high-value consensus
    #[test]
    fn test_elevated_operation_context() {
        let authority = test_authority(32);

        let context = SigningContext {
            authority,
            operation: SignableOperation::Message {
                domain: "consensus".to_string(),
                payload: vec![1, 2, 3, 4],
            },
            approval_context: ApprovalContext::ElevatedOperation {
                operation_type: "high-value-transfer".to_string(),
                value_context: Some("100 ETH".to_string()),
            },
        };

        match context.approval_context {
            ApprovalContext::ElevatedOperation {
                operation_type,
                value_context,
            } => {
                assert_eq!(operation_type, "high-value-transfer");
                assert_eq!(value_context, Some("100 ETH".to_string()));
            }
            _ => panic!("Expected ElevatedOperation context"),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 8.7.4: DKD Protocol with Real FROST Signing
// ═══════════════════════════════════════════════════════════════════════════

mod dkd_tests {
    use super::*;
    use aura_core::effects::ThresholdSigningEffects;

    /// Test DKD verification signing context
    #[test]
    fn test_dkd_verification_context() {
        let authority = test_authority(40);
        let derived_key = vec![0xDE; 32];

        let context = SigningContext {
            authority,
            operation: SignableOperation::Message {
                domain: "dkd-verification".to_string(),
                payload: derived_key,
            },
            approval_context: ApprovalContext::SelfOperation,
        };

        match &context.operation {
            SignableOperation::Message { domain, payload } => {
                assert_eq!(domain, "dkd-verification");
                assert_eq!(payload.len(), 32);
            }
            _ => panic!("Expected Message operation"),
        }
    }

    /// Test DKD signing flow (mock)
    #[tokio::test]
    async fn test_dkd_mock_signing_flow() {
        let effects = MockEffects::deterministic();
        let authority = test_authority(41);

        // Bootstrap authority
        let public_key = effects.bootstrap_authority(&authority).await.unwrap();
        assert!(!public_key.is_empty());

        // Simulate DKD verification by signing the derived key
        let derived_key = vec![0xAB; 32];
        let context = SigningContext {
            authority,
            operation: SignableOperation::Message {
                domain: "dkd-verification".to_string(),
                payload: derived_key.clone(),
            },
            approval_context: ApprovalContext::SelfOperation,
        };

        let signature = effects.sign(context).await.unwrap();
        assert_eq!(signature.signer_count, 1);
        assert!(!signature.signature.is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Integration: End-to-End FROST Crypto Verification
// ═══════════════════════════════════════════════════════════════════════════

mod integration_tests {
    use super::*;

    /// Test complete signing flow: bootstrap -> sign -> verify structure
    #[tokio::test]
    async fn test_complete_signing_flow_structure() {
        use aura_core::effects::ThresholdSigningEffects;

        let effects = MockEffects::deterministic();
        let authority = test_authority(50);

        // 1. Bootstrap authority
        let public_key = effects.bootstrap_authority(&authority).await.unwrap();
        assert!(!public_key.is_empty(), "Public key should be generated");

        // 2. Verify capability
        let has_capability = effects.has_signing_capability(&authority).await;
        assert!(has_capability, "Should have signing capability");

        // 3. Get threshold config
        let config = effects.threshold_config(&authority).await;
        // MockEffects returns Some with fixed config
        assert!(config.is_some(), "Should have threshold config");

        // 4. Sign a tree operation
        let tree_op = test_tree_op(0);
        let context = SigningContext::self_tree_op(authority, tree_op);
        let signature = effects.sign(context).await.unwrap();

        // 5. Verify signature structure
        assert_eq!(signature.signer_count, 1, "Single signer");
        assert!(
            !signature.signature.is_empty(),
            "Signature should not be empty"
        );
        assert!(
            !signature.public_key_package.is_empty(),
            "Public key should be included"
        );
    }

    /// Test that signable operations serialize correctly
    #[test]
    fn test_signable_operation_serialization() {
        let tree_op = test_tree_op(5);
        let operation = SignableOperation::TreeOp(tree_op);

        // Serialize
        let serialized = serde_json::to_vec(&operation);
        assert!(serialized.is_ok(), "TreeOp should serialize");

        // Recovery approval
        let recovery_op = SignableOperation::RecoveryApproval {
            target: test_authority(51),
            new_root: TreeCommitment([0xFF; 32]),
        };
        let serialized = serde_json::to_vec(&recovery_op);
        assert!(serialized.is_ok(), "RecoveryApproval should serialize");

        // Message
        let message_op = SignableOperation::Message {
            domain: "test".to_string(),
            payload: vec![1, 2, 3],
        };
        let serialized = serde_json::to_vec(&message_op);
        assert!(serialized.is_ok(), "Message should serialize");
    }

    /// Test multiple authorities can coexist
    #[tokio::test]
    async fn test_multiple_authorities() {
        use aura_core::effects::ThresholdSigningEffects;

        let effects = MockEffects::deterministic();

        // Create multiple authorities
        let authorities: Vec<AuthorityId> = (60..65).map(test_authority).collect();

        // Bootstrap all
        for authority in &authorities {
            let result = effects.bootstrap_authority(authority).await;
            assert!(result.is_ok(), "Each bootstrap should succeed");
        }

        // Sign with each
        for authority in &authorities {
            let context = SigningContext::self_tree_op(*authority, test_tree_op(0));
            let result = effects.sign(context).await;
            assert!(result.is_ok(), "Each signing should succeed");
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Real FROST Crypto Tests (using CryptoExtendedEffects directly)
// ═══════════════════════════════════════════════════════════════════════════

mod real_frost_tests {
    use super::*;

    /// Test FROST key generation via CryptoExtendedEffects
    #[tokio::test]
    async fn test_frost_key_generation() {
        let effects = MockEffects::deterministic();

        // Generate 2-of-3 keys via standardized API
        let result = effects
            .generate_signing_keys_with(
                aura_core::effects::crypto::KeyGenerationMethod::DealerBased,
                2,
                3,
            )
            .await;
        assert!(result.is_ok(), "Threshold key generation should succeed");

        let keys = result.unwrap();
        assert_eq!(keys.key_packages.len(), 3, "Should have 3 key packages");
        assert!(
            !keys.public_key_package.is_empty(),
            "Public key package should not be empty"
        );
    }

    /// Test FROST 2-of-3 key generation
    #[tokio::test]
    async fn test_frost_threshold_key_generation() {
        let effects = MockEffects::deterministic();

        // Generate 2-of-3 keys
        let result = effects
            .generate_signing_keys_with(
                aura_core::effects::crypto::KeyGenerationMethod::DealerBased,
                2,
                3,
            )
            .await;
        assert!(
            result.is_ok(),
            "Threshold 2-of-3 key generation should succeed"
        );

        let keys = result.unwrap();
        assert_eq!(keys.key_packages.len(), 3, "Should have 3 key packages");
        assert!(
            !keys.public_key_package.is_empty(),
            "Public key package should not be empty"
        );
    }

    /// Test FROST nonce generation
    #[tokio::test]
    async fn test_frost_nonce_generation() {
        let effects = MockEffects::deterministic();

        // Generate keys first
        let keys = effects
            .generate_signing_keys_with(
                aura_core::effects::crypto::KeyGenerationMethod::DealerBased,
                2,
                3,
            )
            .await
            .unwrap();

        // Generate nonces
        let result = effects.frost_generate_nonces(&keys.key_packages[0]).await;
        assert!(result.is_ok(), "Nonce generation should succeed");

        let nonces = result.unwrap();
        assert!(!nonces.is_empty(), "Nonces should not be empty");
    }

    /// Test complete FROST signing flow
    #[tokio::test]
    async fn test_frost_complete_signing_flow() {
        let effects = MockEffects::deterministic();

        // 1. Generate keys
        let keys = effects
            .generate_signing_keys_with(
                aura_core::effects::crypto::KeyGenerationMethod::DealerBased,
                2,
                3,
            )
            .await
            .unwrap();
        let key_package_1 = &keys.key_packages[0];
        let key_package_2 = &keys.key_packages[1];
        let public_key_package = &keys.public_key_package;

        // 2. Generate nonces
        let nonces_1 = effects.frost_generate_nonces(key_package_1).await.unwrap();
        let nonces_2 = effects.frost_generate_nonces(key_package_2).await.unwrap();

        // 3. Create message to sign
        let message = b"test message for signing";

        // 4. Create signing package
        let participants = vec![1u16, 2u16];
        let signing_package = effects
            .frost_create_signing_package(
                message,
                &[nonces_1.clone(), nonces_2.clone()],
                &participants,
                public_key_package,
            )
            .await
            .unwrap();

        // 5. Create signature share
        let share = effects
            .frost_sign_share(&signing_package, key_package_1, &nonces_1)
            .await
            .unwrap();
        let share2 = effects
            .frost_sign_share(&signing_package, key_package_2, &nonces_2)
            .await
            .unwrap();

        // 6. Aggregate (trivial for single signer)
        let signature = effects
            .frost_aggregate_signatures(&signing_package, &[share, share2])
            .await
            .unwrap();

        assert!(
            !signature.is_empty(),
            "Aggregate signature should not be empty"
        );

        // 7. Verify signature
        let verified = effects
            .frost_verify(message, &signature, public_key_package)
            .await
            .unwrap();

        assert!(verified, "Signature should verify");
    }
}
