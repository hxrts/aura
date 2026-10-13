//! Original local sender identity for invitation transfer and confirmation.
use super::*;
use crate::handlers::rendezvous_identity::{
    require_active_identity_signing_context, require_identity_keys,
    require_issued_identity_signing_context,
};
use aura_core::effects::CryptoExtendedEffects;
use aura_core::effects::{SecureStorageCapability, SecureStorageEffects, SecureStorageLocation};
use aura_core::{AuraError, TrustedKeyDomain, TrustedPublicKey};

#[derive(Debug, thiserror::Error)]
pub(crate) enum IssuedInvitationIdentityError {
    #[error("original invitation issuer identity binding is invalid")]
    Binding,
    #[error("enrollment transfer requires its retained original enrollment owner")]
    EnrollmentOwnerRequired,
    #[error("original invitation issuer identity is absent")]
    Missing,
    #[error("original invitation issuer identity exceeds its 4096-byte record bound")]
    Oversized,
}
fn invalid(cause: IssuedInvitationIdentityError) -> AgentError {
    let storage = match &cause {
        IssuedInvitationIdentityError::Missing => true,
        IssuedInvitationIdentityError::Binding
        | IssuedInvitationIdentityError::Oversized
        | IssuedInvitationIdentityError::EnrollmentOwnerRequired => false,
    };
    let message = cause.to_string();
    let source = Some(Arc::new(cause) as Arc<dyn std::error::Error + Send + Sync>);
    AgentError::Aura(if storage {
        AuraError::Storage { message, source }
    } else {
        AuraError::Invalid { message, source }
    })
}
fn invalid_source(
    stage: &'static str,
    source: impl std::error::Error + Send + Sync + 'static,
) -> AgentError {
    AgentError::Aura(AuraError::Invalid {
        message: stage.into(),
        source: Some(Arc::new(source)),
    })
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct OriginalIdentityRecord {
    version: u16,
    authority: AuthorityId,
    device: DeviceId,
    invitation: InvitationId,
    invitation_digest: [u8; 32],
    epoch: u64,
    public_key: [u8; 32],
}

/// A separate retained sender proof for an invitation issued by the native
/// device quorum. It cannot be interpreted as an Ed25519 identity key.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OriginalQuorumProofRecord {
    version: u16,
    authority: AuthorityId,
    device: DeviceId,
    invitation: InvitationId,
    invitation_digest: [u8; 32],
    recipient: AuthorityId,
    recipient_device: DeviceId,
    epoch: u64,
    group_key: [u8; 32],
    code: String,
}

impl OriginalQuorumProofRecord {
    /// Constructed only after the immutable issuer-owned record and its exact
    /// invitation binding have been checked; wire proof bytes cannot mint it.
    fn retained_verification_key(&self) -> TrustedPublicKey {
        TrustedPublicKey::active(
            TrustedKeyDomain::AuthorityThreshold,
            Some(self.epoch),
            self.group_key.to_vec(),
            aura_core::Hash32(aura_core::hash::hash(&self.group_key)),
        )
    }
}

/// Only the active native threshold-signing service can mint this capability.
/// Its fields are private so a reserved id or decoded wire proof cannot be
/// substituted for the verified original issuance.
pub(crate) struct VerifiedQuorumInvitationIssuance {
    runtime_owner: Arc<AuraEffectSystem>,
    original: OriginalQuorumProofRecord,
}

impl VerifiedQuorumInvitationIssuance {
    pub(crate) fn code(&self) -> &str {
        &self.original.code
    }

    pub(crate) async fn sign(
        effects: Arc<AuraEffectSystem>,
        signing: &crate::runtime::services::threshold_signing::ThresholdSigningService,
        reserved: &ReservedInvitationIssuance,
        invitation: &Invitation,
        recipient_device: DeviceId,
        transport: &ShareableInvitationTransportMetadata,
    ) -> AgentResult<Self> {
        signing.require_effects(effects.as_ref())?;
        if !reserved.owns_effects(effects.as_ref())
            || reserved.issuer_binding() != (effects.runtime_authority_id(), effects.device_id())
            || reserved.invitation_id() != &invitation.invitation_id
            || reserved.created_at_ms() != invitation.created_at
            || invitation.sender_id != effects.runtime_authority_id()
            || invitation.receiver_id == invitation.sender_id
            || !matches!(
                invitation.invitation_type,
                InvitationType::Contact { .. } | InvitationType::Guardian { .. }
            )
            || transport.sender_device_id != Some(effects.device_id())
        {
            return Err(invalid(IssuedInvitationIdentityError::Binding));
        }
        let mut shareable = ShareableInvitation::from(invitation);
        shareable.version = ShareableInvitation::CONTACT_GUARDIAN_QUORUM_VERSION;
        let context = shareable
            .quorum_signing_context(invitation.receiver_id, recipient_device, transport)
            .map_err(|source| invalid_source("invalid quorum invitation intent", source))?;
        let signature = signing.sign_with_device_quorum(context.clone()).await?;
        if signature.signer_count < 2
            || signature.signature.len() != ShareableInvitation::SENDER_PROOF_SIGNATURE_BYTES
        {
            return Err(AgentError::invalid(
                "invitation lacks a native threshold quorum signature",
            ));
        }
        use aura_core::effects::ThresholdSigningEffects;
        let retained_state = signing
            .threshold_state(&invitation.sender_id)
            .await
            .ok_or_else(|| AgentError::invalid("native threshold state is unavailable"))?;
        let retained_package = signing
            .public_key_package(&invitation.sender_id)
            .await
            .ok_or_else(|| AgentError::invalid("native threshold verifier is unavailable"))?;
        if retained_state.threshold < 2
            || retained_state.epoch != signature.epoch
            || retained_package != signature.public_key_package
        {
            return Err(AgentError::invalid(
                "signed invitation differs from retained native threshold policy",
            ));
        }
        let package = frost_ed25519::keys::PublicKeyPackage::deserialize(&retained_package)
            .map_err(|source| invalid_source("invalid retained native group package", source))?;
        let group_key = package.verifying_key().serialize();
        let trusted_key = TrustedPublicKey::active(
            TrustedKeyDomain::AuthorityThreshold,
            Some(retained_state.epoch),
            group_key.to_vec(),
            aura_core::Hash32(aura_core::hash::hash(&group_key)),
        );
        let transcript =
            aura_signature::threshold_signing_context_transcript_bytes(&context, signature.epoch)
                .map_err(|source| invalid_source("invalid quorum invitation transcript", source))?;
        if !effects
            .frost_verify(&transcript, &signature.signature, trusted_key.bytes())
            .await?
        {
            return Err(AgentError::invalid(
                "invitation quorum proof did not verify",
            ));
        }
        let code = shareable
            .to_quorum_signed_code(
                invitation.receiver_id,
                recipient_device,
                ShareableInvitationSenderProof {
                    scheme: ShareableInvitation::CONTACT_GUARDIAN_QUORUM_SCHEME.into(),
                    public_key: group_key.to_vec(),
                    signature: signature.signature,
                    sender_device_id: transport.sender_device_id,
                    key_epoch: Some(signature.epoch),
                },
                transport.clone(),
            )
            .map_err(|source| invalid_source("invalid signed quorum invitation", source))?;
        Ok(Self {
            runtime_owner: effects,
            original: OriginalQuorumProofRecord {
                version: 1,
                authority: invitation.sender_id,
                device: reserved.issuer_binding().1,
                invitation: invitation.invitation_id.clone(),
                invitation_digest: digest(invitation)?,
                recipient: invitation.receiver_id,
                recipient_device,
                epoch: signature.epoch,
                group_key,
                code,
            },
        })
    }

