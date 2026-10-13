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

/// Finite public negative decision tags. Tags do not establish decision custody.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConditionalNegativeOutcome {
    /// The original participant refused.
    Rejected,
    /// The original operation was cancelled.
    Cancelled,
    /// Its original local deadline expired.
    TimedOut,
    /// The original choreography failed.
    ChoreographyFailed,
    /// The original runtime failed.
    RuntimeFailed,
    /// A later authoritative operation superseded it.
    Superseded,
}

/// Public original-coordinator decision statement. Only runtime-owned approval
/// and native verifier custody can admit this data as a conditional request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NegativeDecisionAttestation {
    /// Current attestation grammar version.
    pub version: u16,
    /// Exact reviewed intent.
    pub approved_intent_digest: [u8; 32],
    /// Complete canonical manifest transcript; interpreted by its feature owner.
    pub manifest: Vec<u8>,
    /// Digest of the complete manifest transcript.
    pub manifest_digest: [u8; 32],
    /// Coordinator whose native key must be independently retained.
    pub coordinator: DeviceId,
    /// Exact originally approved remote participant.
    pub participant: DeviceId,
    /// Original conditional round binding; never a new execution window.
    pub session: [u8; 32],
    /// Decision admission precedes commitment phase one and must be zero.
    pub sequence: u8,
    /// Finite negative outcome asserted by the original coordinator.
    pub outcome: ConditionalNegativeOutcome,
    /// Digest of the original protected first-decision bytes.
    pub decision_digest: [u8; 32],
    /// Complete canonical negative control transcript.
    pub terminal_transcript: Vec<u8>,
    /// Digest of the exact negative control transcript.
    pub transcript_digest: [u8; 32],
}
impl SecurityTranscript for NegativeDecisionAttestation {
    type Payload = Self;
    const DOMAIN_SEPARATOR: &'static str = "aura.protocol.original-negative-decision.v1";
    fn transcript_payload(&self) -> Self {
        self.clone()
    }
}
impl NegativeDecisionAttestation {
    /// Validate public shape only. The feature owner must separately compare
    /// canonical bytes and all scope fields to its actual retained approval.
    pub fn require_shape(&self) -> Result<(), AuraError> {
        require_decision_shape(
            self.version,
            self.sequence,
            &self.manifest,
            self.manifest_digest,
            &self.terminal_transcript,
            self.transcript_digest,
        )
    }
}

/// Native possession proof authenticating the complete decision statement.
/// This is not a quorum signature or a deserializable local decision receipt.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoordinatorProvenNegativeDecision {
    /// Complete decision statement authenticated by the proof.
    pub attestation: NegativeDecisionAttestation,
    /// Proof checked against the independently retained native coordinator key.
    pub possession_proof: Vec<u8>,
}
impl CoordinatorProvenNegativeDecision {
    /// Decode bounded canonical public data; this grants no runtime authority.
    pub fn decode_bounded(bytes: &[u8]) -> Result<Self, AuraError> {
        let decoded: Self = decode_canonical_packet(bytes)?;
        if decoded.possession_proof.len() != 64 {
            return Err(reject(RoundAdmissionError::PacketBounds));
        }
        decoded.attestation.require_shape()?;
        Ok(decoded)
    }
    /// Authenticate the whole statement with the original retained native key.
    /// Public coordinator fields cannot supply the expected verifier.
    pub fn verify_exact_coordinator(&self, retained_verifier: &[u8]) -> Result<(), AuraError> {
        self.attestation.require_shape()?;
        verify_decision_proof(&self.attestation, &self.possession_proof, retained_verifier)
    }
}

/// Public assertion of the original coordinator's protected Committed decision.
/// Native peer activation and original approval must independently admit it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PositiveDecisionAttestation {
    /// Current attestation grammar version.
    pub version: u16,
    /// Exact originally approved intent.
    pub approved_intent_digest: [u8; 32],
    /// Complete canonical manifest transcript.
    pub manifest: Vec<u8>,
    /// Digest of that complete manifest.
    pub manifest_digest: [u8; 32],
    /// Original coordinator, authenticated using a retained native key.
    pub coordinator: DeviceId,
    /// Original approved physical participant.
    pub participant: DeviceId,
    /// Exact positive round binding; grants no execution window.
    pub session: [u8; 32],
    /// Decision admission precedes commitment phase one and must be zero.
    pub sequence: u8,
    /// Digest of the original protected first Committed decision bytes.
    pub decision_digest: [u8; 32],
    /// Complete canonical committed confirmation transcript.
    pub terminal_transcript: Vec<u8>,
    /// Digest of that exact confirmation transcript.
    pub transcript_digest: [u8; 32],
}

impl SecurityTranscript for PositiveDecisionAttestation {
    type Payload = Self;
    const DOMAIN_SEPARATOR: &'static str = "aura.protocol.original-positive-decision.v1";
    fn transcript_payload(&self) -> Self {
        self.clone()
    }
}

