//! Inviter confirmation of contact invitation acceptances.
//!
//! An invitee does not treat a contact link as established when it imports
//! and accepts a code. It sends a signed acceptance and waits for the
//! inviter's signed response, bound to that exact acceptance. Only a
//! confirmation materializes the contact; a rejection settles the invitation
//! with a typed reason; no response leaves it pending until a bounded wait
//! expires. A response that arrives after the wait is applied by the
//! acceptance-processing loop.
//!
//! The response is signed with the key that signed the invitation code's
//! sender proof, which the invitee stored at import, so it authenticates the
//! party that issued the code the invitee chose to accept.

use super::*;
use aura_core::effects::CryptoCoreEffects;
use aura_signature::SecurityTranscript;

pub(super) const CONTACT_INVITATION_RESPONSE_CONTENT_TYPE: &str =
    "application/aura-contact-invitation-response";
/// How long an accepting invitee waits for the inviter's response.
pub(super) const CONTACT_CONFIRMATION_WAIT_MS: u64 = 30_000;
/// How often the invitee re-sends its (identical) acceptance while waiting.
const CONTACT_ACCEPTANCE_RESEND_MS: u64 = 5_000;
const CONTACT_CONFIRMATION_POLL_MS: u64 = 100;

/// The inviter's decision on a contact invitation acceptance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum ContactInvitationDecision {
    Confirmed,
    Revoked,
    Expired,
    AlreadySettled,
}

impl ContactInvitationDecision {
    /// Invitation status the invitee records for this decision.
    fn settled_status(self) -> InvitationStatus {
        match self {
            Self::Confirmed => InvitationStatus::Accepted,
            Self::Revoked => InvitationStatus::Cancelled,
            Self::Expired => InvitationStatus::Expired,
            Self::AlreadySettled => InvitationStatus::Declined,
        }
    }

    fn from_settled_status(status: InvitationStatus) -> Option<Self> {
        match status {
            InvitationStatus::Accepted => Some(Self::Confirmed),
            InvitationStatus::Cancelled => Some(Self::Revoked),
            InvitationStatus::Expired => Some(Self::Expired),
            InvitationStatus::Declined => Some(Self::AlreadySettled),
            InvitationStatus::Pending => None,
        }
    }
}

/// Why a contact invitation acceptance did not establish a link.
/// Runtime normalization and app policy use the concrete source, not its display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ContactConfirmationError {
    Rejected(ContactInvitationDecision),
    Unconfirmed(u64),
}

impl std::fmt::Display for ContactConfirmationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rejected(ContactInvitationDecision::Revoked) => {
                write!(f, "The inviter revoked this contact invitation")
            }
            Self::Rejected(ContactInvitationDecision::Expired) => {
                write!(f, "This contact invitation has expired")
            }
            Self::Rejected(
                ContactInvitationDecision::AlreadySettled | ContactInvitationDecision::Confirmed,
            ) => write!(f, "This contact invitation was already used"),
            Self::Unconfirmed(wait_ms) => write!(
                f,
                "The inviter did not confirm this contact invitation within {}s; try again when they are online",
                wait_ms / 1000
            ),
        }
    }
}

impl std::error::Error for ContactConfirmationError {}