    pub(crate) async fn retain(
        self,
        reserved: &ReservedInvitationIssuance,
        invitation: &Invitation,
    ) -> AgentResult<()> {
        let effects = self.runtime_owner.as_ref();
        if !reserved.owns_effects(effects)
            || reserved.invitation_id() != &self.original.invitation
            || self.original.invitation != invitation.invitation_id
            || self.original.invitation_digest != digest(invitation)?
            || self.original.authority != effects.runtime_authority_id()
            || self.original.device != effects.device_id()
            || self.original.recipient != invitation.receiver_id
        {
            return Err(invalid(IssuedInvitationIdentityError::Binding));
        }
        let bytes = serde_json::to_vec(&self.original).map_err(|source| {
            AgentError::Aura(AuraError::Serialization {
                message: "encode original quorum invitation proof".into(),
                source: Some(Arc::new(source)),
            })
        })?;
        let address = SecureStorageLocation::with_sub_key(
            "issued_invitation_quorum_v1",
            invitation.sender_id.to_string(),
            invitation.invitation_id.to_string(),
        );
        effects
            .secure_store_immutable(
                &address,
                &bytes,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await?;
        Ok(())
    }
}

pub(crate) struct IssuedQuorumInvitationCapability<'runtime> {
    sender: SenderOwnership<'runtime>,
    original: OriginalQuorumProofRecord,
}

pub(crate) enum IssuedSenderProofCapability<'runtime> {
    Solo(IssuedInvitationIdentityCapability<'runtime>),
    Quorum(IssuedQuorumInvitationCapability<'runtime>),
}

impl IssuedSenderProofCapability<'_> {
    pub(crate) fn invitation(&self) -> &Invitation {
        match self {
            Self::Solo(issued) => issued.invitation(),
            Self::Quorum(issued) => issued.invitation(),
        }
    }
}

#[aura_macros::capability_boundary(category = "capability_gated",
    capability = "issued_sender_proof", capability_type = IssuedSenderProofCapability,
    family = "proof_issuer")]
#[aura_macros::authoritative_source(kind = "proof_issuer")]
pub(crate) async fn select_original_sender_proof<'runtime>(
    effects: &'runtime AuraEffectSystem,
    selector: &InvitationId,
) -> AgentResult<IssuedSenderProofCapability<'runtime>> {
    let invitation = super::required_channel_read::created_required(
        effects,
        effects.runtime_authority_id(),
        selector,
    )
    .await
    .map_err(AgentError::from)?;
    let quorum_address = SecureStorageLocation::with_sub_key(
        "issued_invitation_quorum_v1",
        invitation.sender_id.to_string(),
        invitation.invitation_id.to_string(),
    );
    if effects.secure_exists(&quorum_address).await? {
        let original = read_quorum(effects, &invitation).await?;
        return Ok(IssuedSenderProofCapability::Quorum(
            IssuedQuorumInvitationCapability {
                sender: SenderOwnership::Borrowed {
                    effects,
                    invitation,
                },
                original,
            },
        ));
    }
    let original = read_original(effects, &invitation).await?;
    Ok(IssuedSenderProofCapability::Solo(
        IssuedInvitationIdentityCapability {
            sender: SenderOwnership::Borrowed {
                effects,
                invitation,
            },
            original,
        },
    ))
}

