//! Consensus-Based Guardian Ceremony
//!
//! This module implements a safe guardian key rotation ceremony using Aura Consensus
//! to ensure linearizable agreement on guardian set changes. The ceremony uses
//! session types to enforce protocol linearity and prestate binding to prevent forks.
//!
//! # Safety Properties
//!
//! 1. **Linear Protocol Flow**: Session types ensure exactly one outcome (commit/abort)
//! 2. **Prestate Binding**: Operations are bound to current guardian state, preventing concurrent ceremonies
//! 3. **Epoch Isolation**: Uncommitted key rotations don't affect signing capability
//! 4. **Consensus Agreement**: Guardians act as witnesses; threshold must agree for commit
//!
//! # Protocol Flow
//!
//! ```text
//! 1. Initiator proposes new guardian set with threshold k-of-n
//! 2. System computes prestate hash from current guardian configuration
//! 3. ConsensusId derived from (prestate_hash, operation_hash, nonce)
//! 4. FROST keys generated at new epoch (old epoch remains active)
//! 5. Guardians receive encrypted key shares and respond (accept/decline)
//! 6. If threshold accepts: CommitFact produced, new epoch activated
//! 7. If any declines or timeout: Ceremony fails, old epoch remains active
//! ```
//!
//! # Key Insight: Epoch Isolation
//!
//! The critical safety property is that key packages stored at uncommitted epochs
//! are inert. Only the committed epoch is used for signing. This eliminates the
//! need for explicit rollback - simply not committing is sufficient.

use crate::{
    effects::RecoveryEffects, utils::workflow::current_physical_time_or_zero, RecoveryError,
    RecoveryResult,
};
use aura_core::{
    effects::{CryptoEffects, JournalEffects, PhysicalTimeEffects},
    hash,
    key_resolution::TrustedKeyResolver,
    threshold::{policy_for, AgreementMode, CeremonyFlow},
    time::PhysicalTime,
    types::AuthorityId,
    AuraError, Hash32,
};
use aura_macros::tell;
use aura_signature::{sign_ed25519_transcript, verify_ed25519_transcript, SecurityTranscript};
use curve25519_dalek::{montgomery::MontgomeryPoint, scalar::Scalar};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

const GUARDIAN_CEREMONY_ENCRYPTION_PROTOCOL_VERSION: u8 = 1;
const GUARDIAN_CEREMONY_ENCRYPTION_KDF_DOMAIN: &[u8] = b"aura.recovery.guardian-ceremony.v1";

// ============================================================================
// Core Types
// ============================================================================

/// Unique identifier for a guardian ceremony instance
///
/// Derived from prestate hash, operation hash, and nonce to ensure
/// uniqueness and binding to current state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CeremonyId(pub Hash32);

impl CeremonyId {
    /// Create a new ceremony ID from components
    pub fn new(prestate_hash: Hash32, operation_hash: Hash32, nonce: u64) -> Self {
        let mut h = hash::hasher();
        h.update(b"GUARDIAN_CEREMONY_ID");
        h.update(&prestate_hash.0);
        h.update(&operation_hash.0);
        h.update(&nonce.to_le_bytes());
        CeremonyId(Hash32(h.finalize()))
    }
}

impl std::fmt::Display for CeremonyId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ceremony:{}", hex::encode(&self.0 .0[..8]))
    }
}

/// Operation to change guardian configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianRotationOp {
    /// New threshold required for operations (k)
    pub threshold_k: u16,
    /// Total number of guardians (n)
    pub total_n: u16,
    /// Authority IDs of the new guardian set
    pub guardian_ids: Vec<AuthorityId>,
    /// New epoch for the key rotation
    pub new_epoch: u64,
}

impl GuardianRotationOp {
    /// Compute the hash of this operation
    pub fn compute_hash(&self) -> Hash32 {
        let mut h = hash::hasher();
        h.update(b"GUARDIAN_ROTATION_OP");
        h.update(&self.threshold_k.to_le_bytes());
        h.update(&self.total_n.to_le_bytes());
        h.update(&(self.guardian_ids.len() as u32).to_le_bytes());
        for id in &self.guardian_ids {
            h.update(&id.to_bytes());
        }
        h.update(&self.new_epoch.to_le_bytes());
        Hash32(h.finalize())
    }
}

/// Current guardian state used for prestate computation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianState {
    /// Current epoch
    pub epoch: u64,
    /// Current threshold (k)
    pub threshold_k: u16,
    /// Current guardian authorities
    pub guardian_ids: Vec<AuthorityId>,
    /// Hash of the current public key package
    pub public_key_hash: Hash32,
}

impl GuardianState {
    /// Compute prestate hash for this guardian configuration
    pub fn compute_prestate_hash(&self, authority_id: &AuthorityId) -> Hash32 {
        let mut h = hash::hasher();
        h.update(b"GUARDIAN_PRESTATE");
        h.update(&authority_id.to_bytes());
        h.update(&self.epoch.to_le_bytes());
        h.update(&self.threshold_k.to_le_bytes());
        h.update(&(self.guardian_ids.len() as u32).to_le_bytes());

        // Sort guardian IDs for determinism
        let mut sorted_ids = self.guardian_ids.clone();
        sorted_ids.sort();
        for id in sorted_ids {
            h.update(&id.to_bytes());
        }

        h.update(&self.public_key_hash.0);
        Hash32(h.finalize())
    }

    /// Create an empty/initial guardian state
    pub fn empty() -> Self {
        Self {
            epoch: 0,
            threshold_k: 0,
            guardian_ids: Vec::new(),
            public_key_hash: Hash32::default(),
        }
    }
}

/// Guardian's response to a ceremony invitation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CeremonyResponse {
    /// Guardian accepts the new configuration
    Accept,
    /// Guardian declines participation
    Decline,
    /// Guardian hasn't responded yet
    Pending,
}

/// Status of a ceremony
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CeremonyStatus {
    /// Ceremony is waiting for guardian responses
    AwaitingResponses {
        accepted: u32,
        declined: u32,
        pending: u32,
    },
    /// Ceremony completed successfully
    Committed { new_epoch: u64 },
    /// Ceremony was aborted
    Aborted { reason: CeremonyAbortReason },
}

/// Typed terminal failure for guardian ceremonies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CeremonyAbortReason {
    /// One or more guardians explicitly declined the rotation.
    GuardianDeclined,
    /// All guardians responded but the acceptance threshold was not met.
    InsufficientAcceptances { accepted: u16, required: u16 },
    /// The ceremony was manually cancelled.
    Manual { reason: String },
}

impl std::fmt::Display for CeremonyAbortReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CeremonyAbortReason::GuardianDeclined => {
                write!(f, "One or more guardians declined")
            }
            CeremonyAbortReason::InsufficientAcceptances { accepted, required } => {
                write!(
                    f,
                    "Insufficient acceptances: got {accepted}, need {required}"
                )
            }
            CeremonyAbortReason::Manual { reason } => write!(f, "{reason}"),
        }
    }
}

/// Complete state of an ongoing ceremony
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CeremonyState {
    /// Unique ceremony identifier
    pub ceremony_id: CeremonyId,
    /// Authority initiating the ceremony
    pub initiator_id: AuthorityId,
    /// Prestate hash this ceremony is bound to
    pub prestate_hash: Hash32,
    /// The proposed rotation operation
    pub operation: GuardianRotationOp,
    /// Responses from each guardian
    pub responses: HashMap<AuthorityId, CeremonyResponse>,
    /// Encrypted key packages for each guardian (after key generation)
    // aura-security: raw-secret-field-justified owner=security-refactor expires=before-release remediation=work/2.md encrypted ceremony wire payloads; plaintext packages must use secret wrappers before wrapping.
    pub key_packages: Vec<Vec<u8>>,
    /// Untrusted key material: proposed ceremony configuration; verification must resolve expected authority/epoch key separately.
    pub public_key_package: Vec<u8>,
    /// Current status
    pub status: CeremonyStatus,
    /// When the ceremony was initiated
    pub initiated_at: PhysicalTime,
    /// When the ceremony was completed (if completed)
    pub completed_at: Option<PhysicalTime>,
    /// Agreement mode (A1/A2/A3)
    pub agreement_mode: AgreementMode,
}

