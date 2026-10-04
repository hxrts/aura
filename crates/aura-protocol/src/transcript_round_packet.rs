//! Layer 4 typed packet grammar. Possession evidence authenticates a participant
//! contribution; it never establishes membership or user signing authorization.
use aura_core::effects::crypto::SigningMode;
use aura_core::effects::crypto::{FrostPublicCommitment, FrostSigningPackage};
use aura_core::{AuraError, DeviceId};
use aura_signature::SecurityTranscript;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// Public messages in one approved transcript commitment/share exchange.
pub enum TranscriptRoundBody {
    /// Request the original approved participant commitment.
    RequestCommitment,
    /// Return the public commitment with its native participant index.
    Commitment(FrostPublicCommitment),
    /// Request a share for the exact public package under original approval.
    RequestShare(FrostSigningPackage),
    /// Return the audited public signature share.
    Share(Vec<u8>),
    /// Retire the original participant round without signing.
    Cancel,
}

/// A session binds an exact locally selected transcript domain and intent.
/// It carries no physical timestamp and grants no remote execution window.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptRoundPacket {
    /// Version of the canonical round packet grammar.
    pub version: u16,
    /// Digest binding this original local participant session.
    pub session: [u8; 32],
    /// Digest of the exact explicitly approved initiation intent.
    pub approved_intent_digest: [u8; 32],
    /// Digest of the single exact transcript signed by this round.
    pub transcript_digest: [u8; 32],
    /// Selected coordinator device; this identifier is not authentication.
    pub coordinator: DeviceId,
    /// Selected participant device; this identifier is not approval.
    pub participant: DeviceId,
    /// Exact ordered round phase; repeated or skipped phases are rejected.
    pub sequence: u8,
    /// Typed public contribution or request for the admitted phase.
    pub body: TranscriptRoundBody,
}
impl SecurityTranscript for TranscriptRoundPacket {
    type Payload = Self;
    const DOMAIN_SEPARATOR: &'static str = "aura.protocol.approved-enrollment-transcript-round.v1";
    fn transcript_bytes(&self) -> aura_signature::Result<Vec<u8>> {
        aura_signature::encode_transcript(Self::DOMAIN_SEPARATOR, self.version, self)
    }
    fn transcript_payload(&self) -> Self {
        self.clone()
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// Canonical public packet with independently verifiable physical participant possession evidence.
/// Possession alone grants neither membership nor user signing approval.
pub struct ParticipantProvenRoundPacket {
    /// Original packet whose complete canonical transcript is authenticated.
    pub packet: TranscriptRoundPacket,
    /// Audited participant proof; the expected verifier must come from native retained custody.
    pub possession_proof: Vec<u8>,
}
impl ParticipantProvenRoundPacket {
    /// Maximum accepted encoded packet size before deserialization.
    pub const MAXIMUM_WIRE_BYTES: usize = 262_144;
    /// Decode a size-bounded canonical packet and reject malformed proof lengths.
    /// Decoding does not authenticate or admit the packet.
    pub fn decode_bounded(bytes: &[u8]) -> Result<Self, AuraError> {
        if bytes.len() > Self::MAXIMUM_WIRE_BYTES {
            return Err(reject(RoundAdmissionError::PacketBounds));
        }
        let packet: Self = aura_core::util::serialization::from_slice(bytes)?;
        if packet.possession_proof.len() != 64 {
            return Err(reject(RoundAdmissionError::PacketBounds));
        }
        if aura_core::util::serialization::to_vec(&packet)? != bytes {
            return Err(reject(RoundAdmissionError::NonCanonical));
        }
        Ok(packet)
    }
    /// Exact verifier must originate in the retained native active roster.
    /// Source envelope metadata and packet fields cannot provide this argument.
    pub fn verify_exact_participant(
        &self,
        independently_retained_verifier: &[u8],
    ) -> Result<(), AuraError> {
        let transcript = self.packet.transcript_bytes().map_err(|source| {
            AuraError::crypto_with_source(
                "encode original approved round packet",
                std::sync::Arc::new(source),
            )
        })?;
        aura_core::crypto::participant_proof::verify_participant_key_proof(
            &transcript,
            &self.possession_proof,
            independently_retained_verifier,
            SigningMode::Threshold,
        )
    }
}

#[derive(Debug, thiserror::Error)]
/// Typed rejection of public packet shape, binding, or original phase.
pub enum RoundAdmissionError {
    #[error("round packet exceeds original protocol wire bounds")]
    /// Encoded size or possession-proof length exceeds the protocol bounds.
    PacketBounds,
    #[error("round packet is not the canonical wire encoding")]
    /// The packet bytes differ from the canonical encoding.
    NonCanonical,
    #[error("round packet does not belong to the original admitted local session")]
    /// The packet differs from the original admitted session or parties.
    Binding,
    #[error("round packet repeats or skips the original protocol sequence")]
    /// The packet repeats, skips, or follows a finished round.
    Sequence,
    #[error("round packet body does not match the admitted protocol phase")]
    /// The typed message is invalid for the original phase.
    Phase,
}
fn reject(source: RoundAdmissionError) -> AuraError {
    AuraError::PermissionDenied {
        message: "original approved round admission rejected".into(),
        source: Some(std::sync::Arc::new(source)),
    }
}

/// Runtime retains this sequence state inside one already approved owner.
/// Construction of this pure validator grants no signing authority or budget.
pub struct ParticipantRoundSession {
    session: [u8; 32],
    approved_intent_digest: [u8; 32],
    transcript_digest: [u8; 32],
    coordinator: DeviceId,
    participant: DeviceId,
    next_sequence: u8,
    finished: bool,
}
impl ParticipantRoundSession {
    /// Retain exact original session bindings for pure ordered admission checks.
    /// Construction grants no participant execution window or signing authority.
    pub fn new(
        session: [u8; 32],
        approved_intent_digest: [u8; 32],
        transcript_digest: [u8; 32],
        coordinator: DeviceId,
        participant: DeviceId,
    ) -> Self {
        Self {
            session,
            approved_intent_digest,
            transcript_digest,
            coordinator,
            participant,
            next_sequence: 1,
            finished: false,
        }
    }
    fn require_binding(&self, packet: &TranscriptRoundPacket) -> Result<(), AuraError> {
        if packet.version != 1
            || packet.session != self.session
            || packet.approved_intent_digest != self.approved_intent_digest
            || packet.transcript_digest != self.transcript_digest
            || packet.coordinator != self.coordinator
            || packet.participant != self.participant
        {
            return Err(reject(RoundAdmissionError::Binding));
        }
        if self.finished || packet.sequence != self.next_sequence {
            return Err(reject(RoundAdmissionError::Sequence));
        }
        match (&packet.body, self.next_sequence) {
            (TranscriptRoundBody::RequestCommitment, 1)
            | (TranscriptRoundBody::RequestShare(_), 3)
            | (TranscriptRoundBody::Cancel, _) => Ok(()),
            _ => Err(reject(RoundAdmissionError::Phase)),
        }
    }
    /// Authenticate using the independently retained current coordinator share
    /// verifier, then admit exact original bindings and advance once. Invalid
    /// packets cannot consume another owner's approval or move this state.
    pub fn admit(
        &mut self,
        packet: ParticipantProvenRoundPacket,
        independently_retained_coordinator_verifier: &[u8],
    ) -> Result<TranscriptRoundBody, AuraError> {
        packet.verify_exact_participant(independently_retained_coordinator_verifier)?;
        self.require_binding(&packet.packet)?;
        self.finished = matches!(
            &packet.packet.body,
            TranscriptRoundBody::RequestShare(_) | TranscriptRoundBody::Cancel
        );
        self.next_sequence += 2;
        Ok(packet.packet.body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> TranscriptRoundPacket {
        TranscriptRoundPacket {
            version: 1,
            session: [1; 32],
            approved_intent_digest: [2; 32],
            transcript_digest: [3; 32],
            coordinator: DeviceId::from_uuid(uuid::Uuid::from_u128(1)),
            participant: DeviceId::from_uuid(uuid::Uuid::from_u128(2)),
            sequence: 1,
            body: TranscriptRoundBody::RequestCommitment,
        }
    }
    fn owner(packet: &TranscriptRoundPacket) -> ParticipantRoundSession {
        ParticipantRoundSession::new(
            packet.session,
            packet.approved_intent_digest,
            packet.transcript_digest,
            packet.coordinator,
            packet.participant,
        )
    }
    // These are pure admission-validator tests; they claim no cryptographic
    // or runtime approval evidence. Actual authenticated round tests are required.
    #[test]
    fn foreign_intent_party_and_skipped_round_cannot_consume_original_phase() {
        let original = request();
        let state = owner(&original);
        let mut altered = Vec::new();
        let mut packet = original.clone();
        packet.session[0] ^= 1;
        altered.push(packet);
        let mut packet = original.clone();
        packet.approved_intent_digest[0] ^= 1;
        altered.push(packet);
        let mut packet = original.clone();
        packet.transcript_digest[0] ^= 1;
        altered.push(packet);
        let mut packet = original.clone();
        std::mem::swap(&mut packet.coordinator, &mut packet.participant);
        altered.push(packet);
        let mut packet = original.clone();
        packet.sequence = 3;
        altered.push(packet);
        let mut packet = original.clone();
        packet.body = TranscriptRoundBody::Share(vec![0; 64]);
        altered.push(packet);
        for packet in altered {
            assert!(state.require_binding(&packet).is_err());
        }
        assert!(state.require_binding(&original).is_ok());
    }
    #[test]
    fn finished_owner_rejects_even_original_cancel_and_commitment() {
        let original = request();
        let mut state = owner(&original);
        state.finished = true;
        assert!(state.require_binding(&original).is_err());
        let mut cancellation = original;
        cancellation.body = TranscriptRoundBody::Cancel;
        assert!(state.require_binding(&cancellation).is_err());
    }
}
