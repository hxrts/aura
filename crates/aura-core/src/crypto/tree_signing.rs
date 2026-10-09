//! FROST Threshold Signing Primitives for Tree Operations
//!
//! This module provides pure cryptographic primitives for FROST threshold signatures
//! used in commitment tree operations. It contains **NO** tree logic or business logic.
//!
//! ## Design Principles
//!
//! - **Pure Cryptography**: Only signing, aggregation, and verification
//! - **No Tree State**: No knowledge of TreeState, NodeIndex, or tree structure
//! - **Binding Context**: Operations bound to epoch/policy/node to prevent replay
//!
//! ## Architecture
//!
//! FROST signing follows the classic threshold signature flow:
//! 1. Each signer generates a nonce commitment
//! 2. Coordinator collects commitments and opens
//! 3. Each signer creates partial signature with their share
//! 4. Coordinator aggregates partials into group signature
//! 5. Anyone can verify against group public key
//!
//! ## References
//!
//! - [`docs/102_authority_and_identity.md`](../../../../docs/102_authority_and_identity.md) - Tree operations
//! - FROST paper: https://eprint.iacr.org/2020/852

use crate::crypto::hash;
use crate::errors::AuraError;
use crate::{AttestedOp, TreeOpKind};
use frost_ed25519 as frost;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt};
use zeroize::{Zeroize, ZeroizeOnDrop};

// === Size bounds for serialized cryptographic data (Safety §2) ===

/// Maximum size of a FROST signing share (Ed25519 scalar)
pub const MAX_SHARE_BYTES: usize = 32;

/// Exact size of postcard-serialized FROST signing commitments (SigningCommitments).
pub const MAX_COMMITMENT_BYTES: usize = 69;

/// Exact size of postcard-serialized FROST signing nonces (SigningNonces).
pub const MAX_NONCE_BYTES: usize = 138;

/// Maximum size of a FROST partial signature (Ed25519 scalar)
pub const MAX_PARTIAL_SIGNATURE_BYTES: usize = 32;

/// Maximum size of an aggregated Ed25519 signature
pub const MAX_SIGNATURE_BYTES: usize = 64;

/// Maximum size of a public key (Ed25519 compressed point)
pub const MAX_PUBLIC_KEY_BYTES: usize = 32;

/// Maximum size of a message to sign
pub const MAX_MESSAGE_BYTES: usize = 1024;

/// FROST signing share (secret)
///
/// **CRITICAL**: Shares are NEVER stored in the journal. Each device maintains
/// shares locally, keyed by (node_id, epoch). Shares are derived off-chain via
/// separate DKG or resharing ceremonies.
///
/// This wraps the frost-ed25519 SigningShare type for serialization.
// Clone/serde are retained for the explicit FROST share handoff and secure
// storage APIs that currently carry Share as data. Debug remains manually
// redacted because `value` is a serialized signing share.
#[derive(Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct Share {
    /// Share identifier (1..=n)
    pub identifier: u16,
    /// Security-sensitive serialized signing share. Zeroized on drop.
    #[serde(with = "serde_bytes")]
    pub value: Vec<u8>,
}

impl fmt::Debug for Share {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Share")
            .field("identifier", &self.identifier)
            .field("value_len", &self.value.len())
            .field("value", &"<redacted>")
            .finish()
    }
}

impl Share {
    /// Create a new share from a FROST signing share
    pub fn from_frost(identifier: frost::Identifier, share: frost::keys::SigningShare) -> Self {
        let id_bytes = identifier.serialize();
        Self {
            identifier: u16::from_be_bytes([0, id_bytes[0]]),
            value: share.serialize().to_vec(),
        }
    }

    /// Convert to FROST signing share for use in signing
    pub fn to_frost(&self) -> Result<frost::keys::SigningShare, AuraError> {
        if self.value.len() != 32 {
            return Err(AuraError::crypto(format!(
                "Invalid share length: {} (expected 32)",
                self.value.len()
            )));
        }
        let mut array = [0u8; 32];
        array.copy_from_slice(&self.value);
        frost::keys::SigningShare::deserialize(array)
            .map_err(|e| AuraError::crypto(format!("Failed to deserialize signing share: {e}")))
    }

    /// Get FROST identifier
    pub fn frost_identifier(&self) -> Result<frost::Identifier, AuraError> {
        frost::Identifier::try_from(self.identifier)
            .map_err(|e| AuraError::crypto(format!("Invalid identifier: {e}")))
    }
}

/// Commitment to a nonce (public)
///
/// Sent to the coordinator during the commitment phase. Does not reveal
/// the nonce value but commits the signer to a specific nonce.
///
/// This wraps the frost-ed25519 SigningCommitments type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NonceCommitment {
    /// Signer identifier
    pub signer: u16,
    /// Commitment value (serialized frost SigningCommitments)
    #[serde(with = "serde_bytes")]
    pub commitment: Vec<u8>,
}

