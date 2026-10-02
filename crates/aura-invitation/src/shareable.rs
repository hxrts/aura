//! Shareable invitation wire format and verified import provenance.

use crate::{Invitation, InvitationStatus, InvitationType};
use aura_core::effects::crypto::CryptoEffects;
use aura_core::hash::hash;
use aura_core::types::identifiers::{AuthorityId, ContextId, DeviceId, InvitationId};
use aura_signature::{verify_ed25519_transcript, SecurityTranscript};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShareableInvitationError {
    InvalidFormat,
    UnsupportedVersion(u8),
    SizeLimitExceeded(&'static str),
    DecodingFailed,
    ParsingFailed,
    SerializationFailed,
    MissingSenderProof,
    InvalidSenderProof,
    MissingChannelContext,
    Expired,
    VerificationFailed,
}

impl std::fmt::Display for ShareableInvitationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidFormat => write!(f, "invalid invite code format"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported version: {v}"),
            Self::SizeLimitExceeded(field) => {
                write!(f, "invite code field exceeds size limit: {field}")
            }
            Self::DecodingFailed => write!(f, "base64 decoding failed"),
            Self::ParsingFailed => write!(f, "JSON parsing failed"),
            Self::SerializationFailed => write!(f, "JSON serialization failed"),
            Self::MissingSenderProof => write!(f, "invite code is missing sender proof"),
            Self::InvalidSenderProof => write!(f, "invite code sender proof is invalid"),
            Self::MissingChannelContext => {
                write!(f, "channel invitation missing authoritative context")
            }
            Self::Expired => write!(f, "invite code expired"),
            Self::VerificationFailed => write!(f, "invite code sender proof verification failed"),
        }
    }
}