impl IssuedQuorumInvitationCapability<'_> {
    pub(crate) fn invitation(&self) -> &Invitation {
        match &self.sender {
            SenderOwnership::Owned(sender) => sender.invitation(),
            SenderOwnership::Borrowed { invitation, .. } => invitation,
        }
    }

    pub(crate) fn require_runtime_owner(&self, effects: &AuraEffectSystem) -> AgentResult<()> {
        let owns_runtime = match &self.sender {
            SenderOwnership::Owned(sender) => {
                std::ptr::eq(sender.runtime_owner().as_ref(), effects)
            }
            SenderOwnership::Borrowed { effects: owner, .. } => std::ptr::eq(*owner, effects),
        };
        if !owns_runtime
            || self.original.authority != effects.runtime_authority_id()
            || self.original.device != effects.device_id()
        {
            return Err(invalid(IssuedInvitationIdentityError::Binding));
        }
        Ok(())
    }

    pub(crate) fn epoch(&self) -> u64 {
        self.original.epoch
    }

    pub(crate) fn group_key(&self) -> &[u8; 32] {
        &self.original.group_key
    }

    pub(crate) fn recipient_device(&self) -> DeviceId {
        self.original.recipient_device
    }

    pub(crate) async fn verify_response_transcript(
        &self,
        effects: &AuraEffectSystem,
        context: &aura_core::threshold::SigningContext,
        epoch: u64,
        signature: &[u8],
    ) -> AgentResult<()> {
        self.require_runtime_owner(effects)?;
        if epoch != self.original.epoch {
            return Err(invalid(IssuedInvitationIdentityError::Binding));
        }
        let trusted_key = self.original.retained_verification_key();
        if !aura_signature::verify_threshold_signing_context_transcript(
            effects,
            context,
            epoch,
            signature,
            trusted_key.bytes(),
        )
        .await
        .map_err(|source| invalid_source("verify retained quorum response", source))?
        {
            return Err(AgentError::invalid(
                "retained quorum response signature did not verify",
            ));
        }
        Ok(())
    }
}

async fn read_quorum(
    effects: &AuraEffectSystem,
    invitation: &Invitation,
) -> AgentResult<OriginalQuorumProofRecord> {
    if invitation.sender_id != effects.runtime_authority_id()
        || !matches!(
            invitation.invitation_type,
            InvitationType::Contact { .. } | InvitationType::Guardian { .. }
        )
    {
        return Err(invalid(IssuedInvitationIdentityError::Binding));
    }
    let address = SecureStorageLocation::with_sub_key(
        "issued_invitation_quorum_v1",
        invitation.sender_id.to_string(),
        invitation.invitation_id.to_string(),
    );
    if !effects.secure_exists(&address).await? {
        return Err(invalid(IssuedInvitationIdentityError::Missing));
    }
    let bytes = effects
        .secure_retrieve(&address, &[SecureStorageCapability::Read])
        .await?;
    if bytes.len() > 16 * 1024 {
        return Err(invalid(IssuedInvitationIdentityError::Oversized));
    }
    let original: OriginalQuorumProofRecord = serde_json::from_slice(&bytes).map_err(|source| {
        AgentError::Aura(AuraError::Serialization {
            message: "decode required original quorum invitation proof".into(),
            source: Some(Arc::new(source)),
        })
    })?;
    if original.version != 1
        || original.authority != invitation.sender_id
        || original.device != effects.device_id()
        || original.invitation != invitation.invitation_id
        || original.invitation_digest != digest(invitation)?
        || original.recipient != invitation.receiver_id
        || original.recipient == original.authority
        || original.group_key == [0; 32]
    {
        return Err(invalid(IssuedInvitationIdentityError::Binding));
    }
    let (shareable, proof, transport) =
        ShareableInvitation::from_code_with_proof_and_transport(&original.code)
            .map_err(|_| invalid(IssuedInvitationIdentityError::Binding))?;
    let recipient = ShareableInvitation::quorum_recipient_from_code(&original.code)
        .map_err(|_| invalid(IssuedInvitationIdentityError::Binding))?;
    let recipient_device = ShareableInvitation::recipient_device_from_code(&original.code)
        .map_err(|_| invalid(IssuedInvitationIdentityError::Binding))?;
    let proof = proof.ok_or_else(|| invalid(IssuedInvitationIdentityError::Binding))?;
    if shareable.version != ShareableInvitation::CONTACT_GUARDIAN_QUORUM_VERSION
        || shareable.invitation_id != invitation.invitation_id
        || shareable.sender_id != invitation.sender_id
        || shareable.context_id != Some(invitation.context_id)
        || shareable.invitation_type != invitation.invitation_type
        || shareable.expires_at != invitation.expires_at
        || shareable.message != invitation.message
        || recipient != Some(original.recipient)
        || recipient_device != Some(original.recipient_device)
        || transport.sender_device_id != Some(original.device)
        || proof.key_epoch != Some(original.epoch)
        || proof.public_key.as_slice() != original.group_key
    {
        return Err(invalid(IssuedInvitationIdentityError::Binding));
    }
    let context = shareable
        .quorum_signing_context(original.recipient, original.recipient_device, &transport)
        .map_err(|_| invalid(IssuedInvitationIdentityError::Binding))?;
    let trusted_key = original.retained_verification_key();
    if !aura_signature::verify_threshold_signing_context_transcript(
        effects,
        &context,
        original.epoch,
        &proof.signature,
        trusted_key.bytes(),
    )
    .await
    .map_err(|source| invalid_source("verify retained quorum invitation", source))?
    {
        return Err(invalid(IssuedInvitationIdentityError::Binding));
    }
    Ok(original)
}