impl From<ContactConfirmationError> for AgentError {
    fn from(error: ContactConfirmationError) -> Self {
        let timed_out = match &error {
            ContactConfirmationError::Rejected(_) => false,
            ContactConfirmationError::Unconfirmed(_) => true,
        };
        let message = error.to_string();
        let source: Option<Arc<dyn std::error::Error + Send + Sync>> = Some(Arc::new(error));
        if timed_out {
            AgentError::TimeoutWithSource {
                message: message.clone(),
                source: aura_core::AuraError::Internal { message, source },
            }
        } else {
            AgentError::Aura(aura_core::AuraError::Invalid { message, source })
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum ContactAcceptancePrecondition {
    #[error("contact operation {0} lost its acknowledged acceptance ownership")]
    Replaced(InvitationId),
    #[error("contact invitation {0} was not imported")]
    NotImported(InvitationId),
}

impl From<ContactAcceptancePrecondition> for AgentError {
    fn from(error: ContactAcceptancePrecondition) -> Self {
        AgentError::Aura(aura_core::AuraError::Invalid {
            message: error.to_string(),
            source: Some(Arc::new(error)),
        })
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct ContactInvitationResponse {
    pub(super) invitation_id: InvitationId,
    pub(super) inviter_id: AuthorityId,
    pub(super) acceptor_id: AuthorityId,
    pub(super) decision: ContactInvitationDecision,
    /// Digest of the acceptance payload this responds to.
    pub(super) acceptance_digest: [u8; 32],
    pub(super) signature: Vec<u8>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(super) struct ContactInvitationResponseTranscriptPayload {
    invitation_id: InvitationId,
    inviter_id: AuthorityId,
    acceptor_id: AuthorityId,
    decision: ContactInvitationDecision,
    acceptance_digest: [u8; 32],
}

pub(super) struct ContactInvitationResponseTranscript<'a>(pub(super) &'a ContactInvitationResponse);

const CONTACT_RESPONSE_TRANSCRIPT_DOMAIN: &str = "aura.invitation.contact-response";

impl SecurityTranscript for ContactInvitationResponseTranscriptPayload {
    type Payload = Self;

    const DOMAIN_SEPARATOR: &'static str = CONTACT_RESPONSE_TRANSCRIPT_DOMAIN;

    fn transcript_payload(&self) -> Self::Payload {
        self.clone()
    }
}

impl SecurityTranscript for ContactInvitationResponseTranscript<'_> {
    type Payload = ContactInvitationResponseTranscriptPayload;

    const DOMAIN_SEPARATOR: &'static str = CONTACT_RESPONSE_TRANSCRIPT_DOMAIN;

    fn transcript_payload(&self) -> Self::Payload {
        ContactInvitationResponseTranscriptPayload {
            invitation_id: self.0.invitation_id.clone(),
            inviter_id: self.0.inviter_id,
            acceptor_id: self.0.acceptor_id,
            decision: self.0.decision,
            acceptance_digest: self.0.acceptance_digest,
        }
    }
}

/// Inviter: the answer to an authenticated acceptance when the invitation is
/// no longer simply pending, or `None` to accept it now. A duplicate from the
/// acceptor who already accepted is re-confirmed.
pub(super) fn settled_contact_invitation_decision(
    status: &InvitationStatus,
    expired: bool,
    from_accepted_receiver: bool,
) -> Option<ContactInvitationDecision> {
    match status {
        InvitationStatus::Pending if expired => Some(ContactInvitationDecision::Expired),
        InvitationStatus::Pending => None,
        InvitationStatus::Accepted if from_accepted_receiver => {
            Some(ContactInvitationDecision::Confirmed)
        }
        InvitationStatus::Cancelled => Some(ContactInvitationDecision::Revoked),
        InvitationStatus::Expired => Some(ContactInvitationDecision::Expired),
        InvitationStatus::Accepted | InvitationStatus::Declined => {
            Some(ContactInvitationDecision::AlreadySettled)
        }
    }
}

pub(super) fn contact_acceptance_digest(payload: &[u8]) -> [u8; 32] {
    hash(payload)
}

fn is_contact_response_for(envelope: &TransportEnvelope, invitation_id: &InvitationId) -> bool {
    envelope
        .metadata
        .get("content-type")
        .is_some_and(|value| value == CONTACT_INVITATION_RESPONSE_CONTENT_TYPE)
        && envelope
            .metadata
            .get("invitation-id")
            .is_some_and(|value| value == invitation_id.as_str())
}

async fn verify_contact_response_signature_required(
    effects: &AuraEffectSystem,
    original: &RequiredContactResponseVerificationCapability<'_>,
) -> AgentResult<bool> {
    let bytes = ContactInvitationResponseTranscript(&original.response)
        .required_transcript_bytes()
        .map_err(|source| {
            AgentError::Aura(aura_core::AuraError::Serialization {
                message: "encode contact response verification transcript".into(),
                source: Some(Arc::new(source)),
            })
        })?;
    effects
        .ed25519_verify(
            &bytes,
            &original.response.signature,
            original.original_sender_key(effects)?,
        )
        .await
        .map_err(|source| {
            AgentError::Aura(aura_core::AuraError::Crypto {
                message: "verify contact response against original invitation issuer".into(),
                source: Some(Arc::new(source)),
            })
        })
}

/// Unverified response plus its original required import and physical lease.
/// Neither a raw key nor a deserialized record alone can enter verification.
struct RequiredContactResponseVerificationCapability<'runtime> {
    handler: &'runtime InvitationHandler,
    effects: &'runtime AuraEffectSystem,
    stored: StoredImportedInvitation,
    response: ContactInvitationResponse,
    lease: crate::runtime::effects::ImportedInvitationDecisionLeaseCapability<'runtime>,
}
impl RequiredContactResponseVerificationCapability<'_> {
    fn original_sender_key<'owner>(
        &'owner self,
        effects: &AuraEffectSystem,
    ) -> AgentResult<&'owner [u8]> {
        self.lease
            .require_runtime_owner(effects)
            .map_err(AgentError::Aura)?;
        if !std::ptr::eq(self.effects, effects)
            || self.handler.context.authority.authority_id() != effects.runtime_authority_id()
        {
            return Err(AgentError::invalid(
                "contact response verifier belongs to another runtime owner",
            ));
        }
        self.stored.sender_proof_key.as_deref().ok_or_else(|| {
            AgentError::invalid("required contact response verifier lacks original issuer key")
        })
    }
}

/// Required complete imported contact metadata under its actual runtime lease.
pub(super) struct RequiredContactAcceptanceCapability<'runtime> {
    handler: &'runtime InvitationHandler,
    effects: &'runtime AuraEffectSystem,
    stored: StoredImportedInvitation,
    invitation: Invitation,
    lease: crate::runtime::effects::ImportedInvitationDecisionLeaseCapability<'runtime>,
}
impl RequiredContactAcceptanceCapability<'_> {
    pub(super) fn effects(&self) -> &AuraEffectSystem {
        self.effects
    }
    pub(super) fn invitation(&self) -> &Invitation {
        &self.invitation
    }
    pub(super) fn require_handler(&self, handler: &InvitationHandler) -> AgentResult<()> {
        if !std::ptr::eq(self.handler, handler) {
            return Err(AgentError::invalid(
                "contact acceptance belongs to another handler owner",
            ));
        }
        self.lease
            .require_runtime_owner(self.effects)
            .map_err(AgentError::from)
    }
}