impl PositiveDecisionAttestation {
    /// Validate shape only; feature-owned canonical bindings and native
    /// activation provenance remain mandatory at runtime admission.
    pub fn require_shape(&self) -> Result<(), AuraError> {
        require_decision_shape(
            self.version,
            self.sequence,
            &self.manifest,
            self.manifest_digest,
            &self.terminal_transcript,
            self.transcript_digest,
        )
    }
}

/// Original coordinator possession authenticates the whole positive statement.
/// This public proof is neither a local decision receipt nor a quorum signature.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoordinatorProvenPositiveDecision {
    /// Exact original positive decision statement.
    pub attestation: PositiveDecisionAttestation,
    /// Proof checked against the independently retained coordinator verifier.
    pub possession_proof: Vec<u8>,
}

impl CoordinatorProvenPositiveDecision {
    /// Decode bounded canonical public data without admitting execution.
    pub fn decode_bounded(bytes: &[u8]) -> Result<Self, AuraError> {
        let decoded: Self = decode_canonical_packet(bytes)?;
        if decoded.possession_proof.len() != 64 {
            return Err(reject(RoundAdmissionError::PacketBounds));
        }
        decoded.attestation.require_shape()?;
        Ok(decoded)
    }

    /// Authenticate with a verifier retained from the original native roster.
    pub fn verify_exact_coordinator(&self, retained_verifier: &[u8]) -> Result<(), AuraError> {
        self.attestation.require_shape()?;
        verify_decision_proof(&self.attestation, &self.possession_proof, retained_verifier)
    }
}

fn require_decision_shape(
    version: u16,
    sequence: u8,
    manifest: &[u8],
    manifest_digest: [u8; 32],
    terminal_transcript: &[u8],
    transcript_digest: [u8; 32],
) -> Result<(), AuraError> {
    if version != 1 || sequence != 0 {
        return Err(reject(RoundAdmissionError::Binding));
    }
    if [manifest, terminal_transcript].iter().any(|bytes| {
        bytes.is_empty() || bytes.len() > aura_core::effects::crypto::MAX_SIGNING_MESSAGE_BYTES
    }) {
        return Err(reject(RoundAdmissionError::PacketBounds));
    }
    if manifest_digest != aura_core::hash::hash(manifest)
        || transcript_digest != aura_core::hash::hash(terminal_transcript)
    {
        return Err(reject(RoundAdmissionError::Binding));
    }
    Ok(())
}

fn verify_decision_proof(
    attestation: &impl SecurityTranscript,
    proof: &[u8],
    retained_verifier: &[u8],
) -> Result<(), AuraError> {
    let transcript = attestation.required_transcript_bytes().map_err(|source| {
        AuraError::crypto_with_source(
            "encode original decision attestation",
            std::sync::Arc::new(source),
        )
    })?;
    verify_native_packet_proof(&transcript, proof, retained_verifier)
}

/// A physical participant's authenticated conditional-phase readiness.
/// The runtime producer requires actual original initial completion and native
/// guard disposal. This statement is neither a terminal nor a delivery ACK.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionalReadinessAttestation {
    /// Exact readiness grammar version.
    pub version: u16,
    /// Complete original local signing approval binding.
    pub approved_intent_digest: [u8; 32],
    /// Digest of the complete canonical original manifest transcript.
    pub manifest_digest: [u8; 32],
    /// Original approved coordinator, independently authenticated by the runtime.
    pub coordinator: DeviceId,
    /// Physical participant whose original native guards were disposed.
    pub participant: DeviceId,
    /// Stable original approval/participant transport locator.
    pub locator: [u8; 32],
}
impl SecurityTranscript for ConditionalReadinessAttestation {
    type Payload = Self;
    const DOMAIN_SEPARATOR: &'static str = "aura.protocol.original-conditional-readiness.v1";
    fn transcript_bytes(&self) -> aura_signature::Result<Vec<u8>> {
        aura_signature::encode_transcript(Self::DOMAIN_SEPARATOR, self.version, self)
    }
    fn transcript_payload(&self) -> Self {
        self.clone()
    }
}
impl ConditionalReadinessAttestation {
    fn require_shape(&self) -> Result<(), AuraError> {
        if self.version != 1 || self.coordinator == self.participant {
            return Err(reject(RoundAdmissionError::PacketBounds));
        }
        Ok(())
    }
}