impl CeremonyState {
    fn response_count(&self, response: CeremonyResponse) -> usize {
        self.responses.values().filter(|r| **r == response).count()
    }

    fn guardians_with_response(&self, response: CeremonyResponse) -> Vec<AuthorityId> {
        self.responses
            .iter()
            .filter_map(|(guardian_id, current)| (*current == response).then_some(*guardian_id))
            .collect()
    }

    /// Check if enough guardians have accepted
    pub fn has_threshold(&self) -> bool {
        self.response_count(CeremonyResponse::Accept) >= self.operation.threshold_k as usize
    }

    /// Check if any guardian has declined
    pub fn has_decline(&self) -> bool {
        self.response_count(CeremonyResponse::Decline) > 0
    }

    /// Check if all guardians have responded
    pub fn all_responded(&self) -> bool {
        self.response_count(CeremonyResponse::Pending) == 0
    }

    /// Get count of responses by type
    pub fn response_counts(&self) -> (usize, usize, usize) {
        let accepted = self.response_count(CeremonyResponse::Accept);
        let declined = self.response_count(CeremonyResponse::Decline);
        let pending = self.response_count(CeremonyResponse::Pending);
        (accepted, declined, pending)
    }
}

// ============================================================================
// Ceremony Facts
// ============================================================================

/// Facts emitted during guardian ceremonies for journal persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CeremonyFact {
    /// Ceremony was initiated
    Initiated {
        ceremony_id: Hash32,
        initiator_id: AuthorityId,
        prestate_hash: Hash32,
        operation_hash: Hash32,
        threshold_k: u16,
        total_n: u16,
        guardian_ids: Vec<AuthorityId>,
        initiated_at: PhysicalTime,
    },
    /// Guardian responded to ceremony
    GuardianResponded {
        ceremony_id: Hash32,
        guardian_id: AuthorityId,
        response: CeremonyResponse,
        responded_at: PhysicalTime,
    },
    /// Ceremony was committed (new epoch activated)
    Committed {
        ceremony_id: Hash32,
        new_epoch: u64,
        threshold_k: u16,
        guardian_ids: Vec<AuthorityId>,
        committed_at: PhysicalTime,
    },
    /// Ceremony was aborted
    Aborted {
        ceremony_id: Hash32,
        reason: String,
        aborted_at: PhysicalTime,
    },
    /// Ceremony was superseded by a newer ceremony
    ///
    /// Emitted when a new ceremony replaces an existing one. The old ceremony
    /// should stop processing immediately. Supersession propagates via anti-entropy.
    Superseded {
        /// The ceremony being superseded (old ceremony)
        superseded_ceremony_id: Hash32,
        /// The ceremony that supersedes it (new ceremony)
        superseding_ceremony_id: Hash32,
        /// Reason for supersession (e.g., "prestate_stale", "newer_request", "timeout")
        reason: String,
        /// When the supersession was recorded
        superseded_at: PhysicalTime,
    },
}

impl CeremonyFact {
    /// Get a unique key for this fact
    pub fn fact_key(&self) -> String {
        match self {
            CeremonyFact::Initiated { ceremony_id, .. } => {
                format!("ceremony:{}:initiated", hex::encode(&ceremony_id.0[..8]))
            }
            CeremonyFact::GuardianResponded {
                ceremony_id,
                guardian_id,
                ..
            } => {
                format!(
                    "ceremony:{}:response:{}",
                    hex::encode(&ceremony_id.0[..8]),
                    guardian_id
                )
            }
            CeremonyFact::Committed { ceremony_id, .. } => {
                format!("ceremony:{}:committed", hex::encode(&ceremony_id.0[..8]))
            }
            CeremonyFact::Aborted { ceremony_id, .. } => {
                format!("ceremony:{}:aborted", hex::encode(&ceremony_id.0[..8]))
            }
            CeremonyFact::Superseded {
                superseded_ceremony_id,
                superseding_ceremony_id,
                ..
            } => {
                format!(
                    "ceremony:{}:superseded:{}",
                    hex::encode(&superseded_ceremony_id.0[..8]),
                    hex::encode(&superseding_ceremony_id.0[..8])
                )
            }
        }
    }

    /// Get the ceremony ID (returns superseded ceremony ID for supersession facts)
    pub fn ceremony_id(&self) -> Hash32 {
        match self {
            CeremonyFact::Initiated { ceremony_id, .. } => *ceremony_id,
            CeremonyFact::GuardianResponded { ceremony_id, .. } => *ceremony_id,
            CeremonyFact::Committed { ceremony_id, .. } => *ceremony_id,
            CeremonyFact::Aborted { ceremony_id, .. } => *ceremony_id,
            CeremonyFact::Superseded {
                superseded_ceremony_id,
                ..
            } => *superseded_ceremony_id,
        }
    }
}

// ============================================================================
// Choreography Definition
// ============================================================================

// Guardian Ceremony Choreography - uses session types for linear protocol flow
tell!(include_str!("src/guardian_ceremony.tell"));

/// Ceremony proposal sent to guardians
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CeremonyProposal {
    /// Unique ceremony identifier
    pub ceremony_id: CeremonyId,
    /// Authority initiating the ceremony
    pub initiator_id: AuthorityId,
    /// Prestate hash this ceremony is bound to
    pub prestate_hash: Hash32,
    /// The proposed operation
    pub operation: GuardianRotationOp,
    /// Encrypted key package for this specific guardian
    // aura-security: raw-secret-field-justified owner=security-refactor expires=before-release remediation=work/2.md encrypted ceremony wire payload; plaintext package must use secret wrappers before wrapping.
    pub encrypted_key_package: Vec<u8>,
    /// Nonce for decryption
    pub encryption_nonce: [u8; 12],
    /// X25519 ephemeral sender key derived via the reviewed Ed25519->X25519 conversion path.
    pub ephemeral_public_key: Vec<u8>,
    /// Recipient Ed25519 public key authenticated from trusted guardian key state.
    pub recipient_public_key: Vec<u8>,
    /// Protocol version for the encrypted key-package format.
    pub key_package_version: u8,
    /// Hash of the encrypted key package bytes.
    pub encrypted_key_package_hash: Hash32,
    /// Hash binding ceremony scope, recipient, sender ephemeral key, nonce, and ciphertext hash.
    pub binding_hash: Hash32,
}

/// Response from a guardian
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CeremonyResponseMsg {
    /// Ceremony being responded to
    pub ceremony_id: CeremonyId,
    /// Guardian sending the response
    pub guardian_id: AuthorityId,
    /// The response
    pub response: CeremonyResponse,
    /// Hash of the encrypted key package bound into this guardian's proposal.
    pub encrypted_key_package_hash: Hash32,
    /// Guardian's signature over the ceremony (for commit proof)
    pub signature: Vec<u8>,
}

/// Independently verifiable evidence for a committed guardian ceremony.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CeremonyCommitCertificate {
    /// Ceremony being committed.
    pub ceremony_id: CeremonyId,
    /// Authority that initiated the ceremony.
    pub initiator_id: AuthorityId,
    /// Prestate hash the ceremony was bound to.
    pub prestate_hash: Hash32,
    /// Operation approved by the guardians.
    pub operation: GuardianRotationOp,
    /// Signed acceptance responses that satisfied the threshold.
    pub accepted_responses: Vec<CeremonyResponseMsg>,
}