/// Retransmission retains the same canonical invitation, signed payload and
/// original shared window. It cannot be recreated from a raw identifier.
pub(super) struct ContactAcceptanceContinuationCapability<'runtime> {
    handler: &'runtime InvitationHandler,
    effects: &'runtime AuraEffectSystem,
    invitation: Invitation,
    payload: Vec<u8>,
    acceptance_digest: [u8; 32],
    window: &'runtime TimeoutBudget,
}
impl ContactAcceptanceContinuationCapability<'_> {
    pub(super) fn effects(&self) -> &AuraEffectSystem {
        self.effects
    }
    pub(super) fn invitation(&self) -> &Invitation {
        &self.invitation
    }
    pub(super) fn payload(&self) -> &[u8] {
        &self.payload
    }
    pub(super) fn require_handler(&self, handler: &InvitationHandler) -> AgentResult<()> {
        if !std::ptr::eq(self.handler, handler) {
            return Err(AgentError::invalid(
                "contact continuation belongs to another handler owner",
            ));
        }
        Ok(())
    }
}

#[aura_macros::capability_boundary(category = "capability_gated",
    capability = "required_contact_acceptance", capability_type = RequiredContactAcceptanceCapability,
    family = "proof_issuer")]
#[aura_macros::authoritative_source(kind = "proof_issuer")]
async fn select_required_contact_acceptance<'runtime>(
    handler: &'runtime InvitationHandler,
    effects: &'runtime AuraEffectSystem,
    selector: &InvitationId,
) -> AgentResult<Option<RequiredContactAcceptanceCapability<'runtime>>> {
    let own_id = handler.context.authority.authority_id();
    let lease = effects.acquire_imported_invitation_decision().await;
    let Some(stored) =
        InvitationCacheHandler::load_imported_regular_required(effects, own_id, selector, &lease)
            .await?
    else {
        return Ok(None);
    };
    if !matches!(
        stored.shareable.invitation_type,
        InvitationType::Contact { .. }
    ) || stored.shareable.sender_id == own_id
    {
        return Ok(None);
    }
    // This is complete canonical retained import metadata, using the exact local
    // context selected by the original contact importer, not partial view facts.
    let invitation = Invitation {
        invitation_id: stored.shareable.invitation_id.clone(),
        context_id: handler.context.effect_context.context_id(),
        sender_id: stored.shareable.sender_id,
        receiver_id: own_id,
        invitation_type: stored.shareable.invitation_type.clone(),
        status: stored.status.clone(),
        created_at: stored.created_at,
        expires_at: stored.shareable.expires_at,
        message: stored.shareable.message.clone(),
        receiver_nickname: None,
    };
    Ok(Some(RequiredContactAcceptanceCapability {
        handler,
        effects,
        stored,
        invitation,
        lease,
    }))
}

#[aura_macros::capability_boundary(category = "capability_gated",
    capability = "contact_acceptance_continuation", capability_type = ContactAcceptanceContinuationCapability,
    family = "proof_issuer")]
#[aura_macros::authoritative_source(kind = "proof_issuer")]
async fn acknowledge_contact_acceptance<'runtime>(
    mut original: RequiredContactAcceptanceCapability<'runtime>,
    payload: Vec<u8>,
    window: &'runtime TimeoutBudget,
) -> AgentResult<ContactAcceptanceContinuationCapability<'runtime>> {
    original.require_handler(original.handler)?;
    let acceptance_digest = contact_acceptance_digest(&payload);
    original.stored.pending_acceptance_digest = Some(acceptance_digest);
    InvitationCacheHandler::persist_imported_invitation_with_decision_lease(
        original.effects,
        original.invitation.receiver_id,
        &original.stored,
        &original.lease,
    )
    .await?;
    // The required initial write acknowledges before the decision lease moves
    // out of scope; subsequent response owners can now acquire that same gate.
    Ok(ContactAcceptanceContinuationCapability {
        handler: original.handler,
        effects: original.effects,
        invitation: original.invitation,
        payload,
        acceptance_digest,
        window,
    })
}

async fn contact_confirmation_window(
    effects: &AuraEffectSystem,
    original_parent: &TimeoutBudget,
) -> AgentResult<TimeoutBudget> {
    let _observation = original_parent.acquire_observation().await;
    let now = effects.physical_time().await.map_err(|source| {
        AgentError::Aura(aura_core::AuraError::Internal {
            message: "read contact confirmation child admission time".into(),
            source: Some(Arc::new(source)),
        })
    })?;
    let requested = invitation_timeout_profile(effects)
        .scale_duration(Duration::from_millis(CONTACT_CONFIRMATION_WAIT_MS))
        .map_err(|source| {
            super::vm_loop::map_invitation_vm_timeout(
                "contact confirmation",
                original_parent,
                TimeoutRunError::Timeout(source),
            )
        })?;
    original_parent
        .child_budget(&now, requested)
        .map_err(|source| {
            super::vm_loop::map_invitation_vm_timeout(
                "contact confirmation",
                original_parent,
                TimeoutRunError::Timeout(source),
            )
        })
}

async fn observe_contact_confirmation_time(
    effects: &AuraEffectSystem,
    window: &TimeoutBudget,
) -> AgentResult<PhysicalTime> {
    let _observation = window.acquire_observation().await;
    let now = effects.physical_time().await.map_err(|source| {
        AgentError::Aura(aura_core::AuraError::Internal {
            message: "read required contact confirmation time".into(),
            source: Some(Arc::new(source)),
        })
    })?;
    window.remaining_at(&now).map_err(|source| {
        map_contact_confirmation_failure(window, TimeoutRunError::Timeout(source))
    })?;
    Ok(now)
}