/// Native participant possession authenticates every readiness binding.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParticipantProvenConditionalReadiness {
    /// Exact original readiness statement.
    pub statement: ConditionalReadinessAttestation,
    /// Native proof verified only against the independently retained share key.
    pub possession_proof: Vec<u8>,
}
impl ParticipantProvenConditionalReadiness {
    /// Bounded canonical decoding supplies data, not native disposal authority.
    pub fn decode_bounded(bytes: &[u8]) -> Result<Self, AuraError> {
        let decoded: Self = decode_canonical_packet(bytes)?;
        decoded.statement.require_shape()?;
        if decoded.possession_proof.len() != 64 {
            return Err(reject(RoundAdmissionError::PacketBounds));
        }
        Ok(decoded)
    }
    /// The expected key must come from the original native participant inventory.
    pub fn verify_exact_participant(&self, retained_verifier: &[u8]) -> Result<(), AuraError> {
        self.statement.require_shape()?;
        let transcript = self
            .statement
            .required_transcript_bytes()
            .map_err(|source| {
                AuraError::crypto_with_source(
                    "encode original conditional readiness",
                    std::sync::Arc::new(source),
                )
            })?;
        verify_native_packet_proof(&transcript, &self.possession_proof, retained_verifier)
    }
}

/// An original coordinator's finite request to retire conditional signing.
/// This public statement grants neither activation nor a negative decision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionalRetirementRequest {
    /// Exact retirement grammar version.
    pub version: u16,
    /// Complete original signing approval binding.
    pub approved_intent_digest: [u8; 32],
    /// Digest of the complete canonical original manifest transcript.
    pub manifest_digest: [u8; 32],
    /// Original approved coordinator.
    pub coordinator: DeviceId,
    /// Original physical participant to stop.
    pub participant: DeviceId,
    /// Stable original approval/participant transport locator.
    pub locator: [u8; 32],
}
impl SecurityTranscript for ConditionalRetirementRequest {
    type Payload = Self;
    const DOMAIN_SEPARATOR: &'static str =
        "aura.protocol.original-conditional-retirement-request.v1";
    fn transcript_bytes(&self) -> aura_signature::Result<Vec<u8>> {
        aura_signature::encode_transcript(Self::DOMAIN_SEPARATOR, self.version, self)
    }
    fn transcript_payload(&self) -> Self {
        self.clone()
    }
}
impl ConditionalRetirementRequest {
    fn require_shape(&self) -> Result<(), AuraError> {
        if self.version != 1 || self.coordinator == self.participant {
            return Err(reject(RoundAdmissionError::PacketBounds));
        }
        Ok(())
    }
    /// Canonical request binding, independent of proof encoding and transport.
    pub fn digest(&self) -> Result<[u8; 32], AuraError> {
        self.require_shape()?;
        self.required_transcript_bytes()
            .map(|bytes| aura_core::hash::hash(&bytes))
            .map_err(|source| {
                AuraError::crypto_with_source(
                    "encode original retirement request",
                    std::sync::Arc::new(source),
                )
            })
    }
}
/// Coordinator possession authenticates only the finite original stop request.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoordinatorProvenConditionalRetirementRequest {
    /// Exact original stop request.
    pub request: ConditionalRetirementRequest,
    /// Native proof checked against the retained coordinator share verifier.
    pub possession_proof: Vec<u8>,
}
impl CoordinatorProvenConditionalRetirementRequest {
    /// Canonical decoding supplies public data, not stop admission authority.
    pub fn decode_bounded(bytes: &[u8]) -> Result<Self, AuraError> {
        let decoded: Self = decode_canonical_packet(bytes)?;
        decoded.request.require_shape()?;
        if decoded.possession_proof.len() != 64 {
            return Err(reject(RoundAdmissionError::PacketBounds));
        }
        Ok(decoded)
    }
    /// The expected key must originate in the original retained native roster.
    pub fn verify_exact_coordinator(&self, retained_verifier: &[u8]) -> Result<(), AuraError> {
        self.request.require_shape()?;
        let transcript = self.request.required_transcript_bytes().map_err(|source| {
            AuraError::crypto_with_source(
                "encode original conditional stop",
                std::sync::Arc::new(source),
            )
        })?;
        verify_native_packet_proof(&transcript, &self.possession_proof, retained_verifier)
    }
}
/// Public acknowledgment of the exact original conditional retirement request.
/// The runtime must retain this proof privately until actual share retirement;
/// possession verification alone cannot establish local zeroization custody.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionalRetirementAcknowledgment {
    /// Complete original stop scope, never reconstructed from transport ids.
    pub request: ConditionalRetirementRequest,
    /// Digest of the exact original canonical request transcript.
    pub request_digest: [u8; 32],
}
impl SecurityTranscript for ConditionalRetirementAcknowledgment {
    type Payload = Self;
    const DOMAIN_SEPARATOR: &'static str =
        "aura.protocol.original-conditional-retirement-acknowledgment.v1";
    fn transcript_bytes(&self) -> aura_signature::Result<Vec<u8>> {
        aura_signature::encode_transcript(Self::DOMAIN_SEPARATOR, self.request.version, self)
    }
    fn transcript_payload(&self) -> Self {
        self.clone()
    }
}
impl ConditionalRetirementAcknowledgment {
    fn require_shape(&self) -> Result<(), AuraError> {
        if self.request.digest()? != self.request_digest {
            return Err(reject(RoundAdmissionError::Binding));
        }
        Ok(())
    }
    /// Pure exact binding check; it does not create an actual retirement receipt.
    pub fn require_request(
        &self,
        original: &ConditionalRetirementRequest,
    ) -> Result<(), AuraError> {
        self.require_shape()?;
        original.require_shape()?;
        if &self.request != original {
            return Err(reject(RoundAdmissionError::Binding));
        }
        Ok(())
    }
}
/// Native participant proof for an exact original retirement acknowledgment.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParticipantProvenConditionalRetirementAcknowledgment {
    /// Exact original request acknowledgment.
    pub acknowledgment: ConditionalRetirementAcknowledgment,
    /// Native proof checked against the retained participant share verifier.
    pub possession_proof: Vec<u8>,
}
impl ParticipantProvenConditionalRetirementAcknowledgment {
    /// Canonical decoding supplies public data, not actual disposal authority.
    pub fn decode_bounded(bytes: &[u8]) -> Result<Self, AuraError> {
        let decoded: Self = decode_canonical_packet(bytes)?;
        decoded.acknowledgment.require_shape()?;
        if decoded.possession_proof.len() != 64 {
            return Err(reject(RoundAdmissionError::PacketBounds));
        }
        Ok(decoded)
    }
    /// The expected key must originate in the original retained native roster.
    pub fn verify_exact_participant(&self, retained_verifier: &[u8]) -> Result<(), AuraError> {
        self.acknowledgment.require_shape()?;
        let transcript = self
            .acknowledgment
            .required_transcript_bytes()
            .map_err(|source| {
                AuraError::crypto_with_source(
                    "encode original conditional retirement acknowledgment",
                    std::sync::Arc::new(source),
                )
            })?;
        verify_native_packet_proof(&transcript, &self.possession_proof, retained_verifier)
    }
}

