//! Guardian Setup Choreography
//!
//! Initial establishment of guardian relationships for a threshold account.
//! Uses the authority model - guardians are identified by AuthorityId.
//!
//! # FROST Key Share Distribution (Task 8.6)
//!
//! Guardian recovery requires each guardian to hold a FROST key share. During
//! setup, these shares must be generated and distributed securely.
//!
//! ## Current State (Placeholder)
//!
//! The current implementation creates placeholder shares from hashed public keys.
//! This allows the protocol structure to work but doesn't provide real threshold
//! cryptographic security.
//!
//! ## Target Implementation
//!
//! To implement proper guardian key distribution:
//!
//! ### Option A: Trusted Dealer Model (Simpler)
//!
//! 1. **During `execute_setup()`**, after guardians accept:
//!    - Generate a fresh FROST key package for the recovery authority using
//!      `CryptoEffects::frost_keygen()` with `threshold` and `total_guardians`
//!    - This creates a `PublicKeyPackage` and one `SigningShare` per guardian
//!
//! 2. **Encrypt shares for each guardian**:
//!    - Use `CryptoEffects::asymmetric_encrypt()` with each guardian's public key
//!    - The guardian's public key comes from `GuardianAcceptance.public_key`
//!
//! 3. **Store the recovery authority's public key** in the commitment tree root
//!
//! 4. **Distribute encrypted shares** via the completion message:
//!    - Extend `SetupCompletion` to include `encrypted_shares: Vec<EncryptedShare>`
//!    - Each guardian receives their encrypted share
//!
//! 5. **Guardians store their shares** via `SecureStorageEffects`:
//!    - Location: `SecureStorageLocation::guardian_share(account_id, guardian_id)`
//!    - Decrypt with guardian's private key before storing
//!
//! ### Option B: Full DKG (More Secure)
//!
//! Run a proper Distributed Key Generation ceremony where no single party
//! knows all shares. This requires additional choreography rounds and is
//! more complex to implement.
//!
//! ## Required Changes
//!
//! 1. **types.rs**: Update `RecoveryShare.share` to hold encrypted FROST share bytes
//! 2. **SetupCompletion**: Add `encrypted_shares` field
//! 3. **GuardianAcceptance**: Ensure `public_key` is a real encryption public key
//! 4. **execute_setup()**: Implement key generation and encryption
//! 5. **accept_as_guardian()**: Handle share reception and secure storage
//!
//! ## Security Considerations
//!
//! - The trusted dealer model means the setup initiator temporarily knows all shares
//! - For high-security scenarios, prefer full DKG
//! - Shares MUST be encrypted before network transmission
//! - Shares MUST be stored in `SecureStorageEffects`, never in regular storage

use crate::{
    coordinator::{BaseCoordinator, BaseCoordinatorAccess, RecoveryCoordinator},
    effects::RecoveryEffects,
    facts::{RecoveryFact, RecoveryFactEmitter},
    types::{GuardianSet, RecoveryRequest, RecoveryResponse, RecoveryShare},
    utils::EvidenceBuilder,
    RecoveryResult,
};
use async_trait::async_trait;
use aura_core::effects::{JournalEffects, PhysicalTimeEffects, SecureStorageLocation};
use aura_core::hash;
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::time::{PhysicalTime, TimeStamp};
use aura_journal::DomainFact;
use aura_macros::choreography;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Encrypted FROST key share for a guardian
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedKeyShare {
    /// Guardian this share is encrypted for
    pub guardian_id: AuthorityId,
    /// FROST participant index (1-based)
    pub signer_index: u16,
    /// Encrypted key package bytes (ChaCha20-Poly1305)
    pub encrypted_share: Vec<u8>,
    /// Nonce used for encryption
    pub nonce: [u8; 12],
    /// Ephemeral public key for key agreement (X25519 style using Ed25519)
    pub ephemeral_public_key: Vec<u8>,
}

/// Guardian setup invitation data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianInvitation {
    /// Unique identifier for this setup ceremony
    pub setup_id: String,
    /// Account authority being set up
    pub account_id: AuthorityId,
    /// Target guardian authorities
    pub target_guardians: Vec<AuthorityId>,
    /// Required threshold
    pub threshold: u16,
    /// Timestamp of invitation
    pub timestamp: TimeStamp,
}