impl NonceCommitment {
    /// Create from FROST signing commitments
    pub fn from_frost(
        identifier: frost::Identifier,
        commitments: frost::round1::SigningCommitments,
    ) -> Result<Self, AuraError> {
        let id_bytes = identifier.serialize();
        let commitment = commitments.serialize().map_err(|e| {
            AuraError::crypto(format!("Failed to serialize FROST commitments: {e}"))
        })?;
        let commitment_bytes: &[u8] = commitment.as_ref();
        if commitment_bytes.len() != MAX_COMMITMENT_BYTES {
            return Err(AuraError::crypto(format!(
                "Invalid commitment length: {} (expected {MAX_COMMITMENT_BYTES})",
                commitment_bytes.len(),
            )));
        }

        Ok(Self {
            signer: u16::from_be_bytes([0, id_bytes[0]]),
            commitment: commitment_bytes.to_vec(),
        })
    }

    /// Convert to FROST signing commitments
    pub fn to_frost(&self) -> Result<frost::round1::SigningCommitments, AuraError> {
        frost::round1::SigningCommitments::deserialize(&self.commitment)
            .map_err(|e| AuraError::crypto(format!("Failed to deserialize commitments: {e}")))
    }

    /// Get FROST identifier
    pub fn frost_identifier(&self) -> Result<frost::Identifier, AuraError> {
        frost::Identifier::try_from(self.signer)
            .map_err(|e| AuraError::crypto(format!("Invalid identifier: {e}")))
    }

    /// Create from bytes (for testing and mock implementations)
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, AuraError> {
        // For mock implementations, create a simple commitment
        if bytes.len() != MAX_COMMITMENT_BYTES {
            return Err(AuraError::crypto(format!(
                "Invalid commitment length: {} (expected {MAX_COMMITMENT_BYTES})",
                bytes.len(),
            )));
        }

        Ok(Self {
            signer: 1, // Default signer for mock
            commitment: bytes,
        })
    }
}

/// Partial signature from one signer (public)
///
/// Created by applying the signing share to the message. The coordinator
/// aggregates these to form the final signature.
///
/// This wraps the frost-ed25519 SignatureShare type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartialSignature {
    /// Signer identifier
    pub signer: u16,
    /// Partial signature value (serialized frost SignatureShare)
    #[serde(with = "serde_bytes")]
    pub signature: Vec<u8>,
}

impl PartialSignature {
    /// Create from FROST signature share
    pub fn from_frost(identifier: frost::Identifier, share: frost::round2::SignatureShare) -> Self {
        let id_bytes = identifier.serialize();
        Self {
            signer: u16::from_be_bytes([0, id_bytes[0]]),
            signature: share.serialize().to_vec(),
        }
    }

    /// Convert to FROST signature share
    pub fn to_frost(&self) -> Result<frost::round2::SignatureShare, AuraError> {
        if self.signature.len() != 32 {
            return Err(AuraError::crypto(format!(
                "Invalid signature length: {} (expected 32)",
                self.signature.len()
            )));
        }
        let mut array = [0u8; 32];
        array.copy_from_slice(&self.signature);
        frost::round2::SignatureShare::deserialize(array)
            .map_err(|e| AuraError::crypto(format!("Failed to deserialize signature share: {e}")))
    }

    /// Get FROST identifier
    pub fn frost_identifier(&self) -> Result<frost::Identifier, AuraError> {
        frost::Identifier::try_from(self.signer)
            .map_err(|e| AuraError::crypto(format!("Invalid identifier: {e}")))
    }

    /// Create from bytes (for testing and mock implementations)
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, AuraError> {
        // For mock implementations, create a simple partial signature
        if bytes.len() < 32 {
            return Err(AuraError::crypto("Partial signature too short"));
        }

        Ok(Self {
            signer: 1, // Default signer for mock
            signature: bytes,
        })
    }
}

/// Tree signing context for binding
///
/// Binds signatures to specific tree operations to prevent replay attacks.
/// All signing operations must include this context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeSigningContext {
    /// Node identifier in the tree
    pub node_id: u32,
    /// Current epoch
    pub epoch: u64,
    /// Policy hash at this node
    pub policy_hash: [u8; 32],
}

impl TreeSigningContext {
    /// Create a new tree signing context
    pub fn new(node_id: u32, epoch: u64, policy_hash: [u8; 32]) -> Self {
        Self {
            node_id,
            epoch,
            policy_hash,
        }
    }
}