fn decode_canonical_packet<T: Serialize + serde::de::DeserializeOwned>(
    bytes: &[u8],
) -> Result<T, AuraError> {
    if bytes.len() > ParticipantProvenRoundPacket::MAXIMUM_WIRE_BYTES {
        return Err(reject(RoundAdmissionError::PacketBounds));
    }
    let packet: T = aura_core::util::serialization::from_slice(bytes)?;
    if aura_core::util::serialization::to_vec(&packet)? != bytes {
        return Err(reject(RoundAdmissionError::NonCanonical));
    }
    Ok(packet)
}
fn verify_native_packet_proof(
    transcript: &[u8],
    proof: &[u8],
    retained_verifier: &[u8],
) -> Result<(), AuraError> {
    if transcript.len() > aura_core::effects::crypto::MAX_SIGNING_MESSAGE_BYTES || proof.len() != 64
    {
        return Err(reject(RoundAdmissionError::PacketBounds));
    }
    aura_core::crypto::participant_proof::verify_participant_key_proof(
        transcript,
        proof,
        retained_verifier,
        SigningMode::Threshold,
    )
}

#[cfg(test)]
mod negative_decision_tests {
    use super::*;
    use rand::SeedableRng;

    fn native_fixture(
        identity: &[u8],
    ) -> (rand::rngs::StdRng, Vec<frost_ed25519::keys::KeyPackage>) {
        let mut rng = rand::rngs::StdRng::from_seed(aura_core::hash::hash(identity));
        let (shares, _) = frost_ed25519::keys::generate_with_dealer(
            2,
            2,
            frost_ed25519::keys::IdentifierList::Default,
            &mut rng,
        )
        .unwrap();
        let keys = shares
            .values()
            .map(|share| frost_ed25519::keys::KeyPackage::try_from(share.clone()).unwrap())
            .collect();
        (rng, keys)
    }