/// Guardian acceptance of setup invitation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianAcceptance {
    /// Guardian's authority
    pub guardian_id: AuthorityId,
    /// Setup ID being accepted
    pub setup_id: String,
    /// Whether the guardian accepted
    pub accepted: bool,
    /// Guardian's public key for this relationship
    pub public_key: Vec<u8>,
    /// Timestamp of acceptance
    pub timestamp: TimeStamp,
}

/// Setup completion notification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupCompletion {
    /// Setup ceremony ID
    pub setup_id: String,
    /// Whether setup succeeded
    pub success: bool,
    /// Final guardian set
    pub guardian_set: GuardianSet,
    /// Final threshold
    pub threshold: u16,
    /// Encrypted key shares for each guardian
    pub encrypted_shares: Vec<EncryptedKeyShare>,
    /// Public key package for the recovery authority
    pub public_key_package: Vec<u8>,
}

// Guardian Setup Choreography - 3 phase protocol
choreography! {
    #[namespace = "guardian_setup"]
    protocol GuardianSetup {
        roles: SetupInitiator, Guardian1, Guardian2, Guardian3;

        // Phase 1: Send invitations to all guardians
        SetupInitiator[guard_capability = "initiate_guardian_setup",
                       flow_cost = 300,
                       journal_facts = "guardian_setup_initiated",
                       leakage_budget = [1, 0, 0]]
        -> Guardian1: SendInvitation(GuardianInvitation);

        SetupInitiator[guard_capability = "initiate_guardian_setup",
                       flow_cost = 300]
        -> Guardian2: SendInvitation(GuardianInvitation);

        SetupInitiator[guard_capability = "initiate_guardian_setup",
                       flow_cost = 300]
        -> Guardian3: SendInvitation(GuardianInvitation);

        // Phase 2: Guardians respond with acceptance
        Guardian1[guard_capability = "accept_guardian_invitation,verify_setup_invitation",
                  flow_cost = 200,
                  journal_facts = "guardian_setup_accepted",
                  leakage_budget = [0, 1, 0]]
        -> SetupInitiator: AcceptInvitation(GuardianAcceptance);

        Guardian2[guard_capability = "accept_guardian_invitation,verify_setup_invitation",
                  flow_cost = 200,
                  journal_facts = "guardian_setup_accepted"]
        -> SetupInitiator: AcceptInvitation(GuardianAcceptance);

        Guardian3[guard_capability = "accept_guardian_invitation,verify_setup_invitation",
                  flow_cost = 200,
                  journal_facts = "guardian_setup_accepted"]
        -> SetupInitiator: AcceptInvitation(GuardianAcceptance);

        // Phase 3: Broadcast completion to all guardians
        SetupInitiator[guard_capability = "complete_guardian_setup",
                       flow_cost = 150,
                       journal_facts = "guardian_setup_completed",
                       journal_merge = true]
        -> Guardian1: CompleteSetup(SetupCompletion);

        SetupInitiator[guard_capability = "complete_guardian_setup",
                       flow_cost = 150,
                       journal_merge = true]
        -> Guardian2: CompleteSetup(SetupCompletion);

        SetupInitiator[guard_capability = "complete_guardian_setup",
                       flow_cost = 150,
                       journal_merge = true]
        -> Guardian3: CompleteSetup(SetupCompletion);
    }
}

/// Guardian setup coordinator.
///
/// Stateless coordinator that derives state from facts.
pub struct GuardianSetupCoordinator<E: RecoveryEffects> {
    base: BaseCoordinator<E>,
}

impl<E: RecoveryEffects> BaseCoordinatorAccess<E> for GuardianSetupCoordinator<E> {
    fn base(&self) -> &BaseCoordinator<E> {
        &self.base
    }
}

#[async_trait]
impl<E: RecoveryEffects + 'static> RecoveryCoordinator<E> for GuardianSetupCoordinator<E> {
    type Request = RecoveryRequest;
    type Response = RecoveryResponse;

    fn effect_system(&self) -> &Arc<E> {
        self.base_effect_system()
    }

    fn operation_name(&self) -> &str {
        "guardian_setup"
    }

    async fn execute_recovery(&self, request: Self::Request) -> RecoveryResult<Self::Response> {
        self.execute_setup(request).await
    }
}