impl std::error::Error for ShareableInvitationError {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ShareableInvitation {
    pub version: u8,
    pub invitation_id: InvitationId,
    pub sender_id: AuthorityId,
    #[serde(default)]
    pub context_id: Option<ContextId>,
    pub invitation_type: InvitationType,
    pub expires_at: Option<u64>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ShareableInvitationSenderProof {
    pub scheme: String,
    /// Untrusted key material: self-certified for unknown senders. For known senders,
    /// import validation must resolve this key through trusted device state.
    pub public_key: Vec<u8>,
    pub signature: Vec<u8>,
    #[serde(default)]
    pub sender_device_id: Option<DeviceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_epoch: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ShareableInvitationTransportMetadata {
    #[serde(default)]
    pub sender_hint: Option<String>,
    #[serde(default)]
    pub sender_device_id: Option<DeviceId>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ShareableInvitationCodeEnvelope {
    payload: ShareableInvitation,
    #[serde(default)]
    transport: ShareableInvitationTransportMetadata,
    proof: Option<ShareableInvitationSenderProof>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
enum ShareableInvitationCodePayload {
    Envelope(ShareableInvitationCodeEnvelope),
    #[cfg(any(test, feature = "test-support"))]
    Legacy(ShareableInvitation),
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ShareableInvitationTranscriptPayload {
    version: u8,
    invitation_id: InvitationId,
    sender_id: AuthorityId,
    context_id: Option<ContextId>,
    invitation_type: InvitationType,
    expires_at: Option<u64>,
    message: Option<String>,
    transport: ShareableInvitationTransportMetadata,
}

pub struct ShareableInvitationTranscript<'a> {
    invitation: &'a ShareableInvitation,
    transport: ShareableInvitationTransportMetadata,
}

impl<'a> ShareableInvitationTranscript<'a> {
    pub fn new(invitation: &'a ShareableInvitation) -> Self {
        Self {
            invitation,
            transport: ShareableInvitationTransportMetadata::default(),
        }
    }

    pub fn with_transport(
        invitation: &'a ShareableInvitation,
        transport: &ShareableInvitationTransportMetadata,
    ) -> Self {
        Self {
            invitation,
            transport: transport.clone(),
        }
    }
}

impl SecurityTranscript for ShareableInvitationTranscript<'_> {
    type Payload = ShareableInvitationTranscriptPayload;

    const DOMAIN_SEPARATOR: &'static str = "aura.invitation.shareable-code";

    fn transcript_payload(&self) -> Self::Payload {
        ShareableInvitationTranscriptPayload {
            version: self.invitation.version,
            invitation_id: self.invitation.invitation_id.clone(),
            sender_id: self.invitation.sender_id,
            context_id: self.invitation.context_id,
            invitation_type: self.invitation.invitation_type.clone(),
            expires_at: self.invitation.expires_at,
            message: self.invitation.message.clone(),
            transport: self.transport.clone(),
        }
    }
}

impl ShareableInvitation {
    pub const CURRENT_VERSION: u8 = 1;
    pub const PREFIX: &'static str = "aura";
    pub const MAX_JSON_BYTES: usize = aura_core::envelope::MAX_PAYLOAD_BYTES;
    pub const MAX_PAYLOAD_BASE64_CHARS: usize =
        aura_core::envelope::max_base64_encoded_len(Self::MAX_JSON_BYTES);
    pub const MAX_SENDER_HINT_BYTES: usize = 512;
    pub const MAX_SENDER_HINT_BASE64_CHARS: usize =
        aura_core::envelope::max_base64_encoded_len(Self::MAX_SENDER_HINT_BYTES);
    pub const MAX_SENDER_DEVICE_ID_BYTES: usize = 64;
    pub const MAX_SENDER_DEVICE_ID_BASE64_CHARS: usize =
        aura_core::envelope::max_base64_encoded_len(Self::MAX_SENDER_DEVICE_ID_BYTES);
    pub const MAX_CODE_CHARS: usize = "aura:v255:".len()
        + Self::MAX_PAYLOAD_BASE64_CHARS
        + 1
        + Self::MAX_SENDER_HINT_BASE64_CHARS
        + 1
        + Self::MAX_SENDER_DEVICE_ID_BASE64_CHARS;
    pub const MAX_INVITATION_ID_BYTES: usize = 128;
    pub const MAX_MESSAGE_BYTES: usize = 2048;
    pub const MAX_NICKNAME_BYTES: usize = 128;
    pub const MAX_CEREMONY_ID_BYTES: usize = 128;
    pub const SENDER_PROOF_SCHEME: &'static str = "ed25519-transcript-v1";
    pub const SENDER_PROOF_PUBLIC_KEY_BYTES: usize = 32;
    pub const SENDER_PROOF_SIGNATURE_BYTES: usize = 64;

    #[cfg(any(test, feature = "test-support"))]
    pub fn to_code(&self) -> Result<String, ShareableInvitationError> {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

        self.validate_size_limits()?;
        let json =
            serde_json::to_vec(self).map_err(|_| ShareableInvitationError::SerializationFailed)?;
        ensure_len("json", json.len(), Self::MAX_JSON_BYTES)?;
        let b64 = URL_SAFE_NO_PAD.encode(&json);
        Ok(format!("{}:v{}:{}", Self::PREFIX, self.version, b64))
    }

    pub fn to_signed_code(
        &self,
        proof: ShareableInvitationSenderProof,
    ) -> Result<String, ShareableInvitationError> {
        self.to_signed_code_with_transport(proof, ShareableInvitationTransportMetadata::default())
    }

    pub fn to_signed_code_with_transport(
        &self,
        proof: ShareableInvitationSenderProof,
        transport: ShareableInvitationTransportMetadata,
    ) -> Result<String, ShareableInvitationError> {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

        self.validate_size_limits()?;
        validate_sender_proof_shape(&proof)?;
        validate_transport_metadata(&transport)?;
        let envelope = ShareableInvitationCodeEnvelope {
            payload: self.clone(),
            transport,
            proof: Some(proof),
        };
        let json = serde_json::to_vec(&envelope)
            .map_err(|_| ShareableInvitationError::SerializationFailed)?;
        ensure_len("json", json.len(), Self::MAX_JSON_BYTES)?;
        let b64 = URL_SAFE_NO_PAD.encode(&json);
        Ok(format!("{}:v{}:{}", Self::PREFIX, self.version, b64))
    }

    pub fn from_code(code: &str) -> Result<Self, ShareableInvitationError> {
        Ok(Self::from_code_with_proof(code)?.0)
    }

    pub fn proof_from_code(
        code: &str,
    ) -> Result<Option<ShareableInvitationSenderProof>, ShareableInvitationError> {
        Ok(Self::from_code_with_proof(code)?.1)
    }

    pub fn transport_from_code(
        code: &str,
    ) -> Result<ShareableInvitationTransportMetadata, ShareableInvitationError> {
        Ok(Self::from_code_with_proof_and_transport(code)?.2)
    }

    pub fn signing_transcript(&self) -> ShareableInvitationTranscript<'_> {
        ShareableInvitationTranscript::new(self)
    }

    pub fn signing_transcript_with_transport(
        &self,
        transport: &ShareableInvitationTransportMetadata,
    ) -> ShareableInvitationTranscript<'_> {
        ShareableInvitationTranscript::with_transport(self, transport)
    }

    pub fn sender_id_bound_to_public_key(&self, public_key: &[u8]) -> bool {
        public_key.len() == Self::SENDER_PROOF_PUBLIC_KEY_BYTES
            && AuthorityId::new_from_entropy(hash(public_key)) == self.sender_id
    }

    pub fn from_code_with_proof(
        code: &str,
    ) -> Result<(Self, Option<ShareableInvitationSenderProof>), ShareableInvitationError> {
        let (invitation, proof, _) = Self::from_code_with_proof_and_transport(code)?;
        Ok((invitation, proof))
    }

    pub fn from_code_with_proof_and_transport(
        code: &str,
    ) -> Result<
        (
            Self,
            Option<ShareableInvitationSenderProof>,
            ShareableInvitationTransportMetadata,
        ),
        ShareableInvitationError,
    > {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

        let parts = Self::parse_code_parts(code)?;
        if parts.prefix != Self::PREFIX {
            return Err(ShareableInvitationError::InvalidFormat);
        }

        let version_str = parts.version;
        if !version_str.starts_with('v') {
            return Err(ShareableInvitationError::InvalidFormat);
        }
        let version: u8 = version_str[1..]
            .parse()
            .map_err(|_| ShareableInvitationError::InvalidFormat)?;

        if version != Self::CURRENT_VERSION {
            return Err(ShareableInvitationError::UnsupportedVersion(version));
        }

        ensure_encoded_segment(
            "payload",
            parts.payload,
            Self::MAX_PAYLOAD_BASE64_CHARS,
            Self::MAX_JSON_BYTES,
        )?;
        validate_sender_hint_segment(parts.sender_hint)?;
        validate_sender_device_id_segment(parts.sender_device_id)?;

        let json = URL_SAFE_NO_PAD
            .decode(parts.payload)
            .map_err(|_| ShareableInvitationError::DecodingFailed)?;
        ensure_len("json", json.len(), Self::MAX_JSON_BYTES)?;

        let (invitation, proof, transport) =
            match serde_json::from_slice::<ShareableInvitationCodePayload>(&json)
                .map_err(|_| ShareableInvitationError::ParsingFailed)?
            {
                ShareableInvitationCodePayload::Envelope(envelope) => {
                    (envelope.payload, envelope.proof, envelope.transport)
                }
                #[cfg(any(test, feature = "test-support"))]
                ShareableInvitationCodePayload::Legacy(invitation) => (
                    invitation,
                    None,
                    ShareableInvitationTransportMetadata::default(),
                ),
            };
        invitation.validate_size_limits()?;
        validate_transport_metadata(&transport)?;
        #[cfg(not(any(test, feature = "test-support")))]
        if proof.is_none() {
            return Err(ShareableInvitationError::MissingSenderProof);
        }
        if let Some(proof) = &proof {
            validate_sender_proof_shape(proof)?;
        }
        Ok((invitation, proof, transport))
    }

    pub fn sender_addr_from_code(code: &str) -> Option<String> {
        let parts = Self::parse_code_parts(code).ok()?;
        if parts.prefix != Self::PREFIX {
            return None;
        }

        let decoded = decode_sender_hint_segment(parts.sender_hint?).ok()?;
        let addr = String::from_utf8(decoded).ok()?;
        let trimmed = addr.trim();
        if trimmed.is_empty() {
            return None;
        }
        Some(trimmed.to_string())
    }

    pub fn sender_device_id_from_code(code: &str) -> Option<DeviceId> {
        let parts = Self::parse_code_parts(code).ok()?;
        if parts.prefix != Self::PREFIX {
            return None;
        }

        let decoded = decode_sender_device_id_segment(parts.sender_device_id?).ok()?;
        let device_id = String::from_utf8(decoded).ok()?;
        device_id.trim().parse().ok()
    }

    fn parse_code_parts(
        code: &str,
    ) -> Result<ShareableInvitationCodeParts<'_>, ShareableInvitationError> {
        let code = code.trim();
        ensure_len("code", code.len(), Self::MAX_CODE_CHARS)?;

        let mut parts = code.split(':');
        let prefix = parts
            .next()
            .ok_or(ShareableInvitationError::InvalidFormat)?;
        let version = parts
            .next()
            .ok_or(ShareableInvitationError::InvalidFormat)?;
        let payload = parts
            .next()
            .ok_or(ShareableInvitationError::InvalidFormat)?;
        let sender_hint = parts.next();
        let sender_device_id = parts.next();
        if parts.next().is_some() {
            return Err(ShareableInvitationError::InvalidFormat);
        }

        Ok(ShareableInvitationCodeParts {
            prefix,
            version,
            payload,
            sender_hint,
            sender_device_id,
        })
    }