/// Commit message finalizing the ceremony
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CeremonyCommit {
    /// Ceremony being committed
    pub ceremony_id: CeremonyId,
    /// New epoch that is now active
    pub new_epoch: u64,
    /// Independently verifiable evidence for the commit decision.
    pub commit_certificate: CeremonyCommitCertificate,
    /// List of guardians who accepted
    pub participants: Vec<AuthorityId>,
}

/// Abort message canceling the ceremony
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CeremonyAbort {
    /// Ceremony being aborted
    pub ceremony_id: CeremonyId,
    /// Reason for abort
    pub reason: String,
}

/// Unified ceremony result message (commit or abort)
///
/// This replaces the protocol-level choice between CommitCeremony and AbortCeremony
/// with a single message type that indicates the outcome.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CeremonyResult {
    /// Ceremony ID
    pub ceremony_id: CeremonyId,
    /// Whether the ceremony was committed (true) or aborted (false)
    pub committed: bool,
    /// New epoch if committed
    pub new_epoch: Option<u64>,
    /// Participants if committed
    pub participants: Vec<AuthorityId>,
    /// Reason for abort if not committed
    pub abort_reason: Option<String>,
}

fn to_x25519_scalar(private_key: &[u8; 32]) -> Scalar {
    Scalar::from_bytes_mod_order(*private_key)
}

fn x25519_shared_secret(private_key: &[u8; 32], public_key: &[u8; 32]) -> [u8; 32] {
    let scalar = to_x25519_scalar(private_key);
    let point = MontgomeryPoint(*public_key);
    (scalar * point).to_bytes()
}

fn ceremony_proposal_kdf_transcript(proposal: &CeremonyProposal) -> RecoveryResult<Vec<u8>> {
    aura_core::util::serialization::to_vec(&CeremonyProposalKeyAgreementTranscript {
        protocol_version: GUARDIAN_CEREMONY_ENCRYPTION_PROTOCOL_VERSION,
        ceremony_id: proposal.ceremony_id,
        initiator_id: proposal.initiator_id,
        prestate_hash: proposal.prestate_hash,
        operation: proposal.operation.clone(),
        recipient_public_key: &proposal.recipient_public_key,
        ephemeral_public_key: &proposal.ephemeral_public_key,
    })
    .map_err(|error| {
        AuraError::crypto(format!(
            "guardian ceremony proposal transcript encode failed: {error}"
        ))
    })
}

fn ceremony_proposal_binding_hash(proposal: &CeremonyProposal) -> RecoveryResult<Hash32> {
    Hash32::from_value(&CeremonyProposalBindingTranscript {
        protocol_version: proposal.key_package_version,
        ceremony_id: proposal.ceremony_id,
        initiator_id: proposal.initiator_id,
        prestate_hash: proposal.prestate_hash,
        operation: proposal.operation.clone(),
        recipient_public_key: &proposal.recipient_public_key,
        ephemeral_public_key: &proposal.ephemeral_public_key,
        encrypted_key_package_hash: proposal.encrypted_key_package_hash,
        encryption_nonce: proposal.encryption_nonce,
    })
    .map_err(|error| {
        AuraError::crypto(format!(
            "guardian ceremony proposal binding hash failed: {error}"
        ))
    })
}

/// Encrypt a guardian ceremony key package for one guardian.
pub async fn encrypt_ceremony_key_package<E>(
    effects: &E,
    ceremony_id: CeremonyId,
    initiator_id: AuthorityId,
    prestate_hash: Hash32,
    operation: &GuardianRotationOp,
    recipient_public_key: &[u8],
    key_package: &[u8],
) -> RecoveryResult<CeremonyProposal>
where
    E: CryptoEffects + Send + Sync + ?Sized,
{
    let recipient_x25519_public = effects
        .convert_ed25519_to_x25519_public(recipient_public_key)
        .await
        .map_err(|error| {
            AuraError::crypto(format!(
                "guardian ceremony recipient key conversion failed: {error}"
            ))
        })?;
    let (ephemeral_private_key, ephemeral_ed25519_public_key) =
        effects.ed25519_generate_keypair().await.map_err(|error| {
            AuraError::crypto(format!(
                "guardian ceremony ephemeral key generation failed: {error}"
            ))
        })?;
    let ephemeral_x25519_private = effects
        .convert_ed25519_to_x25519_private(&ephemeral_private_key)
        .await
        .map_err(|error| {
            AuraError::crypto(format!(
                "guardian ceremony ephemeral private-key conversion failed: {error}"
            ))
        })?;
    let ephemeral_x25519_public = effects
        .convert_ed25519_to_x25519_public(&ephemeral_ed25519_public_key)
        .await
        .map_err(|error| {
            AuraError::crypto(format!(
                "guardian ceremony ephemeral public-key conversion failed: {error}"
            ))
        })?;
    let shared_secret = x25519_shared_secret(&ephemeral_x25519_private, &recipient_x25519_public);
    let nonce_bytes = effects.random_bytes(12).await;
    let mut encryption_nonce = [0u8; 12];
    encryption_nonce.copy_from_slice(&nonce_bytes);

    let mut proposal = CeremonyProposal {
        ceremony_id,
        initiator_id,
        prestate_hash,
        operation: operation.clone(),
        encrypted_key_package: Vec::new(),
        encryption_nonce,
        ephemeral_public_key: ephemeral_x25519_public.to_vec(),
        recipient_public_key: recipient_public_key.to_vec(),
        key_package_version: GUARDIAN_CEREMONY_ENCRYPTION_PROTOCOL_VERSION,
        encrypted_key_package_hash: Hash32::zero(),
        binding_hash: Hash32::zero(),
    };
    let kdf_info = ceremony_proposal_kdf_transcript(&proposal)?;
    let encryption_key = effects
        .kdf_derive(
            &shared_secret,
            GUARDIAN_CEREMONY_ENCRYPTION_KDF_DOMAIN,
            &kdf_info,
            32,
        )
        .await
        .map_err(|error| {
            AuraError::crypto(format!(
                "guardian ceremony encryption key derivation failed: {error}"
            ))
        })?;
    let mut key_array = [0u8; 32];
    key_array.copy_from_slice(&encryption_key);
    let encrypted_key_package = effects
        .chacha20_encrypt(key_package, &key_array, &encryption_nonce)
        .await
        .map_err(|error| {
            AuraError::crypto(format!(
                "guardian ceremony key-package encryption failed: {error}"
            ))
        })?;
    proposal.encrypted_key_package_hash = Hash32::from_bytes(&encrypted_key_package);
    proposal.encrypted_key_package = encrypted_key_package;
    proposal.binding_hash = ceremony_proposal_binding_hash(&proposal)?;
    Ok(proposal)
}

