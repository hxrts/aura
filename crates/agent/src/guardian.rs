// Guardian invitation and management system
//
// Implements secure guardian invitation flow with:
// - Single-use invitation tokens with expiry
// - QR code generation for out-of-band delivery
// - HPKE-encrypted recovery share distribution
// - Guardian acceptance and approval workflow
// - Cooldown-based removal with veto capability

use crate::{AgentError, Result};
use aura_coordination::KeyShare;
use aura_journal::serialization::{from_cbor_bytes, to_cbor_bytes};
use aura_journal::{AccountId, GuardianId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Guardian invitation token
///
/// Single-use token delivered out-of-band (QR code, Signal, etc.)
/// to invite someone to become a guardian.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvitationToken {
    /// Unique token ID
    pub token_id: Uuid,
    /// Account doing the inviting
    pub inviter_account_id: AccountId,
    /// Guardian role being offered
    pub role: GuardianRole,
    /// Token creation timestamp (unix seconds)
    pub created_at: u64,
    /// Token expiry (unix seconds)
    pub expires_at: u64,
    /// Deep link URI for mobile apps
    pub deep_link: String,
    /// Random verification code (6 digits, for manual entry)
    pub verification_code: String,
}

impl InvitationToken {
    /// Create a new invitation token
    ///
    /// # Arguments
    ///
    /// * `inviter_account_id` - Account ID of the inviter
    /// * `role` - Guardian role to grant
    /// * `ttl_seconds` - Time to live in seconds (default: 24 hours)
    pub fn new(
        inviter_account_id: AccountId,
        role: GuardianRole,
        ttl_seconds: Option<u64>,
        effects: &aura_crypto::Effects,
    ) -> Result<Self> {
        let token_id = effects.gen_uuid();
        let now = effects.now().unwrap_or(0);
        let ttl = ttl_seconds.unwrap_or(24 * 3600); // 24 hours default
        let expires_at = now + ttl;

        // Generate 6-digit verification code using effects
        let random_num = effects.random.gen_u64() as u32 % 1_000_000;
        let verification_code = format!("{:06}", random_num);

        // Create deep link (aura:// custom scheme)
        let deep_link = format!(
            "aura://guardian/invite?token={}&account={}&code={}",
            token_id, inviter_account_id.0, verification_code
        );

        Ok(InvitationToken {
            token_id,
            inviter_account_id,
            role,
            created_at: now,
            expires_at,
            deep_link,
            verification_code,
        })
    }

    /// Check if token is still valid
    pub fn is_valid(&self, effects: &aura_crypto::Effects) -> Result<bool> {
        Ok(effects.now().unwrap_or(0) < self.expires_at)
    }

    /// Generate QR code as SVG
    ///
    /// Requires the `qrcode` feature to be enabled.
    #[cfg(feature = "qrcode")]
    pub fn generate_qr_svg(&self) -> Result<String> {
        use qrcode::{render::svg, QrCode};

        QrCode::new(&self.deep_link)
            .map_err(|e| AgentError::device_not_found(format!("QR generation failed: {}", e)))?
            .render::<svg::Color>()
            .min_dimensions(256, 256)
            .dark_color(svg::Color("#000000"))
            .light_color(svg::Color("#FFFFFF"))
            .build()
            .map_err(|_| AgentError::device_not_found("QR rendering failed"))
            .map(|svg| svg.to_string())
    }

    /// Generate QR code as PNG bytes
    ///
    /// Requires the `qrcode` feature to be enabled.
    #[cfg(feature = "qrcode")]
    pub fn generate_qr_png(&self) -> Result<Vec<u8>> {
        use qrcode::render::png;
        use qrcode::QrCode;

        let code = QrCode::new(&self.deep_link)
            .map_err(|e| AgentError::device_not_found(format!("QR generation failed: {}", e)))?;

        let image = code
            .render::<png::Renderer>()
            .min_dimensions(256, 256)
            .build();

        Ok(image)
    }
}

/// Guardian role definition
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum GuardianRole {
    /// Standard recovery guardian (can approve recovery)
    Recovery,
    /// Emergency guardian (can approve recovery + emergency actions)
    Emergency,
    /// Delegate (can perform limited operations on behalf of account)
    Delegate,
}

/// Guardian invitation request
///
/// Created when a user initiates adding a guardian.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianInvitationRequest {
    /// The invitation token
    pub token: InvitationToken,
    /// Optional personal message from inviter
    pub message: Option<String>,
    /// Invitation status
    pub status: InvitationStatus,
}