    fn validate_size_limits(&self) -> Result<(), ShareableInvitationError> {
        ensure_len(
            "invitation_id",
            self.invitation_id.as_str().len(),
            Self::MAX_INVITATION_ID_BYTES,
        )?;
        if let Some(message) = self.message.as_deref() {
            ensure_len("message", message.len(), Self::MAX_MESSAGE_BYTES)?;
        }
        match &self.invitation_type {
            InvitationType::Channel {
                nickname_suggestion,
                ..
            }
            | InvitationType::DeviceEnrollment {
                nickname_suggestion,
                ..
            } => {
                if let Some(nickname) = nickname_suggestion.as_deref() {
                    ensure_len(
                        "nickname_suggestion",
                        nickname.len(),
                        Self::MAX_NICKNAME_BYTES,
                    )?;
                }
            }
            InvitationType::Contact { nickname } => {
                if let Some(nickname) = nickname.as_deref() {
                    ensure_len("nickname", nickname.len(), Self::MAX_NICKNAME_BYTES)?;
                }
            }
            InvitationType::Guardian { .. } => {}
        }
        if let InvitationType::DeviceEnrollment { ceremony_id, .. } = &self.invitation_type {
            ensure_len(
                "ceremony_id",
                ceremony_id.as_str().len(),
                Self::MAX_CEREMONY_ID_BYTES,
            )?;
        }
        Ok(())
    }
}

fn validate_sender_proof_shape(
    proof: &ShareableInvitationSenderProof,
) -> Result<(), ShareableInvitationError> {
    if proof.scheme != ShareableInvitation::SENDER_PROOF_SCHEME {
        return Err(ShareableInvitationError::InvalidSenderProof);
    }
    if proof.public_key.len() != ShareableInvitation::SENDER_PROOF_PUBLIC_KEY_BYTES {
        return Err(ShareableInvitationError::InvalidSenderProof);
    }
    if proof.signature.len() != ShareableInvitation::SENDER_PROOF_SIGNATURE_BYTES {
        return Err(ShareableInvitationError::InvalidSenderProof);
    }
    Ok(())
}

fn validate_transport_metadata(
    transport: &ShareableInvitationTransportMetadata,
) -> Result<(), ShareableInvitationError> {
    if let Some(sender_hint) = transport.sender_hint.as_deref() {
        ensure_len(
            "sender_hint",
            sender_hint.trim().len(),
            ShareableInvitation::MAX_SENDER_HINT_BYTES,
        )?;
    }
    Ok(())
}

struct ShareableInvitationCodeParts<'a> {
    prefix: &'a str,
    version: &'a str,
    payload: &'a str,
    sender_hint: Option<&'a str>,
    sender_device_id: Option<&'a str>,
}

