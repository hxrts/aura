//! Guardian Setup Choreography
//!
//! Establishes guardian relationships for a threshold account. Guardians are
//! identified by `AuthorityId` and hold encrypted FROST key shares for recovery.
//!
//! The setup is a three-phase choreography (defined via `tell!` macro):
//! invitation, acceptance, and completion. `GuardianSetupCoordinator` drives
//! the ceremony through the `RecoveryCoordinator` trait.
//!
//! Key types: `GuardianInvitation`, `GuardianAcceptance`, `SetupCompletion`,
//! `EncryptedKeyShare`. Capability-gated helpers `validate_setup_inputs` and
//! `build_setup_completion` enforce parameter shape at the feature boundary.
//!
//! Guardian setup transition sketch:
//! `Initiated -> InvitationsIssued -> AcceptancesCollected -> SharesGenerated -> Completed`
//! `Initiated -> InvitationsIssued -> AcceptancesCollected -> Failed(InsufficientAcceptances)`
//! `Initiated -> Failed(NoGuardiansSpecified)`

use crate::{
    coordinator::{BaseCoordinator, BaseCoordinatorAccess, RecoveryCoordinator},
    effects::RecoveryEffects,
    facts::RecoveryFact,
    types::{GuardianProfile, GuardianSet, RecoveryRequest, RecoveryResponse, RecoveryShare},
    utils::{
        workflow::{
            context_id_from_operation_id, current_physical_time_or_zero, exact_physical_time,
            persist_recovery_fact, trace_id,
        },
        EvidenceBuilder,
    },
    RecoveryResult,
};
use async_trait::async_trait;
use aura_core::effects::{PhysicalTimeEffects, SecureStorageLocation};
use aura_core::time::TimeStamp;
use aura_core::types::identifiers::AuthorityId;
use aura_macros::tell;
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
    // aura-security: raw-secret-field-justified owner=security-refactor expires=before-release remediation=work/2.md encrypted recovery wire payload; plaintext share must use secret wrappers before encryption.
    pub encrypted_share: Vec<u8>,
    /// Nonce used for encryption
    pub nonce: [u8; 12],
    /// Untrusted key material: ephemeral agreement key carried by the remote setup payload.
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
    /// Untrusted key material: claimed guardian relationship key; verification must resolve expected guardian state separately.
    pub public_key: Vec<u8>,
    /// Timestamp of acceptance
    pub timestamp: TimeStamp,
}

/// Explicit guardian decision for setup participation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardianDecision {
    Accepted,
    Declined,
}

impl GuardianAcceptance {
    /// Return the explicit decision encoded by this acceptance payload.
    pub fn decision(&self) -> GuardianDecision {
        if self.accepted {
            GuardianDecision::Accepted
        } else {
            GuardianDecision::Declined
        }
    }
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
    /// Untrusted key material: setup completion payload; verification must resolve expected recovery authority key separately.
    pub public_key_package: Vec<u8>,
}

/// Explicit outcome for a guardian setup completion payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupCompletionOutcome {
    Succeeded,
    Failed,
}

impl SetupCompletion {
    /// Return the explicit outcome encoded by this completion payload.
    pub fn outcome(&self) -> SetupCompletionOutcome {
        if self.success {
            SetupCompletionOutcome::Succeeded
        } else {
            SetupCompletionOutcome::Failed
        }
    }
}

const GUARDIAN_SETUP_INPUT_VALIDATION_CAPABILITY: &str = "guardian_setup_input_validation";
const GUARDIAN_SETUP_COMPLETION_BUILD_CAPABILITY: &str = "guardian_setup_completion_build";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SetupAcceptanceProgress {
    BelowThreshold { accepted: usize, threshold: u16 },
    ThresholdReached { accepted: usize, threshold: u16 },
}

fn classify_setup_acceptances(accepted: usize, threshold: u16) -> SetupAcceptanceProgress {
    if accepted >= threshold as usize {
        SetupAcceptanceProgress::ThresholdReached {
            accepted,
            threshold,
        }
    } else {
        SetupAcceptanceProgress::BelowThreshold {
            accepted,
            threshold,
        }
    }
}

/// Validate the feature-level guardian setup parameter shape.
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "guardian_setup_input_validation",
    family = "runtime_helper"
)]
pub fn validate_setup_inputs(guardians: &[AuthorityId], threshold: u16) -> Result<(), String> {
    let _ = GUARDIAN_SETUP_INPUT_VALIDATION_CAPABILITY;
    if guardians.len() != 3 {
        return Err("Guardian setup requires exactly three guardians".to_string());
    }

    if threshold == 0 {
        return Err("Guardian setup threshold must be at least 1".to_string());
    }

    if threshold as usize > guardians.len() {
        return Err(format!(
            "Guardian setup threshold {} exceeds guardian count {}",
            threshold,
            guardians.len()
        ));
    }

    Ok(())
}