fn map_contact_confirmation_failure(
    window: &TimeoutBudget,
    error: TimeoutRunError<AgentError>,
) -> AgentError {
    match error {
        TimeoutRunError::Timeout(
            source @ aura_core::TimeoutBudgetError::DeadlineExceeded { .. },
        ) => {
            let message = ContactConfirmationError::Unconfirmed(window.timeout_ms()).to_string();
            AgentError::TimeoutWithSource {
                message: message.clone(),
                source: aura_core::AuraError::Internal {
                    message,
                    source: Some(Arc::new(source)),
                },
            }
        }
        other => super::vm_loop::map_invitation_vm_timeout("contact confirmation", window, other),
    }
}

fn contact_acceptance_retryable(source: &AgentError, expected: AuthorityId) -> bool {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(source);
    while let Some(cause) = source {
        if let Some(TransportError::DestinationUnreachable { destination }) =
            cause.downcast_ref::<TransportError>()
        {
            return *destination == expected;
        }
        source = cause.source();
    }
    false
}

/// Native signature verification and required retained import state jointly
/// issue this move-only continuation. No raw identifier or observed Invitation
/// can materialize a contact after this handoff.
struct VerifiedContactResponseCapability<'runtime> {
    handler: &'runtime InvitationHandler,
    effects: &'runtime AuraEffectSystem,
    stored: StoredImportedInvitation,
    response: ContactInvitationResponse,
    lease: crate::runtime::effects::ImportedInvitationDecisionLeaseCapability<'runtime>,
}

#[aura_macros::capability_boundary(category = "capability_gated",
    capability = "required_contact_response_verification", capability_type = RequiredContactResponseVerificationCapability,
    family = "proof_issuer")]
#[aura_macros::authoritative_source(kind = "proof_issuer")]
async fn select_required_contact_response_verification<'runtime>(
    handler: &'runtime InvitationHandler,
    effects: &'runtime AuraEffectSystem,
    envelope: &TransportEnvelope,
) -> AgentResult<Option<RequiredContactResponseVerificationCapability<'runtime>>> {
    let own_id = handler.context.authority.authority_id();
    if effects.runtime_authority_id() != own_id {
        return Err(AgentError::invalid(
            "contact response handler differs from actual runtime owner",
        ));
    }
    let Ok(response) = serde_json::from_slice::<ContactInvitationResponse>(&envelope.payload)
    else {
        tracing::warn!("Ignoring malformed contact invitation response");
        return Ok(None);
    };
    if envelope.source != response.inviter_id
        || envelope.destination != own_id
        || response.acceptor_id != own_id
    {
        return Ok(None);
    }
    let lease = effects.acquire_imported_invitation_decision().await;
    let Some(stored) = InvitationCacheHandler::load_imported_regular_required(
        effects,
        own_id,
        &response.invitation_id,
        &lease,
    )
    .await?
    else {
        return Ok(None);
    };
    let awaited = matches!(
        stored.shareable.invitation_type,
        InvitationType::Contact { .. }
    ) && stored.shareable.sender_id == response.inviter_id
        && stored.status == InvitationStatus::Pending
        && stored.pending_acceptance_digest == Some(response.acceptance_digest);
    if !awaited {
        return Ok(None);
    }
    // Original code-key continuity is not device-membership authorization.
    if stored.sender_proof_key.is_none() {
        return Ok(None);
    }
    Ok(Some(RequiredContactResponseVerificationCapability {
        handler,
        effects,
        stored,
        response,
        lease,
    }))
}

#[aura_macros::capability_boundary(category = "capability_gated",
    capability = "verified_contact_response", capability_type = VerifiedContactResponseCapability,
    family = "proof_issuer")]
#[aura_macros::authoritative_source(kind = "proof_issuer")]
async fn verify_contact_response_required<'runtime>(
    handler: &'runtime InvitationHandler,
    effects: &'runtime AuraEffectSystem,
    envelope: &TransportEnvelope,
) -> AgentResult<Option<VerifiedContactResponseCapability<'runtime>>> {
    let Some(original) =
        select_required_contact_response_verification(handler, effects, envelope).await?
    else {
        return Ok(None);
    };
    if !verify_contact_response_signature_required(effects, &original).await? {
        return Ok(None);
    }
    Ok(Some(VerifiedContactResponseCapability {
        handler: original.handler,
        effects: original.effects,
        stored: original.stored,
        response: original.response,
        lease: original.lease,
    }))
}

#[aura_macros::capability_boundary(category = "capability_gated",
    capability = "verified_contact_response", capability_type = VerifiedContactResponseCapability,
    family = "runtime_helper")]
