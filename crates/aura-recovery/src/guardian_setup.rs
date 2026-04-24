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
    types::{GuardianProfile, GuardianSet, RecoveryRequest, RecoveryResponse},
    utils::workflow::{
        context_id_from_operation_id, current_physical_time_or_zero, persist_recovery_fact,
        trace_id,
    },
    RecoveryResult,
};
use async_trait::async_trait;
use aura_core::effects::{CryptoEffects, SecureStorageLocation};
use aura_core::key_resolution::TrustedKeyResolver;
use aura_core::time::TimeStamp;
use aura_core::types::identifiers::AuthorityId;
use aura_core::{AuraError, Hash32};
use aura_macros::tell;
use aura_signature::{sign_ed25519_transcript, verify_ed25519_transcript, SecurityTranscript};
use curve25519_dalek::{montgomery::MontgomeryPoint, scalar::Scalar};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::sync::Arc;

const GUARDIAN_SHARE_ENCRYPTION_PROTOCOL_VERSION: u8 = 1;
const GUARDIAN_SHARE_ENCRYPTION_KDF_DOMAIN: &[u8] = b"aura.recovery.guardian-share.v1";

/// Encrypted FROST key share for a guardian.
///
/// Untrusted key material: remote guardian-share payload; guardian identity,
/// recipient key, and ephemeral key bytes must be authenticated against
/// trusted guardian setup state before use.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedKeyShare {
    /// Guardian share encryption protocol version.
    pub protocol_version: u8,
    /// Guardian this share is encrypted for
    pub guardian_id: AuthorityId,
    /// FROST participant index (1-based)
    pub signer_index: u16,
    /// Encrypted key package bytes (ChaCha20-Poly1305)
    // aura-security: raw-secret-field-justified owner=security-refactor expires=before-release remediation=work/2.md encrypted recovery wire payload; plaintext share must use secret wrappers before encryption.
    pub encrypted_share: Vec<u8>,
    /// Nonce used for encryption
    pub nonce: [u8; 12],
    /// Untrusted key material: remote X25519 ephemeral sender key bytes; bind to
    /// the authenticated guardian-share transcript before deriving shared
    /// secrets.
    pub ephemeral_public_key: Vec<u8>,
    /// Untrusted key material: remote recipient guardian key bytes; authenticate
    /// against trusted guardian acceptance state before use.
    pub recipient_public_key: Vec<u8>,
    /// Hash of the setup completion public key package this share is bound to.
    pub public_key_package_hash: Hash32,
    /// Hash binding ciphertext, nonce, setup/account/guardian scope, and key-agreement keys.
    pub binding_hash: Hash32,
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
    /// Account authority being set up
    pub account_id: AuthorityId,
    /// Whether the guardian accepted
    pub accepted: bool,
    /// Untrusted key material: claimed guardian relationship key; verification must resolve expected guardian state separately.
    pub public_key: Vec<u8>,
    /// Timestamp of acceptance
    pub timestamp: TimeStamp,
    /// Cryptographic signature binding the guardian decision to the full setup transcript.
    pub signature: Vec<u8>,
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

#[derive(Debug, Clone, Serialize)]
struct GuardianAcceptanceTranscriptPayload {
    setup_id: String,
    account_id: AuthorityId,
    target_guardians: Vec<AuthorityId>,
    threshold: u16,
    invitation_timestamp: TimeStamp,
    guardian_id: AuthorityId,
    accepted: bool,
    public_key: Vec<u8>,
    acceptance_timestamp: TimeStamp,
}

struct GuardianAcceptanceTranscript<'a> {
    invitation: &'a GuardianInvitation,
    guardian_id: AuthorityId,
    accepted: bool,
    public_key: &'a [u8],
    acceptance_timestamp: &'a TimeStamp,
}

#[derive(Debug, Clone, Serialize)]
struct GuardianShareKeyAgreementTranscript<'a> {
    protocol_version: u8,
    setup_id: &'a str,
    account_id: AuthorityId,
    guardian_id: AuthorityId,
    signer_index: u16,
    recipient_public_key: &'a [u8],
    ephemeral_public_key: &'a [u8],
    public_key_package_hash: Hash32,
}

#[derive(Debug, Clone, Serialize)]
struct GuardianShareBindingTranscript<'a> {
    protocol_version: u8,
    setup_id: &'a str,
    account_id: AuthorityId,
    guardian_id: AuthorityId,
    signer_index: u16,
    recipient_public_key: &'a [u8],
    ephemeral_public_key: &'a [u8],
    public_key_package_hash: Hash32,
    encrypted_share_hash: Hash32,
    nonce: [u8; 12],
}