    #[test]
    fn native_conditional_readiness_authenticates_original_scope_without_outcome_authority() {
        let (mut rng, keys) = native_fixture(
            b"aura-protocol.native_conditional_readiness_authenticates_original_scope_without_outcome_authority",
        );
        let original = ConditionalReadinessAttestation {
            version: 1,
            approved_intent_digest: [1; 32],
            manifest_digest: [2; 32],
            coordinator: DeviceId::from_uuid(uuid::Uuid::from_u128(1)),
            participant: DeviceId::from_uuid(uuid::Uuid::from_u128(2)),
            locator: [3; 32],
        };
        let proof = aura_core::crypto::participant_proof::sign_participant_key_proof(
            &original.required_transcript_bytes().unwrap(),
            &keys[0].serialize().unwrap(),
            SigningMode::Threshold,
            &mut rng,
        )
        .unwrap();
        let verifier = keys[0].verifying_share().serialize();
        for field in 0..6 {
            let mut statement = original.clone();
            match field {
                0 => statement.version += 1,
                1 => statement.approved_intent_digest[0] ^= 1,
                2 => statement.manifest_digest[0] ^= 1,
                3 => statement.coordinator = DeviceId::from_uuid(uuid::Uuid::from_u128(3)),
                4 => statement.participant = DeviceId::from_uuid(uuid::Uuid::from_u128(4)),
                5 => statement.locator[0] ^= 1,
                _ => unreachable!(),
            }
            assert!(
                ParticipantProvenConditionalReadiness {
                    statement,
                    possession_proof: proof.clone(),
                }
                .verify_exact_participant(&verifier)
                .is_err(),
                "substituted readiness field {field}"
            );
        }
        let packet = ParticipantProvenConditionalReadiness {
            statement: original,
            possession_proof: proof,
        };
        packet.verify_exact_participant(&verifier).unwrap();
        assert!(packet
            .verify_exact_participant(&keys[1].verifying_share().serialize())
            .is_err());
        let encoded = aura_core::util::serialization::to_vec(&packet).unwrap();
        ParticipantProvenConditionalReadiness::decode_bounded(&encoded)
            .unwrap()
            .verify_exact_participant(&verifier)
            .unwrap();
        assert!(ParticipantProvenConditionalReadiness::decode_bounded(&vec![
            0;
            ParticipantProvenRoundPacket::MAXIMUM_WIRE_BYTES
                + 1
        ],)
        .is_err());
        assert!(ParticipantProvenConditionalReadiness::decode_bounded(&[0xff]).is_err());
        let negative = CoordinatorProvenNegativeDecision {
            attestation: statement(),
            possession_proof: packet.possession_proof.clone(),
        };
        assert!(
            negative.verify_exact_coordinator(&verifier).is_err(),
            "readiness proof cannot authenticate a terminal decision"
        );
        let mut malformed = packet;
        malformed.possession_proof.pop();
        assert!(ParticipantProvenConditionalReadiness::decode_bounded(
            &aura_core::util::serialization::to_vec(&malformed).unwrap(),
        )
        .is_err());
    }