/// Decrypt and verify a guardian ceremony proposal key package.
pub async fn decrypt_ceremony_key_package<E>(
    effects: &E,
    proposal: &CeremonyProposal,
    recipient_private_key: &[u8],
) -> RecoveryResult<Vec<u8>>
where
    E: CryptoEffects + Send + Sync + ?Sized,
{
    if proposal.key_package_version != GUARDIAN_CEREMONY_ENCRYPTION_PROTOCOL_VERSION {
        return Err(AuraError::invalid(format!(
            "unsupported guardian ceremony proposal version {}",
            proposal.key_package_version
        )));
    }
    let derived_recipient_public_key = effects
        .ed25519_public_key(recipient_private_key)
        .await
        .map_err(|error| {
            AuraError::crypto(format!(
                "guardian ceremony recipient public-key derivation failed: {error}"
            ))
        })?;
    if derived_recipient_public_key != proposal.recipient_public_key {
        return Err(AuraError::invalid(
            "guardian ceremony recipient key does not match proposal binding".to_string(),
        ));
    }
    let expected_package_hash = Hash32::from_bytes(&proposal.encrypted_key_package);
    if proposal.encrypted_key_package_hash != expected_package_hash {
        return Err(AuraError::invalid(
            "guardian ceremony encrypted key-package hash does not match ciphertext".to_string(),
        ));
    }
    if proposal.binding_hash != ceremony_proposal_binding_hash(proposal)? {
        return Err(AuraError::invalid(
            "guardian ceremony proposal binding hash does not match ciphertext or metadata"
                .to_string(),
        ));
    }
    let recipient_x25519_private = effects
        .convert_ed25519_to_x25519_private(recipient_private_key)
        .await
        .map_err(|error| {
            AuraError::crypto(format!(
                "guardian ceremony recipient private-key conversion failed: {error}"
            ))
        })?;
    let ephemeral_public_key: [u8; 32] = proposal
        .ephemeral_public_key
        .as_slice()
        .try_into()
        .map_err(|_| AuraError::invalid("guardian ceremony ephemeral key must be 32 bytes"))?;
    let shared_secret = x25519_shared_secret(&recipient_x25519_private, &ephemeral_public_key);
    let kdf_info = ceremony_proposal_kdf_transcript(proposal)?;
    let decryption_key = effects
        .kdf_derive(
            &shared_secret,
            GUARDIAN_CEREMONY_ENCRYPTION_KDF_DOMAIN,
            &kdf_info,
            32,
        )
        .await
        .map_err(|error| {
            AuraError::crypto(format!(
                "guardian ceremony decryption key derivation failed: {error}"
            ))
        })?;
    let mut key_array = [0u8; 32];
    key_array.copy_from_slice(&decryption_key);
    effects
        .chacha20_decrypt(
            &proposal.encrypted_key_package,
            &key_array,
            &proposal.encryption_nonce,
        )
        .await
        .map_err(|error| {
            AuraError::crypto(format!(
                "guardian ceremony key-package decryption failed: {error}"
            ))
        })
}

/// Sign a guardian ceremony response.
pub async fn sign_guardian_ceremony_response<E>(
    effects: &E,
    proposal: &CeremonyProposal,
    guardian_id: AuthorityId,
    response: CeremonyResponse,
    guardian_private_key: &[u8],
) -> RecoveryResult<Vec<u8>>
where
    E: CryptoEffects + Send + Sync + ?Sized,
{
    sign_guardian_ceremony_response_with_context(
        effects,
        proposal.ceremony_id,
        proposal.initiator_id,
        proposal.prestate_hash,
        &proposal.operation,
        guardian_id,
        response,
        proposal.encrypted_key_package_hash,
        guardian_private_key,
    )
    .await
}

/// Sign a guardian ceremony response against explicit ceremony context.
pub async fn sign_guardian_ceremony_response_with_context<E>(
    effects: &E,
    ceremony_id: CeremonyId,
    initiator_id: AuthorityId,
    prestate_hash: Hash32,
    operation: &GuardianRotationOp,
    guardian_id: AuthorityId,
    response: CeremonyResponse,
    encrypted_key_package_hash: Hash32,
    guardian_private_key: &[u8],
) -> RecoveryResult<Vec<u8>>
where
    E: CryptoEffects + Send + Sync + ?Sized,
{
    let transcript = CeremonyResponseTranscript {
        ceremony_id,
        initiator_id,
        prestate_hash,
        operation,
        guardian_id,
        response,
        encrypted_key_package_hash,
    };
    sign_ed25519_transcript(effects, &transcript, guardian_private_key)
        .await
        .map_err(|error| {
            AuraError::crypto(format!(
                "guardian ceremony response signing failed: {error}"
            ))
        })
}

/// Verify a guardian ceremony response against trusted guardian keys.
pub async fn verify_guardian_ceremony_response_signature<E>(
    effects: &E,
    proposal: &CeremonyProposal,
    response: &CeremonyResponseMsg,
    key_resolver: &impl TrustedKeyResolver,
) -> RecoveryResult<bool>
where
    E: CryptoEffects + Send + Sync + ?Sized,
{
    verify_guardian_ceremony_response_signature_with_context(
        effects,
        proposal.ceremony_id,
        proposal.initiator_id,
        proposal.prestate_hash,
        &proposal.operation,
        proposal.encrypted_key_package_hash,
        response,
        key_resolver,
    )
    .await
}

/// Verify a guardian ceremony response against explicit ceremony context.
pub async fn verify_guardian_ceremony_response_signature_with_context<E>(
    effects: &E,
    ceremony_id: CeremonyId,
    initiator_id: AuthorityId,
    prestate_hash: Hash32,
    operation: &GuardianRotationOp,
    encrypted_key_package_hash: Hash32,
    response: &CeremonyResponseMsg,
    key_resolver: &impl TrustedKeyResolver,
) -> RecoveryResult<bool>
where
    E: CryptoEffects + Send + Sync + ?Sized,
{
    if response.ceremony_id != ceremony_id
        || response.encrypted_key_package_hash != encrypted_key_package_hash
    {
        return Ok(false);
    }
    let trusted_key = key_resolver
        .resolve_guardian_key(response.guardian_id)
        .map_err(|error| {
            AuraError::crypto(format!(
                "trusted guardian ceremony key resolution failed for {}: {error}",
                response.guardian_id
            ))
        })?;
    let transcript = CeremonyResponseTranscript {
        ceremony_id,
        initiator_id,
        prestate_hash,
        operation,
        guardian_id: response.guardian_id,
        response: response.response,
        encrypted_key_package_hash: response.encrypted_key_package_hash,
    };
    verify_ed25519_transcript(
        effects,
        &transcript,
        &response.signature,
        trusted_key.bytes(),
    )
    .await
    .map_err(|error| {
        AuraError::crypto(format!(
            "guardian ceremony response verification failed: {error}"
        ))
    })
}

/// Build an independently verifiable guardian ceremony commit certificate.
pub async fn build_guardian_ceremony_commit_certificate<E>(
    effects: &E,
    ceremony_id: CeremonyId,
    initiator_id: AuthorityId,
    prestate_hash: Hash32,
    operation: &GuardianRotationOp,
    guardians: &[AuthorityId],
    threshold_k: u16,
    accepted_responses: &[CeremonyResponseMsg],
    key_resolver: &impl TrustedKeyResolver,
) -> RecoveryResult<CeremonyCommitCertificate>
where
    E: CryptoEffects + Send + Sync + ?Sized,
{
    if accepted_responses.len() < threshold_k as usize {
        return Err(AuraError::invalid(format!(
            "guardian ceremony commit certificate requires at least {threshold_k} acceptances"
        )));
    }

    let guardian_set: BTreeSet<_> = guardians.iter().copied().collect();
    let mut seen_guardians = BTreeSet::new();
    let mut certificate_responses = Vec::with_capacity(accepted_responses.len());

    for response in accepted_responses {
        if response.response != CeremonyResponse::Accept {
            return Err(AuraError::invalid(
                "guardian ceremony commit certificate may only contain acceptance responses",
            ));
        }
        if !guardian_set.contains(&response.guardian_id) {
            return Err(AuraError::invalid(format!(
                "guardian ceremony response from unknown guardian {}",
                response.guardian_id
            )));
        }
        if !seen_guardians.insert(response.guardian_id) {
            return Err(AuraError::invalid(format!(
                "duplicate guardian ceremony acceptance from {}",
                response.guardian_id
            )));
        }
        let verified = verify_guardian_ceremony_response_signature_with_context(
            effects,
            ceremony_id,
            initiator_id,
            prestate_hash,
            operation,
            response.encrypted_key_package_hash,
            response,
            key_resolver,
        )
        .await?;
        if !verified {
            return Err(AuraError::invalid(format!(
                "guardian ceremony signature verification failed for {}",
                response.guardian_id
            )));
        }
        certificate_responses.push(response.clone());
    }

    Ok(CeremonyCommitCertificate {
        ceremony_id,
        initiator_id,
        prestate_hash,
        operation: operation.clone(),
        accepted_responses: certificate_responses,
    })
}