fn ensure_len(field: &'static str, len: usize, max: usize) -> Result<(), ShareableInvitationError> {
    if len > max {
        Err(ShareableInvitationError::SizeLimitExceeded(field))
    } else {
        Ok(())
    }
}

fn ensure_encoded_segment(
    field: &'static str,
    segment: &str,
    max_encoded: usize,
    max_decoded: usize,
) -> Result<(), ShareableInvitationError> {
    ensure_len(field, segment.len(), max_encoded)?;
    if aura_core::envelope::base64_decoded_len_upper_bound(segment.len()) > max_decoded {
        return Err(ShareableInvitationError::SizeLimitExceeded(field));
    }
    Ok(())
}

fn decode_sender_hint_segment(segment: &str) -> Result<Vec<u8>, ShareableInvitationError> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

    ensure_encoded_segment(
        "sender_hint",
        segment,
        ShareableInvitation::MAX_SENDER_HINT_BASE64_CHARS,
        ShareableInvitation::MAX_SENDER_HINT_BYTES,
    )?;
    URL_SAFE_NO_PAD
        .decode(segment)
        .map_err(|_| ShareableInvitationError::DecodingFailed)
}

fn validate_sender_hint_segment(segment: Option<&str>) -> Result<(), ShareableInvitationError> {
    let Some(segment) = segment else {
        return Ok(());
    };
    let decoded = decode_sender_hint_segment(segment)?;
    let hint =
        std::str::from_utf8(&decoded).map_err(|_| ShareableInvitationError::ParsingFailed)?;
    ensure_len(
        "sender_hint",
        hint.trim().len(),
        ShareableInvitation::MAX_SENDER_HINT_BYTES,
    )
}