async fn publish_verified_contact_response(
    mut verified: VerifiedContactResponseCapability<'_>,
) -> AgentResult<Option<ContactInvitationDecision>> {
    let handler = verified.handler;
    let effects = verified.effects;
    let own_id = handler.context.authority.authority_id();
    let now = effects.physical_time().await.map_err(|source| {
        AgentError::Aura(aura_core::AuraError::Internal {
            message: "read required contact response publication time".into(),
            source: Some(Arc::new(source)),
        })
    })?;
    if verified.response.decision == ContactInvitationDecision::Confirmed {
        let InvitationType::Contact { nickname } = &verified.stored.shareable.invitation_type
        else {
            return Err(AgentError::invalid(
                "verified contact response lost its contact shape",
            ));
        };
        let contact_id = verified.stored.shareable.sender_id;
        let context_id = handler.context.effect_context.context_id();
        let fact = ContactFact::Added {
            context_id,
            owner_id: own_id,
            contact_id,
            nickname: nickname.clone().unwrap_or_else(|| contact_id.to_string()),
            added_at: now,
            invitation_code: None,
        };
        handler
            .commit_contact_fact_and_record_observation(effects, context_id, &fact)
            .await?;
        // Descriptor enrichment remains observed, after canonical publication.
        if let Some(rendezvous) = effects.rendezvous_manager() {
            if let Some(peer) = rendezvous.get_lan_discovered_peer(contact_id).await {
                let mut descriptor = peer.descriptor.clone();
                descriptor.context_id = context_id;
                if let Err(error) = rendezvous.cache_descriptor(descriptor).await {
                    tracing::debug!(%error, "contact descriptor enrichment failed after publication");
                }
            }
        }
    }
    let status = verified.response.decision.settled_status();
    verified.stored.status = status.clone();
    verified.stored.pending_acceptance_digest = None;
    InvitationCacheHandler::persist_imported_invitation_with_decision_lease(
        effects,
        own_id,
        &verified.stored,
        &verified.lease,
    )
    .await?;
    // Updating an observed projection does not select authoritative state.
    let _ = handler
        .invitation_cache
        .update_invitation(&verified.response.invitation_id, |invitation| {
            invitation.status = status.clone();
        })
        .await;
    Ok(Some(verified.response.decision))
}

impl InvitationHandler {
    /// Inviter: signs and sends its decision on an authenticated acceptance.
    /// Missing original identity and signing-provider failures remain required
    /// errors; no unauthenticated response is sent.
    pub(super) async fn send_contact_invitation_response(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
        acceptor_id: AuthorityId,
        acceptor_device_id: Option<aura_core::DeviceId>,
        acceptance_digest: [u8; 32],
        decision: ContactInvitationDecision,
    ) -> AgentResult<()> {
        let inviter_id = self.context.authority.authority_id();
        let issued =
            super::issued_identity::select_original_identity(effects, invitation_id).await?;
        if issued.invitation().sender_id != inviter_id
            || !matches!(
                issued.invitation().invitation_type,
                InvitationType::Contact { .. }
            )
        {
            return Err(AgentError::invalid(
                "contact response differs from original issued invitation",
            ));
        }
        let identity =
            crate::handlers::rendezvous_identity::require_issued_identity_signing_context(
                effects, &issued,
            )
            .await
            .map_err(AgentError::EnrollmentManifest)?;
        let (private_key, _public_key) =
            crate::handlers::rendezvous_identity::require_identity_keys(&identity)
                .await
                .map_err(AgentError::EnrollmentManifest)?;
        let transcript = ContactInvitationResponseTranscriptPayload {
            invitation_id: invitation_id.clone(),
            inviter_id,
            acceptor_id,
            decision,
            acceptance_digest,
        };
        let bytes = transcript.required_transcript_bytes().map_err(|source| {
            AgentError::Aura(aura_core::AuraError::Serialization {
                message: "encode contact response signing transcript".into(),
                source: Some(Arc::new(source)),
            })
        })?;
        let private_key = zeroize::Zeroizing::new(private_key);
        let signature = effects
            .ed25519_sign(&bytes, private_key.as_ref())
            .await
            .map_err(|source| {
                AgentError::Aura(aura_core::AuraError::Crypto {
                    message: "sign contact response".into(),
                    source: Some(Arc::new(source)),
                })
            })?;
        let response = ContactInvitationResponse {
            invitation_id: transcript.invitation_id,
            inviter_id: transcript.inviter_id,
            acceptor_id: transcript.acceptor_id,
            decision: transcript.decision,
            acceptance_digest: transcript.acceptance_digest,
            signature,
        };
        let payload = serde_json::to_vec(&response).map_err(|source| {
            AgentError::Aura(aura_core::AuraError::Serialization {
                message: "encode contact response".into(),
                source: Some(Arc::new(source)),
            })
        })?;

        let delivery_context = default_context_id_for_authority(acceptor_id);
        let flow_receipt =
            execute_charge_flow_budget(FlowCost::new(1), delivery_context, acceptor_id, effects)
                .await?;
        let mut metadata = crate::handlers::shared::build_transport_metadata(
            CONTACT_INVITATION_RESPONSE_CONTENT_TYPE,
            [("invitation-id", invitation_id.to_string())],
        );
        if let Some(device_id) = acceptor_device_id {
            metadata.insert(
                "aura-destination-device-id".to_string(),
                device_id.to_string(),
            );
        }
        let mut envelope = TransportEnvelope {
            destination: acceptor_id,
            source: inviter_id,
            context: delivery_context,
            payload,
            metadata,
            receipt: flow_receipt.map(transport_receipt_from_flow),
        };
        attach_invitation_test_receipt_if_needed(effects, &mut envelope);
        tracing::info!(
            invitation_id = %invitation_id,
            acceptor_id = %acceptor_id,
            decision = ?decision,
            "Sending contact invitation response"
        );
        send_guarded_transport_envelope(effects, envelope)
            .await
            .map_err(aura_core::AuraError::from)
            .map_err(AgentError::from)
    }

    /// Invitee: applies an inviter response if it authenticates and answers
    /// the acceptance we are waiting on. Returns the decision it applied;
    /// unrelated, replayed or forged responses are ignored.
    pub(super) async fn apply_contact_invitation_response(
        &self,
        effects: &AuraEffectSystem,
        envelope: &TransportEnvelope,
    ) -> AgentResult<Option<ContactInvitationDecision>> {
        let Some(verified) = verify_contact_response_required(self, effects, envelope).await?
        else {
            return Ok(None);
        };
        publish_verified_contact_response(verified).await
    }