    #[test]
    fn native_conditional_retirement_binds_exact_stop_and_ack_without_decision_authority() {
        let (mut rng, keys) = native_fixture(b"aura-protocol.native_conditional_retirement_binds_exact_stop_and_ack_without_decision_authority");
        let request = ConditionalRetirementRequest {
            version: 1,
            approved_intent_digest: [1; 32],
            manifest_digest: [2; 32],
            coordinator: DeviceId::from_uuid(uuid::Uuid::from_u128(1)),
            participant: DeviceId::from_uuid(uuid::Uuid::from_u128(2)),
            locator: [3; 32],
        };
        let coordinator = keys[0].verifying_share().serialize();
        let participant = keys[1].verifying_share().serialize();
        let sign = |message: &[u8], index: usize, rng: &mut rand::rngs::StdRng| {
            aura_core::crypto::participant_proof::sign_participant_key_proof(
                message,
                &keys[index].serialize().unwrap(),
                SigningMode::Threshold,
                rng,
            )
            .unwrap()
        };
        let stop_proof = sign(&request.required_transcript_bytes().unwrap(), 0, &mut rng);
        let stop = CoordinatorProvenConditionalRetirementRequest {
            request: request.clone(),
            possession_proof: stop_proof.clone(),
        };
        stop.verify_exact_coordinator(&coordinator).unwrap();
        assert!(stop.verify_exact_coordinator(&participant).is_err());
        CoordinatorProvenConditionalRetirementRequest::decode_bounded(
            &aura_core::util::serialization::to_vec(&stop).unwrap(),
        )
        .unwrap()
        .verify_exact_coordinator(&coordinator)
        .unwrap();
        let acknowledgment = ConditionalRetirementAcknowledgment {
            request: request.clone(),
            request_digest: request.digest().unwrap(),
        };
        let ack_proof = sign(
            &acknowledgment.required_transcript_bytes().unwrap(),
            1,
            &mut rng,
        );
        let ack = ParticipantProvenConditionalRetirementAcknowledgment {
            acknowledgment: acknowledgment.clone(),
            possession_proof: ack_proof.clone(),
        };
        ack.verify_exact_participant(&participant).unwrap();
        ack.acknowledgment.require_request(&request).unwrap();
        assert!(ack.verify_exact_participant(&coordinator).is_err());
        ParticipantProvenConditionalRetirementAcknowledgment::decode_bounded(
            &aura_core::util::serialization::to_vec(&ack).unwrap(),
        )
        .unwrap()
        .verify_exact_participant(&participant)
        .unwrap();
        for field in 0..6 {
            let mut altered = request.clone();
            match field {
                0 => altered.version += 1,
                1 => altered.approved_intent_digest[0] ^= 1,
                2 => altered.manifest_digest[0] ^= 1,
                3 => altered.coordinator = DeviceId::from_uuid(uuid::Uuid::from_u128(3)),
                4 => altered.participant = DeviceId::from_uuid(uuid::Uuid::from_u128(4)),
                5 => altered.locator[0] ^= 1,
                _ => unreachable!(),
            }
            assert!(
                CoordinatorProvenConditionalRetirementRequest {
                    request: altered.clone(),
                    possession_proof: stop_proof.clone()
                }
                .verify_exact_coordinator(&coordinator)
                .is_err(),
                "stop field {field}"
            );
            assert!(
                acknowledgment.require_request(&altered).is_err(),
                "foreign original request {field}"
            );
            let altered_digest = altered.digest().unwrap_or([0; 32]);
            assert!(
                ParticipantProvenConditionalRetirementAcknowledgment {
                    acknowledgment: ConditionalRetirementAcknowledgment {
                        request: altered,
                        request_digest: altered_digest
                    },
                    possession_proof: ack_proof.clone()
                }
                .verify_exact_participant(&participant)
                .is_err(),
                "ack field {field}"
            );
        }
        let mut wrong_digest = acknowledgment.clone();
        wrong_digest.request_digest[0] ^= 1;
        assert!(ParticipantProvenConditionalRetirementAcknowledgment {
            acknowledgment: wrong_digest,
            possession_proof: ack_proof.clone()
        }
        .verify_exact_participant(&participant)
        .is_err());
        assert!(
            ParticipantProvenConditionalRetirementAcknowledgment {
                acknowledgment,
                possession_proof: stop_proof.clone()
            }
            .verify_exact_participant(&coordinator)
            .is_err(),
            "stop proof is not an ACK"
        );
        assert!(
            CoordinatorProvenConditionalRetirementRequest {
                request: request.clone(),
                possession_proof: ack_proof.clone()
            }
            .verify_exact_coordinator(&participant)
            .is_err(),
            "ACK proof is not a stop"
        );
        assert!(
            CoordinatorProvenNegativeDecision {
                attestation: statement(),
                possession_proof: stop_proof
            }
            .verify_exact_coordinator(&coordinator)
            .is_err(),
            "stop proof is not a negative decision"
        );
        assert!(
            ParticipantProvenConditionalReadiness {
                statement: ConditionalReadinessAttestation {
                    version: request.version,
                    approved_intent_digest: request.approved_intent_digest,
                    manifest_digest: request.manifest_digest,
                    coordinator: request.coordinator,
                    participant: request.participant,
                    locator: request.locator
                },
                possession_proof: ack_proof
            }
            .verify_exact_participant(&participant)
            .is_err(),
            "ACK proof is not readiness"
        );
        assert!(CoordinatorProvenConditionalRetirementRequest::decode_bounded(&[0xff]).is_err());
        assert!(
            ParticipantProvenConditionalRetirementAcknowledgment::decode_bounded(&[0xff]).is_err()
        );
        let oversized = vec![0; ParticipantProvenRoundPacket::MAXIMUM_WIRE_BYTES + 1];
        assert!(CoordinatorProvenConditionalRetirementRequest::decode_bounded(&oversized).is_err());
        assert!(
            ParticipantProvenConditionalRetirementAcknowledgment::decode_bounded(&oversized)
                .is_err()
        );
        let mut malformed = stop;
        malformed.possession_proof.pop();
        assert!(
            CoordinatorProvenConditionalRetirementRequest::decode_bounded(
                &aura_core::util::serialization::to_vec(&malformed).unwrap()
            )
            .is_err()
        );
        let mut malformed = ack;
        malformed.possession_proof.pop();
        assert!(
            ParticipantProvenConditionalRetirementAcknowledgment::decode_bounded(
                &aura_core::util::serialization::to_vec(&malformed).unwrap()
            )
            .is_err()
        );
    }

    fn statement() -> NegativeDecisionAttestation {
        let manifest = aura_core::util::serialization::to_vec(&("manifest", 1u16)).unwrap();
        let terminal_transcript =
            aura_core::util::serialization::to_vec(&("negative", 2u16)).unwrap();
        NegativeDecisionAttestation {
            version: 1,
            approved_intent_digest: [2; 32],
            manifest_digest: aura_core::hash::hash(&manifest),
            manifest,
            coordinator: DeviceId::from_uuid(uuid::Uuid::from_u128(1)),
            participant: DeviceId::from_uuid(uuid::Uuid::from_u128(2)),
            session: [3; 32],
            sequence: 0,
            outcome: ConditionalNegativeOutcome::Rejected,
            decision_digest: [4; 32],
            transcript_digest: aura_core::hash::hash(&terminal_transcript),
            terminal_transcript,
        }
    }