/// Invitation status tracking
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum InvitationStatus {
    /// Waiting for guardian to accept
    Pending,
    /// Guardian accepted, waiting for inviter approval
    Accepted {
        /// Account ID of the accepting guardian
        guardian_account_id: AccountId,
    },
    /// Inviter approved, shares being distributed
    Approved,
    /// Invitation completed successfully
    Completed,
    /// Invitation expired
    Expired,
    /// Invitation was rejected or cancelled
    Cancelled,
}

/// Encrypted recovery share package
///
/// Delivered to guardian using HPKE encryption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoverySharePackage {
    /// Guardian ID this package is for
    pub guardian_id: GuardianId,
    /// Account this share recovers
    pub account_id: AccountId,
    /// HPKE-encrypted recovery share
    pub encrypted_share: Vec<u8>,
    /// HPKE ephemeral public key
    pub ephemeral_public_key: Vec<u8>,
    /// HPKE ciphertext tag
    pub tag: Vec<u8>,
    /// Share version (for rotation)
    pub version: u64,
    /// Creation timestamp
    pub created_at: u64,
}

impl RecoverySharePackage {
    /// Encrypt a recovery share for a guardian using HPKE
    ///
    /// # Arguments
    ///
    /// * `share` - The key share to encrypt
    /// * `guardian_public_key` - Guardian's public key (X25519)
    /// * `guardian_id` - Guardian ID
    /// * `account_id` - Account ID this share recovers
    ///
    /// # Security
    ///
    /// Uses HPKE (Hybrid Public Key Encryption) for asymmetric encryption.
    /// The guardian can decrypt with their private key.
    pub fn seal_for_guardian(
        share: &KeyShare,
        guardian_public_key: &[u8],
        guardian_id: GuardianId,
        account_id: AccountId,
        effects: &aura_crypto::Effects,
    ) -> Result<Self> {
        use hpke::{
            aead::AesGcm256, kdf::HkdfSha256, kem::X25519HkdfSha256, Deserializable, Kem, OpModeS,
            Serializable,
        };

        // Serialize the share
        let plaintext = to_cbor_bytes(&share).map_err(|e| {
            AgentError::device_not_found(format!("Share serialization failed: {}", e))
        })?;

        // Additional associated data for authenticated encryption
        let info = format!("aura-guardian-share-v1:{}:{}", guardian_id.0, account_id.0);
        let aad = info.as_bytes();

        // Parse guardian's public key
        let recipient_pk = <X25519HkdfSha256 as Kem>::PublicKey::from_bytes(guardian_public_key)
            .map_err(|e| {
                AgentError::device_not_found(format!("Invalid guardian public key: {:?}", e))
            })?;

        // Setup HPKE in base mode (sender)
        let (encapsulated_key, mut sender_ctx) =
            hpke::setup_sender::<AesGcm256, HkdfSha256, X25519HkdfSha256, _>(
                &OpModeS::Base,
                &recipient_pk,
                info.as_bytes(),
                &mut effects.rng(),
            )
            .map_err(|e| AgentError::device_not_found(format!("HPKE setup failed: {:?}", e)))?;

        // Encrypt the share
        let ciphertext = sender_ctx
            .seal(&plaintext, aad)
            .map_err(|e| AgentError::device_not_found(format!("HPKE seal failed: {:?}", e)))?;

        Ok(RecoverySharePackage {
            guardian_id,
            account_id,
            encrypted_share: ciphertext,
            ephemeral_public_key: encapsulated_key.to_bytes().to_vec(),
            tag: vec![], // HPKE integrates auth tag into ciphertext
            version: 1,
            created_at: effects.now().unwrap_or(0),
        })
    }