/// Generate a binding message for tree operations
///
/// Combines the tree operation with the signing context to create a unique
/// message that prevents replay across epochs, policies, and nodes.
///
/// ## Format
///
/// ```text
/// BLAKE3(
///   "TREE_OP_SIG" ||
///   node_id (u32, LE) ||
///   epoch (u64, LE) ||
///   policy_hash (32 bytes) ||
///   parent_epoch (u64, LE) ||
///   parent_commitment (32 bytes) ||
///   serialized_op_kind
/// )
/// ```
///
/// ## Examples
///
/// ```
/// use aura_core::crypto::tree_signing::{TreeSigningContext, binding_message};
/// use aura_core::TreeOp;
///
/// let ctx = TreeSigningContext::new(1, 42, [0u8; 32]);
/// // let op = TreeOp { ... };
/// // let msg = binding_message(&ctx, &op);
/// ```
pub fn binding_message(ctx: &TreeSigningContext, op_bytes: &[u8]) -> Vec<u8> {
    let mut h = hash::hasher();

    // Domain separator
    h.update(b"TREE_OP_SIG");

    // Context binding
    h.update(&ctx.node_id.to_le_bytes());
    h.update(&ctx.epoch.to_le_bytes());
    h.update(&ctx.policy_hash);

    // Operation content
    h.update(op_bytes);

    h.finalize().to_vec()
}

/// Compute a binding message for an attested tree operation using core types.
///
/// This mirrors the binding used by journal verification and keeps the logic
/// near the canonical tree types to avoid duplicated hashing code elsewhere.
pub fn tree_op_binding_message(
    attested: &AttestedOp,
    current_epoch: crate::Epoch,
    group_public_key: &[u8; 32],
) -> Vec<u8> {
    let mut h = hash::hasher();

    // Domain separator
    h.update(b"TREE_OP_VERIFY");

    // Parent metadata
    h.update(&u64::from(attested.op.parent_epoch).to_le_bytes());
    h.update(&attested.op.parent_commitment);
    h.update(&attested.op.version.to_le_bytes());

    // Current epoch
    h.update(&u64::from(current_epoch).to_le_bytes());

    // Group public key binds signature to signing group
    h.update(group_public_key);

    // Serialize operation specifics
    let op_bytes = serialize_tree_op_for_binding(&attested.op.op);
    h.update(&op_bytes);

    h.finalize().to_vec()
}

/// Lightweight serialization for tree operations used in binding calculation.
fn serialize_tree_op_for_binding(op: &TreeOpKind) -> Vec<u8> {
    let mut buffer = Vec::new();
    match op {
        TreeOpKind::AddLeaf { leaf, under } => {
            buffer.extend_from_slice(b"AddLeaf");
            buffer.extend_from_slice(&leaf.leaf_id.0.to_le_bytes());
            buffer.extend_from_slice(&under.0.to_le_bytes());
            buffer.extend_from_slice(&leaf.public_key);
        }
        TreeOpKind::RemoveLeaf { leaf, reason } => {
            buffer.extend_from_slice(b"RemoveLeaf");
            buffer.extend_from_slice(&leaf.0.to_le_bytes());
            buffer.push(*reason);
        }
        TreeOpKind::ChangePolicy { node, new_policy } => {
            buffer.extend_from_slice(b"ChangePolicy");
            buffer.extend_from_slice(&node.0.to_le_bytes());
            buffer.extend_from_slice(&crate::tree::commitment::policy_hash(new_policy));
        }
        TreeOpKind::RotateEpoch { affected } => {
            buffer.extend_from_slice(b"RotateEpoch");
            buffer.extend_from_slice(&(affected.len() as u32).to_le_bytes());
            for node in affected {
                buffer.extend_from_slice(&node.0.to_le_bytes());
            }
        }
    }
    buffer
}

/// One signer's FROST signing nonces, usable for exactly one signature share
/// (docs/122 "Replay Protection").
///
/// Not `Clone`, not serializable, and zeroized on drop: the secret never
/// leaves this value. The public commitment is read from it for round one;
/// signing needs a [`RetiredFrostNonces`], which only [`FrostNonces::retire`]
/// produces after the retirement record is written, and signing consumes it.
///
/// ```compile_fail,E0599
/// use aura_core::crypto::tree_signing::FrostNonces;
/// fn clone_nonces(nonces: FrostNonces) -> (FrostNonces, FrostNonces) {
///     (nonces.clone(), nonces)
/// }
/// ```
///
/// ```compile_fail,E0382
/// use aura_core::crypto::tree_signing::RetiredFrostNonces;
/// fn sign_twice(
///     nonces: RetiredFrostNonces,
///     package: &frost_ed25519::SigningPackage,
///     key: &frost_ed25519::keys::KeyPackage,
/// ) {
///     let _first = nonces.sign(package, key);
///     let _second = nonces.sign(package, key);
/// }
/// ```
///
/// ```compile_fail,E0599
/// use aura_core::crypto::tree_signing::FrostNonces;
/// fn sign_unretired(
///     nonces: FrostNonces,
///     package: &frost_ed25519::SigningPackage,
///     key: &frost_ed25519::keys::KeyPackage,
/// ) {
///     let _share = nonces.sign(package, key);
/// }
/// ```
pub struct FrostNonces {
    participant: u16,
    nonces: frost::round1::SigningNonces,
    commitment: NonceCommitment,
}

impl fmt::Debug for FrostNonces {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FrostNonces")
            .field("participant", &self.participant)
            .field("nonces", &"<redacted>")
            .finish()
    }
}

