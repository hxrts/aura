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
const RESPONSE_POLL_MS: u64 = 50;
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
    nonces: Option<Zeroizing<Vec<u8>>>,
    commitment: Option<Vec<u8>>,
}

/// In-memory co-signer rounds, keyed by session.
#[derive(Default)]
pub(super) struct DeviceQuorumSlots {
    slots: Mutex<HashMap<[u8; 32], ParticipantSlot>>,
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
        let session = request.session;
        let peer = request.participant;
        let started = self.effects.physical_time().await?.ts_ms;
        loop {
            let taken = self.effects.take_inbound_envelope(|envelope| {
                matches!(
                    decode_wire(envelope),
                    Some(DeviceQuorumWire::Round(proven))
                        if proven.packet.session == session
                            && proven.packet.participant == peer
                            && proven.packet.sequence == sequence
                )
            });
            if let Ok(envelope) = taken {
                let Some(DeviceQuorumWire::Round(proven)) = decode_wire(&envelope) else {
                    continue;
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
                return match packet.body {
                    TranscriptRoundBody::Cancel => Err(refused(DeviceQuorumError::Declined)),
                    body => Ok(body),
                };
            }
            let now = self.effects.physical_time().await?.ts_ms;
            if now.saturating_sub(started) >= RESPONSE_TIMEOUT_MS {
                return Err(refused(DeviceQuorumError::TimedOut));
            }
            self.effects
                .sleep_ms(RESPONSE_POLL_MS)
                .await
                .map_err(|source| AuraError::Internal {
                    message: "device quorum response wait".into(),
                    source: Some(Arc::new(source)),
                })?;
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

        let nonces = Zeroizing::new(self.effects.frost_generate_nonces(&local_share).await?);
        let mut commitments = vec![
            self.effects
                .frost_public_commitment(policy.my_index, &nonces)
                .await?,
        ];
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
        shares.insert(
            policy.my_index,
            self.effects
                .frost_sign_share_for_message(
                    &package,
                    &local_share,
                    &nonces,
                    &message,
                    &policy.public_package,
                    policy.threshold,
                )
                .await?,
        );
        drop(nonces);
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
        while let Ok(envelope) = self.effects.take_inbound_envelope(addressed_to_me) {
            processed += 1;
            let source = envelope.source;
            let Some(wire) = decode_wire(&envelope) else {
                tracing::warn!(%source, "dropping undecodable device quorum message");
                continue;
            };
            let outcome = match wire {
                DeviceQuorumWire::Request {
                    request,
                    possession_proof,
                    first_round,
                } => {
                    self.admit_quorum_request(source, request, &possession_proof, first_round)
                        .await
                }
                DeviceQuorumWire::Round(proven) => self.admit_quorum_round(proven).await,
            };
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
            },
        );
        drop(slots);
        // The first round rides with the request; it waits behind consent.
        self.admit_quorum_round(*first_round).await
    }

    async fn admit_quorum_round(
        &self,
        proven: ParticipantProvenRoundPacket,
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
            slot.queued.push_back(proven);
            return Ok(());
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
                let nonces =
                    Zeroizing::new(self.effects.frost_generate_nonces(&local_share).await?);
                let commitment = self
                    .effects
                    .frost_public_commitment(policy.my_index, &nonces)
                    .await?;
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
                    .ok_or_else(|| refused(DeviceQuorumError::Phase))?;
                let share = self
                    .effects
                    .frost_sign_share_for_message(
                        &package,
                        &local_share,
                        &nonces,
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
        let Some(mut slot) = slots.remove(&session).filter(|slot| !slot.approved) else {
            return Err(refused(DeviceQuorumError::UnknownRequest));
        };
        if !approve {
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
        slot.approved = true;
        let mut finished = false;
        while let Some(queued) = slot.queued.pop_front() {
            finished = self.co_sign_round(&mut slot, queued).await?;
        }
        if !finished {
            slots.insert(session, slot);
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

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
