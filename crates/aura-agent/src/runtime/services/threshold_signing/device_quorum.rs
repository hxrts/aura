//! Coordinated quorum signing among this authority's own devices (Task 163).
//!
//! A threshold authority signature (a tree operation or a typed message such
//! as a device-rotation proposal) needs FROST shares from `threshold` of the
//! authority's devices. The coordinating device sends each co-signer a
//! [`QuorumSigningRequest`] carrying the full signing context, proven with
//! the coordinator's own FROST share; the co-signer authenticates it against
//! the coordinator's verifying share in its own retained public package and
//! recomputes the exact message itself. Every later round packet is proven
//! the same way in both directions (the possession grammar of
//! `aura_protocol::transcript_round_packet`).
//!
//! Consent is separate from the mechanism: before contributing anything, a
//! co-signer consults its device-local [`DeviceSigningConsent`] policy. With
//! `EscalateToUser` (the default) the request waits until the user on that
//! device approves or declines it; nothing bypasses that wait. Nonces live
//! only in memory, are generated after consent, and are consumed by the one
//! share they produce, so a restart can never reuse them.

use super::*;
use aura_app::runtime_bridge::{DeviceSigningConsent, PendingSigningRequest};
use aura_core::effects::crypto::{FrostPublicCommitment, FrostSigningPackage};
use aura_core::effects::transport::TransportEnvelope;
use aura_core::util::serialization::to_vec;
use aura_protocol::transcript_round_packet::{
    ParticipantProvenRoundPacket, ParticipantRoundSession, TranscriptRoundBody,
    TranscriptRoundPacket,
};
use aura_signature::SecurityTranscript;
use std::collections::{BTreeMap, VecDeque};
use zeroize::Zeroizing;

/// Transport content type of device quorum signing messages.
pub(crate) const DEVICE_QUORUM_CONTENT_TYPE: &str = "application/aura-device-quorum-signing";
const REQUEST_VERSION: u16 = 1;
const ROUND_VERSION: u16 = 1;
/// How long a coordinator waits for a co-signer, including a user decision.
const RESPONSE_TIMEOUT_MS: u64 = 300_000;
const MAX_WIRE_BYTES: usize = 262_144;
/// Bound on requests a co-signer holds at once (pending or in flight).
const MAX_SLOTS: usize = 16;
/// Bound on round packets queued behind a pending user decision.
const MAX_QUEUED: usize = 2;
const CONSENT_STORAGE_KEY: &str = "device_local/signing_consent";

/// Why a device quorum signing round was refused.
#[derive(Debug, thiserror::Error)]
pub(crate) enum DeviceQuorumError {
    #[error("signing context is not a threshold authority of this device")]
    NotThreshold,
    #[error("quorum roster holds a non-device participant")]
    NonDeviceRoster,
    #[error("too few devices of this authority to reach the threshold")]
    InsufficientRoster,
    #[error("quorum request does not belong to this device's current authority epoch")]
    Binding,
    #[error("quorum packet phase or session differs from the admitted round")]
    Phase,
    #[error("co-signing device declined the request")]
    Declined,
    #[error("co-signing device did not answer before the deadline")]
    TimedOut,
    #[error("aggregated quorum signature does not verify against the group key")]
    Aggregate,
    #[error("too many pending quorum signing requests")]
    Capacity,
    #[error("no pending quorum signing request with that id")]
    UnknownRequest,
}

fn refused(source: DeviceQuorumError) -> AuraError {
    AuraError::PermissionDenied {
        message: "device quorum signing refused".into(),
        source: Some(Arc::new(source)),
    }
}

/// Coordinator's request to one co-signer: the full signing context, so the
/// co-signer derives the exact message from its own retained state.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct QuorumSigningRequest {
    version: u16,
    session: [u8; 32],
    authority: AuthorityId,
    epoch: u64,
    coordinator: DeviceId,
    participant: DeviceId,
    context: SigningContext,
}