/// This token retains required sender storage custody; raw public keys and
/// deserialized epoch records cannot create it or select a historical signer.
enum SenderOwnership<'runtime> {
    Owned(SenderInvitationRecordCapability),
    Borrowed {
        effects: &'runtime AuraEffectSystem,
        invitation: Invitation,
    },
}
pub(crate) struct IssuedInvitationIdentityCapability<'runtime> {
    sender: SenderOwnership<'runtime>,
    original: OriginalIdentityRecord,
}
impl IssuedInvitationIdentityCapability<'_> {
    pub(crate) fn invitation(&self) -> &Invitation {
        match &self.sender {
            SenderOwnership::Owned(sender) => sender.invitation(),
            SenderOwnership::Borrowed { invitation, .. } => invitation,
        }
    }
    pub(crate) fn effects(&self) -> &AuraEffectSystem {
        match &self.sender {
            SenderOwnership::Owned(sender) => sender.runtime_owner.as_ref(),
            SenderOwnership::Borrowed { effects, .. } => effects,
        }
    }
    pub(crate) fn epoch(&self) -> u64 {
        self.original.epoch
    }
    pub(crate) fn public_key(&self) -> [u8; 32] {
        self.original.public_key
    }
    pub(crate) fn require_runtime_owner(&self, effects: &AuraEffectSystem) -> AgentResult<()> {
        if !std::ptr::eq(self.effects(), effects)
            || self.original.device != effects.device_id()
            || self.original.authority != effects.runtime_authority_id()
        {
            return Err(invalid(IssuedInvitationIdentityError::Binding));
        }
        Ok(())
    }
}
fn digest(invitation: &Invitation) -> AgentResult<[u8; 32]> {
    serde_json::to_vec(&(
        ShareableInvitation::from(invitation),
        invitation.receiver_id,
        invitation.context_id,
        invitation.created_at,
    ))
    .map(|bytes| aura_core::hash::hash(&bytes))
    .map_err(|source| {
        AgentError::Aura(AuraError::Serialization {
            message: "encode original invitation identity binding".into(),
            source: Some(Arc::new(source)),
        })
    })
}
/// The issued binding covers the invitation as created. The only later change
/// it admits is an open contact invitation's placeholder receiver (the
/// sender itself) being replaced by the authenticated acceptor once the
/// inviter records the acceptance; every other field must still match.
fn issued_digest_matches(
    original: &OriginalIdentityRecord,
    invitation: &Invitation,
) -> AgentResult<bool> {
    if original.invitation_digest == digest(invitation)? {
        return Ok(true);
    }
    if !matches!(invitation.invitation_type, InvitationType::Contact { .. })
        || invitation.receiver_id == invitation.sender_id
    {
        return Ok(false);
    }
    let mut as_issued = invitation.clone();
    as_issued.receiver_id = invitation.sender_id;
    Ok(original.invitation_digest == digest(&as_issued)?)
}

fn require_sender(sender: &SenderInvitationRecordCapability) -> AgentResult<()> {
    let effects = sender.runtime_owner();
    if !matches!(
        sender.invitation().invitation_type,
        InvitationType::Guardian { .. }
            | InvitationType::Contact { .. }
            | InvitationType::Channel { .. }
    ) || sender.invitation().sender_id != effects.runtime_authority_id()
    {
        return Err(invalid(IssuedInvitationIdentityError::Binding));
    }
    Ok(())
}

#[aura_macros::capability_boundary(category = "capability_gated",
    capability = "reserved_invitation_issuance", capability_type = ReservedInvitationIssuance,
    family = "runtime_helper")]
pub(super) async fn birth_original_identity(
    reserved: &ReservedInvitationIssuance,
    invitation: &Invitation,
) -> AgentResult<()> {
    let effects = reserved.runtime_owner.clone();
    if !reserved.owns_effects(effects.as_ref())
        || reserved.issuer_binding() != (effects.runtime_authority_id(), effects.device_id())
        || reserved.invitation_id() != &invitation.invitation_id
        || reserved.created_at_ms() != invitation.created_at
        || invitation.sender_id != effects.runtime_authority_id()
        || !matches!(
            invitation.invitation_type,
            InvitationType::Guardian { .. }
                | InvitationType::Contact { .. }
                | InvitationType::Channel { .. }
        )
    {
        return Err(invalid(IssuedInvitationIdentityError::Binding));
    }
    let identity = require_active_identity_signing_context(effects.as_ref(), &invitation.sender_id)
        .await
        .map_err(AgentError::EnrollmentManifest)?;
    let (private, public_key) = require_identity_keys(&identity)
        .await
        .map_err(AgentError::EnrollmentManifest)?;
    let _private = zeroize::Zeroizing::new(private);
    let original = OriginalIdentityRecord {
        version: 1,
        authority: invitation.sender_id,
        device: effects.device_id(),
        invitation: invitation.invitation_id.clone(),
        invitation_digest: digest(invitation)?,
        epoch: identity.epoch(),
        public_key,
    };
    let bytes = serde_json::to_vec(&original).map_err(|source| {
        AgentError::Aura(AuraError::Serialization {
            message: "encode original invitation issuer identity".into(),
            source: Some(Arc::new(source)),
        })
    })?;
    let address = SecureStorageLocation::with_sub_key(
        "issued_invitation_identity_v1",
        invitation.sender_id.to_string(),
        invitation.invitation_id.to_string(),
    );
    effects
        .secure_store_immutable(
            &address,
            &bytes,
            &[
                SecureStorageCapability::Read,
                SecureStorageCapability::Write,
            ],
        )
        .await?;
    Ok(())
}

async fn read_original(
    effects: &AuraEffectSystem,
    invitation: &Invitation,
) -> AgentResult<OriginalIdentityRecord> {
    if !matches!(
        invitation.invitation_type,
        InvitationType::Guardian { .. }
            | InvitationType::Contact { .. }
            | InvitationType::Channel { .. }
    ) || invitation.sender_id != effects.runtime_authority_id()
    {
        return Err(invalid(IssuedInvitationIdentityError::Binding));
    }
    let address = SecureStorageLocation::with_sub_key(
        "issued_invitation_identity_v1",
        invitation.sender_id.to_string(),
        invitation.invitation_id.to_string(),
    );
    if !effects.secure_exists(&address).await? {
        return Err(invalid(IssuedInvitationIdentityError::Missing));
    }
    let bytes = effects
        .secure_retrieve(&address, &[SecureStorageCapability::Read])
        .await?;
    if bytes.len() > 4096 {
        return Err(invalid(IssuedInvitationIdentityError::Oversized));
    }
    let original: OriginalIdentityRecord = serde_json::from_slice(&bytes).map_err(|source| {
        AgentError::Aura(AuraError::Serialization {
            message: "decode required original invitation issuer identity".into(),
            source: Some(Arc::new(source)),
        })
    })?;
    if original.version != 1
        || original.authority != invitation.sender_id
        || original.device != effects.device_id()
        || original.invitation != invitation.invitation_id
        || !issued_digest_matches(&original, invitation)?
        || original.public_key == [0; 32]
    {
        return Err(invalid(IssuedInvitationIdentityError::Binding));
    }
    Ok(original)
}