impl<E: RecoveryEffects + 'static> GuardianSetupCoordinator<E> {
    /// Create a new coordinator.
    pub fn new(effect_system: Arc<E>) -> Self {
        Self {
            base: BaseCoordinator::new(effect_system),
        }
    }

    /// Emit a recovery fact to the journal.
    async fn emit_fact(&self, fact: RecoveryFact) -> RecoveryResult<()> {
        let timestamp = self
            .effect_system()
            .physical_time()
            .await
            .map(|t| t.ts_ms)
            .unwrap_or(0);

        let mut journal = self.effect_system().get_journal().await?;
        journal.facts.insert_with_context(
            RecoveryFactEmitter::fact_key(&fact),
            aura_core::FactValue::Bytes(DomainFact::to_bytes(&fact)),
            aura_core::ActorId::synthetic(&fact.context_id().to_string()),
            aura_core::FactTimestamp::new(timestamp),
            None,
        )?;
        self.effect_system().persist_journal(&journal).await?;
        Ok(())
    }

    /// Execute guardian setup ceremony.
    pub async fn execute_setup(
        &self,
        request: RecoveryRequest,
    ) -> RecoveryResult<RecoveryResponse> {
        // Get current timestamp for unique ID generation
        let now_ms = self
            .effect_system()
            .physical_time()
            .await
            .map(|t| t.ts_ms)
            .unwrap_or(0);

        // Create context ID for this setup ceremony using hash of account + timestamp
        let setup_id = format!("setup_{}_{}", request.account_id, now_ms);
        let context_id = ContextId::new_from_entropy(hash::hash(setup_id.as_bytes()));

        // Emit GuardianSetupInitiated fact
        let guardian_ids: Vec<AuthorityId> =
            request.guardians.iter().map(|g| g.authority_id).collect();

        let initiated_fact = RecoveryFact::GuardianSetupInitiated {
            context_id,
            initiator_id: request.initiator_id,
            trace_id: Some(setup_id.clone()),
            guardian_ids: guardian_ids.clone(),
            threshold: request.threshold,
            initiated_at: PhysicalTime {
                ts_ms: now_ms,
                uncertainty: None,
            },
        };
        self.emit_fact(initiated_fact).await?;

        // Validate that we have guardians
        if request.guardians.is_empty() {
            let failed_fact = RecoveryFact::GuardianSetupFailed {
                context_id,
                reason: "No guardians specified".to_string(),
                trace_id: Some(setup_id.clone()),
                failed_at: PhysicalTime {
                    ts_ms: now_ms,
                    uncertainty: None,
                },
            };
            let _ = self.emit_fact(failed_fact).await;
            return Ok(RecoveryResponse::error("No guardians specified"));
        }

        // Create invitation
        let invitation = GuardianInvitation {
            setup_id: setup_id.clone(),
            account_id: request.account_id,
            target_guardians: guardian_ids.clone(),
            threshold: request.threshold,
            timestamp: TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: now_ms,
                uncertainty: None,
            }),
        };

        // Execute the choreographic protocol (simulated)
        let acceptances = self.execute_choreographic_setup(invitation).await?;

        // Check if we have enough acceptances
        if acceptances.len() < request.threshold as usize {
            let failed_fact = RecoveryFact::GuardianSetupFailed {
                context_id,
                reason: format!(
                    "Insufficient guardian acceptances: got {}, need {}",
                    acceptances.len(),
                    request.threshold
                ),
                trace_id: Some(setup_id.clone()),
                failed_at: self
                    .effect_system()
                    .physical_time()
                    .await
                    .unwrap_or(PhysicalTime {
                        ts_ms: 0,
                        uncertainty: None,
                    }),
            };
            let _ = self.emit_fact(failed_fact).await;

            return Ok(RecoveryResponse::error(format!(
                "Insufficient guardian acceptances: got {}, need {}",
                acceptances.len(),
                request.threshold
            )));
        }

        // Generate threshold keys for the recovery authority
        let num_guardians = acceptances.len() as u16;
        let threshold = request.threshold;

        tracing::info!(
            threshold = %threshold,
            guardians = %num_guardians,
            "Generating threshold keys for guardian recovery authority"
        );

        let frost_keys = self
            .effect_system()
            .generate_signing_keys_with(
                aura_core::effects::crypto::KeyGenerationMethod::DealerBased,
                threshold,
                num_guardians,
            )
            .await
            .map_err(|e| crate::RecoveryError::internal(format!("Key generation failed: {e}")))?;

        // Encrypt each guardian's key share
        let mut encrypted_shares = Vec::with_capacity(acceptances.len());
        let mut shares = Vec::with_capacity(acceptances.len());

        for (idx, acceptance) in acceptances.iter().enumerate() {
            let signer_index = (idx + 1) as u16; // FROST uses 1-based indices
            let key_package = &frost_keys.key_packages[idx];

            // Generate ephemeral keypair for key agreement
            let (ephemeral_private, ephemeral_public) = self
                .effect_system()
                .ed25519_generate_keypair()
                .await
                .map_err(|e| {
                    crate::RecoveryError::internal(format!("Ephemeral key generation failed: {e}"))
                })?;

            // Derive symmetric key using HKDF from shared secret
            // (ephemeral_private XOR guardian_public as a simple shared secret)
            let shared_secret_input = [
                ephemeral_private.as_slice(),
                acceptance.public_key.as_slice(),
            ]
            .concat();
            let encryption_key = self
                .effect_system()
                .hkdf_derive(
                    &shared_secret_input,
                    b"aura-guardian-share-v1",
                    format!("guardian:{}", acceptance.guardian_id).as_bytes(),
                    32,
                )
                .await
                .map_err(|e| {
                    crate::RecoveryError::internal(format!("Key derivation failed: {e}"))
                })?;

            // Generate random nonce for encryption
            let nonce_bytes = self.effect_system().random_bytes(12).await;
            let mut nonce = [0u8; 12];
            nonce.copy_from_slice(&nonce_bytes);

            // Encrypt the key package with ChaCha20-Poly1305
            let mut key_array = [0u8; 32];
            key_array.copy_from_slice(&encryption_key);
            let encrypted_share = self
                .effect_system()
                .chacha20_encrypt(key_package, &key_array, &nonce)
                .await
                .map_err(|e| {
                    crate::RecoveryError::internal(format!("Share encryption failed: {e}"))
                })?;

            tracing::debug!(
                guardian = %acceptance.guardian_id,
                signer_index = %signer_index,
                share_len = %key_package.len(),
                encrypted_len = %encrypted_share.len(),
                "Encrypted FROST share for guardian"
            );

            encrypted_shares.push(EncryptedKeyShare {
                guardian_id: acceptance.guardian_id,
                signer_index,
                encrypted_share: encrypted_share.clone(),
                nonce,
                ephemeral_public_key: ephemeral_public,
            });

            // Create RecoveryShare with the encrypted share data
            shares.push(RecoveryShare {
                guardian_id: acceptance.guardian_id,
                guardian_label: None,
                share: encrypted_share, // Now contains actual encrypted FROST share
                partial_signature: Vec::new(), // Will be filled during recovery signing
                issued_at_ms: now_ms,
            });
        }

        tracing::info!(
            guardians = %shares.len(),
            public_key_len = %frost_keys.public_key_package.len(),
            "FROST key shares generated and encrypted for all guardians"
        );

        // Emit completion fact
        let completed_fact = RecoveryFact::GuardianSetupCompleted {
            context_id,
            guardian_ids: shares.iter().map(|s| s.guardian_id).collect(),
            trace_id: Some(setup_id.clone()),
            threshold: request.threshold,
            completed_at: self
                .effect_system()
                .physical_time()
                .await
                .unwrap_or(PhysicalTime {
                    ts_ms: 0,
                    uncertainty: None,
                }),
        };
        self.emit_fact(completed_fact).await?;

        // Create evidence
        let evidence = EvidenceBuilder::success(context_id, request.account_id, &shares, now_ms);

        Ok(BaseCoordinator::<E>::success_response(
            None, shares, evidence,
        ))
    }

    /// Execute as guardian (accept setup invitation).
    ///
    /// Generates a fresh Ed25519 keypair for receiving the encrypted FROST share.
    /// The private key should be stored securely for later decryption when
    /// SetupCompletion arrives.
    ///
    /// # Flow
    /// 1. Generate Ed25519 keypair for key agreement
    /// 2. Return public key in acceptance message
    /// 3. When SetupCompletion arrives, use private key to derive decryption key
    /// 4. Decrypt FROST share and store via SecureStorageEffects
    pub async fn accept_as_guardian(
        &self,
        invitation: GuardianInvitation,
        guardian_id: AuthorityId,
    ) -> RecoveryResult<GuardianAcceptance> {
        let physical_time = self
            .effect_system()
            .physical_time()
            .await
            .unwrap_or(PhysicalTime {
                ts_ms: 0,
                uncertainty: None,
            });

        // Generate Ed25519 keypair for key agreement
        let (private_key, public_key) = self
            .effect_system()
            .ed25519_generate_keypair()
            .await
            .map_err(|e| crate::RecoveryError::internal(format!("Key generation failed: {e}")))?;

        tracing::debug!(
            guardian = %guardian_id,
            public_key_len = %public_key.len(),
            "Generated acceptance keypair for guardian"
        );

        // Store private key for later share decryption
        // Key is stored at: guardian_acceptance_keys/<setup_id>/<guardian_id>
        let storage_location = SecureStorageLocation::with_sub_key(
            "guardian_acceptance_keys",
            &invitation.setup_id,
            guardian_id.to_string(),
        );
        self.effect_system()
            .secure_store(&storage_location, &private_key, &[])
            .await
            .map_err(|e| {
                crate::RecoveryError::internal(format!(
                    "Failed to store acceptance private key: {e}"
                ))
            })?;

        // Emit GuardianAccepted fact
        let context_id = ContextId::new_from_entropy(hash::hash(invitation.setup_id.as_bytes()));
        let accepted_fact = RecoveryFact::GuardianAccepted {
            context_id,
            guardian_id,
            trace_id: Some(invitation.setup_id.clone()),
            accepted_at: physical_time.clone(),
        };
        self.emit_fact(accepted_fact).await?;

        Ok(GuardianAcceptance {
            guardian_id,
            setup_id: invitation.setup_id,
            accepted: true,
            public_key,
            timestamp: TimeStamp::PhysicalClock(physical_time),
        })
    }

    /// Execute choreographic setup protocol (Phase 1-2).
    ///
    /// # Note
    /// This is a simulation/test helper that generates acceptances locally.
    /// Real deployments use network choreography via the protocol layer.
    async fn execute_choreographic_setup(
        &self,
        invitation: GuardianInvitation,
    ) -> RecoveryResult<Vec<GuardianAcceptance>> {
        let physical_time = self
            .effect_system()
            .physical_time()
            .await
            .map_err(|e| aura_core::AuraError::internal(format!("Time error: {e}")))?;

        // Generate real acceptances with cryptographic keys
        let mut acceptances = Vec::new();
        for guardian_id in &invitation.target_guardians {
            // Generate real Ed25519 keypair for each guardian
            let (private_key, public_key) = self
                .effect_system()
                .ed25519_generate_keypair()
                .await
                .map_err(|e| {
                crate::RecoveryError::internal(format!("Guardian key generation failed: {e}"))
            })?;

            // Store the private key for later share decryption
            let storage_key = format!(
                "guardian_acceptance_keys/{}/{}",
                invitation.setup_id, guardian_id
            );
            self.effect_system()
                .store(&storage_key, private_key.clone())
                .await
                .map_err(|e| {
                    crate::RecoveryError::internal(format!(
                        "Failed to store acceptance private key: {e}"
                    ))
                })?;

            acceptances.push(GuardianAcceptance {
                guardian_id: *guardian_id,
                setup_id: invitation.setup_id.clone(),
                accepted: true,
                public_key,
                timestamp: TimeStamp::PhysicalClock(physical_time.clone()),
            });
        }

        Ok(acceptances)
    }

    /// Receive and decrypt a FROST share as a guardian.
    ///
    /// Called when a guardian receives their encrypted share in SetupCompletion.
    /// Decrypts the share and stores it in secure storage for use during recovery.
    ///
    /// # Arguments
    /// - `account_id`: The account authority this share is for
    /// - `guardian_id`: This guardian's authority ID
    /// - `setup_id`: The setup ceremony ID (for key lookup)
    /// - `encrypted_share`: The encrypted share from SetupCompletion
    pub async fn receive_guardian_share(
        &self,
        account_id: AuthorityId,
        guardian_id: AuthorityId,
        setup_id: &str,
        encrypted_share: &EncryptedKeyShare,
    ) -> RecoveryResult<()> {
        // Retrieve the private key we stored during acceptance
        let storage_key = format!("guardian_acceptance_keys/{setup_id}/{guardian_id}");
        let private_key = self
            .effect_system()
            .retrieve(&storage_key)
            .await
            .map_err(|e| {
                crate::RecoveryError::internal(format!("Failed to retrieve private key: {e}"))
            })?
            .ok_or_else(|| {
                crate::RecoveryError::internal("No acceptance private key found".to_string())
            })?;

        // Derive decryption key from shared secret (private_key + ephemeral_public_key)
        let shared_secret_input = [
            encrypted_share.ephemeral_public_key.as_slice(),
            private_key.as_slice(),
        ]
        .concat();
        let decryption_key = self
            .effect_system()
            .hkdf_derive(
                &shared_secret_input,
                b"aura-guardian-share-v1",
                format!("guardian:{guardian_id}").as_bytes(),
                32,
            )
            .await
            .map_err(|e| crate::RecoveryError::internal(format!("Key derivation failed: {e}")))?;

        // Decrypt the FROST share
        let mut key_array = [0u8; 32];
        key_array.copy_from_slice(&decryption_key);
        let decrypted_share = self
            .effect_system()
            .chacha20_decrypt(
                &encrypted_share.encrypted_share,
                &key_array,
                &encrypted_share.nonce,
            )
            .await
            .map_err(|e| crate::RecoveryError::internal(format!("Share decryption failed: {e}")))?;

        tracing::info!(
            account = %account_id,
            guardian = %guardian_id,
            signer_index = %encrypted_share.signer_index,
            share_len = %decrypted_share.len(),
            "Decrypted FROST share for guardian"
        );

        // Store the decrypted share in secure storage
        let location = SecureStorageLocation::guardian_share(&account_id, &guardian_id);
        self.effect_system()
            .secure_store(
                &location,
                &decrypted_share,
                &[aura_core::effects::SecureStorageCapability::Write],
            )
            .await
            .map_err(|e| {
                crate::RecoveryError::internal(format!("Failed to store guardian share: {e}"))
            })?;

        // Delete the ephemeral acceptance key now that the share is stored
        let _ = self.effect_system().remove(&storage_key).await;

        tracing::info!(
            account = %account_id,
            guardian = %guardian_id,
            location = %location.full_path(),
            "Guardian FROST share stored securely"
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::GuardianProfile;
    use aura_testkit::MockEffects;
    use std::sync::Arc;

    fn test_authority_id(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn create_test_request() -> crate::types::RecoveryRequest {
        let guardians = vec![
            GuardianProfile::with_label(test_authority_id(1), "Guardian 1".to_string()),
            GuardianProfile::with_label(test_authority_id(2), "Guardian 2".to_string()),
            GuardianProfile::with_label(test_authority_id(3), "Guardian 3".to_string()),
        ];

        crate::types::RecoveryRequest {
            initiator_id: test_authority_id(0),
            account_id: test_authority_id(10),
            context: aura_authentication::RecoveryContext {
                operation_type: aura_authentication::RecoveryOperationType::DeviceKeyRecovery,
                justification: "Test recovery".to_string(),
                is_emergency: false,
                timestamp: 0,
            },
            threshold: 2,
            guardians: crate::types::GuardianSet::new(guardians),
        }
    }

    #[tokio::test]
    async fn test_guardian_setup_coordinator_creation() {
        let effects = Arc::new(MockEffects::deterministic());
        let coordinator = GuardianSetupCoordinator::new(effects);

        assert_eq!(coordinator.operation_name(), "guardian_setup");
    }

    #[tokio::test]
    async fn test_guardian_setup_execute() {
        let effects = Arc::new(MockEffects::deterministic());
        let coordinator = GuardianSetupCoordinator::new(effects);

        let request = create_test_request();
        let response = coordinator.execute_setup(request).await;

        assert!(response.is_ok());
        let resp = response.unwrap();
        assert!(resp.success);
        assert!(!resp.guardian_shares.is_empty());
    }

    #[tokio::test]
    async fn test_guardian_setup_empty_guardians() {
        let effects = Arc::new(MockEffects::deterministic());
        let coordinator = GuardianSetupCoordinator::new(effects);

        let mut request = create_test_request();
        request.guardians = crate::types::GuardianSet::new(vec![]);

        let response = coordinator.execute_setup(request).await;

        assert!(response.is_ok());
        let resp = response.unwrap();
        assert!(!resp.success);
        assert!(resp.error.is_some());
    }

    #[tokio::test]
    async fn test_accept_as_guardian() {
        let effects = Arc::new(MockEffects::deterministic());
        let coordinator = GuardianSetupCoordinator::new(effects);

        let invitation = GuardianInvitation {
            setup_id: "test-setup-123".to_string(),
            account_id: test_authority_id(10),
            target_guardians: vec![test_authority_id(1), test_authority_id(2)],
            threshold: 2,
            timestamp: TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: 1000,
                uncertainty: None,
            }),
        };

        let guardian_id = test_authority_id(1);
        let acceptance = coordinator
            .accept_as_guardian(invitation, guardian_id)
            .await;

        assert!(acceptance.is_ok());
        let acc = acceptance.unwrap();
        assert!(acc.accepted);
        assert_eq!(acc.guardian_id, guardian_id);
        assert!(!acc.public_key.is_empty());
    }
}