impl SecurityTranscript for QuorumSigningRequest {
    type Payload = Self;
    const DOMAIN_SEPARATOR: &'static str = "aura.agent.device-quorum-signing-request.v1";
    fn transcript_bytes(&self) -> aura_signature::Result<Vec<u8>> {
        aura_signature::encode_transcript(Self::DOMAIN_SEPARATOR, self.version, self)
    }
    fn transcript_payload(&self) -> Self {
        self.clone()
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
enum DeviceQuorumWire {
    Request {
        request: QuorumSigningRequest,
        possession_proof: Vec<u8>,
        first_round: Box<ParticipantProvenRoundPacket>,
    },
    Round(ParticipantProvenRoundPacket),
}

fn decode_wire(envelope: &TransportEnvelope) -> Option<DeviceQuorumWire> {
    if envelope.metadata.get("content-type").map(String::as_str) != Some(DEVICE_QUORUM_CONTENT_TYPE)
        || envelope.payload.len() > MAX_WIRE_BYTES
    {
        return None;
    }
    aura_core::util::serialization::from_slice(&envelope.payload).ok()
}

fn is_quorum_envelope(envelope: &TransportEnvelope) -> bool {
    envelope.metadata.get("content-type").map(String::as_str) == Some(DEVICE_QUORUM_CONTENT_TYPE)
}

#[cfg(test)]
#[allow(clippy::disallowed_types)] // Test-only signal registry; lock guards never cross await.
fn request_intake_test_signals(
) -> &'static std::sync::Mutex<std::collections::HashMap<[u8; 32], tokio::sync::oneshot::Sender<()>>>
{
    static SIGNALS: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<[u8; 32], tokio::sync::oneshot::Sender<()>>>,
    > = std::sync::OnceLock::new();
    SIGNALS.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

#[cfg(test)]
#[allow(clippy::disallowed_types)] // Test-only signal registry; lock guards never cross await.
fn round_intake_test_signals(
) -> &'static std::sync::Mutex<std::collections::HashMap<[u8; 32], tokio::sync::oneshot::Sender<()>>>
{
    static SIGNALS: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<[u8; 32], tokio::sync::oneshot::Sender<()>>>,
    > = std::sync::OnceLock::new();
    SIGNALS.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

fn transcript_digest_of<T: SecurityTranscript>(value: &T) -> Result<[u8; 32], AuraError> {
    let bytes = value.transcript_bytes().map_err(|source| {
        AuraError::crypto_with_source("encode device quorum transcript", Arc::new(source))
    })?;
    Ok(aura_core::hash::hash(&bytes))
}

/// A short description of what a signing context would sign, for the user.
fn describe(context: &SigningContext) -> String {
    match &context.operation {
        SignableOperation::TreeOp(_) => "account device tree update".to_string(),
        SignableOperation::Message { domain, .. } => format!("signed message ({domain})"),
        SignableOperation::RecoveryApproval { .. } => "recovery approval".to_string(),
        SignableOperation::GroupProposal { .. } => "group proposal".to_string(),
        SignableOperation::InvitationCode {
            recipient,
            invitation_type,
            ..
        } => format!("shareable {invitation_type:?} invitation for {recipient}"),
        SignableOperation::InvitationResponse {
            recipient, kind, ..
        } => {
            format!("{kind:?} invitation response for {recipient}")
        }
        other => format!("{other:?}").chars().take(64).collect(),
    }
}

/// A device's proof that it holds its current threshold share.
#[derive(Debug, Clone)]
pub(crate) struct DevicePossessionProof {
    pub(crate) epoch: u64,
    pub(crate) index: u16,
    pub(crate) package_digest: Hash32,
    pub(crate) proof: Vec<u8>,
}

/// Public material of the authority's current threshold epoch.
struct QuorumPolicy {
    authority: AuthorityId,
    epoch: u64,
    threshold: u16,
    public_package: Vec<u8>,
    native: frost_ed25519::keys::PublicKeyPackage,
    roster: Vec<(DeviceId, u16)>,
    my_index: u16,
}

impl QuorumPolicy {
    fn index_of(&self, device: DeviceId) -> Option<u16> {
        self.roster
            .iter()
            .find(|(candidate, _)| *candidate == device)
            .map(|(_, index)| *index)
    }
    fn verifier(&self, index: u16) -> Result<Vec<u8>, AuraError> {
        let identifier = frost_ed25519::Identifier::try_from(index).map_err(|source| {
            AuraError::crypto_with_source("device quorum participant identifier", Arc::new(source))
        })?;
        self.native
            .verifying_shares()
            .get(&identifier)
            .map(|share| share.serialize().to_vec())
            .ok_or_else(|| refused(DeviceQuorumError::Binding))
    }
    fn group_key(&self) -> Vec<u8> {
        self.native.verifying_key().serialize().to_vec()
    }
}

/// One co-signer's round with a coordinator.
struct ParticipantSlot {
    request: QuorumSigningRequest,
    message: Vec<u8>,
    coordinator_verifier: Vec<u8>,
    approved_intent_digest: [u8; 32],
    transcript_digest: [u8; 32],
    validator: ParticipantRoundSession,
    approved: bool,
    queued: VecDeque<ParticipantProvenRoundPacket>,
    nonces: Option<aura_core::effects::crypto::FrostNonces>,
    commitment: Option<Vec<u8>>,
    proposal_consent: Option<OriginalDeviceProposalConsent>,
    proposal_consent_handed_off: bool,
    proposal_consent_failure: Option<AuraError>,
    proposal_policy_origin: Option<OriginalProposalConsentOrigin>,
}

#[derive(Clone, Copy)]
enum OriginalProposalConsentOrigin {
    ExplicitDecision,
    AutoSignVerifiedPolicy,
}

/// Actual device-local approval of one native-authenticated proposal request.
/// Signed scope is data; this private receipt retains the real approval origin.
pub(in crate::runtime::services) struct OriginalDeviceProposalConsent {
    effects: std::sync::Weak<AuraEffectSystem>,
    local_device: DeviceId,
    coordinator: DeviceId,
    _session: [u8; 32],
    context: SigningContext,
    coordinator_verifier: Vec<u8>,
    _origin: OriginalProposalConsentOrigin,
}

impl OriginalDeviceProposalConsent {
    pub(super) fn require_original_scope(
        &self,
        effects: &AuraEffectSystem,
        expected_coordinator_verifier: &[u8],
    ) -> Result<
        aura_sync::protocols::device_epoch_rotation::DeviceEpochProposalTranscriptPayload,
        AuraError,
    > {
        let original = self
            .effects
            .upgrade()
            .ok_or_else(|| refused(DeviceQuorumError::Binding))?;
        if !std::ptr::eq(original.as_ref(), effects)
            || self.local_device != effects.device_id()
            || self.coordinator_verifier != expected_coordinator_verifier
        {
            return Err(refused(DeviceQuorumError::Binding));
        }
        let scope = aura_sync::protocols::device_epoch_rotation::decode_device_epoch_proposal_signing_context(&self.context)?;
        if scope.initiator_device_id != self.coordinator
            || scope.participant_device_id != self.local_device
        {
            return Err(refused(DeviceQuorumError::Binding));
        }
        Ok(scope)
    }

    pub(super) fn require_original_proposal(
        &self,
        effects: &AuraEffectSystem,
        proposal: &aura_sync::protocols::device_epoch_rotation::DeviceEpochProposal,
        expected_coordinator_verifier: &[u8],
    ) -> Result<(), AuraError> {
        self.require_original_scope(effects, expected_coordinator_verifier)?;
        let expected =
            crate::handlers::device_epoch_rotation::device_epoch_proposal_signing_context(proposal)
                .map_err(|source| {
                    AuraError::crypto_with_source(
                        "original proposal consent context",
                        Arc::new(source),
                    )
                })?;
        if to_vec(&expected).map_err(|source| {
            AuraError::crypto_with_source("encode original proposal context", Arc::new(source))
        })? != to_vec(&self.context).map_err(|source| {
            AuraError::crypto_with_source("encode approved proposal context", Arc::new(source))
        })? {
            return Err(refused(DeviceQuorumError::Binding));
        }
        Ok(())
    }
}

/// In-memory co-signer rounds, keyed by session.
#[derive(Default)]
pub(super) struct DeviceQuorumSlots {
    slots: Mutex<HashMap<[u8; 32], ParticipantSlot>>,
    /// Retirement log for quorum nonces, which never leave process memory.
    nonce_retirement: aura_core::crypto::tree_signing::ProcessFrostNonceRetirement,
}

pub(crate) fn original_device_quorum_response_matches(
    envelope: &TransportEnvelope,
    session: [u8; 32],
    participant: DeviceId,
    sequence: u8,
) -> bool {
    is_quorum_envelope(envelope)
        && matches!(decode_wire(envelope), Some(DeviceQuorumWire::Round(proven)) if proven.packet.session == session && proven.packet.participant == participant && proven.packet.sequence == sequence)
}

impl ThresholdSigningService {
    /// This device's consent policy for co-signing (device-local storage).
    pub(crate) async fn device_signing_consent(&self) -> Result<DeviceSigningConsent, AuraError> {
        let stored = self
            .effects
            .retrieve(CONSENT_STORAGE_KEY)
            .await
            .map_err(|source| AuraError::Storage {
                message: "read device signing consent".into(),
                source: Some(Arc::new(source)),
            })?;
        Ok(stored
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .and_then(|value| DeviceSigningConsent::parse(&value))
            .unwrap_or_default())
    }

    /// Set this device's consent policy; it is never replicated.
    pub(crate) async fn set_device_signing_consent(
        &self,
        consent: DeviceSigningConsent,
    ) -> Result<(), AuraError> {
        self.effects
            .store(CONSENT_STORAGE_KEY, consent.as_str().as_bytes().to_vec())
            .await
            .map_err(|source| AuraError::Storage {
                message: "store device signing consent".into(),
                source: Some(Arc::new(source)),
            })
    }

    async fn quorum_policy(&self, authority: &AuthorityId) -> Result<QuorumPolicy, AuraError> {
        let state = self
            .shared
            .state
            .read()
            .await
            .contexts
            .get(authority)
            .cloned()
            .ok_or_else(|| refused(DeviceQuorumError::NotThreshold))?;
        if state.mode != SigningMode::Threshold || state.config.threshold < 2 {
            return Err(refused(DeviceQuorumError::NotThreshold));
        }
        let my_index = state
            .my_signer_index
            .ok_or_else(|| refused(DeviceQuorumError::NotThreshold))?;
        let roster = state
            .participants
            .iter()
            .enumerate()
            .map(|(position, participant)| match participant {
                ParticipantIdentity::Device(device) => u16::try_from(position + 1)
                    .map(|index| (*device, index))
                    .map_err(|_| refused(DeviceQuorumError::NonDeviceRoster)),
                _ => Err(refused(DeviceQuorumError::NonDeviceRoster)),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let native = frost_ed25519::keys::PublicKeyPackage::deserialize(&state.public_key_package)
            .map_err(|source| {
                AuraError::crypto_with_source(
                    "decode device quorum public package",
                    Arc::new(source),
                )
            })?;
        Ok(QuorumPolicy {
            authority: *authority,
            epoch: state.epoch,
            threshold: state.config.threshold,
            public_package: state.public_key_package.clone(),
            native,
            roster,
            my_index,
        })
    }

    async fn signing_message(&self, context: &SigningContext) -> Result<Vec<u8>, AuraError> {
        if let SignableOperation::InvitationResponse {
            recipient,
            transcript_digest,
            ..
        } = &context.operation
        {
            if *recipient == context.authority || *transcript_digest == [0; 32] {
                return Err(refused(DeviceQuorumError::Binding));
            }
        }
        if let SignableOperation::InvitationCode {
            recipient,
            invitation_type,
            transport,
            ..
        } = &context.operation
        {
            if *recipient == context.authority
                || !matches!(
                    invitation_type.as_ref(),
                    aura_core::invitation::InvitationType::Contact { .. }
                        | aura_core::invitation::InvitationType::Guardian { .. }
                )
                || transport.sender_device_id.is_none()
            {
                return Err(refused(DeviceQuorumError::Binding));
            }
        }
        let state = self
            .shared
            .state
            .read()
            .await
            .contexts
            .get(&context.authority)
            .cloned()
            .ok_or_else(|| refused(DeviceQuorumError::NotThreshold))?;
        match &context.operation {
            SignableOperation::TreeOp(op) => Self::tree_op_message(op, &state),
            _ => Self::serialize_signing_context(context, state.epoch),
        }
    }

    async fn local_quorum_share(
        &self,
        policy: &QuorumPolicy,
    ) -> Result<Zeroizing<Vec<u8>>, AuraError> {
        let participant = ParticipantIdentity::device(self.effects.device_id());
        let location =
            Self::participant_share_location(&policy.authority, policy.epoch, &participant);
        Ok(Zeroizing::new(
            self.retrieve_participant_key_package(
                &policy.authority,
                policy.epoch,
                &participant,
                &location,
            )
            .await?,
        ))
    }

    async fn prove_round(
        &self,
        local_share: &[u8],
        packet: TranscriptRoundPacket,
    ) -> Result<ParticipantProvenRoundPacket, AuraError> {
        let message = packet.transcript_bytes().map_err(|source| {
            AuraError::crypto_with_source("encode device quorum round packet", Arc::new(source))
        })?;
        let possession_proof = self
            .effects
            .sign_participant_key_proof(&message, local_share, SigningMode::Threshold)
            .await?;
        Ok(ParticipantProvenRoundPacket {
            packet,
            possession_proof,
        })
    }

    async fn send_quorum_wire(
        &self,
        device: DeviceId,
        wire: &DeviceQuorumWire,
    ) -> Result<(), AuraError> {
        let bytes = aura_core::util::serialization::to_vec(wire)?;
        self.effects
            .send_device_payload(device.uuid(), DEVICE_QUORUM_CONTENT_TYPE, bytes)
            .await
            .map_err(|source| AuraError::Network {
                message: "send device quorum signing message".into(),
                source: Some(Arc::new(source)),
            })
    }

    /// Wait for co-signer `peer`'s proven response at `sequence` of `request`.
    async fn await_quorum_response(
        &self,
        request: &TranscriptRoundPacket,
        sequence: u8,
        verifier: &[u8],
    ) -> Result<TranscriptRoundBody, AuraError> {
        let started = self.effects.physical_time().await?;
        let budget = aura_core::TimeoutBudget::from_start_and_timeout(
            &started,
            std::time::Duration::from_millis(RESPONSE_TIMEOUT_MS),
        )
        .map_err(AuraError::from)?;
        let envelope =
            aura_core::execute_with_timeout_budget(self.effects.as_ref(), &budget, || {
                self.effects.receive_original_device_quorum_response(
                    request.session,
                    request.participant,
                    sequence,
                )
            })
            .await
            .map_err(|source| match source {
                aura_core::TimeoutRunError::Timeout(
                    aura_core::TimeoutBudgetError::DeadlineExceeded { .. },
                ) => refused(DeviceQuorumError::TimedOut),
                aura_core::TimeoutRunError::Timeout(source) => AuraError::from(source),
                aura_core::TimeoutRunError::Operation(source) => source,
            })?;
        let Some(DeviceQuorumWire::Round(proven)) = decode_wire(&envelope) else {
            return Err(refused(DeviceQuorumError::Phase));
        };
        proven.verify_exact_participant(verifier)?;
        let packet = proven.packet;
        if packet.version != ROUND_VERSION
            || packet.approved_intent_digest != request.approved_intent_digest
            || packet.transcript_digest != request.transcript_digest
            || packet.coordinator != request.coordinator
        {
            return Err(refused(DeviceQuorumError::Phase));
        }
        match packet.body {
            TranscriptRoundBody::Cancel => Err(refused(DeviceQuorumError::Declined)),
            body => Ok(body),
        }
    }

    /// Produce this authority's threshold signature over `context` with
    /// FROST shares from this device and enough of the authority's other
    /// devices to reach the threshold. Each co-signer contributes only under
    /// its own consent policy, from its own retained share. A threshold-1
    /// authority signs locally.
    pub(crate) async fn sign_with_device_quorum(
        &self,
        context: SigningContext,
    ) -> Result<ThresholdSignature, AuraError> {
        if matches!(
            context.operation,
            SignableOperation::InvitationCode { .. } | SignableOperation::InvitationResponse { .. }
        ) {
            let policy = self.quorum_policy(&context.authority).await?;
            let source_device = match &context.operation {
                SignableOperation::InvitationCode { transport, .. } => transport.sender_device_id,
                SignableOperation::InvitationResponse { source_device, .. } => Some(*source_device),
                _ => None,
            };
            if policy.threshold < 2
                || !policy
                    .roster
                    .iter()
                    .any(|(device, _)| Some(*device) == source_device)
                || source_device != Some(self.effects.device_id())
            {
                return Err(refused(DeviceQuorumError::Binding));
            }
        }
        if self
            .threshold_config(&context.authority)
            .await
            .is_some_and(|config| config.threshold == 1)
        {
            return self.sign(context).await;
        }
        let policy = self.quorum_policy(&context.authority).await?;

        let me = self.effects.device_id();
        let message = self.signing_message(&context).await?;
        let transcript_digest = aura_core::hash::hash(&message);
        let remotes: Vec<(DeviceId, u16)> = policy
            .roster
            .iter()
            .copied()
            .filter(|(device, _)| *device != me)
            .take(usize::from(policy.threshold).saturating_sub(1))
            .collect();
        if remotes.len() + 1 < usize::from(policy.threshold) {
            return Err(refused(DeviceQuorumError::InsufficientRoster));
        }
        let local_share = self.local_quorum_share(&policy).await?;

        // Requests (with the first round packet) to every co-signer.
        let mut rounds = Vec::with_capacity(remotes.len());
        for (peer, index) in &remotes {
            let nonce = self.effects.random_bytes_32().await;
            let session = aura_core::hash::hash(&aura_core::util::serialization::to_vec(&(
                "aura.agent.device-quorum-session.v1",
                policy.authority,
                policy.epoch,
                me,
                *peer,
                nonce,
            ))?);
            let request = QuorumSigningRequest {
                version: REQUEST_VERSION,
                session,
                authority: policy.authority,
                epoch: policy.epoch,
                coordinator: me,
                participant: *peer,
                context: context.clone(),
            };
            let request_bytes = request.transcript_bytes().map_err(|source| {
                AuraError::crypto_with_source("encode device quorum request", Arc::new(source))
            })?;
            let possession_proof = self
                .effects
                .sign_participant_key_proof(&request_bytes, &local_share, SigningMode::Threshold)
                .await?;
            let first = TranscriptRoundPacket {
                version: ROUND_VERSION,
                session,
                approved_intent_digest: aura_core::hash::hash(&request_bytes),
                transcript_digest,
                coordinator: me,
                participant: *peer,
                sequence: 1,
                body: TranscriptRoundBody::RequestCommitment,
            };
            let first_round = Box::new(self.prove_round(&local_share, first.clone()).await?);
            self.send_quorum_wire(
                *peer,
                &DeviceQuorumWire::Request {
                    request,
                    possession_proof,
                    first_round,
                },
            )
            .await?;
            rounds.push((*peer, *index, first, policy.verifier(*index)?));
        }

        // Commitments: ours, then each co-signer's.

        let nonces = self.effects.frost_generate_nonces(&local_share).await?;
        let mut commitments = vec![nonces.public_commitment()];
        for (_, index, first, verifier) in &rounds {
            match self.await_quorum_response(first, 2, verifier).await? {
                TranscriptRoundBody::Commitment(commitment)
                    if commitment.participant_index == *index =>
                {
                    commitments.push(commitment);
                }
                _ => return Err(refused(DeviceQuorumError::Phase)),
            }
        }
        commitments.sort_by_key(|commitment: &FrostPublicCommitment| commitment.participant_index);
        let package: FrostSigningPackage = self
            .effects
            .frost_create_public_signing_package(
                &message,
                &commitments,
                &policy.public_package,
                policy.threshold,
            )
            .await?;

        // Shares.

        let mut share_requests = Vec::with_capacity(rounds.len());
        for (peer, _, first, _) in &rounds {
            let request = TranscriptRoundPacket {
                sequence: 3,
                body: TranscriptRoundBody::RequestShare(package.clone()),
                ..first.clone()
            };
            let proven = self.prove_round(&local_share, request.clone()).await?;
            self.send_quorum_wire(*peer, &DeviceQuorumWire::Round(proven))
                .await?;
            share_requests.push(request);
        }
        let mut shares = BTreeMap::new();
        let nonces = nonces
            .retire(&self.shared.device_quorum.nonce_retirement)
            .await?;
        shares.insert(
            policy.my_index,
            self.effects
                .frost_sign_share_for_message(
                    &package,
                    &local_share,
                    nonces,
                    &message,
                    &policy.public_package,
                    policy.threshold,
                )
                .await?,
        );
        for ((_, index, _, verifier), request) in rounds.iter().zip(&share_requests) {
            match self.await_quorum_response(request, 4, verifier).await? {
                TranscriptRoundBody::Share(share) => {
                    shares.insert(*index, share);
                }
                _ => return Err(refused(DeviceQuorumError::Phase)),
            }
        }
        let signers: Vec<u16> = shares.keys().copied().collect();
        let shares: Vec<Vec<u8>> = shares.into_values().collect();
        let signature = self
            .effects
            .frost_aggregate_signatures(&package, &shares)
            .await?;
        if !self
            .effects
            .frost_verify(&message, &signature, &policy.group_key())
            .await?
        {
            return Err(refused(DeviceQuorumError::Aggregate));
        }
        Ok(ThresholdSignature::new(
            signature,
            u16::try_from(signers.len()).map_err(|_| refused(DeviceQuorumError::Phase))?,
            signers,
            policy.public_package,
            policy.epoch,
        ))
    }

    /// Admit inbound quorum requests and round packets addressed to this
    /// device as a co-signer. Called from the runtime's participant loop.
    pub(crate) async fn process_device_quorum_inbound(&self) -> Result<usize, AuraError> {
        let mut processed = 0usize;
        let me = self.effects.device_id();
        // Only coordinator-to-co-signer traffic: requests and odd-sequence
        // round packets naming this device. Responses stay queued for the
        // coordinator waiting on them.
        let addressed_to_me = |envelope: &TransportEnvelope| {
            is_quorum_envelope(envelope)
                && match decode_wire(envelope) {
                    Some(DeviceQuorumWire::Request { request, .. }) => request.participant == me,
                    Some(DeviceQuorumWire::Round(proven)) => {
                        proven.packet.participant == me && proven.packet.sequence % 2 == 1
                    }
                    None => true,
                }
        };
        while let Ok(lease) = self.effects.take_inbound_envelope_retained(addressed_to_me) {
            processed += 1;
            let source = lease.source;
            let Some(wire) = decode_wire(&lease) else {
                let _ = lease.consume();
                tracing::warn!(%source, "dropping undecodable device quorum message");
                continue;
            };
            let mut intake = Some(lease);
            #[cfg(test)]
            if let DeviceQuorumWire::Request { request, .. } = &wire {
                let signal = request_intake_test_signals()
                    .lock()
                    .expect("quorum intake test signals lock")
                    .remove(&request.session);
                if let Some(signal) = signal {
                    let _ = signal.send(());
                }
            }
            let outcome = match wire {
                DeviceQuorumWire::Request {
                    request,
                    possession_proof,
                    first_round,
                } => {
                    self.admit_quorum_request(
                        source,
                        request,
                        &possession_proof,
                        first_round,
                        &mut intake,
                    )
                    .await
                }
                DeviceQuorumWire::Round(proven) => {
                    #[cfg(test)]
                    if let Some(signal) = round_intake_test_signals()
                        .lock()
                        .expect("quorum round intake test signals lock")
                        .remove(&proven.packet.session)
                    {
                        let _ = signal.send(());
                    }
                    self.admit_quorum_round(proven, &mut intake).await
                }
            };
            // A returned refusal is definitive. Cancellation before the
            // request's slot insertion drops this lease back into its exact
            // admitted position instead of losing it.
            if let Some(lease) = intake.take() {
                let _ = lease.consume();
            }
            if let Err(error) = outcome {
                tracing::warn!(%source, %error, "device quorum message refused");
            }
        }
        Ok(processed)
    }

    async fn admit_quorum_request(
        &self,
        source: AuthorityId,
        request: QuorumSigningRequest,
        possession_proof: &[u8],
        first_round: Box<ParticipantProvenRoundPacket>,
        intake: &mut Option<crate::runtime::effects::InboundEnvelopeLease<'_>>,
    ) -> Result<(), AuraError> {
        let me = self.effects.device_id();
        if request.version != REQUEST_VERSION
            || source != request.authority
            || request.context.authority != request.authority
            || request.participant != me
            || request.coordinator == me
        {
            return Err(refused(DeviceQuorumError::Binding));
        }
        let policy = self.quorum_policy(&request.authority).await?;
        let invitation_source_device = match &request.context.operation {
            SignableOperation::InvitationCode { transport, .. } => transport.sender_device_id,
            SignableOperation::InvitationResponse { source_device, .. } => Some(*source_device),
            _ => None,
        };
        if matches!(
            &request.context.operation,
            SignableOperation::InvitationCode { .. } | SignableOperation::InvitationResponse { .. }
        ) && (policy.threshold < 2 || invitation_source_device != Some(request.coordinator))
        {
            return Err(refused(DeviceQuorumError::Binding));
        }
        if policy.epoch != request.epoch {
            return Err(refused(DeviceQuorumError::Binding));
        }
        let coordinator_index = policy
            .index_of(request.coordinator)
            .ok_or_else(|| refused(DeviceQuorumError::Binding))?;
        let coordinator_verifier = policy.verifier(coordinator_index)?;
        let request_bytes = request.transcript_bytes().map_err(|source| {
            AuraError::crypto_with_source("encode device quorum request", Arc::new(source))
        })?;
        aura_core::crypto::participant_proof::verify_participant_key_proof(
            &request_bytes,
            possession_proof,
            &coordinator_verifier,
            SigningMode::Threshold,
        )?;
        let message = self.signing_message(&request.context).await?;
        let transcript_digest = aura_core::hash::hash(&message);
        let approved_intent_digest = transcript_digest_of(&request)?;
        let consent = self.device_signing_consent().await?;
        let mut slots = self.shared.device_quorum.slots.lock().await;
        if slots.contains_key(&request.session) {
            return Err(refused(DeviceQuorumError::Phase));
        }
        if slots.len() >= MAX_SLOTS {
            return Err(refused(DeviceQuorumError::Capacity));
        }
        let _ = intake
            .take()
            .expect("validated quorum request retains intake")
            .consume();
        let validator = ParticipantRoundSession::new(
            request.session,
            approved_intent_digest,
            transcript_digest,
            request.coordinator,
            me,
        );
        slots.insert(
            request.session,
            ParticipantSlot {
                request,
                message,
                coordinator_verifier,
                approved_intent_digest,
                transcript_digest,
                validator,
                approved: consent == DeviceSigningConsent::AutoSignVerified,
                queued: VecDeque::new(),
                nonces: None,
                commitment: None,
                proposal_consent: None,
                proposal_consent_handed_off: false,
                proposal_consent_failure: None,
                proposal_policy_origin: (consent == DeviceSigningConsent::AutoSignVerified)
                    .then_some(OriginalProposalConsentOrigin::AutoSignVerifiedPolicy),
            },
        );
        drop(slots);
        // The first round rides with the request; it waits behind consent.
        self.admit_quorum_round(*first_round, &mut None).await
    }

    async fn admit_quorum_round(
        &self,
        proven: ParticipantProvenRoundPacket,
        intake: &mut Option<crate::runtime::effects::InboundEnvelopeLease<'_>>,
    ) -> Result<(), AuraError> {
        let session = proven.packet.session;
        let mut slots = self.shared.device_quorum.slots.lock().await;
        let Some(slot) = slots.get_mut(&session) else {
            return Err(refused(DeviceQuorumError::Phase));
        };
        if !slot.approved {
            if slot.queued.len() >= MAX_QUEUED {
                return Err(refused(DeviceQuorumError::Capacity));
            }
            if let Some(lease) = intake.take() {
                let _ = lease.consume();
            }
            slot.queued.push_back(proven);
            return Ok(());
        }
        // The slot lock is the last cancellation-safe await. Proposal custody,
        // validator sequence and FROST nonces may change after this point;
        // replaying the original packet would reuse cryptographic state.
        if let Some(lease) = intake.take() {
            let _ = lease.consume();
        }
        if let Some(origin) = slot.proposal_policy_origin {
            self.retain_original_proposal_decision(slot, origin).await?;
        }
        let finished = match self.co_sign_round(slot, proven).await {
            Ok(finished) => finished,
            Err(error) => {
                slots.remove(&session);
                return Err(error);
            }
        };
        if finished {
            slots.remove(&session);
        }
        Ok(())
    }

    /// Answer one admitted round packet; returns whether the round finished.
    async fn co_sign_round(
        &self,
        slot: &mut ParticipantSlot,
        proven: ParticipantProvenRoundPacket,
    ) -> Result<bool, AuraError> {
        let body = slot.validator.admit(proven, &slot.coordinator_verifier)?;
        let policy = self.quorum_policy(&slot.request.authority).await?;
        if policy.epoch != slot.request.epoch {
            return Err(refused(DeviceQuorumError::Binding));
        }
        let local_share = self.local_quorum_share(&policy).await?;
        let (sequence, response, finished) = match body {
            TranscriptRoundBody::RequestCommitment => {
                let nonces = self.effects.frost_generate_nonces(&local_share).await?;
                let commitment = nonces.public_commitment();
                slot.commitment = Some(commitment.commitment_bytes.clone());
                slot.nonces = Some(nonces);
                (2, TranscriptRoundBody::Commitment(commitment), false)
            }
            TranscriptRoundBody::RequestShare(package) => {
                if package.message != slot.message
                    || package.public_key_package != policy.public_package
                    || !package.participants.contains(&policy.my_index)
                    || !super::enrollment_transcript_signing::native_commitment_matches(
                        &package,
                        policy.my_index,
                        slot.commitment.as_ref(),
                    )?
                {
                    return Err(refused(DeviceQuorumError::Phase));
                }
                // The nonce leaves the slot before signing: one share per nonce.
                let nonces = slot
                    .nonces
                    .take()
                    .ok_or_else(|| refused(DeviceQuorumError::Phase))?
                    .retire(&self.shared.device_quorum.nonce_retirement)
                    .await?;
                let share = self
                    .effects
                    .frost_sign_share_for_message(
                        &package,
                        &local_share,
                        nonces,
                        &slot.message,
                        &policy.public_package,
                        policy.threshold,
                    )
                    .await?;
                (4, TranscriptRoundBody::Share(share), true)
            }
            TranscriptRoundBody::Cancel => return Ok(true),
            _ => return Err(refused(DeviceQuorumError::Phase)),
        };
        let packet = TranscriptRoundPacket {
            version: ROUND_VERSION,
            session: slot.request.session,
            approved_intent_digest: slot.approved_intent_digest,
            transcript_digest: slot.transcript_digest,
            coordinator: slot.request.coordinator,
            participant: slot.request.participant,
            sequence,
            body: response,
        };
        let proven = self.prove_round(&local_share, packet).await?;
        self.send_quorum_wire(slot.request.coordinator, &DeviceQuorumWire::Round(proven))
            .await?;
        Ok(finished)
    }

    /// Prove that this device holds its current share of `authority`, over
    /// `message` (a participant-device proof, e.g. a rotation acceptance).
    pub(crate) async fn prove_device_possession(
        &self,
        authority: &AuthorityId,
        message: &[u8],
    ) -> Result<DevicePossessionProof, AuraError> {
        let policy = self.quorum_policy(authority).await?;
        let local_share = self.local_quorum_share(&policy).await?;
        let proof = self
            .effects
            .sign_participant_key_proof(message, &local_share, SigningMode::Threshold)
            .await?;
        Ok(DevicePossessionProof {
            epoch: policy.epoch,
            index: policy.my_index,
            package_digest: Hash32::from_bytes(&policy.public_package),
            proof,
        })
    }

    /// Verify `device`'s possession proof over `message` against its
    /// verifying share in this device's own retained current package; the
    /// claimed epoch, index and package digest must match that package.
    pub(crate) async fn verify_device_possession(
        &self,
        authority: &AuthorityId,
        device: DeviceId,
        claimed: &DevicePossessionProof,
        message: &[u8],
    ) -> Result<(), AuraError> {
        let policy = self.quorum_policy(authority).await?;
        if policy.epoch != claimed.epoch
            || policy.index_of(device) != Some(claimed.index)
            || Hash32::from_bytes(&policy.public_package) != claimed.package_digest
        {
            return Err(refused(DeviceQuorumError::Binding));
        }
        aura_core::crypto::participant_proof::verify_participant_key_proof(
            message,
            &claimed.proof,
            &policy.verifier(claimed.index)?,
            SigningMode::Threshold,
        )
    }

    /// Store this device's share of a rotated (pending) epoch received in a
    /// verified rotation proposal, wrapped like every other participant share.
    pub(crate) async fn stage_rotated_device_share(
        &self,
        authority: &AuthorityId,
        pending_epoch: u64,
        key_package: &[u8],
    ) -> Result<(), AuraError> {
        let participant = ParticipantIdentity::device(self.effects.device_id());
        let location = Self::participant_share_location(authority, pending_epoch, &participant);
        self.store_participant_key_package(
            authority,
            pending_epoch,
            &participant,
            &location,
            key_package,
        )
        .await
    }

    /// Quorum signing requests waiting for the user's decision on this device.
    pub(crate) async fn pending_signing_requests(&self) -> Vec<PendingSigningRequest> {
        self.shared
            .device_quorum
            .slots
            .lock()
            .await
            .iter()
            .filter(|(_, slot)| !slot.approved)
            .map(|(session, slot)| PendingSigningRequest {
                id: hex::encode(session),
                requesting_device: slot.request.coordinator,
                operation: describe(&slot.request.context),
            })
            .collect()
    }

    /// The user's decision on a pending request: approval answers the rounds
    /// queued behind it; a decline tells the coordinator and drops the round.
    pub(crate) async fn decide_pending_signing_request(
        &self,
        request_id: &str,
        approve: bool,
    ) -> Result<(), AuraError> {
        let session: [u8; 32] = hex::decode(request_id)
            .ok()
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or_else(|| refused(DeviceQuorumError::UnknownRequest))?;
        let mut slots = self.shared.device_quorum.slots.lock().await;
        if !approve {
            let Some(slot) = slots.remove(&session).filter(|slot| !slot.approved) else {
                return Err(refused(DeviceQuorumError::UnknownRequest));
            };
            let policy = self.quorum_policy(&slot.request.authority).await?;
            let local_share = self.local_quorum_share(&policy).await?;
            let packet = TranscriptRoundPacket {
                version: ROUND_VERSION,
                session,
                approved_intent_digest: slot.approved_intent_digest,
                transcript_digest: slot.transcript_digest,
                coordinator: slot.request.coordinator,
                participant: slot.request.participant,
                sequence: 2,
                body: TranscriptRoundBody::Cancel,
            };
            let proven = self.prove_round(&local_share, packet).await?;
            return self
                .send_quorum_wire(slot.request.coordinator, &DeviceQuorumWire::Round(proven))
                .await;
        }
        let Some(slot) = slots.get_mut(&session).filter(|slot| !slot.approved) else {
            return Err(refused(DeviceQuorumError::UnknownRequest));
        };
        self.retain_original_proposal_decision(
            slot,
            OriginalProposalConsentOrigin::ExplicitDecision,
        )
        .await?;
        slot.approved = true;
        let mut finished = false;
        while let Some(queued) = slot.queued.pop_front() {
            finished = self.co_sign_round(slot, queued).await?;
        }
        if finished {
            slots.remove(&session);
        }
        Ok(())
    }

    async fn retain_original_proposal_decision(
        &self,
        slot: &mut ParticipantSlot,
        origin: OriginalProposalConsentOrigin,
    ) -> Result<(), AuraError> {
        if let Some(source) = slot.proposal_consent_failure.as_ref() {
            return Err(source.clone());
        }
        if slot.proposal_consent_handed_off {
            return Ok(());
        }
        if !matches!(&slot.request.context.operation, SignableOperation::Message { domain, .. }
            if domain == aura_sync::protocols::device_epoch_rotation::DEVICE_EPOCH_PROPOSAL_SIGNING_DOMAIN)
        {
            return Ok(());
        }
        let scope = aura_sync::protocols::device_epoch_rotation::decode_device_epoch_proposal_signing_context(&slot.request.context)?;
        if !matches!(
            scope.purpose,
            aura_sync::protocols::device_epoch_rotation::DeviceEpochProposalPurpose::Enrollment { .. }
        ) {
            return Ok(());
        }
        if slot.proposal_consent.is_none() {
            slot.proposal_consent = Some(OriginalDeviceProposalConsent {
                effects: Arc::downgrade(&self.effects),
                local_device: self.effects.device_id(),
                coordinator: slot.request.coordinator,
                _session: slot.request.session,
                context: slot.request.context.clone(),
                coordinator_verifier: slot.coordinator_verifier.clone(),
                _origin: origin,
            });
        }
        let result = self
            .retain_original_enrollment_proposal_consent(&scope, &mut slot.proposal_consent)
            .await;
        // Only the validated synchronous transfer empties this actual slot.
        // Preserve that fact even if the original executor postcheck fails.
        slot.proposal_consent_handed_off = slot.proposal_consent.is_none();
        if let Err(source) = &result {
            slot.proposal_consent_failure
                .get_or_insert_with(|| source.clone());
        }
        result
    }
}

/// Coordinator state retained by the exact issued enrollment custodian.
/// Every physical request/nonce/consumed response is deposited before Ready.
pub(super) struct OriginalDeviceQuorumSigningSession {
    context: SigningContext,
    policy: Option<QuorumPolicy>,
    message: Option<Vec<u8>>,
    local_share: Option<Zeroizing<Vec<u8>>>,
    rounds: Vec<OriginalDeviceQuorumRound>,
    roster_prepared: bool,
    nonces: Option<aura_core::effects::crypto::FrostNonces>,
    nonce_birth_started: bool,
    nonce_consumption_started: bool,
    package: Option<FrostSigningPackage>,
    local_signature_share: Option<Vec<u8>>,
    signature: Option<ThresholdSignature>,
    first_failure: Option<AuraError>,
}
struct OriginalDeviceQuorumRound {
    peer: DeviceId,
    index: u16,
    request: QuorumSigningRequest,
    first: TranscriptRoundPacket,
    verifier: Vec<u8>,
    initial_wire: Option<DeviceQuorumWire>,
    initial_started: bool,
    initial_completed: bool,
    commitment_reply: Option<ParticipantProvenRoundPacket>,
    share_wire: Option<DeviceQuorumWire>,
    share_started: bool,
    share_completed: bool,
    share_reply: Option<ParticipantProvenRoundPacket>,
}
impl OriginalDeviceQuorumSigningSession {
    pub(super) fn take_original_positive_terminal(
        &mut self,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        child: &super::super::enrollment_window::EnrollmentExecutionChild,
        receipt: &crate::runtime::services::ceremony_tracker::OriginalEnrollmentPositiveDecisionReceipt,
    ) -> Result<super::OriginalPositiveTerminalSigningMaterial, AuraError> {
        receipt.require_original_control(issued)?;
        let fence = receipt
            .committed_ops()
            .last()
            .ok_or_else(|| AuraError::invalid("actual committed fence absent"))?;
        if !matches!(fence.op.op, aura_core::TreeOpKind::RotateEpoch { .. }) {
            return Err(AuraError::permission_denied(
                "positive terminal requires actual epoch fence",
            ));
        }
        self.require_context(&SigningContext::self_tree_op(
            approval.manifest().subject,
            fence.op.clone(),
        ))?;
        if let Some(source) = &self.first_failure {
            return Err(source.clone());
        }
        let signature = self
            .signature
            .as_ref()
            .ok_or_else(|| AuraError::invalid("original fence signing incomplete"))?;
        let policy = self
            .policy
            .as_ref()
            .ok_or_else(|| AuraError::invalid("original fence native policy absent"))?;
        let original_window = child.retain_original_conditional_identity(approval)?;
        let original = receipt.original_owner_for_control(issued)?;
        let parent = approval.manifest().final_inventory().map_err(|source| {
            AuraError::crypto_with_source("original positive policy", Arc::new(source))
        })?;
        if signature.signature != fence.agg_sig
            || signature.signer_count != fence.signer_count
            || policy.authority != approval.manifest().subject
            || policy.epoch != approval.manifest().final_epoch
            || parent.len() != 1
            || parent[0].public_key_package != policy.public_package
        {
            return Err(AuraError::permission_denied(
                "positive terminal differs from actual original signed fence",
            ));
        }
        let local_share = self.local_share.take().ok_or_else(|| {
            AuraError::permission_denied("original positive share already transferred")
        })?;
        Ok(super::OriginalPositiveTerminalSigningMaterial {
            effects: Arc::clone(approval.effects()),
            approved_intent_digest: approval.canonical_intent_digest(),
            device: approval.effects().device_id(),
            index: policy.my_index,
            threshold: policy.threshold,
            ordered_devices: policy.roster.iter().map(|(device, _)| *device).collect(),
            public_package: policy.public_package.clone(),
            local_share,
            original_window,
            origin: super::OriginalPositiveTerminalOrigin::Issuer {
                original,
                decision_digest: receipt.decision_digest(),
            },
        })
    }
    pub(super) fn new(context: SigningContext) -> Self {
        Self {
            context,
            policy: None,
            message: None,
            local_share: None,
            rounds: Vec::new(),
            roster_prepared: false,
            nonces: None,
            nonce_birth_started: false,
            nonce_consumption_started: false,
            package: None,
            local_signature_share: None,
            signature: None,
            first_failure: None,
        }
    }
    pub(super) fn require_context(&self, context: &SigningContext) -> Result<(), AuraError> {
        if aura_core::util::serialization::to_vec(&self.context)?
            != aura_core::util::serialization::to_vec(context)?
        {
            return Err(refused(DeviceQuorumError::Binding));
        }
        Ok(())
    }
    pub(super) fn dispose_local_secrets(&mut self) {
        self.local_share = None;
        self.nonces = None;
    }
    pub(super) async fn run(
        &mut self,
        service: &ThresholdSigningService,
    ) -> Result<ThresholdSignature, AuraError> {
        if let Some(source) = &self.first_failure {
            return Err(source.clone());
        }
        if let Some(signature) = &self.signature {
            return Ok(signature.clone());
        }
        let result = self.run_original(service).await;
        if let Err(source) = &result {
            self.first_failure = Some(source.clone());
        }
        result
    }
    async fn run_original(
        &mut self,
        service: &ThresholdSigningService,
    ) -> Result<ThresholdSignature, AuraError> {
        if self.policy.is_none() {
            self.policy = Some(service.quorum_policy(&self.context.authority).await?);
        }
        if self.message.is_none() {
            self.message = Some(service.signing_message(&self.context).await?);
        }
        let policy = self
            .policy
            .as_ref()
            .ok_or_else(|| refused(DeviceQuorumError::Binding))?;
        if self.local_share.is_none() {
            self.local_share = Some(service.local_quorum_share(policy).await?);
        }
        let local_share = self
            .local_share
            .as_ref()
            .ok_or_else(|| refused(DeviceQuorumError::Binding))?;
        let message = self
            .message
            .as_ref()
            .ok_or_else(|| refused(DeviceQuorumError::Binding))?;
        if !self.roster_prepared {
            let remotes: Vec<_> = policy
                .roster
                .iter()
                .copied()
                .filter(|(device, _)| *device != service.effects.device_id())
                .take(usize::from(policy.threshold).saturating_sub(1))
                .collect();
            if remotes.len() + 1 < usize::from(policy.threshold) {
                return Err(refused(DeviceQuorumError::InsufficientRoster));
            }
            for (peer, index) in remotes {
                if self
                    .rounds
                    .iter()
                    .any(|round| round.peer == peer && round.index == index)
                {
                    continue;
                }
                // Randomness has no published authority until this original request is retained.
                let nonce = service.effects.random_bytes_32().await;
                let session = aura_core::hash::hash(&aura_core::util::serialization::to_vec(&(
                    "aura.agent.device-quorum-session.v1",
                    policy.authority,
                    policy.epoch,
                    service.effects.device_id(),
                    peer,
                    nonce,
                ))?);
                let request = QuorumSigningRequest {
                    version: REQUEST_VERSION,
                    session,
                    authority: policy.authority,
                    epoch: policy.epoch,
                    coordinator: service.effects.device_id(),
                    participant: peer,
                    context: self.context.clone(),
                };
                let bytes = request.transcript_bytes().map_err(|source| {
                    AuraError::crypto_with_source(
                        "encode retained original quorum request",
                        Arc::new(source),
                    )
                })?;
                let first = TranscriptRoundPacket {
                    version: ROUND_VERSION,
                    session,
                    approved_intent_digest: aura_core::hash::hash(&bytes),
                    transcript_digest: aura_core::hash::hash(message),
                    coordinator: service.effects.device_id(),
                    participant: peer,
                    sequence: 1,
                    body: TranscriptRoundBody::RequestCommitment,
                };
                self.rounds.push(OriginalDeviceQuorumRound {
                    peer,
                    index,
                    request,
                    first,
                    verifier: policy.verifier(index)?,
                    initial_wire: None,
                    initial_started: false,
                    initial_completed: false,
                    commitment_reply: None,
                    share_wire: None,
                    share_started: false,
                    share_completed: false,
                    share_reply: None,
                });
            }
            self.roster_prepared = true;
        }
        for round in &mut self.rounds {
            if round.initial_wire.is_none() {
                let bytes = round.request.transcript_bytes().map_err(|source| {
                    AuraError::crypto_with_source(
                        "encode retained original quorum request",
                        Arc::new(source),
                    )
                })?;
                let possession_proof = service
                    .effects
                    .sign_participant_key_proof(&bytes, local_share, SigningMode::Threshold)
                    .await?;
                let first_round = Box::new(
                    service
                        .prove_round(local_share, round.first.clone())
                        .await?,
                );
                round.initial_wire = Some(DeviceQuorumWire::Request {
                    request: round.request.clone(),
                    possession_proof,
                    first_round,
                });
            }
            if !round.initial_completed {
                if round.initial_started {
                    return Err(AuraError::permission_denied(
                        "original quorum emission remains indeterminate",
                    ));
                }
                round.initial_started = true;
                service
                    .send_quorum_wire(
                        round.peer,
                        round
                            .initial_wire
                            .as_ref()
                            .ok_or_else(|| refused(DeviceQuorumError::Phase))?,
                    )
                    .await?;
                round.initial_completed = true;
            }
        }
        if self.nonces.is_none() && !self.nonce_consumption_started {
            if self.nonce_birth_started {
                return Err(AuraError::permission_denied(
                    "original quorum nonce birth remains unresolved",
                ));
            }
            self.nonce_birth_started = true;
            self.nonces = Some(service.effects.frost_generate_nonces(local_share).await?);
        }
        for round in &mut self.rounds {
            if round.commitment_reply.is_none() {
                round.commitment_reply = Some(
                    service
                        .receive_original_quorum_response(&round.first, 2, &round.verifier)
                        .await?,
                );
            }
        }
        if self.package.is_none() {
            let mut commitments = vec![self
                .nonces
                .as_ref()
                .ok_or_else(|| refused(DeviceQuorumError::Phase))?
                .public_commitment()];
            for round in &self.rounds {
                match &round
                    .commitment_reply
                    .as_ref()
                    .ok_or_else(|| refused(DeviceQuorumError::Phase))?
                    .packet
                    .body
                {
                    TranscriptRoundBody::Commitment(commitment)
                        if commitment.participant_index == round.index =>
                    {
                        commitments.push(commitment.clone());
                    }
                    _ => return Err(refused(DeviceQuorumError::Phase)),
                }
            }
            commitments.sort_by_key(|commitment| commitment.participant_index);
            self.package = Some(
                service
                    .effects
                    .frost_create_public_signing_package(
                        message,
                        &commitments,
                        &policy.public_package,
                        policy.threshold,
                    )
                    .await?,
            );
        }
        let package = self
            .package
            .as_ref()
            .ok_or_else(|| refused(DeviceQuorumError::Phase))?;
        for round in &mut self.rounds {
            if round.share_wire.is_none() {
                let packet = TranscriptRoundPacket {
                    sequence: 3,
                    body: TranscriptRoundBody::RequestShare(package.clone()),
                    ..round.first.clone()
                };
                round.share_wire = Some(DeviceQuorumWire::Round(
                    service.prove_round(local_share, packet).await?,
                ));
            }
            if !round.share_completed {
                if round.share_started {
                    return Err(AuraError::permission_denied(
                        "original quorum share emission remains indeterminate",
                    ));
                }
                round.share_started = true;
                service
                    .send_quorum_wire(
                        round.peer,
                        round
                            .share_wire
                            .as_ref()
                            .ok_or_else(|| refused(DeviceQuorumError::Phase))?,
                    )
                    .await?;
                round.share_completed = true;
            }
        }
        if self.local_signature_share.is_none() {
            if self.nonce_consumption_started {
                return Err(AuraError::permission_denied(
                    "original quorum nonce consumption remains unresolved",
                ));
            }
            self.nonce_consumption_started = true;
            let nonces = self
                .nonces
                .take()
                .ok_or_else(|| refused(DeviceQuorumError::Phase))?
                .retire(&service.shared.device_quorum.nonce_retirement)
                .await?;
            self.local_signature_share = Some(
                service
                    .effects
                    .frost_sign_share_for_message(
                        package,
                        local_share,
                        nonces,
                        message,
                        &policy.public_package,
                        policy.threshold,
                    )
                    .await?,
            );
        }
        for round in &mut self.rounds {
            if round.share_reply.is_none() {
                round.share_reply = Some(
                    service
                        .receive_original_quorum_response(&round.first, 4, &round.verifier)
                        .await?,
                );
            }
        }
        let mut shares = BTreeMap::new();
        shares.insert(
            policy.my_index,
            self.local_signature_share
                .as_ref()
                .ok_or_else(|| refused(DeviceQuorumError::Phase))?
                .clone(),
        );
        for round in &self.rounds {
            match &round
                .share_reply
                .as_ref()
                .ok_or_else(|| refused(DeviceQuorumError::Phase))?
                .packet
                .body
            {
                TranscriptRoundBody::Share(share) => {
                    shares.insert(round.index, share.clone());
                }
                _ => return Err(refused(DeviceQuorumError::Phase)),
            }
        }
        let signers: Vec<_> = shares.keys().copied().collect();
        let values: Vec<_> = shares.into_values().collect();
        let signature = service
            .effects
            .frost_aggregate_signatures(package, &values)
            .await?;
        if !service
            .effects
            .frost_verify(message, &signature, &policy.group_key())
            .await?
        {
            return Err(refused(DeviceQuorumError::Aggregate));
        }
        let signature = ThresholdSignature::new(
            signature,
            u16::try_from(signers.len()).map_err(|_| refused(DeviceQuorumError::Phase))?,
            signers,
            policy.public_package.clone(),
            policy.epoch,
        );
        self.signature = Some(signature.clone());
        Ok(signature)
    }
}
impl ThresholdSigningService {
    async fn receive_original_quorum_response(
        &self,
        request: &TranscriptRoundPacket,
        sequence: u8,
        verifier: &[u8],
    ) -> Result<ParticipantProvenRoundPacket, AuraError> {
        let envelope = self
            .effects
            .receive_original_device_quorum_response(request.session, request.participant, sequence)
            .await?;
        let Some(DeviceQuorumWire::Round(proven)) = decode_wire(&envelope) else {
            return Err(refused(DeviceQuorumError::Phase));
        };
        proven.verify_exact_participant(verifier)?;
        if proven.packet.version != ROUND_VERSION
            || proven.packet.approved_intent_digest != request.approved_intent_digest
            || proven.packet.transcript_digest != request.transcript_digest
            || proven.packet.coordinator != request.coordinator
        {
            return Err(refused(DeviceQuorumError::Phase));
        }
        if matches!(proven.packet.body, TranscriptRoundBody::Cancel) {
            return Err(refused(DeviceQuorumError::Declined));
        }
        Ok(proven)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    fn response_wait_fixture(
        label: &str,
    ) -> (
        ThresholdSigningService,
        crate::SharedTransport,
        aura_testkit::time::ManualPhysicalClock,
        AuthorityId,
        DeviceId,
    ) {
        let shared = crate::SharedTransport::new();
        let clock = aura_testkit::time::ManualPhysicalClock::new(5_000);
        let authority = AuthorityId::new_from_entropy([221; 32]);
        let config = crate::core::AgentConfig {
            device_id: DeviceId::new_from_entropy([222; 32]),
            ..Default::default()
        };
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_named_test_with_shared_transport_for_authority(
                &config,
                label,
                authority,
                shared.clone(),
            )
            .expect("isolated quorum response runtime")
            .with_physical_time_provider(Arc::new(clock.clone())),
        );
        (
            ThresholdSigningService::new(effects),
            shared,
            clock,
            authority,
            config.device_id,
        )
    }

    fn first_response_packet(
        coordinator: DeviceId,
        participant: DeviceId,
    ) -> TranscriptRoundPacket {
        TranscriptRoundPacket {
            version: ROUND_VERSION,
            session: [223; 32],
            approved_intent_digest: [224; 32],
            transcript_digest: [225; 32],
            coordinator,
            participant,
            sequence: 1,
            body: TranscriptRoundBody::RequestCommitment,
        }
    }

    #[tokio::test]
    async fn exact_quorum_cancel_wakes_without_advancing_selected_clock() {
        use aura_core::effects::CryptoExtendedEffects;
        let (service, shared, clock, authority, coordinator) =
            response_wait_fixture("quorum-event-cancel");
        let participant = DeviceId::new_from_entropy([226; 32]);
        let first = first_response_packet(coordinator, participant);
        let generated = service.effects.frost_generate_keys(2, 2).await.unwrap();
        let native =
            frost_ed25519::keys::PublicKeyPackage::deserialize(&generated.public_key_package)
                .unwrap();
        let identifier = frost_ed25519::Identifier::try_from(2_u16).unwrap();
        let verifier = native
            .verifying_shares()
            .get(&identifier)
            .unwrap()
            .serialize();
        let cancel = TranscriptRoundPacket {
            sequence: 2,
            body: TranscriptRoundBody::Cancel,
            ..first.clone()
        };
        let proof = service
            .effects
            .sign_participant_key_proof(
                &cancel.transcript_bytes().unwrap(),
                &generated.key_packages[1],
                SigningMode::Threshold,
            )
            .await
            .unwrap();
        let wire = DeviceQuorumWire::Round(ParticipantProvenRoundPacket {
            packet: cancel,
            possession_proof: proof,
        });
        let waiting = service.await_quorum_response(&first, 2, &verifier);
        tokio::pin!(waiting);
        assert!(futures::poll!(waiting.as_mut()).is_pending());
        let mut metadata = std::collections::HashMap::new();
        metadata.insert(
            "content-type".to_owned(),
            DEVICE_QUORUM_CONTENT_TYPE.to_owned(),
        );
        metadata.insert(
            "aura-destination-device-id".to_owned(),
            coordinator.to_string(),
        );
        shared.route_envelope(TransportEnvelope {
            destination: authority,
            source: authority,
            context: crate::core::default_context_id_for_authority(authority),
            payload: to_vec(&wire).unwrap(),
            metadata,
            receipt: None,
        });
        let error = waiting
            .await
            .expect_err("verified cancellation refuses the quorum");
        assert!(matches!(error, AuraError::PermissionDenied { .. }));
        assert_eq!(
            clock.now_ms(),
            5_000,
            "inbox arrival does not advance physical time"
        );
    }

    #[tokio::test]
    async fn exact_quorum_response_wait_expires_on_selected_deadline() {
        let (service, _shared, clock, _authority, coordinator) =
            response_wait_fixture("quorum-event-expiry");
        let first = first_response_packet(coordinator, DeviceId::new_from_entropy([227; 32]));
        let waiting = service.await_quorum_response(&first, 2, &[0; 32]);
        tokio::pin!(waiting);
        assert!(futures::poll!(waiting.as_mut()).is_pending());
        clock.advance(RESPONSE_TIMEOUT_MS);
        let error = waiting
            .await
            .expect_err("selected deadline expires without a response");
        assert!(matches!(error, AuraError::PermissionDenied { .. }));
    }

    #[tokio::test]
    async fn exact_quorum_response_wait_preserves_selected_provider_failure() {
        let (service, _shared, clock, _authority, coordinator) =
            response_wait_fixture("quorum-event-provider-error");
        let first = first_response_packet(coordinator, DeviceId::new_from_entropy([228; 32]));
        clock
            .fail_next_sleep(aura_core::effects::TimeError::OperationFailed {
                reason: "selected quorum clock failed".into(),
            })
            .await;
        let error = service
            .await_quorum_response(&first, 2, &[0; 32])
            .await
            .expect_err("selected provider failure must not become a timeout");
        assert!(format!("{error:?}").contains("selected quorum clock failed"));
    }

    /// Deliver a request to `participant` whose possession proof was made
    /// over `signed` with `share`, while the delivered request is
    /// `delivered`.
    async fn deliver_forged(
        coordinator: &ThresholdSigningService,
        signed: &QuorumSigningRequest,
        delivered: QuorumSigningRequest,
        share: &[u8],
    ) {
        let bytes = signed.transcript_bytes().unwrap();
        let possession_proof = coordinator
            .effects
            .sign_participant_key_proof(&bytes, share, SigningMode::Threshold)
            .await
            .unwrap();
        let first = TranscriptRoundPacket {
            version: ROUND_VERSION,
            session: delivered.session,
            approved_intent_digest: aura_core::hash::hash(&bytes),
            transcript_digest: [0; 32],
            coordinator: delivered.coordinator,
            participant: delivered.participant,
            sequence: 1,
            body: TranscriptRoundBody::RequestCommitment,
        };
        let first_round = Box::new(coordinator.prove_round(share, first).await.unwrap());
        let participant = delivered.participant;
        coordinator
            .send_quorum_wire(
                participant,
                &DeviceQuorumWire::Request {
                    request: delivered,
                    possession_proof,
                    first_round,
                },
            )
            .await
            .unwrap();
    }

    #[test]
    fn cancelled_unapproved_quorum_request_before_policy_read_restores_original_frame() {
        crate::handlers::invitation::tests::run_async_test_on_large_stack(async {
            let (service, _transport, _clock, authority, participant) =
                response_wait_fixture("quorum-retained-unapproved-intake-cancel");
            let coordinator = DeviceId::new_from_entropy([226; 32]);
            let request = QuorumSigningRequest {
                version: REQUEST_VERSION,
                session: aura_core::hash::hash(b"quorum-retained-unapproved-intake-cancel"),
                authority,
                epoch: 0,
                coordinator,
                participant,
                context: SigningContext::message(
                    authority,
                    "aura.test.quorum".into(),
                    b"unapproved retained intake".to_vec(),
                ),
            };
            let first_round = TranscriptRoundPacket {
                version: ROUND_VERSION,
                session: request.session,
                approved_intent_digest: [0; 32],
                transcript_digest: [0; 32],
                coordinator,
                participant,
                sequence: 1,
                body: TranscriptRoundBody::RequestCommitment,
            };
            let wire = DeviceQuorumWire::Request {
                request: request.clone(),
                possession_proof: vec![0x55],
                first_round: Box::new(ParticipantProvenRoundPacket {
                    packet: first_round,
                    possession_proof: vec![0x66],
                }),
            };
            let payload = to_vec(&wire).expect("encode negative-only quorum request");
            let mut metadata = std::collections::HashMap::new();
            metadata.insert(
                "content-type".to_owned(),
                DEVICE_QUORUM_CONTENT_TYPE.to_owned(),
            );
            let envelope = TransportEnvelope {
                destination: authority,
                source: authority,
                context: crate::core::default_context_id_for_authority(authority),
                payload: payload.clone(),
                metadata,
                receipt: None,
            };
            assert!(matches!(
                service.effects.queue_runtime_envelope(envelope),
                crate::runtime::subsystems::transport::QueueEnvelopeOutcome::Queued
            ));
            let (taken_tx, taken_rx) = tokio::sync::oneshot::channel();
            request_intake_test_signals()
                .lock()
                .expect("quorum intake test signals lock")
                .insert(request.session, taken_tx);
            // Hold the first read-only policy await after the real inbox take.
            // No threshold policy is installed: this is custody/refusal evidence,
            // not a positive quorum admission fixture.
            let policy_hold = service.shared.state.write().await;
            let processor = service.clone();
            let mut task =
                tokio::spawn(async move { processor.process_device_quorum_inbound().await });
            tokio::select! {
                taken = taken_rx => taken.expect("actual request intake observed"),
                _ = &mut task => panic!("quorum processor completed before retained intake"),
            }
            task.abort();
            let cancelled = match task.await {
                Ok(_) => panic!("quorum processor completed after cancellation"),
                Err(error) => error,
            };
            assert!(cancelled.is_cancelled());
            drop(policy_hold);
            let restored = service
                .effects
                .take_inbound_envelope_retained(is_quorum_envelope)
                .expect("exact original request restored after cancellation");
            assert_eq!(restored.payload, payload, "original frame bytes restored");
            assert!(matches!(
                decode_wire(&restored),
                Some(DeviceQuorumWire::Request { request: returned, .. })
                    if returned.session == request.session
                        && returned.authority == request.authority
                        && returned.participant == request.participant
            ));
            drop(restored);
            assert_eq!(service.process_device_quorum_inbound().await.unwrap(), 1);
            assert!(service.shared.device_quorum.slots.lock().await.is_empty());
            assert!(service.pending_signing_requests().await.is_empty());
            assert!(service
                .effects
                .take_inbound_envelope_retained(is_quorum_envelope)
                .is_err());
        });
    }

    #[test]
    fn cancelled_quorum_round_before_slot_lock_restores_original_frame() {
        crate::handlers::invitation::tests::run_async_test_on_large_stack(async {
            let (service, _transport, _clock, authority, participant) =
                response_wait_fixture("quorum-retained-round-lock-cancel");
            let coordinator = DeviceId::new_from_entropy([226; 32]);
            let packet = TranscriptRoundPacket {
                version: ROUND_VERSION,
                session: aura_core::hash::hash(b"quorum-retained-round-lock-cancel"),
                approved_intent_digest: [224; 32],
                transcript_digest: [225; 32],
                coordinator,
                participant,
                sequence: 3,
                body: TranscriptRoundBody::RequestCommitment,
            };
            let wire = DeviceQuorumWire::Round(ParticipantProvenRoundPacket {
                packet: packet.clone(),
                possession_proof: vec![0x55],
            });
            let payload = to_vec(&wire).expect("encode negative-only round");
            let mut metadata = std::collections::HashMap::new();
            metadata.insert(
                "content-type".to_owned(),
                DEVICE_QUORUM_CONTENT_TYPE.to_owned(),
            );
            assert!(matches!(
                service.effects.queue_runtime_envelope(TransportEnvelope {
                    destination: authority,
                    source: authority,
                    context: crate::core::default_context_id_for_authority(authority),
                    payload: payload.clone(),
                    metadata,
                    receipt: None,
                }),
                crate::runtime::subsystems::transport::QueueEnvelopeOutcome::Queued
            ));
            let (taken_tx, taken_rx) = tokio::sync::oneshot::channel();
            round_intake_test_signals()
                .lock()
                .expect("quorum round intake test signals lock")
                .insert(packet.session, taken_tx);
            let slots_hold = service.shared.device_quorum.slots.lock().await;
            let processor = service.clone();
            let mut task =
                tokio::spawn(async move { processor.process_device_quorum_inbound().await });
            tokio::select! {
                taken = taken_rx => taken.expect("actual round intake observed"),
                _ = &mut task => panic!("quorum processor completed before retained round intake"),
            }
            task.abort();
            let cancelled = match task.await {
                Ok(_) => panic!("quorum processor completed after cancellation"),
                Err(error) => error,
            };
            assert!(cancelled.is_cancelled());
            drop(slots_hold);
            let restored = service
                .effects
                .take_inbound_envelope_retained(is_quorum_envelope)
                .expect("exact original round restored after cancellation");
            assert_eq!(restored.payload, payload);
            assert!(matches!(
                decode_wire(&restored),
                Some(DeviceQuorumWire::Round(returned)) if returned.packet.session == packet.session
                    && returned.packet.participant == packet.participant
            ));
            drop(restored);
            assert_eq!(service.process_device_quorum_inbound().await.unwrap(), 1);
            assert!(service.shared.device_quorum.slots.lock().await.is_empty());
            assert!(service
                .effects
                .take_inbound_envelope_retained(is_quorum_envelope)
                .is_err());
        });
    }

    /// A co-signer refuses a request whose share proof does not cover the
    /// exact delivered request (an altered or relayed request), and one
    /// proven with a share other than the named coordinator's; neither
    /// reaches the pending list, so no user is asked and nothing is signed.
    #[test]
    fn co_signer_refuses_requests_without_the_coordinators_exact_proof() {
        crate::handlers::invitation::tests::run_async_test_on_large_stack(async {
            let (issuer, joined, joined_device) =
                crate::runtime_bridge::tests::enrolled_two_device_account("quorum-forged-request")
                    .await;
            let coordinator = issuer.runtime().threshold_signing();
            let co_signer = joined.runtime().threshold_signing();
            let authority = issuer.authority_id();
            let policy = coordinator.quorum_policy(&authority).await.expect("policy");
            let coordinator_share = coordinator
                .local_quorum_share(&policy)
                .await
                .expect("coordinator share");
            let co_signer_policy = co_signer.quorum_policy(&authority).await.expect("policy");
            let co_signer_share = co_signer
                .local_quorum_share(&co_signer_policy)
                .await
                .expect("co-signer share");
            let request = |payload: &[u8]| QuorumSigningRequest {
                version: REQUEST_VERSION,
                session: aura_core::hash::hash(payload),
                authority,
                epoch: policy.epoch,
                coordinator: issuer.context().device_id(),
                participant: joined_device,
                context: SigningContext::message(
                    authority,
                    "aura.test.quorum".into(),
                    payload.to_vec(),
                ),
            };
            // Proof over one request, delivered with an altered context.
            deliver_forged(
                &coordinator,
                &request(b"approved"),
                request(b"altered"),
                &coordinator_share,
            )
            .await;
            // Proof made with the co-signer's own share, not the coordinator's.
            deliver_forged(
                &coordinator,
                &request(b"self"),
                request(b"self"),
                &co_signer_share,
            )
            .await;
            for _ in 0..40 {
                co_signer
                    .process_device_quorum_inbound()
                    .await
                    .expect("inbound");
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
            assert!(
                co_signer.pending_signing_requests().await.is_empty(),
                "forged requests never reach the user"
            );
            assert!(co_signer.shared.device_quorum.slots.lock().await.is_empty());
        });
    }
}