/// Build the final setup completion payload from guardian responses.
#[must_use]
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "guardian_setup_completion_build",
    family = "runtime_helper"
)]
pub fn build_setup_completion(
    setup_id: &str,
    threshold: u16,
    acceptances: Vec<GuardianAcceptance>,
) -> SetupCompletion {
    let _ = GUARDIAN_SETUP_COMPLETION_BUILD_CAPABILITY;
    let accepted_guardians: Vec<AuthorityId> = acceptances
        .iter()
        .filter(|acceptance| acceptance.decision() == GuardianDecision::Accepted)
        .map(|acceptance| acceptance.guardian_id)
        .collect();

    let guardian_set = GuardianSet::new(
        accepted_guardians
            .iter()
            .copied()
            .map(GuardianProfile::new)
            .collect(),
    );

    SetupCompletion {
        setup_id: setup_id.to_string(),
        success: accepted_guardians.len() >= threshold as usize,
        guardian_set,
        threshold,
        encrypted_shares: Vec::new(),
        public_key_package: Vec::new(),
    }
}

// Guardian Setup Choreography - 3 phase protocol
tell!(include_str!("src/guardian_setup.tell"));

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
        persist_recovery_fact(self.effect_system().as_ref(), &fact).await
    }

    fn setup_id(account_id: &AuthorityId, now_ms: u64) -> String {
        format!("setup_{account_id}_{now_ms}")
    }

    fn setup_context_id(setup_id: &str) -> aura_core::types::identifiers::ContextId {
        context_id_from_operation_id(setup_id)
    }

    async fn emit_failed_setup(
        &self,
        context_id: aura_core::types::identifiers::ContextId,
        setup_id: &str,
        reason: impl Into<String>,
    ) -> RecoveryResult<()> {
        let failed_fact = RecoveryFact::GuardianSetupFailed {
            context_id,
            reason: reason.into(),
            trace_id: trace_id(setup_id),
            failed_at: current_physical_time_or_zero(self.effect_system().as_ref()).await,
        };
        self.emit_fact(failed_fact).await
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
        let setup_id = Self::setup_id(&request.account_id, now_ms);
        let context_id = Self::setup_context_id(&setup_id);

        // Emit GuardianSetupInitiated fact
        let guardian_ids: Vec<AuthorityId> =
            request.guardians.iter().map(|g| g.authority_id).collect();

        let initiated_fact = RecoveryFact::GuardianSetupInitiated {
            context_id,
            initiator_id: request.initiator_id,
            trace_id: trace_id(&setup_id),
            guardian_ids: guardian_ids.clone(),
            threshold: request.threshold,
            initiated_at: exact_physical_time(now_ms),
        };
        self.emit_fact(initiated_fact).await?;

        // Validate that we have guardians
        if request.guardians.is_empty() {
            let _ = self
                .emit_failed_setup(context_id, &setup_id, "No guardians specified")
                .await;
            return Ok(RecoveryResponse::error("No guardians specified"));
        }

        // Create invitation
        let invitation = GuardianInvitation {
            setup_id: setup_id.clone(),
            account_id: request.account_id,
            target_guardians: guardian_ids.clone(),
            threshold: request.threshold,
            timestamp: TimeStamp::PhysicalClock(exact_physical_time(now_ms)),
        };

        // Execute the choreographic protocol using the runtime adapter.
        let acceptances = self.execute_choreographic_setup(invitation).await?;

        // Check if we have enough acceptances
        debug_assert!(
            acceptances
                .iter()
                .all(|acceptance| acceptance.setup_id == setup_id),
            "guardian acceptances must all belong to the active setup"
        );
        debug_assert!(
            acceptances
                .iter()
                .all(|acceptance| acceptance.decision() == GuardianDecision::Accepted),
            "local setup helper should only surface accepted guardians"
        );

        match classify_setup_acceptances(acceptances.len(), request.threshold) {
            SetupAcceptanceProgress::BelowThreshold {
                accepted,
                threshold,
            } => {
                let reason =
                    format!("Insufficient guardian acceptances: got {accepted}, need {threshold}");
                let _ = self
                    .emit_failed_setup(context_id, &setup_id, reason.clone())
                    .await;

                return Ok(RecoveryResponse::error(reason));
            }
            SetupAcceptanceProgress::ThresholdReached { .. } => {}
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

            // Derive a symmetric key from the shared secret.
            // (ephemeral_private XOR guardian_public as a simple shared secret)
            let shared_secret_input = [
                ephemeral_private.as_slice(),
                acceptance.public_key.as_slice(),
            ]
            .concat();
            let encryption_key = self
                .effect_system()
                .kdf_derive(
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
            trace_id: trace_id(&setup_id),
            threshold: request.threshold,
            completed_at: current_physical_time_or_zero(self.effect_system().as_ref()).await,
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
        let physical_time = current_physical_time_or_zero(self.effect_system().as_ref()).await;

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
        let context_id = Self::setup_context_id(&invitation.setup_id);
        let accepted_fact = RecoveryFact::GuardianAccepted {
            context_id,
            guardian_id,
            trace_id: trace_id(&invitation.setup_id),
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
            .kdf_derive(
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
    use aura_core::time::PhysicalTime;
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

    #[test]
    fn validate_setup_inputs_requires_exactly_three_guardians() {
        let err = match validate_setup_inputs(&[test_authority_id(1), test_authority_id(2)], 2) {
            Ok(()) => panic!("two guardians should be rejected"),
            Err(error) => error,
        };
        assert_eq!(err, "Guardian setup requires exactly three guardians");
    }

    #[test]
    fn build_setup_completion_derives_guardian_set_from_acceptances() {
        let accepted = GuardianAcceptance {
            guardian_id: test_authority_id(1),
            setup_id: "setup-1".to_string(),
            accepted: true,
            public_key: vec![1, 2, 3],
            timestamp: TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: 1,
                uncertainty: None,
            }),
        };
        let declined = GuardianAcceptance {
            guardian_id: test_authority_id(2),
            setup_id: "setup-1".to_string(),
            accepted: false,
            public_key: vec![4, 5, 6],
            timestamp: TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: 2,
                uncertainty: None,
            }),
        };

        let completion = build_setup_completion("setup-1", 1, vec![accepted.clone(), declined]);
        let accepted_guardians: Vec<AuthorityId> = completion
            .guardian_set
            .iter()
            .map(|guardian| guardian.authority_id)
            .collect();

        assert!(completion.success);
        assert_eq!(accepted_guardians, vec![accepted.guardian_id]);
    }

    #[test]
    fn guardian_setup_acceptance_transcript_binds_setup_and_guardian() {
        let acceptance = GuardianAcceptance {
            guardian_id: test_authority_id(1),
            setup_id: "setup-1".to_string(),
            accepted: true,
            public_key: vec![1, 2, 3],
            timestamp: TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: 1,
                uncertainty: None,
            }),
        };
        let mut different_setup = acceptance.clone();
        different_setup.setup_id = "setup-2".to_string();
        let mut different_guardian = acceptance.clone();
        different_guardian.guardian_id = test_authority_id(2);

        let base =
            aura_signature::encode_transcript("aura.guardian-setup.acceptance", 1, &acceptance)
                .unwrap();
        let setup = aura_signature::encode_transcript(
            "aura.guardian-setup.acceptance",
            1,
            &different_setup,
        )
        .unwrap();
        let guardian = aura_signature::encode_transcript(
            "aura.guardian-setup.acceptance",
            1,
            &different_guardian,
        )
        .unwrap();

        assert_ne!(base, setup);
        assert_ne!(base, guardian);
    }

    #[test]
    fn guardian_acceptance_exposes_explicit_decision() {
        let accepted = GuardianAcceptance {
            guardian_id: test_authority_id(1),
            setup_id: "setup-2".to_string(),
            accepted: true,
            public_key: vec![1],
            timestamp: TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: 3,
                uncertainty: None,
            }),
        };
        let declined = GuardianAcceptance {
            accepted: false,
            ..accepted.clone()
        };

        assert_eq!(accepted.decision(), GuardianDecision::Accepted);
        assert_eq!(declined.decision(), GuardianDecision::Declined);
    }

    #[test]
    fn setup_completion_exposes_explicit_outcome() {
        let completion = SetupCompletion {
            setup_id: "setup-3".to_string(),
            success: true,
            guardian_set: GuardianSet::new(vec![GuardianProfile::new(test_authority_id(1))]),
            threshold: 1,
            encrypted_shares: Vec::new(),
            public_key_package: Vec::new(),
        };

        assert_eq!(completion.outcome(), SetupCompletionOutcome::Succeeded);
        assert_eq!(
            SetupCompletion {
                success: false,
                ..completion
            }
            .outcome(),
            SetupCompletionOutcome::Failed
        );
    }
}