impl SecurityTranscript for GuardianAcceptanceTranscript<'_> {
    type Payload = GuardianAcceptanceTranscriptPayload;

    const DOMAIN_SEPARATOR: &'static str = "aura.recovery.guardian-setup.acceptance";

    fn transcript_payload(&self) -> Self::Payload {
        GuardianAcceptanceTranscriptPayload {
            setup_id: self.invitation.setup_id.clone(),
            account_id: self.invitation.account_id,
            target_guardians: self.invitation.target_guardians.clone(),
            threshold: self.invitation.threshold,
            invitation_timestamp: self.invitation.timestamp.clone(),
            guardian_id: self.guardian_id,
            accepted: self.accepted,
            public_key: self.public_key.to_vec(),
            acceptance_timestamp: self.acceptance_timestamp.clone(),
        }
    }
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
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "guardian_setup_completion_build",
    family = "runtime_helper"
)]
pub fn build_setup_completion(
    setup_id: &str,
    threshold: u16,
    acceptances: Vec<GuardianAcceptance>,
) -> Result<SetupCompletion, String> {
    let _ = GUARDIAN_SETUP_COMPLETION_BUILD_CAPABILITY;
    build_setup_completion_with_material(setup_id, threshold, acceptances, Vec::new(), Vec::new())
}

/// Build the final setup completion payload from verified guardian responses and generated shares.
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "guardian_setup_completion_build",
    family = "runtime_helper"
)]
pub fn build_setup_completion_with_material(
    setup_id: &str,
    threshold: u16,
    acceptances: Vec<GuardianAcceptance>,
    encrypted_shares: Vec<EncryptedKeyShare>,
    public_key_package: Vec<u8>,
) -> Result<SetupCompletion, String> {
    let _ = GUARDIAN_SETUP_COMPLETION_BUILD_CAPABILITY;
    let accepted_guardians: Vec<AuthorityId> = acceptances
        .iter()
        .filter(|acceptance| acceptance.decision() == GuardianDecision::Accepted)
        .map(|acceptance| acceptance.guardian_id)
        .collect();
    let success = accepted_guardians.len() >= threshold as usize;
    let share_guardians: BTreeSet<AuthorityId> = encrypted_shares
        .iter()
        .map(|share| share.guardian_id)
        .collect();
    let accepted_guardian_set: BTreeSet<AuthorityId> = accepted_guardians.iter().copied().collect();

    if success {
        if encrypted_shares.is_empty() {
            return Err("guardian setup completion requires encrypted shares".to_string());
        }
        if public_key_package.is_empty() {
            return Err("guardian setup completion requires a public key package".to_string());
        }
        if share_guardians != accepted_guardian_set {
            return Err(
                "guardian setup completion shares must exactly match accepted guardians"
                    .to_string(),
            );
        }
    }

    let guardian_set = GuardianSet::new(
        accepted_guardians
            .iter()
            .copied()
            .map(GuardianProfile::new)
            .collect(),
    );

    Ok(SetupCompletion {
        setup_id: setup_id.to_string(),
        success,
        guardian_set,
        threshold,
        encrypted_shares,
        public_key_package,
    })
}

/// Encode the canonical guardian setup acceptance transcript.
pub fn guardian_setup_acceptance_transcript_bytes(
    invitation: &GuardianInvitation,
    guardian_id: AuthorityId,
    accepted: bool,
    public_key: &[u8],
    acceptance_timestamp: &TimeStamp,
) -> RecoveryResult<Vec<u8>> {
    GuardianAcceptanceTranscript {
        invitation,
        guardian_id,
        accepted,
        public_key,
        acceptance_timestamp,
    }
    .transcript_bytes()
    .map_err(|error| AuraError::crypto(format!("guardian setup transcript failed: {error}")))
}

/// Sign a guardian setup acceptance with the guardian's Ed25519 key.
pub async fn sign_guardian_setup_acceptance<E>(
    effects: &E,
    invitation: &GuardianInvitation,
    guardian_id: AuthorityId,
    accepted: bool,
    public_key: &[u8],
    acceptance_timestamp: &TimeStamp,
    private_key: &[u8],
) -> RecoveryResult<Vec<u8>>
where
    E: CryptoEffects + Send + Sync + ?Sized,
{
    let transcript = GuardianAcceptanceTranscript {
        invitation,
        guardian_id,
        accepted,
        public_key,
        acceptance_timestamp,
    };
    sign_ed25519_transcript(effects, &transcript, private_key)
        .await
        .map_err(|error| {
            AuraError::crypto(format!("guardian setup acceptance signing failed: {error}"))
        })
}

