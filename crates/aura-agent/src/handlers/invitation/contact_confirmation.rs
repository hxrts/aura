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
use aura_signature::sign_ed25519_transcript;

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
    #[error("{0} is not an awaitable contact invitation")]
    NotAwaitable(InvitationId),
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

impl InvitationHandler {
    /// Inviter: signs and sends its decision on an authenticated acceptance.
    /// Without identity keys the inviter cannot authenticate a response, so
    /// it sends none and the invitee's wait expires.
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
        let Some((private_key, _public_key)) =
            crate::handlers::rendezvous_identity::retrieve_identity_keys(effects, &inviter_id)
                .await
        else {
            tracing::warn!(
                invitation_id = %invitation_id,
                "No identity keys to sign a contact invitation response"
            );
            return Ok(());
        };
        let transcript = ContactInvitationResponseTranscriptPayload {
            invitation_id: invitation_id.clone(),
            inviter_id,
            acceptor_id,
            decision,
            acceptance_digest,
        };
        let signature = sign_ed25519_transcript(effects, &transcript, &private_key)
            .await
            .map_err(|error| {
                AgentError::Aura(aura_core::AuraError::Crypto {
                    message: "sign contact response".to_owned(),
                    source: Some(Arc::new(error)),
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
        let payload =
            serde_json::to_vec(&response).map_err(|e| AgentError::internal(e.to_string()))?;

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
        let own_id = self.context.authority.authority_id();
        let Ok(response) = serde_json::from_slice::<ContactInvitationResponse>(&envelope.payload)
        else {
            tracing::warn!("Ignoring malformed contact invitation response");
            return Ok(None);
        };
        if envelope.source != response.inviter_id || response.acceptor_id != own_id {
            tracing::warn!(
                invitation_id = %response.invitation_id,
                "Ignoring contact invitation response with mismatched parties"
            );
            return Ok(None);
        }
        let Some(mut stored) =
            Self::load_imported_invitation(effects, own_id, &response.invitation_id, None).await
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
            tracing::debug!(
                invitation_id = %response.invitation_id,
                status = ?stored.status,
                "Ignoring contact invitation response for an acceptance we are not awaiting"
            );
            return Ok(None);
        }
        // This self_certified_sender_key only proves continuity with the
        // imported invitation; it does not establish trusted device identity.
        let Some(self_certified_sender_key) = stored.sender_proof_key.clone() else {
            tracing::warn!(
                invitation_id = %response.invitation_id,
                "Cannot authenticate contact invitation response: no sender proof key"
            );
            return Ok(None);
        };
        let verified = verify_ed25519_transcript(
            effects,
            &ContactInvitationResponseTranscript(&response),
            &response.signature,
            &self_certified_sender_key,
        )
        .await
        .unwrap_or(false);
        if !verified {
            tracing::warn!(
                invitation_id = %response.invitation_id,
                "Rejected contact invitation response with an invalid signature"
            );
            return Ok(None);
        }

        let now_ms = Self::best_effort_current_timestamp_ms(effects).await;
        if response.decision == ContactInvitationDecision::Confirmed {
            self.materialize_contact_acceptance_if_needed(effects, &response.invitation_id, now_ms)
                .await?;
        }
        let status = response.decision.settled_status();
        stored.status = status.clone();
        stored.pending_acceptance_digest = None;
        Self::persist_imported_invitation(effects, own_id, &stored).await?;
        let _ = self
            .invitation_cache
            .update_invitation(&response.invitation_id, |invitation| {
                invitation.status = status.clone();
            })
            .await;
        tracing::info!(
            invitation_id = %response.invitation_id,
            decision = ?response.decision,
            "Applied contact invitation response"
        );
        Ok(Some(response.decision))
    }

    /// Invitee: sends a signed acceptance and waits, bounded, for the
    /// inviter's response. Succeeds only once the inviter confirms.
    pub(super) async fn confirm_contact_invitation_acceptance(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation_id: &InvitationId,
    ) -> AgentResult<InvitationResult> {
        let own_id = self.context.authority.authority_id();
        let contact = InvitationContactHandler::new(self);
        let Some((invitation, payload)) = contact
            .build_contact_invitation_acceptance(effects.as_ref(), invitation_id)
            .await?
        else {
            return Err(ContactAcceptancePrecondition::NotAwaitable(invitation_id.clone()).into());
        };
        let Some(mut stored) =
            Self::load_imported_invitation(effects.as_ref(), own_id, invitation_id, None).await
        else {
            return Err(ContactAcceptancePrecondition::NotImported(invitation_id.clone()).into());
        };
        stored.pending_acceptance_digest = Some(contact_acceptance_digest(&payload));
        Self::persist_imported_invitation(effects.as_ref(), own_id, &stored).await?;

        let started_ms = Self::best_effort_current_timestamp_ms(effects.as_ref()).await;
        let deadline_ms = started_ms.saturating_add(CONTACT_CONFIRMATION_WAIT_MS);
        let mut next_send_ms = started_ms;
        loop {
            let now_ms = Self::best_effort_current_timestamp_ms(effects.as_ref()).await;
            if now_ms >= next_send_ms {
                if let Err(error) = contact
                    .send_contact_invitation_acceptance(
                        effects.as_ref(),
                        &invitation,
                        payload.clone(),
                    )
                    .await
                {
                    tracing::debug!(
                        invitation_id = %invitation_id,
                        error = %error,
                        "Contact acceptance send failed; retrying while awaiting confirmation"
                    );
                }
                next_send_ms = now_ms.saturating_add(CONTACT_ACCEPTANCE_RESEND_MS);
            }

            while let Ok(envelope) = effects
                .take_inbound_envelope(|envelope| is_contact_response_for(envelope, invitation_id))
            {
                self.apply_contact_invitation_response(effects.as_ref(), &envelope)
                    .await?;
            }

            // The acceptance-processing loop may have applied the response.
            let status =
                Self::load_imported_invitation(effects.as_ref(), own_id, invitation_id, None)
                    .await
                    .map(|stored| stored.status)
                    .unwrap_or(InvitationStatus::Pending);
            match ContactInvitationDecision::from_settled_status(status) {
                Some(ContactInvitationDecision::Confirmed) => {
                    return Ok(InvitationResult::new(
                        invitation_id.clone(),
                        InvitationStatus::Accepted,
                    ));
                }
                Some(decision) => {
                    return Err(ContactConfirmationError::Rejected(decision).into());
                }
                None => {}
            }

            if now_ms >= deadline_ms {
                return Err(
                    ContactConfirmationError::Unconfirmed(CONTACT_CONFIRMATION_WAIT_MS).into(),
                );
            }
            let _ = effects.sleep_ms(CONTACT_CONFIRMATION_POLL_MS).await;
        }
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
            payload.transcript_bytes().unwrap(),
            ContactInvitationResponseTranscript(&response)
                .transcript_bytes()
                .unwrap()
        );
    }
}