/// Verify guardian ceremony commit evidence against trusted guardian keys.
pub async fn verify_guardian_ceremony_commit_certificate<E>(
    effects: &E,
    certificate: &CeremonyCommitCertificate,
    guardians: &[AuthorityId],
    threshold_k: u16,
    key_resolver: &impl TrustedKeyResolver,
) -> RecoveryResult<bool>
where
    E: CryptoEffects + Send + Sync + ?Sized,
{
    if certificate.accepted_responses.len() < threshold_k as usize {
        return Ok(false);
    }

    let guardian_set: BTreeSet<_> = guardians.iter().copied().collect();
    let mut seen_guardians = BTreeSet::new();

    for response in &certificate.accepted_responses {
        if response.response != CeremonyResponse::Accept {
            return Ok(false);
        }
        if !guardian_set.contains(&response.guardian_id)
            || !seen_guardians.insert(response.guardian_id)
        {
            return Ok(false);
        }
        let verified = verify_guardian_ceremony_response_signature_with_context(
            effects,
            certificate.ceremony_id,
            certificate.initiator_id,
            certificate.prestate_hash,
            &certificate.operation,
            response.encrypted_key_package_hash,
            response,
            key_resolver,
        )
        .await?;
        if !verified {
            return Ok(false);
        }
    }

    Ok(true)
}

#[derive(Debug, Clone, Serialize)]
struct CeremonyProposalKeyAgreementTranscript<'a> {
    protocol_version: u8,
    ceremony_id: CeremonyId,
    initiator_id: AuthorityId,
    prestate_hash: Hash32,
    operation: GuardianRotationOp,
    recipient_public_key: &'a [u8],
    ephemeral_public_key: &'a [u8],
}

#[derive(Debug, Clone, Serialize)]
struct CeremonyProposalBindingTranscript<'a> {
    protocol_version: u8,
    ceremony_id: CeremonyId,
    initiator_id: AuthorityId,
    prestate_hash: Hash32,
    operation: GuardianRotationOp,
    recipient_public_key: &'a [u8],
    ephemeral_public_key: &'a [u8],
    encrypted_key_package_hash: Hash32,
    encryption_nonce: [u8; 12],
}

#[derive(Debug, Clone, Serialize)]
struct CeremonyResponseTranscriptPayload {
    ceremony_id: CeremonyId,
    initiator_id: AuthorityId,
    prestate_hash: Hash32,
    operation: GuardianRotationOp,
    guardian_id: AuthorityId,
    response: CeremonyResponse,
    encrypted_key_package_hash: Hash32,
}

struct CeremonyResponseTranscript<'a> {
    ceremony_id: CeremonyId,
    initiator_id: AuthorityId,
    prestate_hash: Hash32,
    operation: &'a GuardianRotationOp,
    guardian_id: AuthorityId,
    response: CeremonyResponse,
    encrypted_key_package_hash: Hash32,
}

impl SecurityTranscript for CeremonyResponseTranscript<'_> {
    type Payload = CeremonyResponseTranscriptPayload;

    const DOMAIN_SEPARATOR: &'static str = "aura.recovery.guardian-ceremony.response";

    fn transcript_payload(&self) -> Self::Payload {
        CeremonyResponseTranscriptPayload {
            ceremony_id: self.ceremony_id,
            initiator_id: self.initiator_id,
            prestate_hash: self.prestate_hash,
            operation: self.operation.clone(),
            guardian_id: self.guardian_id,
            response: self.response,
            encrypted_key_package_hash: self.encrypted_key_package_hash,
        }
    }
}

impl CeremonyResult {
    /// Create a commit result
    pub fn commit(ceremony_id: CeremonyId, new_epoch: u64, participants: Vec<AuthorityId>) -> Self {
        Self {
            ceremony_id,
            committed: true,
            new_epoch: Some(new_epoch),
            participants,
            abort_reason: None,
        }
    }

    /// Create an abort result
    pub fn abort(ceremony_id: CeremonyId, reason: String) -> Self {
        Self {
            ceremony_id,
            committed: false,
            new_epoch: None,
            participants: Vec::new(),
            abort_reason: Some(reason),
        }
    }
}

// ============================================================================
// Ceremony Executor
// ============================================================================

/// Executes guardian ceremonies with consensus guarantees
pub struct GuardianCeremonyExecutor<E: RecoveryEffects> {
    effects: Arc<E>,
}