impl FrostNonces {
    /// Fresh nonces for the signer holding `key_package`, from `rng` (the
    /// caller's random effect or audited OS entropy).
    pub fn generate(
        key_package: &frost::keys::KeyPackage,
        rng: &mut (impl rand::RngCore + rand::CryptoRng),
    ) -> Result<Self, AuraError> {
        Self::generate_for_share(*key_package.identifier(), key_package.signing_share(), rng)
    }

    /// Fresh nonces for signer `identifier` holding `signing_share`.
    pub fn generate_for_share(
        identifier: frost::Identifier,
        signing_share: &frost::keys::SigningShare,
        rng: &mut (impl rand::RngCore + rand::CryptoRng),
    ) -> Result<Self, AuraError> {
        let (nonces, commitments) = frost::round1::commit(signing_share, rng);
        let commitment = NonceCommitment::from_frost(identifier, commitments)?;
        Ok(Self {
            participant: commitment.signer,
            nonces,
            commitment,
        })
    }

    /// The signer's FROST participant index.
    pub fn participant(&self) -> u16 {
        self.participant
    }

    /// The public round-one commitment.
    pub fn commitment(&self) -> &NonceCommitment {
        &self.commitment
    }

    /// The public round-one commitment in the effect DTO shape.
    pub fn public_commitment(&self) -> crate::effects::crypto::FrostPublicCommitment {
        crate::effects::crypto::FrostPublicCommitment {
            participant_index: self.participant,
            commitment_bytes: self.commitment.commitment.clone(),
        }
    }

    /// Record these nonces as retired in `log`, then return the only value
    /// signing accepts. A log that already holds this commitment refuses.
    pub async fn retire(
        self,
        log: &(impl FrostNonceRetirement + ?Sized),
    ) -> Result<RetiredFrostNonces, AuraError> {
        log.retire_frost_nonce(&FrostNonceRetirementRecord {
            participant: self.participant,
            commitment_digest: crate::Hash32(hash::hash(&self.commitment.commitment)),
        })
        .await?;
        Ok(RetiredFrostNonces { nonces: self })
    }
}

/// [`FrostNonces`] whose retirement is recorded; signing consumes it.
pub struct RetiredFrostNonces {
    nonces: FrostNonces,
}

impl fmt::Debug for RetiredFrostNonces {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RetiredFrostNonces")
            .field("participant", &self.nonces.participant)
            .finish()
    }
}

impl RetiredFrostNonces {
    /// The signer's FROST participant index.
    pub fn participant(&self) -> u16 {
        self.nonces.participant
    }

    /// The public round-one commitment these nonces were committed under.
    pub fn commitment(&self) -> &NonceCommitment {
        &self.nonces.commitment
    }

    /// Produce this signer's share over `package`, consuming the nonces. The
    /// package must carry exactly this signer's commitment.
    pub fn sign(
        self,
        package: &frost::SigningPackage,
        key_package: &frost::keys::KeyPackage,
    ) -> Result<frost::round2::SignatureShare, AuraError> {
        let identifier = self.nonces.commitment.frost_identifier()?;
        let own = self.nonces.commitment.to_frost()?;
        if key_package.identifier() != &identifier
            || package.signing_commitments().get(&identifier) != Some(&own)
        {
            return Err(AuraError::crypto(
                "signing package does not carry this signer's nonce commitment",
            ));
        }
        frost::round2::sign(package, &self.nonces.nonces, key_package)
            .map_err(|e| AuraError::crypto(format!("FROST signing failed: {e}")))
    }
}

/// The record a [`FrostNonceRetirement`] log keeps for one retired nonce.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FrostNonceRetirementRecord {
    /// Signer participant index.
    pub participant: u16,
    /// Digest of the public commitment the nonces were committed under.
    pub commitment_digest: crate::Hash32,
}

/// Where FROST nonce retirements are recorded before signing. An
/// implementation must refuse a record it already holds. Nonces whose
/// secret is persisted need a durable log; nonces that never leave process
/// memory may use [`ProcessFrostNonceRetirement`].
#[async_trait::async_trait]
pub trait FrostNonceRetirement: Send + Sync {
    /// Record `record` as retired, or refuse if it already is.
    async fn retire_frost_nonce(
        &self,
        record: &FrostNonceRetirementRecord,
    ) -> Result<(), AuraError>;
}

/// In-process retirement log for nonces that never leave memory (a restart
/// drops them, so no durable record is needed).
#[derive(Debug, Default)]
pub struct ProcessFrostNonceRetirement {
    retired: futures::lock::Mutex<std::collections::HashSet<FrostNonceRetirementRecord>>,
}

#[async_trait::async_trait]
impl FrostNonceRetirement for ProcessFrostNonceRetirement {
    async fn retire_frost_nonce(
        &self,
        record: &FrostNonceRetirementRecord,
    ) -> Result<(), AuraError> {
        let mut retired = self.retired.lock().await;
        if !retired.insert(record.clone()) {
            return Err(AuraError::crypto("FROST nonces already retired"));
        }
        Ok(())
    }
}