/// Verify a guardian setup acceptance against trusted guardian verification keys.
pub async fn verify_guardian_setup_acceptance_signature<E>(
    effects: &E,
    invitation: &GuardianInvitation,
    acceptance: &GuardianAcceptance,
    key_resolver: &impl TrustedKeyResolver,
) -> RecoveryResult<bool>
where
    E: CryptoEffects + Send + Sync + ?Sized,
{
    if acceptance.setup_id != invitation.setup_id || acceptance.account_id != invitation.account_id
    {
        return Ok(false);
    }
    let trusted_key = key_resolver
        .resolve_guardian_key(acceptance.guardian_id)
        .map_err(|error| {
            AuraError::crypto(format!(
                "trusted guardian setup key resolution failed for {}: {error}",
                acceptance.guardian_id
            ))
        })?;
    let transcript = GuardianAcceptanceTranscript {
        invitation,
        guardian_id: acceptance.guardian_id,
        accepted: acceptance.accepted,
        public_key: &acceptance.public_key,
        acceptance_timestamp: &acceptance.timestamp,
    };
    verify_ed25519_transcript(
        effects,
        &transcript,
        &acceptance.signature,
        trusted_key.bytes(),
    )
    .await
    .map_err(|error| {
        AuraError::crypto(format!(
            "guardian setup acceptance verification failed: {error}"
        ))
    })
}

fn to_x25519_scalar(private_key: &[u8; 32]) -> Scalar {
    Scalar::from_bytes_mod_order(*private_key)
}

fn x25519_shared_secret(private_key: &[u8; 32], public_key: &[u8; 32]) -> [u8; 32] {
    let scalar = to_x25519_scalar(private_key);
    let point = MontgomeryPoint(*public_key);
    (scalar * point).to_bytes()
}

fn guardian_share_kdf_transcript(
    setup_id: &str,
    account_id: AuthorityId,
    guardian_id: AuthorityId,
    signer_index: u16,
    recipient_public_key: &[u8],
    ephemeral_public_key: &[u8],
    public_key_package_hash: Hash32,
) -> RecoveryResult<Vec<u8>> {
    aura_core::util::serialization::to_vec(&GuardianShareKeyAgreementTranscript {
        protocol_version: GUARDIAN_SHARE_ENCRYPTION_PROTOCOL_VERSION,
        setup_id,
        account_id,
        guardian_id,
        signer_index,
        recipient_public_key,
        ephemeral_public_key,
        public_key_package_hash,
    })
    .map_err(|error| AuraError::crypto(format!("guardian share transcript encode failed: {error}")))
}

fn guardian_share_binding_hash(
    setup_id: &str,
    account_id: AuthorityId,
    guardian_id: AuthorityId,
    signer_index: u16,
    recipient_public_key: &[u8],
    ephemeral_public_key: &[u8],
    public_key_package_hash: Hash32,
    encrypted_share_hash: Hash32,
    nonce: [u8; 12],
) -> RecoveryResult<Hash32> {
    Hash32::from_value(&GuardianShareBindingTranscript {
        protocol_version: GUARDIAN_SHARE_ENCRYPTION_PROTOCOL_VERSION,
        setup_id,
        account_id,
        guardian_id,
        signer_index,
        recipient_public_key,
        ephemeral_public_key,
        public_key_package_hash,
        encrypted_share_hash,
        nonce,
    })
    .map_err(|error| AuraError::crypto(format!("guardian share binding hash failed: {error}")))
}