    /// Invitee: sends a signed acceptance and waits, bounded, for the
    /// inviter's response. Succeeds only once the inviter confirms.
    pub(super) async fn confirm_contact_invitation_acceptance(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation_id: &InvitationId,
        original_parent: &TimeoutBudget,
    ) -> AgentResult<InvitationResult> {
        let confirmation_window =
            contact_confirmation_window(effects.as_ref(), original_parent).await?;
        execute_with_timeout_budget(effects.as_ref(), &confirmation_window, || async {
        let Some(original) = select_required_contact_acceptance(self, effects.as_ref(), invitation_id).await? else {
            return Err(ContactAcceptancePrecondition::NotImported(invitation_id.clone()).into());
        };
        if let Some(decision) = ContactInvitationDecision::from_settled_status(original.stored.status.clone()) {
            return match decision {
                ContactInvitationDecision::Confirmed => Ok(InvitationResult::new(invitation_id.clone(), InvitationStatus::Accepted)),
                other => Err(ContactConfirmationError::Rejected(other).into()),
            };
        }
        let contact = InvitationContactHandler::new(self);
        let payload = contact.build_contact_invitation_acceptance(&original).await?;
        let continuation = acknowledge_contact_acceptance(original, payload, &confirmation_window).await?;
        let mut next_send_ms = continuation.window.started_at_ms();
        loop {
            let now = observe_contact_confirmation_time(continuation.effects, continuation.window).await?;
            if now.ts_ms >= next_send_ms {
                if let Err(error) = contact.send_contact_invitation_acceptance(&continuation).await {
                    if !contact_acceptance_retryable(&error, continuation.invitation.sender_id) { return Err(error); }
                    tracing::debug!(%error, "retrying definitely-unsent contact acceptance within original window");
                }
                next_send_ms = now.ts_ms.checked_add(CONTACT_ACCEPTANCE_RESEND_MS)
                    .unwrap_or(continuation.window.deadline_at_ms())
                    .min(continuation.window.deadline_at_ms());
            }
            loop {
                match effects.take_inbound_envelope(|envelope| is_contact_response_for(envelope, invitation_id)) {
                    Ok(envelope) => { self.apply_contact_invitation_response(effects.as_ref(), &envelope).await?; }
                    Err(TransportError::NoMessage) => break,
                    Err(source) => return Err(AgentError::Aura(source.into())),
                }
            }
            let lease = effects.acquire_imported_invitation_decision().await;
            let stored = InvitationCacheHandler::load_imported_regular_required(
                effects.as_ref(), self.context.authority.authority_id(), invitation_id, &lease,
            ).await?.ok_or_else(|| AgentError::from(ContactAcceptancePrecondition::NotImported(invitation_id.clone())))?;
            drop(lease);
            if stored.status == InvitationStatus::Pending
                && stored.pending_acceptance_digest != Some(continuation.acceptance_digest)
            { return Err(AgentError::from(ContactAcceptancePrecondition::Replaced(invitation_id.clone()))); }
            match ContactInvitationDecision::from_settled_status(stored.status) {
                Some(ContactInvitationDecision::Confirmed) => return Ok(InvitationResult::new(invitation_id.clone(), InvitationStatus::Accepted)),
                Some(other) => return Err(ContactConfirmationError::Rejected(other).into()),
                None => {}
            }
            effects.sleep_ms(CONTACT_CONFIRMATION_POLL_MS).await.map_err(|source| {
                AgentError::Aura(aura_core::AuraError::Internal {
                    message: "required contact confirmation poll timer failed".into(), source: Some(Arc::new(source)),
                })
            })?;
        }
    }).await.map_err(|error| map_contact_confirmation_failure(&confirmation_window, error))
    }
}

#[cfg(test)]
mod transcript_tests {
    use super::*;

    #[test]
    fn response_signing_payload_matches_verification_transcript() {
        let payload = ContactInvitationResponseTranscriptPayload {
            invitation_id: InvitationId::new("contact-response-transcript"),
            inviter_id: AuthorityId::new_from_entropy([41; 32]),
            acceptor_id: AuthorityId::new_from_entropy([42; 32]),
            decision: ContactInvitationDecision::Confirmed,
            acceptance_digest: [43; 32],
        };
        let response = ContactInvitationResponse {
            invitation_id: payload.invitation_id.clone(),
            inviter_id: payload.inviter_id,
            acceptor_id: payload.acceptor_id,
            decision: payload.decision,
            acceptance_digest: payload.acceptance_digest,
            signature: vec![44; 64],
        };

        assert_eq!(
            payload.required_transcript_bytes().unwrap(),
            ContactInvitationResponseTranscript(&response)
                .required_transcript_bytes()
                .unwrap()
        );
    }
}

#[cfg(test)]
mod required_contact_identity_tests {
    use super::*;