/// Aggregate partial signatures using FROST
///
/// Combines threshold number of partial signatures into a single
/// group signature that can be verified against the group public key.
///
/// ## Parameters
///
/// - `partials`: Slice of partial signatures from threshold participants
/// - `msg`: The message that was signed
/// - `commitments`: Map of nonce commitments from all signers
/// - `pubkey_package`: The group's public key package
///
/// ## Returns
///
/// The aggregated Ed25519 signature (64 bytes)
///
/// ## Errors
///
/// Returns error if:
/// - Partial signature deserialization fails
/// - Commitment deserialization fails
/// - FROST aggregation fails (e.g., invalid shares)
pub fn frost_aggregate(
    partials: &[PartialSignature],
    msg: &[u8],
    commitments: &BTreeMap<u16, NonceCommitment>,
    pubkey_package: &frost::keys::PublicKeyPackage,
) -> Result<Vec<u8>, AuraError> {
    // Convert partial signatures to FROST format
    let mut frost_shares = BTreeMap::new();
    for partial in partials {
        let identifier = partial.frost_identifier()?;
        let share = partial.to_frost()?;
        frost_shares.insert(identifier, share);
    }

    // Convert commitments to FROST format
    let mut frost_commitments = BTreeMap::new();
    for (signer_id, commitment) in commitments {
        let frost_id = frost::Identifier::try_from(*signer_id)
            .map_err(|e| AuraError::crypto(format!("Invalid signer ID {signer_id}: {e}")))?;
        let frost_commit = commitment.to_frost()?;
        frost_commitments.insert(frost_id, frost_commit);
    }

    // Create signing package
    let signing_package = frost::SigningPackage::new(frost_commitments, msg);

    // Aggregate signature shares
    let group_signature = frost::aggregate(&signing_package, &frost_shares, pubkey_package)
        .map_err(|e| AuraError::crypto(format!("FROST aggregation failed: {e}")))?;

    // Return serialized signature
    Ok(group_signature.serialize().as_ref().to_vec())
}

/// Verify an aggregate signature using FROST
///
/// Verifies that an aggregate signature is valid for the given message
/// and group public key.
///
/// ## Parameters
///
/// - `group_pk`: The group's verification key (from PublicKeyPackage)
/// - `msg`: The message that was signed
/// - `signature`: The aggregated signature bytes (64 bytes for Ed25519)
///
/// ## Returns
///
/// `Ok(())` if signature is valid, `Err(AuraError)` otherwise
pub fn frost_verify_aggregate(
    group_pk: &frost::VerifyingKey,
    msg: &[u8],
    signature: &[u8],
) -> Result<(), AuraError> {
    // Deserialize signature
    if signature.len() != 64 {
        return Err(AuraError::crypto(format!(
            "Invalid signature length: {} (expected 64)",
            signature.len()
        )));
    }
    let mut sig_array = [0u8; 64];
    sig_array.copy_from_slice(signature);
    let sig = frost::Signature::deserialize(sig_array)
        .map_err(|e| AuraError::crypto(format!("Invalid signature format: {e}")))?;

    // Verify signature
    group_pk
        .verify(msg, &sig)
        .map_err(|e| AuraError::crypto(format!("Signature verification failed: {e}")))
}

/// Threshold signature result (aggregated signature)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdSignature {
    /// The aggregated Ed25519 signature bytes (64 bytes)
    pub signature: Vec<u8>,
    /// Signers who participated in this signature
    pub signers: Vec<u16>,
}

impl ThresholdSignature {
    /// Create a new threshold signature
    pub fn new(signature: Vec<u8>, signers: Vec<u16>) -> Self {
        Self { signature, signers }
    }

    /// Get the signature bytes
    pub fn as_bytes(&self) -> &[u8] {
        &self.signature
    }
}

/// Public key package from DKG ceremony
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicKeyPackage {
    /// The group's public key for verification
    pub group_public_key: Vec<u8>,
    /// Individual signer public keys
    pub signer_public_keys: std::collections::BTreeMap<u16, Vec<u8>>,
    /// Threshold parameters
    pub threshold: u16,
    /// Maximum number of signers
    pub max_signers: u16,
}

impl PublicKeyPackage {
    /// Create a new public key package
    pub fn new(
        group_public_key: Vec<u8>,
        signer_public_keys: std::collections::BTreeMap<u16, Vec<u8>>,
        threshold: u16,
        max_signers: u16,
    ) -> Self {
        Self {
            group_public_key,
            signer_public_keys,
            threshold,
            max_signers,
        }
    }
}

// Type conversions between aura-core FROST types and frost_ed25519 types
// These enable seamless interoperability across architectural layers