/// Encrypt a guardian key share using X25519 key agreement derived from the
/// reviewed Ed25519->X25519 conversion path.
pub async fn encrypt_guardian_share<E>(
    effects: &E,
    invitation: &GuardianInvitation,
    acceptance: &GuardianAcceptance,
    signer_index: u16,
    key_package: &[u8],
    public_key_package: &[u8],
) -> RecoveryResult<EncryptedKeyShare>
where
    E: CryptoEffects + Send + Sync + ?Sized,
{
    if !acceptance.accepted {
        return Err(AuraError::invalid(
            "guardian share encryption requires an accepted guardian".to_string(),
        ));
    }
    if acceptance.setup_id != invitation.setup_id || acceptance.account_id != invitation.account_id
    {
        return Err(AuraError::invalid(
            "guardian share encryption requires an acceptance bound to the active setup"
                .to_string(),
        ));
    }

    let recipient_x25519_public = effects
        .convert_ed25519_to_x25519_public(&acceptance.public_key)
        .await
        .map_err(|error| {
            AuraError::crypto(format!("guardian recipient key conversion failed: {error}"))
        })?;
    let (ephemeral_private_key, ephemeral_ed25519_public_key) =
        effects.ed25519_generate_keypair().await.map_err(|error| {
            AuraError::crypto(format!(
                "guardian share ephemeral key generation failed: {error}"
            ))
        })?;
    let ephemeral_x25519_private = effects
        .convert_ed25519_to_x25519_private(&ephemeral_private_key)
        .await
        .map_err(|error| {
            AuraError::crypto(format!(
                "guardian share ephemeral private-key conversion failed: {error}"
            ))
        })?;
    let ephemeral_x25519_public = effects
        .convert_ed25519_to_x25519_public(&ephemeral_ed25519_public_key)
        .await
        .map_err(|error| {
            AuraError::crypto(format!(
                "guardian share ephemeral public-key conversion failed: {error}"
            ))
        })?;
    let shared_secret = x25519_shared_secret(&ephemeral_x25519_private, &recipient_x25519_public);
    let public_key_package_hash = Hash32::from_bytes(public_key_package);
    let kdf_info = guardian_share_kdf_transcript(
        &invitation.setup_id,
        invitation.account_id,
        acceptance.guardian_id,
        signer_index,
        &acceptance.public_key,
        &ephemeral_x25519_public,
        public_key_package_hash,
    )?;
    let encryption_key = effects
        .kdf_derive(
            &shared_secret,
            GUARDIAN_SHARE_ENCRYPTION_KDF_DOMAIN,
            &kdf_info,
            32,
        )
        .await
        .map_err(|error| {
            AuraError::crypto(format!(
                "guardian share encryption key derivation failed: {error}"
            ))
        })?;
    let nonce_bytes = effects.random_bytes(12).await;
    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(&nonce_bytes);
    let mut key_array = [0u8; 32];
    key_array.copy_from_slice(&encryption_key);
    let encrypted_share = effects
        .chacha20_encrypt(key_package, &key_array, &nonce)
        .await
        .map_err(|error| AuraError::crypto(format!("guardian share encryption failed: {error}")))?;
    let encrypted_share_hash = Hash32::from_bytes(&encrypted_share);
    let binding_hash = guardian_share_binding_hash(
        &invitation.setup_id,
        invitation.account_id,
        acceptance.guardian_id,
        signer_index,
        &acceptance.public_key,
        &ephemeral_x25519_public,
        public_key_package_hash,
        encrypted_share_hash,
        nonce,
    )?;

    Ok(EncryptedKeyShare {
        protocol_version: GUARDIAN_SHARE_ENCRYPTION_PROTOCOL_VERSION,
        guardian_id: acceptance.guardian_id,
        signer_index,
        encrypted_share,
        nonce,
        ephemeral_public_key: ephemeral_x25519_public.to_vec(),
        recipient_public_key: acceptance.public_key.clone(),
        public_key_package_hash,
        binding_hash,
    })
}