    /// Decrypt a recovery share using guardian's private key
    ///
    /// # Arguments
    ///
    /// * `guardian_private_key` - Guardian's X25519 private key
    ///
    /// # Returns
    ///
    /// The decrypted KeyShare, or error if decryption fails
    pub fn unseal_with_guardian_key(&self, guardian_private_key: &[u8]) -> Result<KeyShare> {
        use hpke::{
            aead::AesGcm256, kdf::HkdfSha256, kem::X25519HkdfSha256, Deserializable, Kem, OpModeR,
        };

        // Additional associated data (must match what was used for sealing)
        let info = format!(
            "aura-guardian-share-v1:{}:{}",
            self.guardian_id.0, self.account_id.0
        );
        let aad = info.as_bytes();

        // Parse keys
        let recipient_sk = <X25519HkdfSha256 as Kem>::PrivateKey::from_bytes(guardian_private_key)
            .map_err(|e| {
                AgentError::device_not_found(format!("Invalid guardian private key: {:?}", e))
            })?;

        let encapsulated_key =
            <X25519HkdfSha256 as Kem>::EncappedKey::from_bytes(&self.ephemeral_public_key)
                .map_err(|e| {
                    AgentError::device_not_found(format!("Invalid encapsulated key: {:?}", e))
                })?;

        // Setup HPKE in base mode (receiver)
        let mut receiver_ctx = hpke::setup_receiver::<AesGcm256, HkdfSha256, X25519HkdfSha256>(
            &OpModeR::Base,
            &recipient_sk,
            &encapsulated_key,
            info.as_bytes(),
        )
        .map_err(|e| AgentError::device_not_found(format!("HPKE setup failed: {:?}", e)))?;

        // Decrypt the share
        let plaintext = receiver_ctx
            .open(&self.encrypted_share, aad)
            .map_err(|e| AgentError::device_not_found(format!("HPKE open failed: {:?}", e)))?;

        // Deserialize
        from_cbor_bytes(&plaintext)
            .map_err(|e| AgentError::device_not_found(format!("Share deserialization failed: {}", e)))
    }
}

/// Guardian manager
///
/// Handles invitation creation, acceptance, and removal.
pub struct GuardianManager {
    /// Pending invitations (token_id -> invitation)
    pending_invitations: HashMap<Uuid, GuardianInvitationRequest>,
    /// Used tokens (for replay prevention)
    used_tokens: HashMap<Uuid, u64>, // token_id -> used_at timestamp
}

impl GuardianManager {
    /// Create a new guardian manager
    pub fn new() -> Self {
        GuardianManager {
            pending_invitations: HashMap::new(),
            used_tokens: HashMap::new(),
        }
    }

    /// Create a guardian invitation
    ///
    /// # Arguments
    ///
    /// * `inviter_account_id` - Account creating the invitation
    /// * `role` - Guardian role to grant
    /// * `message` - Optional personal message
    /// * `ttl_seconds` - Token time-to-live (default: 24 hours)
    ///
    /// # Returns
    ///
    /// The invitation request with token and deep link
    pub fn create_invitation(
        &mut self,
        inviter_account_id: AccountId,
        role: GuardianRole,
        message: Option<String>,
        ttl_seconds: Option<u64>,
        effects: &aura_crypto::Effects,
    ) -> Result<GuardianInvitationRequest> {
        let token = InvitationToken::new(inviter_account_id, role, ttl_seconds, effects)?;

        let invitation = GuardianInvitationRequest {
            token: token.clone(),
            message,
            status: InvitationStatus::Pending,
        };

        self.pending_invitations
            .insert(token.token_id, invitation.clone());

        Ok(invitation)
    }

    /// Accept a guardian invitation
    ///
    /// Called by the guardian when they scan the QR code or click the deep link.
    ///
    /// # Arguments
    ///
    /// * `token_id` - The invitation token ID
    /// * `verification_code` - 6-digit verification code (for manual entry)
    /// * `guardian_account_id` - Guardian's account ID
    ///
    /// # Returns
    ///
    /// Updated invitation with Accepted status, or error if invalid
    pub fn accept_invitation(
        &mut self,
        token_id: Uuid,
        verification_code: &str,
        guardian_account_id: AccountId,
        effects: &aura_crypto::Effects,
    ) -> Result<GuardianInvitationRequest> {
        // Check if token was already used
        if self.used_tokens.contains_key(&token_id) {
            return Err(AgentError::device_not_found("Token already used"));
        }

        // Get pending invitation
        let invitation = self
            .pending_invitations
            .get_mut(&token_id)
            .ok_or_else(|| AgentError::device_not_found("Invitation not found"))?;

        // Verify token is still valid
        if !invitation.token.is_valid(effects)? {
            invitation.status = InvitationStatus::Expired;
            return Err(AgentError::device_not_found("Token expired"));
        }

        // Verify code matches
        if invitation.token.verification_code != verification_code {
            return Err(AgentError::device_not_found(
                "Invalid verification code",
            ));
        }

        // Update status to accepted
        invitation.status = InvitationStatus::Accepted {
            guardian_account_id,
        };

        Ok(invitation.clone())
    }