impl From<frost_ed25519::keys::PublicKeyPackage> for PublicKeyPackage {
    fn from(frost_pkg: frost_ed25519::keys::PublicKeyPackage) -> Self {
        // Extract the group public key
        let group_public_key = frost_pkg.verifying_key().serialize().to_vec();

        // Extract individual signer public keys
        let mut signer_public_keys = std::collections::BTreeMap::new();
        for (frost_id, verifying_share) in frost_pkg.verifying_shares() {
            // Convert frost Identifier to u16
            let signer_id = u16::from_be_bytes([0, frost_id.serialize()[0]]);
            signer_public_keys.insert(signer_id, verifying_share.serialize().to_vec());
        }

        // Note: FROST PublicKeyPackage doesn't expose threshold/max_signers directly
        // We'll use reasonable defaults based on the number of signers
        let max_signers = signer_public_keys.len() as u16;
        let threshold = max_signers.div_ceil(2); // Simple majority threshold

        Self {
            group_public_key,
            signer_public_keys,
            threshold,
            max_signers,
        }
    }
}

impl TryFrom<PublicKeyPackage> for frost_ed25519::keys::PublicKeyPackage {
    type Error = AuraError;

    fn try_from(aura_pkg: PublicKeyPackage) -> Result<Self, Self::Error> {
        // Parse the group verifying key
        if aura_pkg.group_public_key.len() != 32 {
            return Err(AuraError::crypto(format!(
                "Invalid group public key length: {} (expected 32)",
                aura_pkg.group_public_key.len()
            )));
        }
        let mut group_key_bytes = [0u8; 32];
        group_key_bytes.copy_from_slice(&aura_pkg.group_public_key);
        let group_verifying_key = frost_ed25519::VerifyingKey::deserialize(group_key_bytes)
            .map_err(|e| {
                AuraError::crypto(format!("Failed to deserialize group verifying key: {e}"))
            })?;

        // Parse individual signer verifying shares
        let mut signer_verifying_keys = std::collections::BTreeMap::new();
        for (signer_id, key_bytes) in &aura_pkg.signer_public_keys {
            if key_bytes.len() != 32 {
                return Err(AuraError::crypto(format!(
                    "Invalid signer key length for signer {}: {} (expected 32)",
                    signer_id,
                    key_bytes.len()
                )));
            }

            // Convert u16 signer ID to frost Identifier
            let frost_id = frost::Identifier::try_from(*signer_id)
                .map_err(|e| AuraError::crypto(format!("Invalid signer ID {signer_id}: {e}")))?;

            let mut key_array = [0u8; 32];
            key_array.copy_from_slice(key_bytes);
            let verifying_share = frost_ed25519::keys::VerifyingShare::deserialize(key_array)
                .map_err(|e| {
                    AuraError::crypto(format!(
                        "Failed to deserialize signer {signer_id} verifying share: {e}"
                    ))
                })?;

            signer_verifying_keys.insert(frost_id, verifying_share);
        }

        // Create FROST PublicKeyPackage
        Ok(frost_ed25519::keys::PublicKeyPackage::new(
            signer_verifying_keys,
            group_verifying_key,
        ))
    }
}

/// Deserialize a FROST public key package from bytes.
pub fn public_key_package_from_bytes(bytes: &[u8]) -> Result<PublicKeyPackage, AuraError> {
    let frost_pkg = frost_ed25519::keys::PublicKeyPackage::deserialize(bytes)
        .map_err(|e| AuraError::crypto(format!("Failed to deserialize public key package: {e}")))?;
    Ok(PublicKeyPackage::from(frost_pkg))
}

/// Deserialize a FROST key package from bytes and convert to an Aura signing share.
/// A key package must additionally pass `validate_retained_threshold_key_package`
/// before it can establish a restored local signing context.
pub fn share_from_key_package_bytes(bytes: &[u8]) -> Result<Share, AuraError> {
    let frost_pkg = frost_ed25519::keys::KeyPackage::deserialize(bytes)
        .map_err(|e| AuraError::crypto(format!("Failed to deserialize key package: {e}")))?;
    Ok(Share::from(frost_pkg))
}