    #[test]
    fn native_negative_attestation_proof_authenticates_every_binding() {
        let (mut rng, keys) = native_fixture(
            b"aura-protocol.native_negative_attestation_proof_authenticates_every_binding",
        );
        let attestation = statement();
        let proof = aura_core::crypto::participant_proof::sign_participant_key_proof(
            &attestation.required_transcript_bytes().unwrap(),
            &keys[0].serialize().unwrap(),
            SigningMode::Threshold,
            &mut rng,
        )
        .unwrap();
        let verifier = keys[0].verifying_share().serialize();
        let original = CoordinatorProvenNegativeDecision {
            attestation: attestation.clone(),
            possession_proof: proof.clone(),
        };
        original.verify_exact_coordinator(&verifier).unwrap();
        assert!(original
            .verify_exact_coordinator(&keys[1].verifying_share().serialize())
            .is_err());
        for field in 0..12 {
            let mut altered = attestation.clone();
            match field {
                0 => altered.version += 1,
                1 => altered.approved_intent_digest[0] ^= 1,
                2 => {
                    altered.manifest.push(0);
                    altered.manifest_digest = aura_core::hash::hash(&altered.manifest);
                }
                3 => altered.manifest_digest[0] ^= 1,
                4 => std::mem::swap(&mut altered.coordinator, &mut altered.participant),
                5 => altered.participant = DeviceId::from_uuid(uuid::Uuid::from_u128(3)),
                6 => altered.session[0] ^= 1,
                7 => altered.sequence = 1,
                8 => altered.outcome = ConditionalNegativeOutcome::Cancelled,
                9 => altered.decision_digest[0] ^= 1,
                10 => {
                    altered.terminal_transcript.push(0);
                    altered.transcript_digest = aura_core::hash::hash(&altered.terminal_transcript);
                }
                11 => altered.transcript_digest[0] ^= 1,
                _ => unreachable!(),
            }
            assert!(
                CoordinatorProvenNegativeDecision {
                    attestation: altered,
                    possession_proof: proof.clone()
                }
                .verify_exact_coordinator(&verifier)
                .is_err(),
                "substituted field {field} must invalidate the original proof"
            );
        }
        let mut corrupt = original;
        corrupt.possession_proof[0] ^= 1;
        assert!(corrupt.verify_exact_coordinator(&verifier).is_err());
    }

    #[test]
    fn negative_attestation_decode_rejects_bounds_noncanonical_and_unknown_outcomes() {
        let original = CoordinatorProvenNegativeDecision {
            attestation: statement(),
            possession_proof: vec![0; 64],
        };
        let bytes = aura_core::util::serialization::to_vec(&original).unwrap();
        let decoded = CoordinatorProvenNegativeDecision::decode_bounded(&bytes).unwrap();
        assert_eq!(decoded.attestation, original.attestation);
        // Decoding public data deliberately does not authenticate its zero proof.
        assert!(decoded.verify_exact_coordinator(&[0; 32]).is_err());
        assert!(CoordinatorProvenNegativeDecision::decode_bounded(&[0xff]).is_err());
        assert!(CoordinatorProvenNegativeDecision::decode_bounded(&vec![
            0;
            ParticipantProvenRoundPacket::MAXIMUM_WIRE_BYTES
                + 1
        ])
        .is_err());
        assert_eq!(bytes[0], 0xa2);
        let mut noncanonical = vec![0xbf];
        noncanonical.extend_from_slice(&bytes[1..]);
        noncanonical.push(0xff);
        assert!(CoordinatorProvenNegativeDecision::decode_bounded(&noncanonical).is_err());
        let mut invalid = original;
        invalid.possession_proof.pop();
        assert!(CoordinatorProvenNegativeDecision::decode_bounded(
            &aura_core::util::serialization::to_vec(&invalid).unwrap()
        )
        .is_err());
        invalid.possession_proof.push(0);
        invalid.attestation.manifest =
            vec![0; aura_core::effects::crypto::MAX_SIGNING_MESSAGE_BYTES + 1];
        assert!(invalid.attestation.require_shape().is_err());
        let unknown = aura_core::util::serialization::to_vec(&"Committed").unwrap();
        assert!(
            aura_core::util::serialization::from_slice::<ConditionalNegativeOutcome>(&unknown)
                .is_err()
        );
    }

    fn positive_statement() -> PositiveDecisionAttestation {
        let original = statement();
        PositiveDecisionAttestation {
            version: original.version,
            approved_intent_digest: original.approved_intent_digest,
            manifest: original.manifest,
            manifest_digest: original.manifest_digest,
            coordinator: original.coordinator,
            participant: original.participant,
            session: original.session,
            sequence: original.sequence,
            decision_digest: original.decision_digest,
            terminal_transcript: original.terminal_transcript,
            transcript_digest: original.transcript_digest,
        }
    }