fn decode_sender_device_id_segment(segment: &str) -> Result<Vec<u8>, ShareableInvitationError> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

    ensure_encoded_segment(
        "sender_device_id",
        segment,
        ShareableInvitation::MAX_SENDER_DEVICE_ID_BASE64_CHARS,
        ShareableInvitation::MAX_SENDER_DEVICE_ID_BYTES,
    )?;
    URL_SAFE_NO_PAD
        .decode(segment)
        .map_err(|_| ShareableInvitationError::DecodingFailed)
}

fn validate_sender_device_id_segment(
    segment: Option<&str>,
) -> Result<(), ShareableInvitationError> {
    let Some(segment) = segment else {
        return Ok(());
    };
    let decoded = decode_sender_device_id_segment(segment)?;
    let device_id =
        std::str::from_utf8(&decoded).map_err(|_| ShareableInvitationError::ParsingFailed)?;
    device_id
        .trim()
        .parse::<DeviceId>()
        .map(|_| ())
        .map_err(|_| ShareableInvitationError::InvalidFormat)
}

impl From<&Invitation> for ShareableInvitation {
    fn from(inv: &Invitation) -> Self {
        Self {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: inv.invitation_id.clone(),
            sender_id: inv.sender_id,
            context_id: Some(inv.context_id),
            invitation_type: inv.invitation_type.clone(),
            expires_at: inv.expires_at,
            message: inv.message.clone(),
        }
    }
}

/// An imported invitation proven by the shareable-code verifier. Its fields
/// are private so callers cannot turn a raw cached record into creation evidence.
pub struct ValidatedImportedInvitation {
    invitation: Invitation,
}

impl ValidatedImportedInvitation {
    /// Decode a code and verify its sender proof before issuing import evidence.
    pub async fn verify_code<E: CryptoEffects + Send + Sync + ?Sized>(
        crypto: &E,
        code: &str,
        own_id: AuthorityId,
        default_context_id: ContextId,
        now_ms: u64,
    ) -> Result<Self, ShareableInvitationError> {
        let (shareable, proof, transport) =
            ShareableInvitation::from_code_with_proof_and_transport(code)?;
        let proof = proof.ok_or(ShareableInvitationError::MissingSenderProof)?;
        // This checks code integrity against the key carried in the code.
        // Known-sender identity trust is resolved separately by the importer.
        let self_certified_sender_key = proof.public_key.as_slice();
        let verified = verify_ed25519_transcript(
            crypto,
            &shareable.signing_transcript_with_transport(&transport),
            &proof.signature,
            self_certified_sender_key,
        )
        .await
        .map_err(|_| ShareableInvitationError::VerificationFailed)?;
        if !verified {
            return Err(ShareableInvitationError::InvalidSenderProof);
        }
        Self::after_verified_code(shareable, own_id, default_context_id, now_ms)
    }

    fn after_verified_code(
        shareable: ShareableInvitation,
        own_id: AuthorityId,
        default_context_id: ContextId,
        now_ms: u64,
    ) -> Result<Self, ShareableInvitationError> {
        if shareable
            .expires_at
            .is_some_and(|expires_at| now_ms > expires_at)
        {
            return Err(ShareableInvitationError::Expired);
        }
        let context_id = if matches!(shareable.invitation_type, InvitationType::Channel { .. }) {
            shareable
                .context_id
                .ok_or(ShareableInvitationError::MissingChannelContext)?
        } else {
            default_context_id
        };
        let receiver_id = match &shareable.invitation_type {
            InvitationType::DeviceEnrollment {
                invitee_authority: Some(invitee),
                ..
            } => *invitee,
            _ => own_id,
        };
        Ok(Self {
            invitation: Invitation {
                invitation_id: shareable.invitation_id,
                context_id,
                sender_id: shareable.sender_id,
                receiver_id,
                invitation_type: shareable.invitation_type,
                status: InvitationStatus::Pending,
                created_at: now_ms,
                expires_at: shareable.expires_at,
                message: shareable.message,
                receiver_nickname: None,
            },
        })
    }

    /// Return the verified import record without releasing its provenance.
    pub fn invitation(&self) -> &Invitation {
        &self.invitation
    }
}