/// Mismatches between a retained FROST share and authenticated signing policy.
#[derive(Debug, thiserror::Error)]
pub enum RetainedThresholdKeyError {
    #[error("invalid retained threshold policy")]
    InvalidPolicy,
    /// The domain permits this policy, but the selected FROST backend does not.
    #[error("retained threshold {threshold} is unsupported by the FROST backend")]
    BackendThresholdUnsupported { threshold: u16 },
    #[error("retained FROST package cannot be decoded: {0}")]
    Encoding(#[from] frost::Error),
    #[error("retained FROST package has the wrong signer or threshold")]
    SignerPolicyMismatch,
    #[error("FROST public package does not contain the exact participant inventory")]
    ParticipantInventoryMismatch,
    #[error("retained FROST package does not match the group or verifying share")]
    PublicPackageMismatch,
    #[error("retained FROST signing scalar does not match its verifying share")]
    SigningShareMismatch,
}

/// Validate one retained local share without requiring a signing quorum.
///
/// The caller separately authenticates the storage envelope's authority, epoch
/// and participant. Native public packages carry no threshold policy: use the
/// authenticated policy, never the lossy Aura public-package conversion.
pub fn validate_retained_threshold_key_package(
    key_bytes: &[u8],
    public_bytes: &[u8],
    signer_index: u16,
    threshold: u16,
    participants: u16,
) -> Result<(), RetainedThresholdKeyError> {
    if threshold == 0
        || threshold > participants
        || signer_index == 0
        || signer_index > participants
    {
        return Err(RetainedThresholdKeyError::InvalidPolicy);
    }
    // The domain accepts k=1, but frost-core 1.0.0 requires min_signers >= 2.
    // Do not admit a hand-encoded package or silently reinterpret it as solo.
    if threshold == 1 {
        return Err(RetainedThresholdKeyError::BackendThresholdUnsupported { threshold });
    }
    let key = frost::keys::KeyPackage::deserialize(key_bytes)?;
    let public = frost::keys::PublicKeyPackage::deserialize(public_bytes)?;
    let expected = frost::Identifier::try_from(signer_index)?;
    if key.identifier() != &expected || key.min_signers() != &threshold {
        return Err(RetainedThresholdKeyError::SignerPolicyMismatch);
    }
    if public.verifying_shares().len() != usize::from(participants)
        || !(1..=participants).all(|index| {
            frost::Identifier::try_from(index)
                .is_ok_and(|id| public.verifying_shares().contains_key(&id))
        })
    {
        return Err(RetainedThresholdKeyError::ParticipantInventoryMismatch);
    }
    if key.verifying_key() != public.verifying_key()
        || public.verifying_shares().get(&expected) != Some(key.verifying_share())
    {
        return Err(RetainedThresholdKeyError::PublicPackageMismatch);
    }
    if frost::keys::VerifyingShare::from(*key.signing_share()) != *key.verifying_share() {
        return Err(RetainedThresholdKeyError::SigningShareMismatch);
    }
    Ok(())
}

impl From<frost_ed25519::keys::KeyPackage> for Share {
    fn from(frost_key_pkg: frost_ed25519::keys::KeyPackage) -> Self {
        let identifier = frost_key_pkg.identifier();
        let signing_share = frost_key_pkg.signing_share();

        Self::from_frost(*identifier, *signing_share)
    }
}

impl TryFrom<Share> for frost_ed25519::keys::SigningShare {
    type Error = AuraError;

    fn try_from(aura_share: Share) -> Result<Self, Self::Error> {
        aura_share.to_frost()
    }
}

/// Signing session state for coordinating signatures
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SigningSession {
    /// Session identifier
    pub session_id: String,
    /// Message being signed
    pub message: Vec<u8>,
    /// Tree signing context
    pub context: TreeSigningContext,
    /// Threshold required for signing
    pub threshold: u16,
    /// Available signers
    pub available_signers: Vec<u16>,
    /// Collected nonce commitments
    pub commitments: std::collections::BTreeMap<u16, NonceCommitment>,
    /// Collected partial signatures
    pub partial_signatures: std::collections::BTreeMap<u16, PartialSignature>,
    /// Session state
    pub state: SigningSessionState,
}

/// States for a signing session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SigningSessionState {
    /// Collecting nonce commitments
    CollectingCommitments,
    /// Collecting partial signatures
    CollectingSignatures,
    /// Aggregating final signature
    Aggregating,
    /// Session completed successfully
    Completed(ThresholdSignature),
    /// Session failed
    Failed(String),
}

impl SigningSession {
    /// Create a new signing session
    pub fn new(
        session_id: String,
        message: Vec<u8>,
        context: TreeSigningContext,
        threshold: u16,
        available_signers: Vec<u16>,
    ) -> Self {
        Self {
            session_id,
            message,
            context,
            threshold,
            available_signers,
            commitments: std::collections::BTreeMap::new(),
            partial_signatures: std::collections::BTreeMap::new(),
            state: SigningSessionState::CollectingCommitments,
        }
    }

    /// Get the threshold required for this session
    pub fn threshold(&self) -> u16 {
        self.threshold
    }

    /// Add a nonce commitment
    pub fn add_commitment(&mut self, commitment: NonceCommitment) {
        self.commitments.insert(commitment.signer, commitment);
    }