impl<E: RecoveryEffects + 'static> GuardianCeremonyExecutor<E> {
    /// Create a new ceremony executor
    pub fn new(effects: Arc<E>) -> Self {
        Self { effects }
    }

    /// Emit a ceremony fact to the journal
    async fn emit_fact(&self, fact: CeremonyFact) -> RecoveryResult<()> {
        let timestamp = self
            .effects
            .physical_time()
            .await
            .map(|t| t.ts_ms)
            .unwrap_or(0);

        let mut journal = self.effects.get_journal().await?;
        journal.facts.insert_with_context(
            fact.fact_key(),
            aura_core::FactValue::Bytes(serde_json::to_vec(&fact).unwrap_or_default()),
            aura_core::ActorId::synthetic(&format!(
                "ceremony:{}",
                hex::encode(&fact.ceremony_id().0[..8])
            )),
            aura_core::FactTimestamp::new(timestamp),
            None,
        )?;
        self.effects.persist_journal(&journal).await?;
        Ok(())
    }

    async fn current_time_or_zero(&self) -> PhysicalTime {
        current_physical_time_or_zero(self.effects.as_ref()).await
    }

    fn insufficient_acceptances_reason(state: &CeremonyState) -> CeremonyAbortReason {
        CeremonyAbortReason::InsufficientAcceptances {
            accepted: state.response_count(CeremonyResponse::Accept) as u16,
            required: state.operation.threshold_k,
        }
    }

    async fn emit_aborted_fact(
        &self,
        ceremony_id: CeremonyId,
        reason: &CeremonyAbortReason,
        aborted_at: PhysicalTime,
    ) -> RecoveryResult<()> {
        self.emit_fact(CeremonyFact::Aborted {
            ceremony_id: ceremony_id.0,
            reason: reason.to_string(),
            aborted_at,
        })
        .await
    }

    /// Get current guardian state for prestate computation
    pub async fn get_current_guardian_state(
        &self,
        authority_id: &AuthorityId,
    ) -> RecoveryResult<GuardianState> {
        // Get full threshold state from the signing service
        if let Some(state) = self.effects.threshold_state(authority_id).await {
            let public_key = self
                .effects
                .public_key_package(authority_id)
                .await
                .unwrap_or_default();
            let public_key_hash = Hash32(hash::hash(&public_key));

            // Extract guardian IDs from the generic participant list.
            let guardian_ids: Vec<AuthorityId> = state
                .participants
                .iter()
                .filter_map(|p| match p {
                    aura_core::threshold::ParticipantIdentity::Guardian(id) => Some(*id),
                    _ => None,
                })
                .collect();

            Ok(GuardianState {
                epoch: state.epoch,
                threshold_k: state.threshold,
                guardian_ids,
                public_key_hash,
            })
        } else {
            // No existing guardian configuration
            Ok(GuardianState::empty())
        }
    }

    /// Check if there's a pending ceremony for this authority
    pub async fn has_pending_ceremony(&self, _authority_id: &AuthorityId) -> RecoveryResult<bool> {
        // Check journal for initiated but not committed/aborted ceremonies
        let journal = self.effects.get_journal().await?;

        // Look for ceremony facts
        for (key, _) in journal.facts.iter() {
            let key_str = key.as_str();
            if key_str.starts_with("ceremony:") && key_str.ends_with(":initiated") {
                // Check if this ceremony has a corresponding commit or abort
                let ceremony_prefix = key_str.trim_end_matches(":initiated");
                let has_commit = journal
                    .facts
                    .contains_key(&format!("{ceremony_prefix}:committed"));
                let has_abort = journal
                    .facts
                    .contains_key(&format!("{ceremony_prefix}:aborted"));

                if !has_commit && !has_abort {
                    return Ok(true); // Found a pending ceremony
                }
            }
        }

        Ok(false)
    }

    /// Initiate a new guardian ceremony
    ///
    /// This is the main entry point for starting a guardian rotation.
    /// Returns the ceremony state which can be used to track progress.
    pub async fn initiate_ceremony(
        &self,
        authority_id: AuthorityId,
        new_threshold_k: u16,
        new_guardian_ids: Vec<AuthorityId>,
    ) -> RecoveryResult<CeremonyState> {
        let total_n = new_guardian_ids.len() as u16;

        // Validate inputs
        if new_threshold_k > total_n {
            return Err(RecoveryError::invalid(format!(
                "Threshold {new_threshold_k} cannot exceed total guardians {total_n}"
            )));
        }

        if new_guardian_ids.is_empty() {
            return Err(RecoveryError::invalid("Must have at least one guardian"));
        }

        // Check for pending ceremony
        if self.has_pending_ceremony(&authority_id).await? {
            return Err(RecoveryError::invalid(
                "Cannot start ceremony: another ceremony is already pending",
            ));
        }

        // Get current state and compute prestate hash
        let current_state = self.get_current_guardian_state(&authority_id).await?;
        let prestate_hash = current_state.compute_prestate_hash(&authority_id);

        // Generate nonce for ceremony ID
        let nonce_bytes = self.effects.random_bytes(8).await;
        let nonce = u64::from_le_bytes(nonce_bytes.try_into().unwrap_or([0u8; 8]));

        // Create the operation
        let new_epoch = current_state.epoch + 1;
        let operation = GuardianRotationOp {
            threshold_k: new_threshold_k,
            total_n,
            guardian_ids: new_guardian_ids.clone(),
            new_epoch,
        };
        let operation_hash = operation.compute_hash();

        // Derive ceremony ID
        let ceremony_id = CeremonyId::new(prestate_hash, operation_hash, nonce);

        tracing::info!(
            %ceremony_id,
            %authority_id,
            threshold = new_threshold_k,
            guardians = total_n,
            new_epoch,
            "Initiating guardian ceremony"
        );

        // Generate new FROST keys at the new epoch
        // IMPORTANT: These keys are stored but NOT activated until commit
        let participants: Vec<aura_core::threshold::ParticipantIdentity> = new_guardian_ids
            .iter()
            .map(|id| aura_core::threshold::ParticipantIdentity::guardian(*id))
            .collect();

        let (_epoch, key_packages, public_key_package) = self
            .effects
            .rotate_keys(&authority_id, new_threshold_k, total_n, &participants)
            .await
            .map_err(|e| RecoveryError::internal(format!("Key rotation failed: {e}")))?;

        // Initialize response tracking
        let mut responses = HashMap::new();
        for guardian_id in &new_guardian_ids {
            responses.insert(*guardian_id, CeremonyResponse::Pending);
        }

        let now = self.current_time_or_zero().await;

        // Emit initiated fact
        self.emit_fact(CeremonyFact::Initiated {
            ceremony_id: ceremony_id.0,
            initiator_id: authority_id,
            prestate_hash,
            operation_hash,
            threshold_k: new_threshold_k,
            total_n,
            guardian_ids: new_guardian_ids.clone(),
            initiated_at: now.clone(),
        })
        .await?;

        let state = CeremonyState {
            ceremony_id,
            initiator_id: authority_id,
            prestate_hash,
            operation,
            responses,
            key_packages,
            public_key_package,
            status: CeremonyStatus::AwaitingResponses {
                accepted: 0,
                declined: 0,
                pending: new_guardian_ids.len() as u32,
            },
            initiated_at: now,
            completed_at: None,
            agreement_mode: policy_for(CeremonyFlow::GuardianSetupRotation).initial_mode(),
        };

        Ok(state)
    }

    /// Record a guardian's response to the ceremony
    pub async fn record_response(
        &self,
        state: &mut CeremonyState,
        guardian_id: AuthorityId,
        response: CeremonyResponse,
    ) -> RecoveryResult<()> {
        // Verify guardian is part of this ceremony
        if !state.responses.contains_key(&guardian_id) {
            return Err(RecoveryError::invalid(format!(
                "Guardian {guardian_id} is not part of this ceremony"
            )));
        }

        // Record the response
        state.responses.insert(guardian_id, response);

        let now = self.current_time_or_zero().await;

        // Emit response fact
        self.emit_fact(CeremonyFact::GuardianResponded {
            ceremony_id: state.ceremony_id.0,
            guardian_id,
            response,
            responded_at: now,
        })
        .await?;

        // Update status
        let (accepted, declined, pending) = state.response_counts();
        state.status = CeremonyStatus::AwaitingResponses {
            accepted: accepted as u32,
            declined: declined as u32,
            pending: pending as u32,
        };

        tracing::info!(
            ceremony_id = %state.ceremony_id,
            %guardian_id,
            ?response,
            accepted,
            declined,
            pending,
            "Guardian responded to ceremony"
        );

        Ok(())
    }

    /// Attempt to commit the ceremony
    ///
    /// This should be called when all guardians have responded (or timeout).
    /// Returns true if the ceremony was committed, false if it was aborted.
    pub async fn try_commit(
        &self,
        state: &mut CeremonyState,
        authority_id: &AuthorityId,
    ) -> RecoveryResult<bool> {
        let now = self.current_time_or_zero().await;

        // Check if any guardian declined
        if state.has_decline() {
            let reason = CeremonyAbortReason::GuardianDeclined;
            state.status = CeremonyStatus::Aborted {
                reason: reason.clone(),
            };
            state.completed_at = Some(now.clone());

            self.emit_aborted_fact(state.ceremony_id, &reason, now)
                .await?;

            // Note: No explicit rollback needed - the new epoch keys are simply never activated
            tracing::info!(
                ceremony_id = %state.ceremony_id,
                "Guardian ceremony aborted - guardian declined"
            );

            return Ok(false);
        }

        // Check if we have threshold
        if !state.has_threshold() {
            if state.all_responded() {
                // All responded but didn't reach threshold
                let reason = Self::insufficient_acceptances_reason(state);
                state.status = CeremonyStatus::Aborted {
                    reason: reason.clone(),
                };
                state.completed_at = Some(now.clone());

                self.emit_aborted_fact(state.ceremony_id, &reason, now)
                    .await?;

                tracing::info!(
                    ceremony_id = %state.ceremony_id,
                    "Guardian ceremony aborted - insufficient acceptances"
                );

                return Ok(false);
            }

            // Still waiting for more responses
            return Ok(false);
        }

        // Threshold reached! Commit the key rotation
        let new_epoch = state.operation.new_epoch;
        let policy = policy_for(CeremonyFlow::GuardianSetupRotation);
        if !policy.allows_mode(AgreementMode::ConsensusFinalized) {
            return Err(RecoveryError::invalid(
                "Guardian rotation does not permit consensus finalization",
            ));
        }

        self.effects
            .commit_key_rotation(authority_id, new_epoch)
            .await
            .map_err(|e| RecoveryError::internal(format!("Failed to commit key rotation: {e}")))?;

        if let Some(threshold_state) = self.effects.threshold_state(authority_id).await {
            if threshold_state.agreement_mode != AgreementMode::ConsensusFinalized {
                tracing::warn!(
                    ceremony_id = %state.ceremony_id,
                    agreement_mode = ?threshold_state.agreement_mode,
                    "Guardian rotation committed without consensus finalization"
                );
            }
        }

        // Get participants who accepted
        let participants = state.guardians_with_response(CeremonyResponse::Accept);

        state.status = CeremonyStatus::Committed { new_epoch };
        state.completed_at = Some(now.clone());
        state.agreement_mode = AgreementMode::ConsensusFinalized;

        // Emit commit fact
        self.emit_fact(CeremonyFact::Committed {
            ceremony_id: state.ceremony_id.0,
            new_epoch,
            threshold_k: state.operation.threshold_k,
            guardian_ids: participants.clone(),
            committed_at: now,
        })
        .await?;

        tracing::info!(
            ceremony_id = %state.ceremony_id,
            new_epoch,
            threshold = state.operation.threshold_k,
            participants = participants.len(),
            "Guardian ceremony committed successfully"
        );

        Ok(true)
    }

    /// Abort a ceremony manually
    pub async fn abort_ceremony(
        &self,
        state: &mut CeremonyState,
        reason: String,
    ) -> RecoveryResult<()> {
        let now = self.current_time_or_zero().await;

        let abort_reason = CeremonyAbortReason::Manual {
            reason: reason.clone(),
        };
        state.status = CeremonyStatus::Aborted {
            reason: abort_reason.clone(),
        };
        state.completed_at = Some(now.clone());

        self.emit_aborted_fact(state.ceremony_id, &abort_reason, now)
            .await?;

        // Note: The new epoch keys are simply orphaned, not explicitly deleted
        // This is the "epoch isolation" property - uncommitted epochs are inert

        tracing::info!(
            ceremony_id = %state.ceremony_id,
            %reason,
            "Guardian ceremony aborted"
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
    use aura_effects::crypto::RealCryptoHandler;
    use aura_testkit::MockEffects;
    use std::collections::BTreeMap;

    fn test_authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    #[derive(Default)]
    struct TestGuardianKeyResolver {
        guardian_keys: BTreeMap<AuthorityId, TrustedPublicKey>,
    }

    impl TestGuardianKeyResolver {
        fn register_guardian_key(&mut self, guardian_id: AuthorityId, public_key: Vec<u8>) {
            self.guardian_keys.insert(
                guardian_id,
                TrustedPublicKey::active(
                    TrustedKeyDomain::Guardian,
                    None,
                    public_key.clone(),
                    Hash32::from_bytes(&public_key),
                ),
            );
        }
    }

    impl TrustedKeyResolver for TestGuardianKeyResolver {
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
            _device: aura_core::DeviceId,
        ) -> Result<TrustedPublicKey, KeyResolutionError> {
            Err(KeyResolutionError::Unknown {
                domain: TrustedKeyDomain::Device,
            })
        }

        fn resolve_guardian_key(
            &self,
            guardian: AuthorityId,
        ) -> Result<TrustedPublicKey, KeyResolutionError> {
            self.guardian_keys
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

    fn test_rotation_operation() -> GuardianRotationOp {
        GuardianRotationOp {
            threshold_k: 2,
            total_n: 3,
            guardian_ids: vec![test_authority(2), test_authority(3), test_authority(4)],
            new_epoch: 10,
        }
    }

    async fn real_crypto_ceremony_response(
        crypto: &RealCryptoHandler,
        ceremony_id: CeremonyId,
        initiator_id: AuthorityId,
        prestate_hash: Hash32,
        operation: &GuardianRotationOp,
        guardian_id: AuthorityId,
        key_package_hash: Hash32,
    ) -> (CeremonyResponseMsg, Vec<u8>, Vec<u8>) {
        let (guardian_private_key, guardian_public_key) =
            crypto.ed25519_generate_keypair().await.unwrap();
        let signature = sign_guardian_ceremony_response_with_context(
            crypto,
            ceremony_id,
            initiator_id,
            prestate_hash,
            operation,
            guardian_id,
            CeremonyResponse::Accept,
            key_package_hash,
            &guardian_private_key,
        )
        .await
        .unwrap();
        (
            CeremonyResponseMsg {
                ceremony_id,
                guardian_id,
                response: CeremonyResponse::Accept,
                encrypted_key_package_hash: key_package_hash,
                signature,
            },
            guardian_private_key,
            guardian_public_key,
        )
    }

    #[test]
    fn test_ceremony_id_deterministic() {
        let prestate = Hash32([1u8; 32]);
        let operation = Hash32([2u8; 32]);

        let id1 = CeremonyId::new(prestate, operation, 42);
        let id2 = CeremonyId::new(prestate, operation, 42);
        let id3 = CeremonyId::new(prestate, operation, 43);

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_guardian_state_prestate_hash_deterministic() {
        let authority = test_authority(1);

        let state1 = GuardianState {
            epoch: 1,
            threshold_k: 2,
            guardian_ids: vec![test_authority(2), test_authority(3)],
            public_key_hash: Hash32([0xAB; 32]),
        };

        let state2 = GuardianState {
            epoch: 1,
            threshold_k: 2,
            guardian_ids: vec![test_authority(3), test_authority(2)], // Different order
            public_key_hash: Hash32([0xAB; 32]),
        };

        // Should be equal due to sorting
        assert_eq!(
            state1.compute_prestate_hash(&authority),
            state2.compute_prestate_hash(&authority)
        );
    }

    #[test]
    fn test_ceremony_state_threshold_check() {
        let mut responses = HashMap::new();
        responses.insert(test_authority(1), CeremonyResponse::Accept);
        responses.insert(test_authority(2), CeremonyResponse::Accept);
        responses.insert(test_authority(3), CeremonyResponse::Pending);

        let state = CeremonyState {
            ceremony_id: CeremonyId(Hash32([0; 32])),
            initiator_id: test_authority(0),
            prestate_hash: Hash32([0; 32]),
            operation: GuardianRotationOp {
                threshold_k: 2,
                total_n: 3,
                guardian_ids: vec![test_authority(1), test_authority(2), test_authority(3)],
                new_epoch: 1,
            },
            responses,
            key_packages: vec![],
            public_key_package: vec![],
            status: CeremonyStatus::AwaitingResponses {
                accepted: 2,
                declined: 0,
                pending: 1,
            },
            initiated_at: PhysicalTime {
                ts_ms: 0,
                uncertainty: None,
            },
            completed_at: None,
            agreement_mode: AgreementMode::CoordinatorSoftSafe,
        };

        assert!(state.has_threshold()); // 2-of-3 met
        assert!(!state.has_decline());
        assert!(!state.all_responded()); // One still pending
    }

    #[test]
    fn guardian_rotation_transcript_binds_epoch_and_response() {
        let proposal = CeremonyProposal {
            ceremony_id: CeremonyId(Hash32([1u8; 32])),
            initiator_id: test_authority(1),
            prestate_hash: Hash32([2u8; 32]),
            operation: test_rotation_operation(),
            encrypted_key_package: vec![5],
            encryption_nonce: [6u8; 12],
            ephemeral_public_key: vec![7; 32],
            recipient_public_key: vec![8; 32],
            key_package_version: GUARDIAN_CEREMONY_ENCRYPTION_PROTOCOL_VERSION,
            encrypted_key_package_hash: Hash32::from_bytes(&[5u8]),
            binding_hash: Hash32::from_bytes(&[9u8]),
        };
        let mut different_epoch = proposal.clone();
        different_epoch.operation.new_epoch = 11;

        let accept = CeremonyResponseMsg {
            ceremony_id: proposal.ceremony_id,
            guardian_id: test_authority(2),
            response: CeremonyResponse::Accept,
            encrypted_key_package_hash: proposal.encrypted_key_package_hash,
            signature: vec![8],
        };
        let mut decline = accept.clone();
        decline.response = CeremonyResponse::Decline;

        let proposal_bytes =
            aura_signature::encode_transcript("aura.guardian-rotation.proposal", 1, &proposal)
                .unwrap();
        let epoch_bytes = aura_signature::encode_transcript(
            "aura.guardian-rotation.proposal",
            1,
            &different_epoch,
        )
        .unwrap();
        let accept_bytes =
            aura_signature::encode_transcript("aura.guardian-rotation.response", 1, &accept)
                .unwrap();
        let decline_bytes =
            aura_signature::encode_transcript("aura.guardian-rotation.response", 1, &decline)
                .unwrap();

        assert_ne!(proposal_bytes, epoch_bytes);
        assert_ne!(accept_bytes, decline_bytes);
    }

    #[tokio::test]
    async fn test_ceremony_executor_creation() {
        let effects = Arc::new(MockEffects::deterministic());
        let executor = GuardianCeremonyExecutor::new(effects);

        let authority = test_authority(0);
        let state = executor.get_current_guardian_state(&authority).await;
        assert!(state.is_ok());
    }

    #[tokio::test]
    async fn guardian_ceremony_key_package_round_trip_uses_bound_x25519_key_agreement() {
        let crypto = RealCryptoHandler::for_simulation_seed([0x41; 32]);
        let initiator_id = test_authority(1);
        let ceremony_id = CeremonyId(Hash32([0x51; 32]));
        let prestate_hash = Hash32([0x61; 32]);
        let operation = test_rotation_operation();
        let (_guardian_private_key, guardian_public_key) =
            crypto.ed25519_generate_keypair().await.unwrap();
        let key_package = vec![0xAB; 64];

        let proposal = encrypt_ceremony_key_package(
            &crypto,
            ceremony_id,
            initiator_id,
            prestate_hash,
            &operation,
            &guardian_public_key,
            &key_package,
        )
        .await
        .unwrap();
        let (guardian_private_key, derived_public_key) =
            crypto.ed25519_generate_keypair().await.unwrap();
        assert_ne!(guardian_public_key, derived_public_key);

        let decrypted =
            decrypt_ceremony_key_package(&crypto, &proposal, &guardian_private_key).await;
        assert!(decrypted.is_err());

        let (recipient_private_key, recipient_public_key) =
            crypto.ed25519_generate_keypair().await.unwrap();
        let proposal = encrypt_ceremony_key_package(
            &crypto,
            ceremony_id,
            initiator_id,
            prestate_hash,
            &operation,
            &recipient_public_key,
            &key_package,
        )
        .await
        .unwrap();
        let decrypted = decrypt_ceremony_key_package(&crypto, &proposal, &recipient_private_key)
            .await
            .unwrap();
        assert_eq!(decrypted, key_package);
        assert_eq!(
            proposal.key_package_version,
            GUARDIAN_CEREMONY_ENCRYPTION_PROTOCOL_VERSION
        );
    }

    #[tokio::test]
    async fn guardian_ceremony_commit_certificate_rejects_invalid_accepts() {
        let crypto = RealCryptoHandler::for_simulation_seed([0x42; 32]);
        let ceremony_id = CeremonyId(Hash32([0x52; 32]));
        let initiator_id = test_authority(1);
        let prestate_hash = Hash32([0x62; 32]);
        let operation = test_rotation_operation();
        let guardians = operation.guardian_ids.clone();
        let package_hash_a = Hash32([0x71; 32]);
        let package_hash_b = Hash32([0x72; 32]);

        let (response_a, _private_a, public_a) = real_crypto_ceremony_response(
            &crypto,
            ceremony_id,
            initiator_id,
            prestate_hash,
            &operation,
            guardians[0],
            package_hash_a,
        )
        .await;
        let (mut response_b, _private_b, public_b) = real_crypto_ceremony_response(
            &crypto,
            ceremony_id,
            initiator_id,
            prestate_hash,
            &operation,
            guardians[1],
            package_hash_b,
        )
        .await;

        let mut resolver = TestGuardianKeyResolver::default();
        resolver.register_guardian_key(guardians[0], public_a);
        resolver.register_guardian_key(guardians[1], public_b.clone());

        let certificate = build_guardian_ceremony_commit_certificate(
            &crypto,
            ceremony_id,
            initiator_id,
            prestate_hash,
            &operation,
            &guardians,
            operation.threshold_k,
            &[response_a.clone(), response_b.clone()],
            &resolver,
        )
        .await
        .unwrap();
        assert!(verify_guardian_ceremony_commit_certificate(
            &crypto,
            &certificate,
            &guardians,
            operation.threshold_k,
            &resolver,
        )
        .await
        .unwrap());

        response_b.guardian_id = guardians[0];
        let duplicate = build_guardian_ceremony_commit_certificate(
            &crypto,
            ceremony_id,
            initiator_id,
            prestate_hash,
            &operation,
            &guardians,
            operation.threshold_k,
            &[response_a.clone(), response_b.clone()],
            &resolver,
        )
        .await;
        assert!(duplicate.is_err());

        let mut forged = response_a.clone();
        forged.signature[0] ^= 0x01;
        let forged_certificate = build_guardian_ceremony_commit_certificate(
            &crypto,
            ceremony_id,
            initiator_id,
            prestate_hash,
            &operation,
            &guardians,
            operation.threshold_k,
            &[forged, response_b.clone()],
            &resolver,
        )
        .await;
        assert!(forged_certificate.is_err());

        let mut empty_signature = response_a.clone();
        empty_signature.signature.clear();
        let empty_signature_certificate = build_guardian_ceremony_commit_certificate(
            &crypto,
            ceremony_id,
            initiator_id,
            prestate_hash,
            &operation,
            &guardians,
            operation.threshold_k,
            &[empty_signature, response_b.clone()],
            &resolver,
        )
        .await;
        assert!(empty_signature_certificate.is_err());

        let mut unknown_guardian = response_b;
        unknown_guardian.guardian_id = test_authority(9);
        let unknown_guardian_certificate = build_guardian_ceremony_commit_certificate(
            &crypto,
            ceremony_id,
            initiator_id,
            prestate_hash,
            &operation,
            &guardians,
            operation.threshold_k,
            &[response_a, unknown_guardian],
            &resolver,
        )
        .await;
        assert!(unknown_guardian_certificate.is_err());
    }
}

#[cfg(test)]
mod theorem_pack_tests {
    use super::telltale_session_types_guardian_ceremony;
    use aura_protocol::admission::{
        CAPABILITY_PROTOCOL_ENVELOPE_BRIDGE, CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADHERENCE,
        CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADMISSION, THEOREM_PACK_AURA_AUTHORITY_EVIDENCE,
    };

    #[test]
    fn guardian_ceremony_proof_status_exposes_required_authority_pack() {
        assert_eq!(
            telltale_session_types_guardian_ceremony::proof_status::REQUIRED_THEOREM_PACKS,
            &[THEOREM_PACK_AURA_AUTHORITY_EVIDENCE]
        );
    }

    #[test]
    fn guardian_ceremony_manifest_emits_authority_evidence_metadata() {
        let manifest =
            telltale_session_types_guardian_ceremony::vm_artifacts::composition_manifest();
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