    #[tokio::test]
    async fn required_contact_verifier_retains_native_failure_and_distinguishes_invalid_signature()
    {
        use futures::FutureExt;
        let pair = Box::pin(super::super::tests::contact_pair(217)).await;
        let invitation = pair.create_contact_invitation().await;
        let imported = pair.import(&pair.signed_code(&invitation).await).await;
        let original = select_required_contact_acceptance(
            &pair.receiver_handler,
            pair.receiver_effects.as_ref(),
            &imported.invitation_id,
        )
        .await
        .unwrap()
        .unwrap();
        let window = TimeoutBudget::from_start_and_timeout(
            &PhysicalTimeEffects::physical_time(pair.receiver_effects.as_ref())
                .await
                .unwrap(),
            Duration::from_secs(30),
        )
        .unwrap();
        let continuation =
            acknowledge_contact_acceptance(original, b"original-acceptance".to_vec(), &window)
                .await
                .unwrap();
        let issued = super::super::issued_identity::select_original_identity(
            pair.sender_effects.as_ref(),
            &invitation.invitation_id,
        )
        .await
        .unwrap();
        let signing =
            crate::handlers::rendezvous_identity::require_issued_identity_signing_context(
                pair.sender_effects.as_ref(),
                &issued,
            )
            .await
            .unwrap();
        let (private, _) = crate::handlers::rendezvous_identity::require_identity_keys(&signing)
            .await
            .unwrap();
        let private = zeroize::Zeroizing::new(private);
        let mut response = ContactInvitationResponse {
            invitation_id: imported.invitation_id.clone(),
            inviter_id: pair.sender_id,
            acceptor_id: pair.receiver_id,
            decision: ContactInvitationDecision::Confirmed,
            acceptance_digest: continuation.acceptance_digest,
            signature: Vec::new(),
        };
        let bytes = ContactInvitationResponseTranscript(&response)
            .required_transcript_bytes()
            .expect("actual canonical response transcript");
        response.signature = pair
            .sender_effects
            .ed25519_sign(&bytes, private.as_ref())
            .await
            .expect("actual native Ed25519 signature");
        let envelope = TransportEnvelope {
            destination: pair.receiver_id,
            source: pair.sender_id,
            context: continuation.invitation.context_id,
            payload: serde_json::to_vec(&response).unwrap(),
            metadata: std::collections::HashMap::default(),
            receipt: None,
        };
        let original = select_required_contact_response_verification(
            &pair.receiver_handler,
            pair.receiver_effects.as_ref(),
            &envelope,
        )
        .await
        .unwrap()
        .unwrap();
        assert!(verify_contact_response_signature_required(
            pair.receiver_effects.as_ref(),
            &original
        )
        .await
        .unwrap());
        assert!(
            verify_contact_response_signature_required(pair.sender_effects.as_ref(), &original)
                .await
                .is_err(),
            "same key cannot substitute a foreign actual runtime"
        );
        let competing = pair.receiver_effects.acquire_imported_invitation_decision();
        tokio::pin!(competing);
        assert!(
            competing.as_mut().now_or_never().is_none(),
            "verification retains original decision custody"
        );
        let mut stored = original.stored.clone();
        drop(original);
        drop(competing.await);
        response.signature[0] ^= 1;
        let mut invalid = envelope.clone();
        invalid.payload = serde_json::to_vec(&response).unwrap();
        assert!(
            verify_contact_response_required(
                &pair.receiver_handler,
                pair.receiver_effects.as_ref(),
                &invalid
            )
            .await
            .unwrap()
            .is_none(),
            "invalid signature cannot issue terminal proof"
        );
        stored.sender_proof_key.as_mut().unwrap().truncate(31);
        InvitationCacheHandler::persist_imported_invitation(
            pair.receiver_effects.as_ref(),
            pair.receiver_id,
            &stored,
        )
        .await
        .unwrap();
        let failure = verify_contact_response_required(
            &pair.receiver_handler,
            pair.receiver_effects.as_ref(),
            &envelope,
        )
        .await
        .err()
        .expect("actual required stored key-length failure is not false verification");
        let native = std::error::Error::source(&failure)
            .and_then(|source| source.source())
            .and_then(|source| source.downcast_ref::<aura_core::AuraError>())
            .expect("original native provider error survives standard source chain");
        assert!(matches!(native, aura_core::AuraError::Invalid { .. }));
    }

    #[tokio::test]
    async fn required_contact_identity_missing_key_is_typed_failure() {
        let authority = AuthorityId::new_from_entropy([228; 32]);
        let config = crate::AgentConfig::default();
        let effects =
            crate::testing::simulation_effect_system_for_authority_arc(&config, authority);
        let handler = InvitationHandler::new(crate::core::AuthorityContext::new(authority))
            .expect("actual invitation handler");
        let error = handler
            .send_contact_invitation_response(
                effects.as_ref(),
                &InvitationId::new("required-contact-identity"),
                AuthorityId::new_from_entropy([229; 32]),
                None,
                [230; 32],
                ContactInvitationDecision::Confirmed,
            )
            .await
            .expect_err("missing actual identity cannot silently omit signed response");
        assert!(matches!(&error, AgentError::Aura(_)));
        assert!(
            std::error::Error::source(&error).is_some(),
            "original required provider failure survives response boundary"
        );
    }
}