/// Decrypt and verify a guardian key share.
pub async fn decrypt_guardian_share<E>(
    effects: &E,
    account_id: AuthorityId,
    guardian_id: AuthorityId,
    setup_id: &str,
    public_key_package: &[u8],
    encrypted_share: &EncryptedKeyShare,
    recipient_private_key: &[u8],
) -> RecoveryResult<Vec<u8>>
where
    E: CryptoEffects + Send + Sync + ?Sized,
{
    if encrypted_share.protocol_version != GUARDIAN_SHARE_ENCRYPTION_PROTOCOL_VERSION {
        return Err(AuraError::invalid(format!(
            "unsupported guardian share encryption version {}",
            encrypted_share.protocol_version
        )));
    }
    if encrypted_share.guardian_id != guardian_id {
        return Err(AuraError::invalid(
            "guardian share recipient does not match the requested guardian".to_string(),
        ));
    }
    let derived_recipient_public_key = effects
        .ed25519_public_key(recipient_private_key)
        .await
        .map_err(|error| {
            AuraError::crypto(format!(
                "guardian recipient public-key derivation failed: {error}"
            ))
        })?;
    if derived_recipient_public_key != encrypted_share.recipient_public_key {
        return Err(AuraError::invalid(
            "guardian share recipient key does not match the stored acceptance key".to_string(),
        ));
    }
    let recipient_x25519_private = effects
        .convert_ed25519_to_x25519_private(recipient_private_key)
        .await
        .map_err(|error| {
            AuraError::crypto(format!(
                "guardian recipient private-key conversion failed: {error}"
            ))
        })?;
    let ephemeral_public_key: [u8; 32] = encrypted_share
        .ephemeral_public_key
        .as_slice()
        .try_into()
        .map_err(|_| AuraError::invalid("guardian share ephemeral key must be 32 bytes"))?;
    let public_key_package_hash = Hash32::from_bytes(public_key_package);
    if encrypted_share.public_key_package_hash != public_key_package_hash {
        return Err(AuraError::invalid(
            "guardian share public key package hash does not match completion data".to_string(),
        ));
    }
    let encrypted_share_hash = Hash32::from_bytes(&encrypted_share.encrypted_share);
    let expected_binding_hash = guardian_share_binding_hash(
        setup_id,
        account_id,
        guardian_id,
        encrypted_share.signer_index,
        &encrypted_share.recipient_public_key,
        &encrypted_share.ephemeral_public_key,
        public_key_package_hash,
        encrypted_share_hash,
        encrypted_share.nonce,
    )?;
    if encrypted_share.binding_hash != expected_binding_hash {
        return Err(AuraError::invalid(
            "guardian share binding hash does not match ciphertext or setup metadata".to_string(),
        ));
    }
    let shared_secret = x25519_shared_secret(&recipient_x25519_private, &ephemeral_public_key);
    let kdf_info = guardian_share_kdf_transcript(
        setup_id,
        account_id,
        guardian_id,
        encrypted_share.signer_index,
        &encrypted_share.recipient_public_key,
        &encrypted_share.ephemeral_public_key,
        public_key_package_hash,
    )?;
    let decryption_key = effects
        .kdf_derive(
            &shared_secret,
            GUARDIAN_SHARE_ENCRYPTION_KDF_DOMAIN,
            &kdf_info,
            32,
        )
        .await
        .map_err(|error| {
            AuraError::crypto(format!(
                "guardian share decryption key derivation failed: {error}"
            ))
        })?;
    let mut key_array = [0u8; 32];
    key_array.copy_from_slice(&decryption_key);
    effects
        .chacha20_decrypt(
            &encrypted_share.encrypted_share,
            &key_array,
            &encrypted_share.nonce,
        )
        .await
        .map_err(|error| AuraError::crypto(format!("guardian share decryption failed: {error}")))
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

    fn setup_context_id(setup_id: &str) -> aura_core::types::identifiers::ContextId {
        context_id_from_operation_id(setup_id)
    }

    /// Execute guardian setup ceremony.
    pub async fn execute_setup(
        &self,
        _request: RecoveryRequest,
    ) -> RecoveryResult<RecoveryResponse> {
        Err(crate::RecoveryError::internal(
            "guardian setup coordinator no longer fabricates local guardian acceptances; use the authenticated runtime choreography",
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
        guardian_signing_private_key: &[u8],
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

        let timestamp = TimeStamp::PhysicalClock(physical_time);
        let signature = sign_guardian_setup_acceptance(
            self.effect_system().as_ref(),
            &invitation,
            guardian_id,
            true,
            &public_key,
            &timestamp,
            guardian_signing_private_key,
        )
        .await?;

        Ok(GuardianAcceptance {
            guardian_id,
            setup_id: invitation.setup_id.clone(),
            account_id: invitation.account_id,
            accepted: true,
            public_key,
            timestamp,
            signature,
        })
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
        public_key_package: &[u8],
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

        let decrypted_share = decrypt_guardian_share(
            self.effect_system().as_ref(),
            account_id,
            guardian_id,
            setup_id,
            public_key_package,
            encrypted_share,
            &private_key,
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
    use aura_core::effects::CryptoCoreEffects;
    use aura_core::key_resolution::{
        KeyResolutionError, TrustedKeyDomain, TrustedKeyResolver, TrustedPublicKey,
    };
    use aura_core::time::PhysicalTime;
    use aura_core::{DeviceId, Hash32};
    use aura_effects::crypto::RealCryptoHandler;
    use aura_testkit::MockEffects;
    use std::collections::BTreeMap;
    use std::sync::Arc;

    fn test_authority_id(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn test_invitation() -> GuardianInvitation {
        GuardianInvitation {
            setup_id: "test-setup-123".to_string(),
            account_id: test_authority_id(10),
            target_guardians: vec![
                test_authority_id(1),
                test_authority_id(2),
                test_authority_id(3),
            ],
            threshold: 2,
            timestamp: TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: 1000,
                uncertainty: None,
            }),
        }
    }

    fn test_acceptance(
        invitation: &GuardianInvitation,
        guardian_id: AuthorityId,
        accepted: bool,
    ) -> GuardianAcceptance {
        GuardianAcceptance {
            guardian_id,
            setup_id: invitation.setup_id.clone(),
            account_id: invitation.account_id,
            accepted,
            public_key: vec![1; 32],
            timestamp: TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: 1_100,
                uncertainty: None,
            }),
            signature: vec![7; 64],
        }
    }

    async fn real_crypto_acceptance(
        crypto: &RealCryptoHandler,
        invitation: &GuardianInvitation,
        guardian_id: AuthorityId,
    ) -> (GuardianAcceptance, Vec<u8>) {
        let (guardian_private_key, guardian_public_key) =
            crypto.ed25519_generate_keypair().await.unwrap();
        let timestamp = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 1_200,
            uncertainty: None,
        });
        let signature = sign_guardian_setup_acceptance(
            crypto,
            invitation,
            guardian_id,
            true,
            &guardian_public_key,
            &timestamp,
            &guardian_private_key,
        )
        .await
        .unwrap();
        (
            GuardianAcceptance {
                guardian_id,
                setup_id: invitation.setup_id.clone(),
                account_id: invitation.account_id,
                accepted: true,
                public_key: guardian_public_key,
                timestamp,
                signature,
            },
            guardian_private_key,
        )
    }

    #[derive(Default)]
    struct StaticGuardianKeyResolver {
        guardians: BTreeMap<AuthorityId, TrustedPublicKey>,
    }

    impl StaticGuardianKeyResolver {
        fn with_guardian(mut self, guardian: AuthorityId, key: Vec<u8>) -> Self {
            self.guardians.insert(
                guardian,
                TrustedPublicKey::active(
                    TrustedKeyDomain::Guardian,
                    None,
                    key.clone(),
                    Hash32::from_bytes(&key),
                ),
            );
            self
        }
    }

    impl TrustedKeyResolver for StaticGuardianKeyResolver {
        fn resolve_authority_threshold_key(
            &self,
            _authority: AuthorityId,
            _epoch: u64,
        ) -> Result<TrustedPublicKey, KeyResolutionError> {
            Err(KeyResolutionError::Unknown {
                domain: TrustedKeyDomain::AuthorityThreshold,
            })
        }

        fn resolve_device_key(
            &self,
            _device: DeviceId,
        ) -> Result<TrustedPublicKey, KeyResolutionError> {
            Err(KeyResolutionError::Unknown {
                domain: TrustedKeyDomain::Device,
            })
        }

        fn resolve_guardian_key(
            &self,
            guardian: AuthorityId,
        ) -> Result<TrustedPublicKey, KeyResolutionError> {
            self.guardians
                .get(&guardian)
                .cloned()
                .ok_or(KeyResolutionError::Unknown {
                    domain: TrustedKeyDomain::Guardian,
                })
        }

        fn resolve_release_key(
            &self,
            _authority: AuthorityId,
        ) -> Result<TrustedPublicKey, KeyResolutionError> {
            Err(KeyResolutionError::Unknown {
                domain: TrustedKeyDomain::Release,
            })
        }
    }

    #[tokio::test]
    async fn test_guardian_setup_coordinator_creation() {
        let effects = Arc::new(MockEffects::deterministic());
        let coordinator = GuardianSetupCoordinator::new(effects);

        assert_eq!(coordinator.operation_name(), "guardian_setup");
    }

    #[tokio::test]
    async fn test_guardian_setup_execute_is_fail_closed_without_runtime_choreography() {
        let effects = Arc::new(MockEffects::deterministic());
        let coordinator = GuardianSetupCoordinator::new(effects);

        let request = crate::types::RecoveryRequest {
            initiator_id: test_authority_id(0),
            account_id: test_authority_id(10),
            context: aura_authentication::RecoveryContext {
                operation_type: aura_authentication::RecoveryOperationType::DeviceKeyRecovery,
                justification: "Test recovery".to_string(),
                is_emergency: false,
                timestamp: 0,
            },
            threshold: 2,
            guardians: crate::types::GuardianSet::new(vec![]),
        };
        let response = coordinator.execute_setup(request).await;

        assert!(response.is_err());
        assert!(response
            .unwrap_err()
            .to_string()
            .contains("no longer fabricates local guardian acceptances"));
    }

    #[tokio::test]
    async fn test_accept_as_guardian_signs_acceptance() {
        let effects = Arc::new(MockEffects::deterministic());
        let coordinator = GuardianSetupCoordinator::new(effects.clone());
        let invitation = test_invitation();
        let guardian_id = test_authority_id(1);
        let (guardian_signing_private_key, signing_public_key) =
            effects.ed25519_generate_keypair().await.unwrap();
        let acceptance = coordinator
            .accept_as_guardian(
                invitation.clone(),
                guardian_id,
                &guardian_signing_private_key,
            )
            .await;

        assert!(acceptance.is_ok());
        let acc = acceptance.unwrap();
        assert!(acc.accepted);
        assert_eq!(acc.guardian_id, guardian_id);
        assert!(!acc.public_key.is_empty());
        assert!(!acc.signature.is_empty());

        let resolver =
            StaticGuardianKeyResolver::default().with_guardian(guardian_id, signing_public_key);
        assert!(verify_guardian_setup_acceptance_signature(
            effects.as_ref(),
            &invitation,
            &acc,
            &resolver,
        )
        .await
        .unwrap());
    }

    #[tokio::test]
    async fn guardian_share_round_trip_uses_converted_x25519_key_agreement() {
        let crypto = RealCryptoHandler::for_simulation_seed([0x31; 32]);
        let invitation = test_invitation();
        let (acceptance, guardian_private_key) =
            real_crypto_acceptance(&crypto, &invitation, test_authority_id(1)).await;
        let public_key_package = vec![0xAB; 48];
        let key_package = vec![0xCD; 64];

        let encrypted_share = encrypt_guardian_share(
            &crypto,
            &invitation,
            &acceptance,
            1,
            &key_package,
            &public_key_package,
        )
        .await
        .unwrap();
        let decrypted = decrypt_guardian_share(
            &crypto,
            invitation.account_id,
            acceptance.guardian_id,
            &invitation.setup_id,
            &public_key_package,
            &encrypted_share,
            &guardian_private_key,
        )
        .await
        .unwrap();

        assert_eq!(decrypted, key_package);
        assert_eq!(
            encrypted_share.protocol_version,
            GUARDIAN_SHARE_ENCRYPTION_PROTOCOL_VERSION
        );
    }

    #[tokio::test]
    async fn guardian_share_rejects_swapped_recipient_and_ephemeral_keys() {
        let crypto = RealCryptoHandler::for_simulation_seed([0x32; 32]);
        let invitation = test_invitation();
        let (acceptance_a, guardian_private_key_a) =
            real_crypto_acceptance(&crypto, &invitation, test_authority_id(1)).await;
        let (acceptance_b, guardian_private_key_b) =
            real_crypto_acceptance(&crypto, &invitation, test_authority_id(2)).await;
        let public_key_package = vec![0x55; 48];
        let key_package_a = vec![0x11; 64];
        let key_package_b = vec![0x22; 64];

        let share_a = encrypt_guardian_share(
            &crypto,
            &invitation,
            &acceptance_a,
            1,
            &key_package_a,
            &public_key_package,
        )
        .await
        .unwrap();
        let share_b = encrypt_guardian_share(
            &crypto,
            &invitation,
            &acceptance_b,
            2,
            &key_package_b,
            &public_key_package,
        )
        .await
        .unwrap();

        let wrong_recipient = decrypt_guardian_share(
            &crypto,
            invitation.account_id,
            acceptance_a.guardian_id,
            &invitation.setup_id,
            &public_key_package,
            &share_a,
            &guardian_private_key_b,
        )
        .await;
        assert!(wrong_recipient.is_err());

        let mut swapped_ephemeral = share_a.clone();
        swapped_ephemeral.ephemeral_public_key = share_b.ephemeral_public_key.clone();
        let swapped_ephemeral_result = decrypt_guardian_share(
            &crypto,
            invitation.account_id,
            acceptance_a.guardian_id,
            &invitation.setup_id,
            &public_key_package,
            &swapped_ephemeral,
            &guardian_private_key_a,
        )
        .await;
        assert!(swapped_ephemeral_result.is_err());
    }

    #[tokio::test]
    async fn guardian_share_rejects_tampered_ciphertext_and_wrong_setup() {
        let crypto = RealCryptoHandler::for_simulation_seed([0x33; 32]);
        let invitation = test_invitation();
        let (acceptance, guardian_private_key) =
            real_crypto_acceptance(&crypto, &invitation, test_authority_id(1)).await;
        let public_key_package = vec![0x77; 48];
        let key_package = vec![0x44; 64];

        let share = encrypt_guardian_share(
            &crypto,
            &invitation,
            &acceptance,
            1,
            &key_package,
            &public_key_package,
        )
        .await
        .unwrap();

        let mut tampered = share.clone();
        tampered.encrypted_share[0] ^= 0x01;
        let tampered_result = decrypt_guardian_share(
            &crypto,
            invitation.account_id,
            acceptance.guardian_id,
            &invitation.setup_id,
            &public_key_package,
            &tampered,
            &guardian_private_key,
        )
        .await;
        assert!(tampered_result.is_err());

        let wrong_setup_result = decrypt_guardian_share(
            &crypto,
            invitation.account_id,
            acceptance.guardian_id,
            "wrong-setup-id",
            &public_key_package,
            &share,
            &guardian_private_key,
        )
        .await;
        assert!(wrong_setup_result.is_err());
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
        let invitation = test_invitation();
        let accepted = test_acceptance(&invitation, test_authority_id(1), true);
        let declined = test_acceptance(&invitation, test_authority_id(2), false);
        let completion = build_setup_completion_with_material(
            &invitation.setup_id,
            1,
            vec![accepted.clone(), declined],
            vec![EncryptedKeyShare {
                protocol_version: GUARDIAN_SHARE_ENCRYPTION_PROTOCOL_VERSION,
                guardian_id: accepted.guardian_id,
                signer_index: 1,
                encrypted_share: vec![1, 2, 3],
                nonce: [0u8; 12],
                ephemeral_public_key: vec![9; 32],
                recipient_public_key: accepted.public_key.clone(),
                public_key_package_hash: Hash32::from_bytes(&[8; 32]),
                binding_hash: Hash32::from_bytes(&[1, 2, 3]),
            }],
            vec![8; 32],
        )
        .unwrap();
        let accepted_guardians: Vec<AuthorityId> = completion
            .guardian_set
            .iter()
            .map(|guardian| guardian.authority_id)
            .collect();

        assert!(completion.success);
        assert_eq!(accepted_guardians, vec![accepted.guardian_id]);
    }

    #[test]
    fn guardian_setup_acceptance_transcript_binds_setup_guardian_and_account() {
        let invitation = test_invitation();
        let base = guardian_setup_acceptance_transcript_bytes(
            &invitation,
            test_authority_id(1),
            true,
            &[1, 2, 3],
            &TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: 1_100,
                uncertainty: None,
            }),
        )
        .unwrap();
        let setup = guardian_setup_acceptance_transcript_bytes(
            &GuardianInvitation {
                setup_id: "different".to_string(),
                ..invitation.clone()
            },
            test_authority_id(1),
            true,
            &[1, 2, 3],
            &TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: 1_100,
                uncertainty: None,
            }),
        )
        .unwrap();
        let account = guardian_setup_acceptance_transcript_bytes(
            &GuardianInvitation {
                account_id: test_authority_id(99),
                ..invitation.clone()
            },
            test_authority_id(1),
            true,
            &[1, 2, 3],
            &TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: 1_100,
                uncertainty: None,
            }),
        )
        .unwrap();
        let guardian = guardian_setup_acceptance_transcript_bytes(
            &invitation,
            test_authority_id(2),
            true,
            &[1, 2, 3],
            &TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: 1_100,
                uncertainty: None,
            }),
        )
        .unwrap();

        assert_ne!(base, setup);
        assert_ne!(base, account);
        assert_ne!(base, guardian);
    }

    #[test]
    fn build_setup_completion_rejects_empty_crypto_material_for_success() {
        let invitation = test_invitation();
        let accepted = test_acceptance(&invitation, test_authority_id(1), true);
        let error = build_setup_completion(&invitation.setup_id, 1, vec![accepted]).unwrap_err();
        assert!(error.contains("encrypted shares"));
    }

    #[test]
    fn guardian_acceptance_exposes_explicit_decision() {
        let invitation = test_invitation();
        let accepted = test_acceptance(&invitation, test_authority_id(1), true);
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
            guardian_set: GuardianSet::new(vec![crate::types::GuardianProfile::new(
                test_authority_id(1),
            )]),
            threshold: 1,
            encrypted_shares: vec![EncryptedKeyShare {
                protocol_version: GUARDIAN_SHARE_ENCRYPTION_PROTOCOL_VERSION,
                guardian_id: test_authority_id(1),
                signer_index: 1,
                encrypted_share: vec![1],
                nonce: [0u8; 12],
                ephemeral_public_key: vec![2; 32],
                recipient_public_key: vec![3; 32],
                public_key_package_hash: Hash32::from_bytes(&[4; 32]),
                binding_hash: Hash32::from_bytes(&[5; 32]),
            }],
            public_key_package: vec![3; 32],
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