    #[test]
    fn native_positive_attestation_binds_every_field_and_separates_negative_authority() {
        let (mut rng, keys) = native_fixture(b"aura-protocol.native-positive-attestation-bindings");
        let attestation = positive_statement();
        let sign = |message: &[u8], rng: &mut rand::rngs::StdRng| {
            aura_core::crypto::participant_proof::sign_participant_key_proof(
                message,
                &keys[0].serialize().unwrap(),
                SigningMode::Threshold,
                rng,
            )
            .unwrap()
        };
        let proof = sign(&attestation.required_transcript_bytes().unwrap(), &mut rng);
        let verifier = keys[0].verifying_share().serialize();
        let original = CoordinatorProvenPositiveDecision {
            attestation: attestation.clone(),
            possession_proof: proof.clone(),
        };
        original.verify_exact_coordinator(&verifier).unwrap();
        assert!(original
            .verify_exact_coordinator(&keys[1].verifying_share().serialize())
            .is_err());
        for field in 0..11 {
            let mut altered = attestation.clone();
            match field {
                0 => altered.version += 1,
                1 => altered.approved_intent_digest[0] ^= 1,
                2 => {
                    altered.manifest.push(0);
                    altered.manifest_digest = aura_core::hash::hash(&altered.manifest);
                }
                3 => altered.manifest_digest[0] ^= 1,
                4 => std::mem::swap(&mut altered.coordinator, &mut altered.participant),
                5 => altered.participant = DeviceId::from_uuid(uuid::Uuid::from_u128(3)),
                6 => altered.session[0] ^= 1,
                7 => altered.sequence = 1,
                8 => altered.decision_digest[0] ^= 1,
                9 => {
                    altered.terminal_transcript.push(0);
                    altered.transcript_digest = aura_core::hash::hash(&altered.terminal_transcript);
                }
                10 => altered.transcript_digest[0] ^= 1,
                _ => unreachable!(),
            }
            assert!(
                CoordinatorProvenPositiveDecision {
                    attestation: altered,
                    possession_proof: proof.clone(),
                }
                .verify_exact_coordinator(&verifier)
                .is_err(),
                "substituted positive field {field}"
            );
        }
        let negative_proof = sign(&statement().required_transcript_bytes().unwrap(), &mut rng);
        assert!(
            CoordinatorProvenPositiveDecision {
                attestation,
                possession_proof: negative_proof,
            }
            .verify_exact_coordinator(&verifier)
            .is_err(),
            "negative admission cannot authenticate positive authority"
        );
        assert!(
            CoordinatorProvenNegativeDecision {
                attestation: statement(),
                possession_proof: proof,
            }
            .verify_exact_coordinator(&verifier)
            .is_err(),
            "positive admission cannot authenticate negative authority"
        );
    }

    #[test]
    fn positive_attestation_decode_requires_canonical_bounded_current_grammar() {
        let mut packet = CoordinatorProvenPositiveDecision {
            attestation: positive_statement(),
            possession_proof: vec![0; 64],
        };
        let bytes = aura_core::util::serialization::to_vec(&packet).unwrap();
        let decoded = CoordinatorProvenPositiveDecision::decode_bounded(&bytes).unwrap();
        assert_eq!(decoded.attestation, packet.attestation);
        assert!(decoded.verify_exact_coordinator(&[0; 32]).is_err());
        assert!(CoordinatorProvenPositiveDecision::decode_bounded(&[0xff]).is_err());
        assert!(CoordinatorProvenPositiveDecision::decode_bounded(&vec![
            0;
            ParticipantProvenRoundPacket::MAXIMUM_WIRE_BYTES
                + 1
        ])
        .is_err());
        assert_eq!(bytes[0], 0xa2);
        let mut noncanonical = vec![0xbf];
        noncanonical.extend_from_slice(&bytes[1..]);
        noncanonical.push(0xff);
        assert!(CoordinatorProvenPositiveDecision::decode_bounded(&noncanonical).is_err());
        packet.possession_proof.pop();
        assert!(CoordinatorProvenPositiveDecision::decode_bounded(
            &aura_core::util::serialization::to_vec(&packet).unwrap()
        )
        .is_err());
        packet.possession_proof.push(0);
        packet.attestation.terminal_transcript =
            vec![0; aura_core::effects::crypto::MAX_SIGNING_MESSAGE_BYTES + 1];
        assert!(packet.attestation.require_shape().is_err());
        packet.attestation = positive_statement();
        packet.attestation.manifest.clear();
        assert!(packet.attestation.require_shape().is_err());
        let negative = CoordinatorProvenNegativeDecision {
            attestation: statement(),
            possession_proof: vec![0; 64],
        };
        assert!(
            CoordinatorProvenPositiveDecision::decode_bounded(
                &aura_core::util::serialization::to_vec(&negative).unwrap()
            )
            .is_err(),
            "negative grammar must not decode as a positive attestation"
        );
    }
}