#[aura_macros::capability_boundary(category = "capability_gated",
    capability = "issued_invitation_identity", capability_type = IssuedInvitationIdentityCapability<'static>,
    family = "proof_issuer")]
#[aura_macros::authoritative_source(kind = "proof_issuer")]
pub(crate) async fn load_original_identity(
    sender: SenderInvitationRecordCapability,
) -> AgentResult<IssuedInvitationIdentityCapability<'static>> {
    require_sender(&sender)?;
    let original = read_original(sender.runtime_owner().as_ref(), sender.invitation()).await?;
    Ok(IssuedInvitationIdentityCapability {
        sender: SenderOwnership::Owned(sender),
        original,
    })
}

/// Initial selector ingress for a dispatcher that borrows its actual runtime.
/// Canonical required storage, never the caller's observed Invitation, selects
/// the sender record; subsequent operations retain this exact opaque token.
#[aura_macros::capability_boundary(category = "capability_gated",
    capability = "issued_invitation_identity", capability_type = IssuedInvitationIdentityCapability,
    family = "proof_issuer")]
#[aura_macros::authoritative_source(kind = "proof_issuer")]
pub(crate) async fn select_original_identity<'runtime>(
    effects: &'runtime AuraEffectSystem,
    selector: &InvitationId,
) -> AgentResult<IssuedInvitationIdentityCapability<'runtime>> {
    let invitation = super::required_channel_read::created_required(
        effects,
        effects.runtime_authority_id(),
        selector,
    )
    .await
    .map_err(AgentError::from)?;
    let original = read_original(effects, &invitation).await?;
    Ok(IssuedInvitationIdentityCapability {
        sender: SenderOwnership::Borrowed {
            effects,
            invitation,
        },
        original,
    })
}

#[aura_macros::capability_boundary(category = "capability_gated",
    capability = "issued_invitation_identity", capability_type = IssuedInvitationIdentityCapability,
    family = "runtime_helper")]