#[cfg(test)]
mod theorem_pack_tests {
    use super::telltale_session_types_guardian_setup;
    use aura_protocol::admission::{
        CAPABILITY_PROTOCOL_ENVELOPE_BRIDGE, CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADHERENCE,
        CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADMISSION, THEOREM_PACK_AURA_AUTHORITY_EVIDENCE,
    };

    #[test]
    fn guardian_setup_proof_status_exposes_required_authority_pack() {
        assert_eq!(
            telltale_session_types_guardian_setup::proof_status::REQUIRED_THEOREM_PACKS,
            &[THEOREM_PACK_AURA_AUTHORITY_EVIDENCE]
        );
    }

    #[test]
    fn guardian_setup_manifest_emits_authority_evidence_metadata() {
        let manifest = telltale_session_types_guardian_setup::vm_artifacts::composition_manifest();
        let mut capabilities = manifest.required_theorem_pack_capabilities.clone();
        capabilities.sort();
        assert_eq!(
            manifest.required_theorem_packs,
            vec![THEOREM_PACK_AURA_AUTHORITY_EVIDENCE.to_string()]
        );
        assert_eq!(
            capabilities,
            vec![
                CAPABILITY_PROTOCOL_ENVELOPE_BRIDGE.to_string(),
                CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADHERENCE.to_string(),
                CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADMISSION.to_string(),
            ]
        );
    }
}