    /// Add a partial signature
    pub fn add_partial_signature(&mut self, signature: PartialSignature) {
        self.partial_signatures.insert(signature.signer, signature);
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    #[test]
    fn one_of_many_is_domain_valid_but_native_frost_backend_is_unavailable() {
        use rand::SeedableRng;
        assert!(matches!(
            super::validate_retained_threshold_key_package(&[], &[], 1, 1, 2),
            Err(super::RetainedThresholdKeyError::BackendThresholdUnsupported { threshold: 1 })
        ));
        assert!(matches!(
            super::validate_retained_threshold_key_package(&[], &[], 1, 0, 2),
            Err(super::RetainedThresholdKeyError::InvalidPolicy)
        ));
        let mut rng = rand::rngs::StdRng::from_seed([187; 32]);
        let native =
            frost::keys::generate_with_dealer(2, 1, frost::keys::IdentifierList::Default, &mut rng)
                .expect_err("audited FROST backend does not support a one-signature policy");
        assert!(matches!(native, frost::Error::InvalidMinSigners));
    }

    use super::*;

    #[test]
    fn test_binding_message_deterministic() {
        let ctx = TreeSigningContext::new(1, 42, [0xAA; 32]);
        let op = b"test_operation";

        let msg1 = binding_message(&ctx, op);
        let msg2 = binding_message(&ctx, op);

        assert_eq!(msg1, msg2, "Binding message should be deterministic");
    }

    #[test]
    fn test_binding_message_different_contexts() {
        let ctx1 = TreeSigningContext::new(1, 42, [0xAA; 32]);
        let ctx2 = TreeSigningContext::new(2, 42, [0xAA; 32]); // Different node
        let op = b"test_operation";

        let msg1 = binding_message(&ctx1, op);
        let msg2 = binding_message(&ctx2, op);

        assert_ne!(
            msg1, msg2,
            "Different nodes should produce different bindings"
        );
    }

    #[test]
    fn test_binding_message_different_epochs() {
        let ctx1 = TreeSigningContext::new(1, 42, [0xAA; 32]);
        let ctx2 = TreeSigningContext::new(1, 43, [0xAA; 32]); // Different epoch
        let op = b"test_operation";

        let msg1 = binding_message(&ctx1, op);
        let msg2 = binding_message(&ctx2, op);

        assert_ne!(
            msg1, msg2,
            "Different epochs should produce different bindings"
        );
    }

    #[test]
    fn test_binding_message_different_policies() {
        let ctx1 = TreeSigningContext::new(1, 42, [0xAA; 32]);
        let ctx2 = TreeSigningContext::new(1, 42, [0xBB; 32]); // Different policy
        let op = b"test_operation";

        let msg1 = binding_message(&ctx1, op);
        let msg2 = binding_message(&ctx2, op);

        assert_ne!(
            msg1, msg2,
            "Different policies should produce different bindings"
        );
    }

    #[test]
    fn test_nonce_commitment_size() {
        use rand::SeedableRng;

        let share = frost::keys::SigningShare::deserialize([1u8; 32]).expect("valid signing share");
        let mut rng = rand::rngs::StdRng::from_seed([2u8; 32]);
        let identifier = frost::Identifier::try_from(1u16).expect("valid identifier");
        let nonces =
            FrostNonces::generate_for_share(identifier, &share, &mut rng).expect("nonces generate");
        assert_eq!(nonces.participant(), 1);
        assert_eq!(nonces.commitment().commitment.len(), MAX_COMMITMENT_BYTES);
    }

    /// Single-use FROST nonces (Task 171): signing needs a recorded
    /// retirement, a log refuses the same nonces twice, and a share made
    /// from retired nonces aggregates into a valid signature.
    #[tokio::test]
    async fn frost_nonces_are_retired_once_and_sign_once() {
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::from_seed([7u8; 32]);
        let (shares, public) =
            frost::keys::generate_with_dealer(2, 2, frost::keys::IdentifierList::Default, &mut rng)
                .expect("dealer keys");
        let keys: Vec<frost::keys::KeyPackage> = shares
            .into_values()
            .map(|secret| frost::keys::KeyPackage::try_from(secret).expect("key package"))
            .collect();
        let log = ProcessFrostNonceRetirement::default();
        let nonces: Vec<FrostNonces> = keys
            .iter()
            .map(|key| FrostNonces::generate(key, &mut rng).expect("nonces"))
            .collect();
        let commitments: BTreeMap<_, _> = nonces
            .iter()
            .map(|n| {
                (
                    n.commitment().frost_identifier().expect("id"),
                    n.commitment().to_frost().expect("commitment"),
                )
            })
            .collect();
        let package = frost::SigningPackage::new(commitments, b"message");
        let mut signature_shares = BTreeMap::new();
        for (nonces, key) in nonces.into_iter().zip(&keys) {
            let digest_record = FrostNonceRetirementRecord {
                participant: nonces.participant(),
                commitment_digest: crate::Hash32(hash::hash(&nonces.commitment().commitment)),
            };
            let retired = nonces.retire(&log).await.expect("first retirement");
            assert!(
                log.retire_frost_nonce(&digest_record).await.is_err(),
                "a retired nonce cannot be retired again"
            );
            signature_shares.insert(
                *key.identifier(),
                retired.sign(&package, key).expect("share"),
            );
        }
        let signature = frost::aggregate(&package, &signature_shares, &public).expect("aggregate");
        public
            .verifying_key()
            .verify(b"message", &signature)
            .expect("valid signature");
    }

    #[test]
    fn test_commitment_invalid_bytes_rejected() {
        let commitment = NonceCommitment {
            signer: 1,
            commitment: vec![0u8; 10],
        };
        assert!(commitment.to_frost().is_err());
    }
}