#[cfg(test)]
mod owned_contact_continuation_tests {
    use super::*;
    fn source_of<'source, T: std::error::Error + 'static>(
        error: &'source (dyn std::error::Error + 'static),
    ) -> Option<&'source T> {
        let mut cursor = Some(error);
        while let Some(cause) = cursor {
            if let Some(typed) = cause.downcast_ref::<T>() {
                return Some(typed);
            }
            cursor = cause.source();
        }
        None
    }

    #[tokio::test]
    async fn required_contact_import_corruption_is_not_absence_or_cached_pending() {
        let pair = Box::pin(super::super::tests::contact_pair(231)).await;
        let invitation = pair.create_contact_invitation().await;
        let imported = pair.import(&pair.signed_code(&invitation).await).await;
        let key = InvitationCacheHandler::imported_invitation_key(
            pair.receiver_id,
            &imported.invitation_id,
        );
        pair.receiver_effects
            .store(&key, b"{invalid required metadata".to_vec())
            .await
            .expect("inject actual backing metadata corruption");
        let failure = select_required_contact_acceptance(
            &pair.receiver_handler,
            pair.receiver_effects.as_ref(),
            &imported.invitation_id,
        )
        .await
        .err()
        .expect("cached imported invitation must not mask required codec failure");
        assert!(
            source_of::<serde_json::Error>(&failure).is_some(),
            "actual standard source chain retains native codec failure"
        );
        assert!(!failure.is_timeout());
        let lease = pair
            .receiver_effects
            .acquire_imported_invitation_decision()
            .await;
        pair.receiver_effects
            .store(&key, vec![0; 1024 * 1024 + 1])
            .await
            .expect("inject oversized retained record");
        let oversized = InvitationCacheHandler::load_imported_regular_required(
            pair.receiver_effects.as_ref(),
            pair.receiver_id,
            &imported.invitation_id,
            &lease,
        )
        .await
        .expect_err("bounds checked before codec");
        assert!(matches!(
            source_of::<super::super::cache::RequiredImportedInvitationRecordError>(&oversized),
            Some(super::super::cache::RequiredImportedInvitationRecordError::Oversized)
        ));
    }

    #[tokio::test]
    async fn required_contact_decision_lease_rejects_foreign_runtime_and_releases_on_drop() {
        use futures::FutureExt;
        let pair = Box::pin(super::super::tests::contact_pair(232)).await;
        let original = pair
            .receiver_effects
            .acquire_imported_invitation_decision()
            .await;
        assert!(original
            .require_runtime_owner(pair.sender_effects.as_ref())
            .is_err());
        let competing = pair.receiver_effects.acquire_imported_invitation_decision();
        tokio::pin!(competing);
        assert!(
            competing.as_mut().now_or_never().is_none(),
            "same runtime decision custody remains exclusive across await"
        );
        drop(original);
        let next = competing.await;
        next.require_runtime_owner(pair.receiver_effects.as_ref())
            .expect("original runtime owner can continue after lexical drop");
    }
    #[tokio::test]
    async fn contact_confirmation_child_preserves_parent_deadline_and_shared_rollback() {
        let clock = Arc::new(aura_testkit::time::ManualPhysicalClock::new(1_000));
        let effects = crate::testing::simulation_effect_system_for_authority(
            &crate::AgentConfig::default(),
            AuthorityId::new_from_entropy([233; 32]),
        )
        .with_physical_time_provider(clock.clone());
        let parent = TimeoutBudget::from_start_and_timeout(
            &PhysicalTime {
                ts_ms: 1_000,
                uncertainty: None,
            },
            Duration::from_millis(200),
        )
        .expect("original admitted operation window");
        clock.set_time(1_050);
        let child = contact_confirmation_window(&effects, &parent)
            .await
            .expect("attenuated child");
        assert_eq!(
            child.deadline_at_ms(),
            parent.deadline_at_ms(),
            "child cannot extend short original parent"
        );
        clock.set_time(1_100);
        observe_contact_confirmation_time(&effects, &child)
            .await
            .expect("real progress");
        clock.set_time(1_080);
        let rollback = observe_contact_confirmation_time(&effects, &child.clone())
            .await
            .expect_err("rollback after progress above original start");
        assert!(!rollback.is_timeout());
        assert!(matches!(
            source_of::<aura_core::TimeoutBudgetError>(&rollback),
            Some(aura_core::TimeoutBudgetError::ClockRollback { .. })
        ));
        let parent_failure = parent
            .remaining_at(&PhysicalTime {
                ts_ms: 1_110,
                uncertainty: None,
            })
            .expect_err("parent shares sticky rollback with continuation clone");
        assert!(matches!(
            parent_failure,
            aura_core::TimeoutBudgetError::ClockRollback { .. }
        ));
    }

    #[tokio::test]
    async fn contact_confirmation_required_clock_fault_retains_provider_source() {
        let clock = Arc::new(aura_testkit::time::ManualPhysicalClock::new(2_000));
        let effects = crate::testing::simulation_effect_system_for_authority(
            &crate::AgentConfig::default(),
            AuthorityId::new_from_entropy([234; 32]),
        )
        .with_physical_time_provider(clock.clone());
        let parent = TimeoutBudget::from_start_and_timeout(
            &PhysicalTime {
                ts_ms: 2_000,
                uncertainty: None,
            },
            Duration::from_millis(200),
        )
        .expect("original operation window");
        clock
            .fail_next_observation(aura_core::effects::TimeError::OperationFailed {
                reason: "actual provider unavailable fixture".into(),
            })
            .await;
        let failure = contact_confirmation_window(&effects, &parent)
            .await
            .expect_err("child allocation cannot replace failed required clock with zero");
        assert!(!failure.is_timeout());
        assert!(
            matches!(source_of::<aura_core::effects::TimeError>(&failure), Some(aura_core::effects::TimeError::OperationFailed { reason }) if reason == "actual provider unavailable fixture")
        );
        assert_eq!(
            parent.deadline_at_ms(),
            2_200,
            "fault cannot renew original owner"
        );
    }
}