    /// Approve a guardian invitation
    ///
    /// Called by the inviter after the guardian accepts.
    ///
    /// # Arguments
    ///
    /// * `token_id` - The invitation token ID
    ///
    /// # Returns
    ///
    /// Updated invitation with Approved status
    pub fn approve_invitation(&mut self, token_id: Uuid) -> Result<GuardianInvitationRequest> {
        let invitation = self
            .pending_invitations
            .get_mut(&token_id)
            .ok_or_else(|| AgentError::device_not_found("Invitation not found"))?;

        // Can only approve if guardian accepted
        if !matches!(invitation.status, InvitationStatus::Accepted { .. }) {
            return Err(AgentError::device_not_found(
                "Guardian has not accepted yet",
            ));
        }

        // Update status
        invitation.status = InvitationStatus::Approved;

        Ok(invitation.clone())
    }

    /// Complete an invitation
    ///
    /// Called after shares are distributed successfully.
    pub fn complete_invitation(
        &mut self,
        token_id: Uuid,
        effects: &aura_crypto::Effects,
    ) -> Result<()> {
        let invitation = self
            .pending_invitations
            .get_mut(&token_id)
            .ok_or_else(|| AgentError::device_not_found("Invitation not found"))?;

        invitation.status = InvitationStatus::Completed;

        // Mark token as used
        self.used_tokens
            .insert(token_id, effects.now().unwrap_or(0));

        // Clean up old tokens (keep last 1000)
        if self.used_tokens.len() > 1000 {
            let mut tokens: Vec<_> = self.used_tokens.iter().map(|(id, ts)| (*id, *ts)).collect();
            tokens.sort_by_key(|(_, timestamp)| *timestamp);
            if let Some((oldest_token, _)) = tokens.first() {
                self.used_tokens.remove(oldest_token);
            }
        }

        Ok(())
    }

    /// Cancel an invitation
    pub fn cancel_invitation(&mut self, token_id: Uuid) -> Result<()> {
        if let Some(invitation) = self.pending_invitations.get_mut(&token_id) {
            invitation.status = InvitationStatus::Cancelled;
        }
        Ok(())
    }

    /// Clean up expired invitations
    ///
    /// Should be called periodically to remove stale invitations.
    pub fn cleanup_expired(&mut self, effects: &aura_crypto::Effects) {
        self.pending_invitations
            .retain(|_, invitation| invitation.token.is_valid(effects).unwrap_or(false));
    }
}

impl Default for GuardianManager {
    fn default() -> Self {
        Self::new()
    }
}

// Removed deprecated current_timestamp() function - use effects.now() instead

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invitation_token_creation() {
        let effects = aura_crypto::Effects::test();
        let account_id = AccountId::new_with_effects(&effects);
        let effects = aura_crypto::Effects::test();
        let token =
            InvitationToken::new(account_id, GuardianRole::Recovery, None, &effects).unwrap();

        assert!(token.is_valid(&effects).unwrap());
        assert_eq!(token.verification_code.len(), 6);
        assert!(token.deep_link.starts_with("aura://guardian/invite"));
    }

    #[test]
    fn test_guardian_invitation_flow() {
        let effects = aura_crypto::Effects::test();
        let mut manager = GuardianManager::new();
        let inviter_id = AccountId::new_with_effects(&effects);
        let guardian_id = AccountId::new_with_effects(&effects);

        // Create invitation
        let invitation = manager
            .create_invitation(inviter_id, GuardianRole::Recovery, None, None, &effects)
            .unwrap();

        assert_eq!(invitation.status, InvitationStatus::Pending);
        let token_id = invitation.token.token_id;
        let code = invitation.token.verification_code.clone();

        // Guardian accepts
        let accepted = manager
            .accept_invitation(token_id, &code, guardian_id, &effects)
            .unwrap();

        assert!(matches!(accepted.status, InvitationStatus::Accepted { .. }));

        // Inviter approves
        let approved = manager.approve_invitation(token_id).unwrap();
        assert_eq!(approved.status, InvitationStatus::Approved);

        // Complete
        manager.complete_invitation(token_id, &effects).unwrap();

        // Cannot reuse token
        let result = manager.accept_invitation(token_id, &code, guardian_id, &effects);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_verification_code() {
        let effects = aura_crypto::Effects::test();
        let mut manager = GuardianManager::new();
        let inviter_id = AccountId::new_with_effects(&effects);
        let guardian_id = AccountId::new_with_effects(&effects);

        let invitation = manager
            .create_invitation(inviter_id, GuardianRole::Recovery, None, None, &effects)
            .unwrap();

        let token_id = invitation.token.token_id;

        // Wrong code should fail
        let result = manager.accept_invitation(token_id, "000000", guardian_id, &effects);
        assert!(result.is_err());
    }
}