pub(crate) async fn export_owned_invitation_code(
    issued: IssuedInvitationIdentityCapability<'_>,
    transport: &ShareableInvitationTransportMetadata,
) -> AgentResult<String> {
    let effects = issued.effects();
    if transport.sender_device_id != Some(effects.device_id()) {
        return Err(invalid(IssuedInvitationIdentityError::Binding));
    }
    let identity = require_issued_identity_signing_context(effects, &issued)
        .await
        .map_err(AgentError::EnrollmentManifest)?;
    let (private, public) = require_identity_keys(&identity)
        .await
        .map_err(AgentError::EnrollmentManifest)?;
    let private = zeroize::Zeroizing::new(private);
    let shareable = ShareableInvitation::from(issued.invitation());
    let signature = aura_signature::sign_ed25519_transcript(
        effects,
        &shareable.signing_transcript_with_transport(transport),
        private.as_ref(),
    )
    .await
    .map_err(AgentError::from)?;
    shareable
        .to_signed_code_with_transport(
            ShareableInvitationSenderProof {
                scheme: ShareableInvitation::SENDER_PROOF_SCHEME.to_string(),
                public_key: public.to_vec(),
                signature,
                sender_device_id: Some(effects.device_id()),
                key_epoch: Some(issued.epoch()),
            },
            transport.clone(),
        )
        .map_err(|source| {
            AgentError::Aura(AuraError::Serialization {
                message: "encode original invitation transfer".into(),
                source: Some(Arc::new(source)),
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn orphaned_quorum_record_cannot_select_sender_without_required_fact() {
        let (_config, effects, handler, _existing) = actual_sender(247).await;
        let reserved = handler.reserve_invitation_issuance(&effects).await.unwrap();
        let recipient = AuthorityId::new_from_entropy([247; 32]);
        let invitation = Invitation {
            invitation_id: reserved.invitation_id().clone(),
            context_id: default_context_id_for_authority(effects.runtime_authority_id()),
            sender_id: effects.runtime_authority_id(),
            receiver_id: recipient,
            invitation_type: InvitationType::Guardian {
                subject_authority: effects.runtime_authority_id(),
            },
            status: InvitationStatus::Pending,
            created_at: reserved.created_at_ms(),
            expires_at: None,
            message: None,
            receiver_nickname: None,
        };
        // Simulate a later guard/commit failure after immutable proof retention.
        // The record is deliberately test-authored; no production constructor
        // accepts this data as authorization without the required sender fact.
        let orphan = OriginalQuorumProofRecord {
            version: 1,
            authority: invitation.sender_id,
            device: effects.device_id(),
            invitation: invitation.invitation_id.clone(),
            invitation_digest: digest(&invitation).unwrap(),
            recipient,
            recipient_device: DeviceId::new_from_entropy([246; 32]),
            epoch: 7,
            group_key: [8; 32],
            code: "orphaned-test-proof".to_owned(),
        };
        let address = SecureStorageLocation::with_sub_key(
            "issued_invitation_quorum_v1",
            invitation.sender_id.to_string(),
            invitation.invitation_id.to_string(),
        );
        effects
            .secure_store_immutable(
                &address,
                &serde_json::to_vec(&orphan).unwrap(),
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await
            .unwrap();
        assert!(
            select_original_sender_proof(effects.as_ref(), &invitation.invitation_id)
                .await
                .is_err(),
            "orphaned proof cannot create sender authorization"
        );
        assert!(
            handler
                .created_invitation_required(effects.clone(), &invitation.invitation_id)
                .await
                .is_err(),
            "orphaned proof cannot materialize a canonical invitation"
        );
        let reissued = handler.reserve_invitation_issuance(&effects).await.unwrap();
        assert_ne!(
            reissued.invitation_id(),
            reserved.invitation_id(),
            "a new issuance cannot collide with the orphaned reservation"
        );
    }

    #[tokio::test]
    async fn quorum_capability_cannot_fall_through_to_channel_issuance() {
        let (_config, effects, handler, _existing) = actual_sender(245).await;
        let reserved = handler.reserve_invitation_issuance(&effects).await.unwrap();
        let recipient = AuthorityId::new_from_entropy([244; 32]);
        let kind = InvitationType::Channel {
            home_id: ChannelId::from_bytes([243; 32]),
            nickname_suggestion: None,
            bootstrap: None,
            home: false,
        };
        let invitation = Invitation {
            invitation_id: reserved.invitation_id().clone(),
            context_id: default_context_id_for_authority(effects.runtime_authority_id()),
            sender_id: effects.runtime_authority_id(),
            receiver_id: recipient,
            invitation_type: kind.clone(),
            status: InvitationStatus::Pending,
            created_at: reserved.created_at_ms(),
            expires_at: None,
            message: None,
            receiver_nickname: None,
        };
        // The test authors an opaque value only to exercise the downstream
        // fail-closed boundary; production can mint it only through sign().
        let proof = VerifiedQuorumInvitationIssuance {
            runtime_owner: effects.clone(),
            original: OriginalQuorumProofRecord {
                version: 1,
                authority: invitation.sender_id,
                device: effects.device_id(),
                invitation: invitation.invitation_id.clone(),
                invitation_digest: digest(&invitation).unwrap(),
                recipient,
                recipient_device: DeviceId::new_from_entropy([242; 32]),
                epoch: 7,
                group_key: [8; 32],
                code: "test-only-proof".to_owned(),
            },
        };
        let invitation_id = reserved.invitation_id().clone();
        assert!(
            handler
                .prepare_reserved_invitation_with_proof(
                    effects.clone(),
                    reserved,
                    recipient,
                    kind,
                    None,
                    Some(invitation.context_id),
                    None,
                    None,
                    Some(proof),
                )
                .await
                .is_err(),
            "unsupported kind must refuse before guard effects or facts"
        );
        assert!(
            handler
                .created_invitation_required(effects, &invitation_id)
                .await
                .is_err(),
            "an unsupported quorum kind cannot publish an invitation"
        );
    }

    #[tokio::test]
    async fn original_quorum_issuance_refuses_solo_policy_before_storage_or_fact() {
        let (_config, effects, handler, _existing) = actual_sender(248).await;
        let recipient = AuthorityId::new_from_entropy([248; 32]);
        let signer = ThresholdSigningService::new(effects.clone());
        for invitation_type in [
            InvitationType::Contact { nickname: None },
            InvitationType::Guardian {
                subject_authority: effects.runtime_authority_id(),
            },
        ] {
            let reserved = handler.reserve_invitation_issuance(&effects).await.unwrap();
            let invitation = Invitation {
                invitation_id: reserved.invitation_id().clone(),
                context_id: default_context_id_for_authority(effects.runtime_authority_id()),
                sender_id: effects.runtime_authority_id(),
                receiver_id: recipient,
                invitation_type,
                status: InvitationStatus::Pending,
                created_at: reserved.created_at_ms(),
                expires_at: None,
                message: None,
                receiver_nickname: None,
            };
            assert!(
                VerifiedQuorumInvitationIssuance::sign(
                    effects.clone(),
                    &signer,
                    &reserved,
                    &invitation,
                    DeviceId::new_from_entropy([249; 32]),
                    &transport(effects.as_ref()),
                )
                .await
                .is_err(),
                "solo policy cannot mint a quorum-issued capability"
            );
            let address = SecureStorageLocation::with_sub_key(
                "issued_invitation_quorum_v1",
                invitation.sender_id.to_string(),
                invitation.invitation_id.to_string(),
            );
            assert!(
                !effects.secure_exists(&address).await.unwrap(),
                "refusal cannot retain an issued proof"
            );
            assert!(
                handler
                    .created_invitation_required(effects.clone(), &invitation.invitation_id)
                    .await
                    .is_err(),
                "refusal cannot publish an invitation fact"
            );
        }
    }

    #[test]
    fn issued_binding_admits_only_the_open_contact_receiver_transition() {
        let sender = AuthorityId::new_from_entropy([0x51; 32]);
        let acceptor = AuthorityId::new_from_entropy([0x52; 32]);
        let issued = Invitation {
            invitation_id: InvitationId::new("inv-open-contact"),
            context_id: ContextId::new_from_entropy([0x53; 32]),
            sender_id: sender,
            receiver_id: sender,
            invitation_type: InvitationType::Contact { nickname: None },
            status: InvitationStatus::Pending,
            created_at: 7,
            expires_at: None,
            message: None,
            receiver_nickname: None,
        };
        let original = OriginalIdentityRecord {
            version: 1,
            authority: sender,
            device: DeviceId::new_from_entropy([0x54; 32]),
            invitation: issued.invitation_id.clone(),
            invitation_digest: digest(&issued).unwrap(),
            epoch: 0,
            public_key: [1; 32],
        };
        let mut accepted = issued.clone();
        accepted.status = InvitationStatus::Accepted;
        accepted.receiver_id = acceptor;
        assert!(issued_digest_matches(&original, &issued).unwrap());
        assert!(
            issued_digest_matches(&original, &accepted).unwrap(),
            "recording the acceptor of an open contact invitation keeps its binding"
        );

        let mut altered = accepted.clone();
        altered.created_at = 8;
        assert!(!issued_digest_matches(&original, &altered).unwrap());

        // A directed invitation's receiver is part of the issued binding.
        let mut directed = issued.clone();
        directed.receiver_id = acceptor;
        let directed_original = OriginalIdentityRecord {
            invitation_digest: digest(&directed).unwrap(),
            ..original
        };
        let mut redirected = directed.clone();
        redirected.receiver_id = AuthorityId::new_from_entropy([0x55; 32]);
        assert!(!issued_digest_matches(&directed_original, &redirected).unwrap());

        // Only contact invitations admit the transition.
        let mut channel = issued.clone();
        channel.invitation_type = InvitationType::Channel {
            home_id: ChannelId::from_bytes([0x56; 32]),
            nickname_suggestion: None,
            bootstrap: None,
            home: false,
        };
        let channel_original = OriginalIdentityRecord {
            invitation_digest: digest(&channel).unwrap(),
            ..directed_original
        };
        let mut channel_accepted = channel.clone();
        channel_accepted.receiver_id = acceptor;
        assert!(!issued_digest_matches(&channel_original, &channel_accepted).unwrap());
    }

    use crate::runtime::services::ThresholdSigningService;
    use aura_core::effects::ThresholdSigningEffects;
    use base64::Engine;

    trait AmbiguousIfClone<Marker> {
        fn assert_absent() {}
    }
    impl<T: ?Sized> AmbiguousIfClone<()> for T {}
    struct Clonable;
    impl<T: Clone> AmbiguousIfClone<Clonable> for T {}
    const _: fn() =
        <IssuedInvitationIdentityCapability<'static> as AmbiguousIfClone<_>>::assert_absent;
    trait AmbiguousIfDeserializable<Marker> {
        fn assert_absent() {}
    }
    impl<T: ?Sized> AmbiguousIfDeserializable<()> for T {}
    struct Deserializable;
    impl<T: serde::Deserialize<'static>> AmbiguousIfDeserializable<Deserializable> for T {}
    const _: fn() =
        <IssuedInvitationIdentityCapability<'static> as AmbiguousIfDeserializable<_>>::assert_absent;

    fn configure_authorization(effects: &Arc<AuraEffectSystem>) {
        let authority = effects.runtime_authority_id();
        let issuer = aura_authorization::TokenAuthority::new(authority);
        let token = issuer
            .create_token(
                authority,
                crate::token_profiles::TokenCapabilityProfile::StandardDevice,
            )
            .expect("actual test invitation authorization");
        let engine = base64::engine::general_purpose::STANDARD;
        effects.set_biscuit_cache(crate::runtime::effects::BiscuitCache {
            token_b64: engine.encode(token.to_vec().expect("actual authorization bytes")),
            issuer_authority: authority,
            root_pk_b64: engine.encode(issuer.root_public_key().to_bytes()),
        });
    }
    fn owned_effects(config: &crate::AgentConfig, authority: AuthorityId) -> Arc<AuraEffectSystem> {
        let owner = crate::runtime::builder::TestingOwnedProfileCapability::acquire(config)
            .expect("actual original selected physical profile lease");
        Arc::new(
            AuraEffectSystem::testing_with_owned_profile(config, authority, None, owner, None)
                .expect("actual selected provider and lifetime custody"),
        )
    }
    async fn actual_sender(
        _fixture_label: u64,
    ) -> (
        crate::AgentConfig,
        Arc<AuraEffectSystem>,
        InvitationHandler,
        Invitation,
    ) {
        let authority = AuthorityId::new_from_entropy([234; 32]);
        let config = crate::AgentConfig {
            device_id: DeviceId::new_from_entropy([235; 32]),
            storage: crate::core::config::StorageConfig {
                base_path: tempfile::Builder::new()
                    .prefix("aura-owned-guardian-identity-")
                    .tempdir()
                    .expect("actual isolated profile")
                    .keep(),
                ..Default::default()
            },
            ..Default::default()
        };
        let effects = owned_effects(&config, authority);
        ThresholdSigningService::new(effects.clone())
            .bootstrap_authority(&authority)
            .await
            .expect("actual physical participant bootstrap");
        configure_authorization(&effects);
        let handler = InvitationHandler::new(AuthorityContext::new_with_device(
            authority,
            config.device_id,
        ))
        .expect("actual sender handler");
        let invitation = handler
            .create_invitation(
                effects.clone(),
                AuthorityId::new_from_entropy([236; 32]),
                InvitationType::Guardian {
                    subject_authority: authority,
                },
                None,
                None,
            )
            .await
            .expect("actual guard-created persisted guardian invitation");
        (config, effects, handler, invitation)
    }
    fn transport(effects: &AuraEffectSystem) -> ShareableInvitationTransportMetadata {
        ShareableInvitationTransportMetadata {
            sender_device_id: Some(effects.device_id()),
            ..Default::default()
        }
    }
    async fn export(
        handler: &InvitationHandler,
        effects: &Arc<AuraEffectSystem>,
        invitation: &Invitation,
    ) -> String {
        let sender = handler
            .created_invitation_required(effects.clone(), &invitation.invitation_id)
            .await
            .expect("actual original sender record");
        let issued = load_original_identity(sender)
            .await
            .expect("actual retained original issuer");
        export_owned_invitation_code(issued, &transport(effects))
            .await
            .expect("actual original issuer export")
    }

    #[tokio::test]
    async fn required_guardian_identity_copied_record_cannot_reconstruct_original_issuer() {
        use aura_core::effects::StorageCoreEffects;
        let (mut config, effects, handler, invitation) = actual_sender(606).await;
        config.storage.base_path = tempfile::Builder::new()
            .prefix("aura-guardian-no-original-")
            .tempdir()
            .expect("actual separate physical provider")
            .keep();
        let foreign = owned_effects(&config, invitation.sender_id);
        let key = InvitationCacheHandler::created_invitation_key(
            invitation.sender_id,
            &invitation.invitation_id,
        );
        let bytes = effects
            .retrieve(&key)
            .await
            .expect("actual original regular record")
            .expect("actual canonical sender metadata exists");
        foreign
            .store(&key, bytes)
            .await
            .expect("copy observed regular metadata into foreign profile");
        let sender = handler
            .created_invitation_required(foreign.clone(), &invitation.invitation_id)
            .await
            .expect("required read of copied metadata still has no protected issuer evidence");
        let failure = match load_original_identity(sender).await {
            Ok(_) => panic!("observed record cannot reconstruct original issuer"),
            Err(error) => error,
        };
        let mut source: Option<&(dyn std::error::Error + 'static)> = Some(&failure);
        let mut missing = false;
        while let Some(cause) = source {
            missing |= matches!(
                cause.downcast_ref::<IssuedInvitationIdentityError>(),
                Some(IssuedInvitationIdentityError::Missing)
            );
            source = cause.source();
        }
        assert!(
            missing,
            "missing original issuer remains a typed required failure"
        );
        let address = SecureStorageLocation::with_sub_key(
            "issued_invitation_identity_v1",
            invitation.sender_id.to_string(),
            invitation.invitation_id.to_string(),
        );
        assert!(
            !foreign
                .secure_exists(&address)
                .await
                .expect("required physical absence"),
            "failed recovery never initializes an original identity"
        );
    }

    #[tokio::test]
    async fn required_guardian_identity_preserves_original_issuer_after_rotation_and_restart() {
        let (config, effects, handler, invitation) = actual_sender(601).await;
        let initial = export(&handler, &effects, &invitation).await;
        let original_proof = ShareableInvitation::from_code_with_proof(&initial)
            .expect("actual signed original code")
            .1
            .expect("actual original proof");
        let participants = [aura_core::threshold::ParticipantIdentity::device(
            effects.device_id(),
        )];
        let (epoch, _, _) = effects
            .rotate_keys(&invitation.sender_id, 1, 1, &participants)
            .await
            .expect("actual participant key rotation producer");
        ThresholdSigningService::new(effects.clone())
            .commit_key_rotation(&invitation.sender_id, epoch)
            .await
            .expect("actual rotated signer activation");
        {
            let active =
                require_active_identity_signing_context(effects.as_ref(), &invitation.sender_id)
                    .await
                    .expect("actual rotated physical policy");
            assert_ne!(
                require_identity_keys(&active)
                    .await
                    .expect("actual new key")
                    .1
                    .as_slice(),
                original_proof.public_key.as_slice()
            );
        }
        let rotated = export(&handler, &effects, &invitation).await;
        assert_eq!(
            ShareableInvitation::from_code_with_proof(&rotated)
                .expect("actual reexport")
                .1
                .expect("actual reexport proof")
                .public_key,
            original_proof.public_key
        );
        drop(effects);
        let restarted = owned_effects(&config, invitation.sender_id);
        let recovered = export(&handler, &restarted, &invitation).await;
        let (shareable, proof, hints) =
            ShareableInvitation::from_code_with_proof_and_transport(&recovered)
                .expect("actual restarted original code");
        let proof = proof.expect("actual recovered issuer proof");
        assert_eq!(proof.key_epoch, original_proof.key_epoch);
        assert_eq!(proof.public_key, original_proof.public_key);
        assert!(aura_signature::verify_ed25519_transcript(
            restarted.as_ref(),
            &shareable.signing_transcript_with_transport(&hints),
            &proof.signature,
            &proof.public_key
        )
        .await
        .expect("actual recovered issuer verification"));
    }

    #[tokio::test]
    async fn required_contact_identity_preserves_original_issuer_after_rotation_and_restart() {
        let (config, effects, handler, _guardian) = actual_sender(608).await;
        let invitation = handler
            .create_invitation(
                effects.clone(),
                AuthorityId::new_from_entropy([238; 32]),
                InvitationType::Contact { nickname: None },
                None,
                None,
            )
            .await
            .expect("actual guard-created original contact invitation");
        let initial = export(&handler, &effects, &invitation).await;
        let original_proof = ShareableInvitation::from_code_with_proof(&initial)
            .expect("actual signed original code")
            .1
            .expect("actual original proof");
        let participants = [aura_core::threshold::ParticipantIdentity::device(
            effects.device_id(),
        )];
        let (epoch, _, _) = effects
            .rotate_keys(&invitation.sender_id, 1, 1, &participants)
            .await
            .expect("actual participant key rotation producer");
        ThresholdSigningService::new(effects.clone())
            .commit_key_rotation(&invitation.sender_id, epoch)
            .await
            .expect("actual rotated signer activation");
        {
            let active =
                require_active_identity_signing_context(effects.as_ref(), &invitation.sender_id)
                    .await
                    .expect("actual rotated physical policy");
            assert_ne!(
                require_identity_keys(&active)
                    .await
                    .expect("actual new key")
                    .1
                    .as_slice(),
                original_proof.public_key.as_slice()
            );
        }
        let rotated = export(&handler, &effects, &invitation).await;
        assert_eq!(
            ShareableInvitation::from_code_with_proof(&rotated)
                .expect("actual reexport")
                .1
                .expect("actual reexport proof")
                .public_key,
            original_proof.public_key
        );
        drop(effects);
        let restarted = owned_effects(&config, invitation.sender_id);
        let recovered = export(&handler, &restarted, &invitation).await;
        let (shareable, proof, hints) =
            ShareableInvitation::from_code_with_proof_and_transport(&recovered)
                .expect("actual restarted original code");
        let proof = proof.expect("actual recovered issuer proof");
        assert_eq!(proof.key_epoch, original_proof.key_epoch);
        assert_eq!(proof.public_key, original_proof.public_key);
        assert!(aura_signature::verify_ed25519_transcript(
            restarted.as_ref(),
            &shareable.signing_transcript_with_transport(&hints),
            &proof.signature,
            &proof.public_key
        )
        .await
        .expect("actual recovered issuer verification"));
    }

    #[tokio::test]
    async fn required_guardian_identity_rejects_foreign_runtime_owner() {
        let (_config, effects, handler, invitation) = actual_sender(604).await;
        export(&handler, &effects, &invitation).await;
        let sender = handler
            .created_invitation_required(effects, &invitation.invitation_id)
            .await
            .expect("actual original record");
        let issued = load_original_identity(sender)
            .await
            .expect("actual original issued capability");
        let (_foreign_config, foreign, _foreign_handler, _foreign_invitation) =
            actual_sender(605).await;
        assert!(
            issued.require_runtime_owner(foreign.as_ref()).is_err(),
            "equal authority and physical device ids cannot retarget original physical owner"
        );
    }
}
