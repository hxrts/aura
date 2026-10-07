//! Invitation Handlers
//!
//! Handlers for invitation-related operations including creating, accepting,
//! and declining invitations for channels, guardians, and contacts.
//!
//! This module uses `aura_invitation::InvitationService` internally for
//! guard chain integration. Types are re-exported from `aura_invitation`.

use super::shared::{
    load_relational_fact_envelopes_by_type, resolve_charge_peer, HandlerContext,
    HandlerUtilities,
};
use cache::InvitationCacheHandler;
use channel::InvitationChannelHandler;
use contact::InvitationContactHandler;
use execution::{
    attempt_network_send_envelope, emit_browser_harness_debug_event, invitation_timeout_budget,
    invitation_timeout_profile, timeout_deferred_network_stage, timeout_invitation_stage_with_budget,
    timeout_prepare_invitation_stage,
};
use crate::core::{default_context_id_for_authority, AgentError, AgentResult, AuthorityContext};
use crate::runtime::services::InvitationManager;
use crate::runtime::AuraEffectSystem;
#[cfg(feature = "choreo-backend-telltale-machine")]
use crate::runtime::vm_host_bridge::AuraVmHostWaitStatus;
use crate::InvitationServiceApi;
use device_enrollment::InvitationDeviceEnrollmentHandler;
use guardian::InvitationGuardianHandler;
use aura_chat::{ChatFact, CHAT_FACT_TYPE_ID};
use aura_core::crypto::single_signer::SigningMode;
use aura_core::effects::amp::{ChannelBootstrapPackage, ChannelCreateParams};
use aura_core::effects::storage::StorageCoreEffects;
use aura_core::effects::RandomExtendedEffects;
use aura_core::effects::{
    AmpChannelEffects, ChannelJoinParams, CryptoExtendedEffects, FlowBudgetEffects,
    ThresholdSigningEffects, TransportEffects, TransportEnvelope, TransportReceipt,
};
use aura_core::effects::{SecureStorageCapability, SecureStorageEffects, SecureStorageLocation};
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::hash::hash;
use aura_core::threshold::{ApprovalContext, SignableOperation, SigningContext, ThresholdSignature};
use aura_core::types::identifiers::{
    AuthorityId, CeremonyId, ChannelId, ContextId, DeviceId, InvitationId,
};
use aura_core::time::PhysicalTime;
use aura_core::Hash32;
use aura_core::FlowCost;
use aura_core::Receipt;
use aura_core::CapabilityName;
use aura_core::{
    execute_with_retry_budget, execute_with_timeout_budget, ExponentialBackoffPolicy,
    RetryBudgetPolicy, RetryRunError, TimeoutBudget, TimeoutExecutionProfile, TimeoutRunError,
};
use aura_guards::types::CapabilityId;
use aura_invitation::capabilities::evaluation_candidates_for_invitation_guard;
use aura_invitation::guards::GuardSnapshot;
use aura_invitation::{InvitationConfig, InvitationService as CoreInvitationService};
use aura_invitation::{InvitationFact, INVITATION_FACT_TYPE_ID};
use aura_invitation::shareable::ValidatedImportedInvitation;
#[cfg(not(feature = "choreo-backend-telltale-machine"))]
use aura_invitation::protocol::exchange_runners::InvitationExchangeRole;
use aura_invitation::protocol::guardian::telltale_session_types_invitation_guardian::message_wrappers::{
    GuardianAccept as GuardianInvitationAccept, GuardianConfirm as GuardianInvitationConfirm,
    GuardianRequest as GuardianInvitationRequest,
};
use aura_invitation::protocol::device_enrollment::telltale_session_types_invitation_device_enrollment::message_wrappers::{
    DeviceEnrollmentResponse as DeviceEnrollmentResponseWrapper,
};
use aura_invitation::{
    DeviceEnrollmentResponse, GuardianAccept, GuardianConfirm, GuardianRequest,
    InvitationOperation,
};
#[cfg(test)]
use aura_invitation::DeviceEnrollmentAccept;

use crate::runtime::services::TrustedKeyResolutionService;
use crate::runtime::transport_boundary::send_guarded_transport_envelope;
use aura_core::effects::TransportError;
use aura_core::util::serialization::{from_slice, to_vec};
use aura_journal::DomainFact;
use aura_protocol::amp::AmpJournalEffects;
use aura_protocol::effects::ChoreographyError;
#[cfg(feature = "choreo-backend-telltale-machine")]
use aura_protocol::effects::{ChoreographicRole, RoleIndex};
use aura_relational::{ContactFact, CONTACT_FACT_TYPE_ID};
use aura_rendezvous::{RendezvousDescriptor, TransportHint};
use aura_signature::{threshold_signing_context_transcript_bytes, SecurityTranscript};
use std::collections::{BTreeSet, HashMap};
use std::future::Future;
#[cfg(test)]
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
#[cfg(feature = "choreo-backend-telltale-machine")]
use telltale_machine::StepResult;
use uuid::Uuid;
use validation::InvitationValidationHandler;

mod cache;
mod channel;
mod contact;
pub(crate) mod contact_confirmation;
mod device_enrollment;
pub(crate) use device_enrollment::EnrollmentVmTeardownFailure;
pub(crate) mod enrollment_manifest_admission;
pub(crate) mod enrollment_parent_archive;
/// Guard preparation owns its exact sender record and commands. It cannot
/// authorize enrollment terminal mutation or be constructed by callers.
/// Required sender storage custody; private fields prevent observed Invitation
/// values from becoming cancellation preparation input.
pub(crate) struct SenderInvitationRecordCapability {
    runtime_owner: Arc<AuraEffectSystem>,
    invitation: Invitation,
}
impl SenderInvitationRecordCapability {
    pub(crate) fn invitation(&self) -> &Invitation {
        &self.invitation
    }
    pub(crate) fn runtime_owner(&self) -> Arc<AuraEffectSystem> {
        self.runtime_owner.clone()
    }
}

pub(crate) struct AuthorizedInvitationCancellationCapability {
    runtime_owner: Arc<AuraEffectSystem>,

    invitation: Invitation,
    outcome: aura_invitation::guards::GuardOutcome,
}
impl AuthorizedInvitationCancellationCapability {
    pub(crate) fn invitation(&self) -> &Invitation {
        &self.invitation
    }
}

mod enrollment_terminal_notice;
pub(crate) use enrollment_terminal_notice::{
    deliver_settled_cancelled_notice, settle_cancelled_notice,
};
pub(crate) mod enrollment_trust;
mod enrollment_vm_admission;
pub(super) use enrollment_vm_admission::retain_quorum_initial_request;
mod required_channel_read;
pub(crate) use enrollment_trust::VerifiedEnrollmentResponse;
pub(crate) use enrollment_vm_admission::{
    EnrollmentVmAdmissionError, VerifiedEnrollmentFailureCapability,
};
mod exchange;
mod execution;
mod guardian;
#[cfg(test)]
pub(crate) use guardian::{finish_guardian_vm_operation, GuardianVmTerminalFailure};
pub(crate) mod issued_identity;

pub(crate) fn guardian_confirmation_storage_key(invitation_id: &InvitationId) -> String {
    format!("guardian-confirmation:{invitation_id}")
}
mod shareable;
pub(crate) mod validation;
mod vm_loop;

// Re-export types from aura_invitation for public API
pub use aura_invitation::{Invitation, InvitationStatus, InvitationType};
use shareable::{ImportedSenderTrust, StoredImportedInvitation};
pub use shareable::{
    ShareableInvitation, ShareableInvitationError, ShareableInvitationSenderProof,
    ShareableInvitationTransportMetadata,
};

const CONTACT_INVITATION_ACCEPTANCE_CONTENT_TYPE: &str =
    "application/aura-contact-invitation-acceptance";
const CHANNEL_INVITATION_ACCEPTANCE_CONTENT_TYPE: &str =
    "application/aura-channel-invitation-acceptance";
const CHAT_FACT_CONTENT_TYPE: &str = "application/aura-chat-fact";
const INVITATION_CONTENT_TYPE: &str = "application/aura-invitation";
const INVITATION_PREPARE_STAGE_TIMEOUT_MS: u64 = 4_000;
const INVITATION_BEST_EFFORT_NETWORK_TIMEOUT_MS: u64 = 2_000;
const INVITATION_BEST_EFFORT_NETWORK_SEND_ATTEMPTS: usize = 8;
const INVITATION_BEST_EFFORT_NETWORK_SEND_BACKOFF_MS: u64 = 200;
const INVITATION_ACCEPT_OPERATION_TIMEOUT_MS: u64 = 60_000;
const INVITATION_ACCEPT_VALIDATE_STAGE_TIMEOUT_MS: u64 = 5_000;
const INVITATION_ACCEPT_PREPARE_STAGE_TIMEOUT_MS: u64 = 5_000;
const INVITATION_ACCEPT_GUARD_STAGE_TIMEOUT_MS: u64 = 5_000;
const INVITATION_ACCEPT_MATERIALIZE_STAGE_TIMEOUT_MS: u64 = 15_000;
const INVITATION_ACCEPT_CHOREOGRAPHY_STAGE_TIMEOUT_MS: u64 = 30_000;
const INVITATION_VM_LOOP_TIMEOUT_MS: u64 = 30_000;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ContactInvitationAcceptance {
    invitation_id: InvitationId,
    acceptor_id: AuthorityId,
    signature: ThresholdSignature,
    /// Accepter's own nickname, so the inviter can label the new contact.
    #[serde(default)]
    nickname_suggestion: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ChannelInvitationAcceptance {
    invitation_id: InvitationId,
    acceptor_id: AuthorityId,
    context_id: ContextId,
    channel_id: ChannelId,
    channel_name: Option<String>,
    signature: ThresholdSignature,
}

#[derive(Debug, Clone, serde::Serialize)]
struct ContactInvitationAcceptanceTranscriptPayload {
    invitation_id: InvitationId,
    sender_id: AuthorityId,
    acceptor_id: AuthorityId,
    nickname_suggestion: Option<String>,
    expires_at: Option<u64>,
    decision: &'static str,
}

struct ContactInvitationAcceptanceTranscript<'a> {
    invitation: &'a Invitation,
    acceptor_id: AuthorityId,
    nickname_suggestion: Option<String>,
}

impl SecurityTranscript for ContactInvitationAcceptanceTranscript<'_> {
    type Payload = ContactInvitationAcceptanceTranscriptPayload;

    const DOMAIN_SEPARATOR: &'static str = "aura.invitation.contact-acceptance";

    fn transcript_payload(&self) -> Self::Payload {
        ContactInvitationAcceptanceTranscriptPayload {
            invitation_id: self.invitation.invitation_id.clone(),
            sender_id: self.invitation.sender_id,
            acceptor_id: self.acceptor_id,
            nickname_suggestion: self.nickname_suggestion.clone(),
            expires_at: self.invitation.expires_at,
            decision: "accepted",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
struct ChannelInvitationAcceptanceTranscriptPayload {
    invitation_id: InvitationId,
    sender_id: AuthorityId,
    acceptor_id: AuthorityId,
    context_id: ContextId,
    channel_id: ChannelId,
    channel_name: Option<String>,
    expires_at: Option<u64>,
    decision: &'static str,
}

struct ChannelInvitationAcceptanceTranscript<'a> {
    invitation: &'a Invitation,
    acceptor_id: AuthorityId,
    context_id: ContextId,
    channel_id: ChannelId,
    channel_name: Option<String>,
}

impl SecurityTranscript for ChannelInvitationAcceptanceTranscript<'_> {
    type Payload = ChannelInvitationAcceptanceTranscriptPayload;

    const DOMAIN_SEPARATOR: &'static str = "aura.invitation.channel-acceptance";

    fn transcript_payload(&self) -> Self::Payload {
        ChannelInvitationAcceptanceTranscriptPayload {
            invitation_id: self.invitation.invitation_id.clone(),
            sender_id: self.invitation.sender_id,
            acceptor_id: self.acceptor_id,
            context_id: self.context_id,
            channel_id: self.channel_id,
            channel_name: self.channel_name.clone(),
            expires_at: self.invitation.expires_at,
            decision: "accepted",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
struct DeviceEnrollmentAcceptanceTranscriptPayload {
    manifest_digest: [u8; 32],
    setup_binding: Option<aura_invitation::enrollment_setup::DeviceEnrollmentSetupBinding>,
    invitation_id: InvitationId,
    subject_authority: AuthorityId,
    acceptor_id: AuthorityId,
    ceremony_id: CeremonyId,
    device_id: DeviceId,
    expires_at: Option<u64>,
    decision: &'static str,
}

/// Transcript the invitee signs when accepting a device-enrollment invitation.
///
/// Binds the acceptance to the invitation, the account being joined, the
/// ceremony, and the enrolled device so it cannot be replayed elsewhere.
struct DeviceEnrollmentAcceptanceTranscript<'a> {
    manifest_digest: [u8; 32],
    invitation: &'a Invitation,
    acceptor_id: AuthorityId,
    subject_authority: AuthorityId,
    ceremony_id: CeremonyId,
    device_id: DeviceId,
}

impl SecurityTranscript for DeviceEnrollmentAcceptanceTranscript<'_> {
    type Payload = DeviceEnrollmentAcceptanceTranscriptPayload;

    const DOMAIN_SEPARATOR: &'static str = "aura.invitation.device-enrollment-acceptance.v3";

    fn transcript_payload(&self) -> Self::Payload {
        DeviceEnrollmentAcceptanceTranscriptPayload {
            manifest_digest: self.manifest_digest,
            setup_binding: match &self.invitation.invitation_type {
                InvitationType::DeviceEnrollment { setup_binding, .. } => setup_binding.clone(),
                _ => None,
            },
            invitation_id: self.invitation.invitation_id.clone(),
            subject_authority: self.subject_authority,
            acceptor_id: self.acceptor_id,
            ceremony_id: self.ceremony_id.clone(),
            device_id: self.device_id,
            expires_at: self.invitation.expires_at,
            decision: "accepted",
        }
    }
}

/// Refusal never reuses an acceptance signature or acceptance capability.
struct DeviceEnrollmentRefusalTranscript<'a>(DeviceEnrollmentAcceptanceTranscript<'a>);
impl SecurityTranscript for DeviceEnrollmentRefusalTranscript<'_> {
    type Payload = DeviceEnrollmentAcceptanceTranscriptPayload;
    const DOMAIN_SEPARATOR: &'static str = "aura.invitation.device-enrollment-refusal.v1";
    fn transcript_payload(&self) -> Self::Payload {
        let mut payload = self.0.transcript_payload();
        payload.decision = "refused";
        payload
    }
}

/// Result of an invitation action
///
/// The outer `AgentResult<_>` owns terminal success or failure; this inner
/// value only carries the authoritative postcondition on success.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InvitationResult {
    /// Invitation ID affected
    pub invitation_id: InvitationId,
    /// New status after the action
    pub new_status: InvitationStatus,
}

impl InvitationResult {
    fn new(invitation_id: InvitationId, new_status: InvitationStatus) -> Self {
        Self {
            invitation_id,
            new_status,
        }
    }
}

/// Count of sender-side contact invitation acceptances that were fully
/// materialized into local sender state.
pub type ProcessedContactInvitationAcceptanceCount = usize;

#[derive(Debug)]
pub(crate) struct DeferredInvitationNetworkEffects {
    commands: Vec<aura_invitation::guards::EffectCommand>,
}

impl DeferredInvitationNetworkEffects {
    fn new(commands: Vec<aura_invitation::guards::EffectCommand>) -> Self {
        Self { commands }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    pub(crate) fn commands(&self) -> &[aura_invitation::guards::EffectCommand] {
        &self.commands
    }

    pub(crate) fn into_commands(self) -> Vec<aura_invitation::guards::EffectCommand> {
        self.commands
    }
}

#[derive(Debug)]
pub(crate) struct PreparedInvitation {
    pub(crate) invitation: Invitation,
    pub(crate) deferred_network_effects: DeferredInvitationNetworkEffects,
}

/// Single-use issuance identity. Reservation performs no fact commit or send.
/// This identifies an operation; it is not evidence of invitation trust.
#[must_use]
pub(crate) struct ReservedInvitationIssuance {
    runtime_owner: Arc<AuraEffectSystem>,
    invitation_id: InvitationId,
    authority: AuthorityId,
    device: DeviceId,
    created_at_ms: u64,
}

impl ReservedInvitationIssuance {
    pub(crate) fn owns_effects(&self, effects: &AuraEffectSystem) -> bool {
        std::ptr::eq(self.runtime_owner.as_ref(), effects)
    }

    pub(crate) fn issuer_binding(&self) -> (AuthorityId, DeviceId) {
        (self.authority, self.device)
    }

    pub(crate) fn invitation_id(&self) -> &InvitationId {
        &self.invitation_id
    }
    pub(crate) fn created_at_ms(&self) -> u64 {
        self.created_at_ms
    }
}

pub(crate) struct ChannelInviteDetails {
    /// The accepted invitation: it names the membership episode a home join
    /// starts, on both the inviter and the invitee.
    pub(crate) invitation_id: InvitationId,
    pub(crate) context_id: ContextId,
    pub(crate) channel_id: ChannelId,
    pub(crate) home_name: String,
    pub(crate) sender_id: AuthorityId,
    bootstrap: Option<ChannelBootstrapPackage>,
    /// A home invitation: accepting joins the inviter's home.
    pub(crate) home: bool,
}

#[derive(Debug, Clone, Copy)]
enum CachedInvitationActionValidation {
    Accept { now_ms: u64 },
    Decline,
}

fn is_generic_contact_invitation(
    sender_id: AuthorityId,
    receiver_id: AuthorityId,
    invitation_type: &InvitationType,
) -> bool {
    matches!(invitation_type, InvitationType::Contact { .. }) && sender_id == receiver_id
}

fn require_channel_invitation_name(
    home_id: ChannelId,
    nickname_suggestion: Option<String>,
) -> AgentResult<String> {
    let Some(home_name) = nickname_suggestion
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        return Err(AgentError::invalid(format!(
            "channel invitation {} missing canonical channel metadata",
            home_id
        )));
    };
    Ok(home_name)
}

fn require_channel_invitation_context(
    invitation_id: &InvitationId,
    sender_id: AuthorityId,
    context_id: Option<ContextId>,
) -> AgentResult<ContextId> {
    context_id.ok_or_else(|| {
        AgentError::invalid(format!(
            "channel invitation {} from {} missing authoritative context",
            invitation_id, sender_id
        ))
    })
}

#[cfg(test)]
fn channel_id_from_home_id(home_id: &str) -> AgentResult<ChannelId> {
    ChannelId::from_str(home_id).map_err(|e| {
        AgentError::invalid(format!(
            "invalid channel/home id `{home_id}`: expected canonical ChannelId format ({e})"
        ))
    })
}

/// Invitation handler
///
/// Uses `aura_invitation::InvitationService` for guard chain integration.
/// Contact verification custody is internal to the original response path.
/// Observers may borrow the public handler without minting verifier authority.
///
/// ```
/// use aura_agent::handlers::InvitationHandler;
/// fn observe(handler: &InvitationHandler) -> &InvitationHandler { handler }
/// ```
///
/// A raw public key cannot reconstruct the private required-response owner.
///
/// ```compile_fail
/// use aura_agent::handlers::invitation::contact_confirmation::RequiredContactResponseVerificationCapability;
/// fn forge<'a>(key: &'a [u8]) -> RequiredContactResponseVerificationCapability<'a> {
///     RequiredContactResponseVerificationCapability { stored: key }
/// }
/// ```
///
/// Observed envelopes cannot invoke the internal terminal publication path.
///
/// ```compile_fail
/// use aura_agent::handlers::InvitationHandler;
/// use aura_agent::runtime::AuraEffectSystem;
/// use aura_core::effects::TransportEnvelope;
/// async fn publish(handler: &InvitationHandler, effects: &AuraEffectSystem, observed: &TransportEnvelope) {
///     handler.apply_contact_invitation_response(effects, observed).await;
/// }
/// ```
///
/// Guardian local-pair, imported-continuity and first-binding possession owners
/// cannot be reconstructed by external callers from raw keys or responses.
///
/// ```compile_fail
/// use aura_agent::handlers::invitation::guardian::RequiredGuardianPairVerificationCapability;
/// fn forge(key: Vec<u8>) { let _ = RequiredGuardianPairVerificationCapability { public: key }; }
/// ```
///
/// ```compile_fail
/// use aura_agent::handlers::invitation::guardian::RequiredGuardianConfirmationVerificationCapability;
/// fn forge(key: Vec<u8>) { let _ = RequiredGuardianConfirmationVerificationCapability { stored: key }; }
/// ```
///
/// ```compile_fail
/// use aura_agent::handlers::invitation::guardian::RequiredGuardianPossessionVerificationCapability;
/// fn forge(key: Vec<u8>) { let _ = RequiredGuardianPossessionVerificationCapability { accept: key }; }
/// ```
pub struct InvitationHandler {
    context: HandlerContext,
    /// Core invitation service from aura_invitation
    service: CoreInvitationService,
    /// Cache of pending invitations (for quick lookup)
    invitation_cache: Arc<InvitationManager>,
    trusted_key_resolver: TrustedKeyResolutionService,
}

impl Clone for InvitationHandler {
    fn clone(&self) -> Self {
        // `CoreInvitationService` is stateless: it only stores the authority id
        // and immutable config used to derive guard outcomes.
        let service =
            CoreInvitationService::new(self.service.authority_id(), self.service.config().clone());
        Self {
            context: self.context.clone(),
            service,
            invitation_cache: Arc::clone(&self.invitation_cache),
            trusted_key_resolver: self.trusted_key_resolver.clone(),
        }
    }
}

impl InvitationHandler {
    const IMPORTED_INVITATION_STORAGE_PREFIX: &'static str = "invitation/imported";
    const CREATED_INVITATION_STORAGE_PREFIX: &'static str = "invitation/created";

    /// Create a new invitation handler
    pub fn new(authority: AuthorityContext) -> AgentResult<Self> {
        HandlerUtilities::validate_authority_context(&authority)?;

        let service =
            CoreInvitationService::new(authority.authority_id(), InvitationConfig::default());

        Ok(Self {
            context: HandlerContext::new(authority),
            service,
            invitation_cache: Arc::new(InvitationManager::new()),
            trusted_key_resolver: TrustedKeyResolutionService::new(),
        })
    }

    async fn persist_created_invitation(
        effects: &AuraEffectSystem,
        authority_id: AuthorityId,
        invitation: &Invitation,
    ) -> AgentResult<()> {
        InvitationCacheHandler::persist_created_invitation(effects, authority_id, invitation).await
    }

    async fn best_effort_current_timestamp_ms(effects: &AuraEffectSystem) -> u64 {
        if effects.harness_mode_enabled() {
            let Ok(started_at) = effects.physical_time().await else {
                return 0;
            };
            let Ok(budget) =
                TimeoutBudget::from_start_and_timeout(&started_at, Duration::from_millis(50))
            else {
                return 0;
            };

            return match execute_with_timeout_budget(effects, &budget, || effects.physical_time())
                .await
            {
                Ok(value) => value.ts_ms,
                Err(TimeoutRunError::Operation(_)) | Err(TimeoutRunError::Timeout(_)) => 0,
            };
        }

        effects
            .physical_time()
            .await
            .map(|time| time.ts_ms)
            .unwrap_or(0)
    }

    fn decode_invitation_biscuit_frontier(
        &self,
        effects: &AuraEffectSystem,
    ) -> AgentResult<
        Option<(
            aura_authorization::VerifiedBiscuitToken,
            aura_authorization::BiscuitAuthorizationBridge,
        )>,
    > {
        effects
            .verified_biscuit_frontier()
            .map_err(|error| AgentError::effects(format!("decode biscuit frontier cache: {error}")))
    }

    fn invitation_capability_check_timestamp_seconds(now_ms: u64) -> Option<u64> {
        if now_ms == 0 {
            None
        } else {
            Some(now_ms / 1_000)
        }
    }

    async fn build_invitation_capabilities(
        &self,
        effects: &AuraEffectSystem,
        now_ms: u64,
    ) -> Vec<CapabilityId> {
        let Some((token, bridge)) = (match self.decode_invitation_biscuit_frontier(effects) {
            Ok(frontier) => frontier,
            Err(error) => {
                tracing::warn!(
                    authority = %self.context.authority.authority_id(),
                    error = %error,
                    "failed to decode Biscuit frontier for invitation guard snapshot"
                );
                return Vec::new();
            }
        }) else {
            tracing::debug!(
                authority = %self.context.authority.authority_id(),
                "no Biscuit frontier available for invitation guard snapshot"
            );
            return Vec::new();
        };

        let current_time_seconds = Self::invitation_capability_check_timestamp_seconds(now_ms);
        evaluation_candidates_for_invitation_guard()
            .iter()
            .filter_map(|capability| {
                let capability_name: CapabilityName = capability.as_name();
                match bridge.has_capability_with_time(
                    &token,
                    capability_name.as_str(),
                    current_time_seconds,
                ) {
                    Ok(true) => Some(capability_name),
                    Ok(false) => None,
                    Err(error) => {
                        tracing::warn!(
                            authority = %self.context.authority.authority_id(),
                            capability = capability_name.as_str(),
                            error = %error,
                            "failed to evaluate invitation Biscuit capability"
                        );
                        None
                    }
                }
            })
            .collect()
    }

    pub(crate) async fn load_created_invitation(
        effects: &AuraEffectSystem,
        authority_id: AuthorityId,
        invitation_id: &InvitationId,
    ) -> Option<Invitation> {
        InvitationCacheHandler::load_created_invitation(effects, authority_id, invitation_id).await
    }

    async fn persist_imported_invitation(
        effects: &AuraEffectSystem,
        authority_id: AuthorityId,
        invitation: &StoredImportedInvitation,
    ) -> AgentResult<()> {
        InvitationCacheHandler::persist_imported_invitation(effects, authority_id, invitation).await
    }

    async fn load_imported_invitation(
        effects: &AuraEffectSystem,
        authority_id: AuthorityId,
        invitation_id: &InvitationId,
        preserved: Option<&Invitation>,
    ) -> Option<StoredImportedInvitation> {
        InvitationCacheHandler::load_imported_invitation(
            effects,
            authority_id,
            invitation_id,
            preserved,
        )
        .await
    }

    async fn update_imported_invitation_status_if_present(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
        status: InvitationStatus,
        created_at: u64,
    ) -> AgentResult<()> {
        let own_id = self.context.authority.authority_id();
        let Some(mut invitation) =
            Self::load_imported_invitation(effects, own_id, invitation_id, None).await
        else {
            return Ok(());
        };
        invitation.status = status;
        if invitation.created_at == 0 {
            invitation.created_at = created_at;
        }
        Self::persist_imported_invitation(effects, own_id, &invitation).await
    }

    async fn update_created_invitation_status_if_present(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
        status: InvitationStatus,
    ) -> AgentResult<()> {
        let own_id = self.context.authority.authority_id();
        let Some(mut invitation) =
            Self::load_created_invitation(effects, own_id, invitation_id).await
        else {
            return Ok(());
        };
        invitation.status = status;
        Self::persist_created_invitation(effects, own_id, &invitation).await
    }

    /// Whether `key` signed a contact invitation from `sender` that this
    /// authority imported and the sender confirmed (status Accepted).
    async fn confirmed_sender_proof_key(
        effects: &AuraEffectSystem,
        own_id: AuthorityId,
        sender: AuthorityId,
        key: &[u8],
    ) -> bool {
        let prefix = InvitationCacheHandler::imported_invitation_prefix(own_id);
        let Ok(keys) = effects.list_keys(Some(&prefix)).await else {
            return false;
        };
        for storage_key in keys {
            let Ok(Some(bytes)) = effects.retrieve(&storage_key).await else {
                continue;
            };
            let Some(stored) =
                InvitationCacheHandler::parse_imported_invitation_bytes(&bytes, None)
            else {
                continue;
            };
            if stored.shareable.sender_id == sender
                && stored.status == InvitationStatus::Accepted
                && matches!(
                    stored.shareable.invitation_type,
                    InvitationType::Contact { .. }
                )
                && stored.sender_proof_key.as_deref() == Some(key)
            {
                return true;
            }
        }
        false
    }

    async fn classify_imported_sender_trust(
        &self,
        effects: &AuraEffectSystem,
        shareable: &ShareableInvitation,
        sender_proof: Option<&ShareableInvitationSenderProof>,
    ) -> AgentResult<ImportedSenderTrust> {
        let local_authority = self.context.authority.authority_id();
        if self
            .invitation_cache
            .contact_exists(local_authority, shareable.sender_id)
            .await
        {
            let proof = sender_proof.ok_or_else(|| {
                AgentError::invalid("known sender invitation requires trusted sender proof")
            })?;
            let device_id = proof.sender_device_id.ok_or_else(|| {
                AgentError::invalid("known sender invitation requires sender device id")
            })?;
            // The key that signed a code we already accepted from this contact
            // (confirmed by their signed response) is trusted for new codes.
            if Self::confirmed_sender_proof_key(
                effects,
                local_authority,
                shareable.sender_id,
                &proof.public_key,
            )
            .await
            {
                return Ok(ImportedSenderTrust::ConfirmedInvitationKey {
                    device_id,
                    key_epoch: proof.key_epoch,
                });
            }
            let trusted_key = self
                .trusted_key_resolver
                .resolve_device_key(device_id)
                .map_err(|source| AgentError::UnresolvedDeviceBinding {
                    authority: shareable.sender_id,
                    device: device_id,
                    source,
                })?;
            if trusted_key.bytes() != proof.public_key.as_slice() {
                return Err(AgentError::invalid(
                    "known sender invitation proof key does not match trusted device key",
                ));
            }
            return Ok(ImportedSenderTrust::UnboundDeviceKeyMatch {
                device_id,
                key_epoch: proof.key_epoch,
            });
        }
        Ok(ImportedSenderTrust::SelfCertified)
    }

    async fn refresh_contact_index(
        &self,
        effects: &AuraEffectSystem,
        owner_id: AuthorityId,
    ) -> AgentResult<()> {
        let envelopes =
            load_relational_fact_envelopes_by_type(effects, owner_id, CONTACT_FACT_TYPE_ID).await?;
        let mut index = aura_relational::ContactExistenceIndex::new();
        for envelope in envelopes {
            let Some(contact_fact) = ContactFact::from_envelope(&envelope) else {
                continue;
            };
            index.apply_fact(&contact_fact);
        }
        self.invitation_cache.replace_contact_index(index).await;
        Ok(())
    }

    async fn sender_contact_exists(
        &self,
        effects: &AuraEffectSystem,
        owner_id: AuthorityId,
        contact_id: AuthorityId,
    ) -> bool {
        if !self.invitation_cache.contact_index_seeded().await
            && self.refresh_contact_index(effects, owner_id).await.is_err()
        {
            return false;
        }

        if self
            .invitation_cache
            .contact_exists(owner_id, contact_id)
            .await
        {
            return true;
        }

        self.refresh_contact_index(effects, owner_id).await.is_ok()
            && self
                .invitation_cache
                .contact_exists(owner_id, contact_id)
                .await
    }

    /// Get the authority context
    pub fn authority_context(&self) -> &AuthorityContext {
        &self.context.authority
    }

    /// Required reservation execution never substitutes time, budget, or authorization faults.
    async fn build_required_snapshot_for_context(
        &self,
        effects: &AuraEffectSystem,
        context_id: ContextId,
    ) -> AgentResult<GuardSnapshot> {
        let now_ms = effects
            .physical_time()
            .await
            .map_err(|source| {
                AgentError::from(aura_core::AuraError::Internal {
                    message: "read reserved invitation guard time".into(),
                    source: Some(Arc::new(source)),
                })
            })?
            .ts_ms;
        let mut capabilities = Vec::new();
        if let Some((token, bridge)) = effects
            .verified_biscuit_frontier()
            .map_err(AgentError::from)?
        {
            for capability in evaluation_candidates_for_invitation_guard() {
                let capability_name: CapabilityName = capability.as_name();
                let allowed = bridge
                    .has_capability_with_time(
                        &token,
                        capability_name.as_str(),
                        Some(now_ms / 1_000),
                    )
                    .map_err(|source| {
                        AgentError::from(aura_core::AuraError::Internal {
                            message: "evaluate reserved invitation capability".into(),
                            source: Some(Arc::new(source)),
                        })
                    })?;
                if allowed {
                    capabilities.push(capability_name);
                }
            }
        }
        let budget = aura_core::effects::JournalEffects::get_flow_budget(
            effects,
            &context_id,
            &self.context.authority.authority_id(),
        )
        .await
        .map_err(AgentError::from)?;
        let remaining = u32::try_from(budget.remaining()).map_err(|source| {
            AgentError::from(aura_core::AuraError::Internal {
                message: "reserved invitation flow budget exceeds guard representation".into(),
                source: Some(Arc::new(source)),
            })
        })?;
        Ok(GuardSnapshot::new(
            self.context.authority.authority_id(),
            context_id,
            FlowCost::new(remaining),
            capabilities,
            u64::from(budget.epoch),
            now_ms,
        ))
    }

    /// Build a guard snapshot from the provided context and effects.
    async fn build_snapshot_for_context(
        &self,
        effects: &AuraEffectSystem,
        context_id: ContextId,
    ) -> GuardSnapshot {
        let now_ms = Self::best_effort_current_timestamp_ms(effects).await;
        let capabilities = self.build_invitation_capabilities(effects, now_ms).await;
        let budget = aura_core::effects::JournalEffects::get_flow_budget(
            effects,
            &context_id,
            &self.context.authority.authority_id(),
        )
        .await
        .unwrap_or_else(|error| {
            tracing::warn!(
                authority = %self.context.authority.authority_id(),
                context_id = %context_id,
                error = %error,
                "failed to read authoritative invitation flow budget; using bootstrap fallback"
            );
            aura_core::FlowBudget {
                limit: 100,
                spent: 0,
                epoch: aura_core::Epoch::new(1),
            }
        });

        GuardSnapshot::new(
            self.context.authority.authority_id(),
            context_id,
            FlowCost::new(u32::try_from(budget.remaining()).unwrap_or(u32::MAX)),
            capabilities,
            u64::from(budget.epoch),
            now_ms,
        )
    }

    /// Build a guard snapshot from the handler's default context.
    async fn build_snapshot(&self, effects: &AuraEffectSystem) -> GuardSnapshot {
        self.build_snapshot_for_context(effects, self.context.effect_context.context_id())
            .await
    }

    async fn refresh_channel_context_index(
        &self,
        effects: &AuraEffectSystem,
        authority_id: AuthorityId,
    ) -> AgentResult<()> {
        let envelopes =
            load_relational_fact_envelopes_by_type(effects, authority_id, CHAT_FACT_TYPE_ID)
                .await?;
        let mut index = aura_chat::ChannelContextIndex::new();
        for envelope in envelopes {
            let Some(chat_fact) = ChatFact::from_envelope(&envelope) else {
                continue;
            };
            index.apply_fact(&chat_fact);
        }
        self.invitation_cache
            .replace_channel_context_index(index)
            .await;
        Ok(())
    }

    /// Resolve the effective invitation context for the outgoing invitation type.
    async fn resolve_invitation_context(
        &self,
        effects: &AuraEffectSystem,
        invitation_type: &InvitationType,
    ) -> AgentResult<ContextId> {
        let InvitationType::Channel { home_id, .. } = invitation_type else {
            return Ok(self.context.effect_context.context_id());
        };

        let own_id = self.context.authority.authority_id();
        if !self.invitation_cache.channel_context_index_seeded().await {
            self.refresh_channel_context_index(effects, own_id).await?;
        }

        if let Some(context_id) = self
            .invitation_cache
            .channel_context(*home_id, own_id)
            .await
        {
            return Ok(context_id);
        }

        self.refresh_channel_context_index(effects, own_id).await?;
        if let Some(context_id) = self
            .invitation_cache
            .channel_context(*home_id, own_id)
            .await
        {
            return Ok(context_id);
        }

        Err(AgentError::context(format!(
            "Failed to resolve authoritative invitation context for channel {home_id}"
        )))
    }

    async fn validate_cached_invitation_accept(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
        now_ms: u64,
    ) -> AgentResult<()> {
        InvitationValidationHandler::new(self)
            .validate_cached_invitation_accept(effects, invitation_id, now_ms)
            .await
    }

    async fn validate_cached_invitation_decline(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
    ) -> AgentResult<()> {
        InvitationValidationHandler::new(self)
            .validate_cached_invitation_decline(effects, invitation_id)
            .await
    }

    async fn validate_cached_invitation_for_action(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
        action: CachedInvitationActionValidation,
    ) -> AgentResult<()> {
        HandlerUtilities::validate_authority_context(&self.context.authority)?;
        match action {
            CachedInvitationActionValidation::Accept { now_ms } => {
                self.validate_cached_invitation_accept(effects, invitation_id, now_ms)
                    .await
            }
            CachedInvitationActionValidation::Decline => {
                self.validate_cached_invitation_decline(effects, invitation_id)
                    .await
            }
        }
    }

    /// Create an invitation
    pub async fn create_invitation(
        &self,
        effects: Arc<AuraEffectSystem>,
        receiver_id: AuthorityId,
        invitation_type: InvitationType,
        message: Option<String>,
        expires_in_ms: Option<u64>,
    ) -> AgentResult<Invitation> {
        self.create_invitation_with_context(
            effects,
            receiver_id,
            invitation_type,
            None,
            None,
            message,
            expires_in_ms,
        )
        .await
    }

    /// Create an invitation with an optional explicit context override.
    pub async fn create_invitation_with_context(
        &self,
        effects: Arc<AuraEffectSystem>,
        receiver_id: AuthorityId,
        invitation_type: InvitationType,
        receiver_nickname: Option<String>,
        context_override: Option<ContextId>,
        message: Option<String>,
        expires_in_ms: Option<u64>,
    ) -> AgentResult<Invitation> {
        let prepared = self
            .prepare_invitation_with_context(
                effects.clone(),
                receiver_id,
                invitation_type,
                receiver_nickname,
                context_override,
                message,
                expires_in_ms,
            )
            .await?;

        execute_invitation_effect_commands(
            prepared.deferred_network_effects.commands,
            &self.context.authority,
            effects.as_ref(),
            true,
        )
        .await?;

        Ok(prepared.invitation)
    }

    pub(crate) async fn prepare_invitation_with_context(
        &self,
        effects: Arc<AuraEffectSystem>,
        receiver_id: AuthorityId,
        invitation_type: InvitationType,
        receiver_nickname: Option<String>,
        context_override: Option<ContextId>,
        message: Option<String>,
        expires_in_ms: Option<u64>,
    ) -> AgentResult<PreparedInvitation> {
        let reserved = self.reserve_invitation_issuance(&effects).await?;
        Box::pin(self.prepare_reserved_invitation_with_context(
            effects,
            reserved,
            receiver_id,
            invitation_type,
            receiver_nickname,
            context_override,
            message,
            expires_in_ms,
        ))
        .await
    }

    pub(crate) async fn reserve_invitation_issuance(
        &self,
        effects: &Arc<AuraEffectSystem>,
    ) -> AgentResult<ReservedInvitationIssuance> {
        HandlerUtilities::validate_authority_context(&self.context.authority)?;
        Ok(ReservedInvitationIssuance {
            runtime_owner: effects.clone(),
            invitation_id: InvitationId::new(format!(
                "inv-{}",
                effects.random_uuid().await.simple()
            )),
            authority: self.context.authority.authority_id(),
            device: effects.device_id(),
            created_at_ms: effects
                .physical_time()
                .await
                .map_err(|source| {
                    AgentError::from(aura_core::AuraError::Internal {
                        message: "read invitation reservation time".into(),
                        source: Some(Arc::new(source)),
                    })
                })?
                .ts_ms,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn prepare_reserved_invitation_with_context(
        &self,
        effects: Arc<AuraEffectSystem>,
        reserved: ReservedInvitationIssuance,
        receiver_id: AuthorityId,
        invitation_type: InvitationType,
        receiver_nickname: Option<String>,
        context_override: Option<ContextId>,
        message: Option<String>,
        expires_in_ms: Option<u64>,
    ) -> AgentResult<PreparedInvitation> {
        #[cfg(test)]
        eprintln!("enrollment reserved handler stage: enter required reserved handler");
        HandlerUtilities::validate_authority_context(&self.context.authority)?;
        let sender_id = self.context.authority.authority_id();
        if !reserved.owns_effects(effects.as_ref())
            || reserved.authority != sender_id
            || reserved.device != effects.device_id()
        {
            return Err(AgentError::from(aura_core::AuraError::invalid(
                "Invitation reservation belongs to another issuer",
            )));
        }
        let invitation_id = reserved.invitation_id.clone();
        let current_time = reserved.created_at_ms;
        let expires_at = expires_in_ms
            .map(|ms| {
                current_time.checked_add(ms).ok_or_else(|| {
                    AgentError::from(aura_core::AuraError::invalid(
                        "invitation expiration overflow",
                    ))
                })
            })
            .transpose()?;

        #[cfg(test)]
        eprintln!("enrollment reserved handler stage: resolve exact invitation context");
        let invitation_context = if let Some(context_id) = context_override {
            context_id
        } else {
            timeout_prepare_invitation_stage(
                effects.as_ref(),
                "resolve_invitation_context",
                async {
                    self.resolve_invitation_context(effects.as_ref(), &invitation_type)
                        .await
                },
            )
            .await?
        };
        tracing::debug!(
            receiver_id = %receiver_id,
            invitation_type = ?invitation_type,
            "Preparing invitation with resolved context override={:?} context={}",
            context_override,
            invitation_context
        );

        let invitation = Invitation {
            invitation_id: invitation_id.clone(),
            context_id: invitation_context,
            sender_id,
            receiver_id,
            invitation_type,
            status: InvitationStatus::Pending,
            created_at: current_time,
            expires_at,
            message,
            receiver_nickname,
        };

        let deferred_network_effects = if is_generic_contact_invitation(
            invitation.sender_id,
            invitation.receiver_id,
            &invitation.invitation_type,
        ) {
            timeout_prepare_invitation_stage(
                effects.as_ref(),
                "retain_original_contact_identity",
                issued_identity::birth_original_identity(&reserved, &invitation),
            )
            .await?;
            let fact = InvitationFact::Sent {
                context_id: invitation.context_id,
                invitation_id: invitation.invitation_id.clone(),
                sender_id: invitation.sender_id,
                receiver_id: invitation.receiver_id,
                invitation_type: invitation.invitation_type.clone(),
                sent_at: PhysicalTime {
                    ts_ms: current_time,
                    uncertainty: None,
                },
                expires_at: invitation.expires_at.map(|ts_ms| PhysicalTime {
                    ts_ms,
                    uncertainty: None,
                }),
                receiver_nickname: invitation.receiver_nickname.clone(),
                message: invitation.message.clone(),
            };
            timeout_prepare_invitation_stage(
                effects.as_ref(),
                "commit_generic_contact_invitation_fact",
                execute_journal_append(
                    fact,
                    &self.context.authority,
                    invitation.context_id,
                    effects.as_ref(),
                ),
            )
            .await?;
            DeferredInvitationNetworkEffects::new(Vec::new())
        } else {
            // Build snapshot and prepare through service.
            // For channel invitations this must use the channel context so the
            // generated invitation facts and transport metadata are scoped correctly.
            #[cfg(test)]
            eprintln!("enrollment reserved handler stage: build invitation guard snapshot");
            let snapshot = self
                .build_required_snapshot_for_context(effects.as_ref(), invitation_context)
                .await?;

            #[cfg(test)]
            eprintln!("enrollment reserved handler stage: prepare send guard outcome");
            let outcome = self
                .service
                .prepare_reserved_send_invitation(&snapshot, &invitation);

            let execution_plan = aura_invitation::guards::plan_required_send_execution(outcome)
                .map_err(|source| AgentError::from(source.into_native_error()))?;
            if matches!(
                invitation.invitation_type,
                InvitationType::Guardian { .. }
                    | InvitationType::Contact { .. }
                    | InvitationType::Channel { .. }
            ) {
                timeout_prepare_invitation_stage(
                    effects.as_ref(),
                    "retain_original_guardian_identity",
                    issued_identity::birth_original_identity(&reserved, &invitation),
                )
                .await?;
            }
            tracing::debug!(
                authority = %self.context.authority.authority_id(),
                local_effect_count = execution_plan.local_effects.len(),
                deferred_network_effect_count = execution_plan.deferred_network_effects.len(),
                "Prepared invitation guard outcome with deferred network side effects"
            );
            #[cfg(test)]
            eprintln!("enrollment reserved handler stage: execute original local guard effects");
            timeout_prepare_invitation_stage(
                effects.as_ref(),
                "execute_local_effects",
                execute_invitation_effect_commands(
                    execution_plan.local_effects,
                    &self.context.authority,
                    effects.as_ref(),
                    false,
                ),
            )
            .await?;
            DeferredInvitationNetworkEffects::new(execution_plan.deferred_network_effects)
        };

        if matches!(invitation.invitation_type, InvitationType::Contact { .. })
            && !is_generic_contact_invitation(
                invitation.sender_id,
                invitation.receiver_id,
                &invitation.invitation_type,
            )
        {
            // Reissuance keeps contact membership materialized, but shareable
            // codes are no longer reconstructed from unsigned invitation state.
            let sender_contact_exists = self
                .sender_contact_exists(
                    effects.as_ref(),
                    invitation.sender_id,
                    invitation.receiver_id,
                )
                .await;
            let should_emit_contact_fact = !sender_contact_exists;
            let should_update_code = sender_contact_exists;
            if should_emit_contact_fact || should_update_code {
                let causal = crate::handlers::shared::stamp_contact_causal(
                    effects.as_ref(),
                    self.context.authority.authority_id(),
                    aura_relational::ContactCausalKey::Add {
                        owner: invitation.sender_id,
                        contact: invitation.receiver_id,
                    },
                )
                .await?;
                let contact_fact = ContactFact::Added {
                    context_id: invitation.context_id,
                    owner_id: invitation.sender_id,
                    contact_id: invitation.receiver_id,
                    nickname: invitation.receiver_id.to_string(),
                    added_at: PhysicalTime {
                        ts_ms: current_time,
                        uncertainty: None,
                    },
                    invitation_code: None,
                    causal,
                };

                timeout_prepare_invitation_stage(
                    effects.as_ref(),
                    "commit_sender_contact_fact",
                    self.commit_contact_fact_and_record_observation(
                        effects.as_ref(),
                        invitation.context_id,
                        &contact_fact,
                    ),
                )
                .await?;
            }
        }

        #[cfg(test)]
        eprintln!("enrollment reserved handler stage: persist exact created invitation");
        // Persist the invitation to storage (so it survives service recreation)
        timeout_prepare_invitation_stage(
            effects.as_ref(),
            "persist_created_invitation",
            Self::persist_created_invitation(
                effects.as_ref(),
                self.context.authority.authority_id(),
                &invitation,
            ),
        )
        .await?;

        #[cfg(test)]
        eprintln!("enrollment reserved handler stage: cache committed pending invitation");
        // Cache the pending invitation (for fast lookup within same service instance)
        self.invitation_cache
            .cache_invitation(invitation.clone())
            .await;

        match invitation.invitation_type {
            InvitationType::Contact { .. } => {
                tracing::debug!(
                    invitation_id = %invitation.invitation_id,
                    "Skipping synchronous invitation exchange sender for contact invitation"
                );
            }
            InvitationType::Guardian { .. } => {
                // The principal choreography waits for the guardian's signed
                // acceptance, which requires the invitation to be delivered
                // first; the invitation service runs it after delivery.
            }
            InvitationType::DeviceEnrollment { .. } => {}
            InvitationType::Channel { .. } => {}
        }

        #[cfg(test)]
        eprintln!("enrollment reserved handler stage: return required prepared invitation");
        Ok(PreparedInvitation {
            invitation,
            deferred_network_effects,
        })
    }

    /// Accept an invitation
    pub async fn accept_invitation(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation_id: &InvitationId,
    ) -> AgentResult<InvitationResult> {
        tracing::debug!(
            invitation_id = %invitation_id,
            authority = %self.context.authority.authority_id(),
            "Accepting invitation"
        );

        // Boxed: the accept state machine is large; keep every caller future bounded.
        Box::pin(self.accept_invitation_owned(effects, invitation_id)).await
    }

    async fn accept_invitation_owned(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation_id: &InvitationId,
    ) -> AgentResult<InvitationResult> {
        let operation_budget = invitation_timeout_budget(
            effects.as_ref(),
            "accept_invitation",
            INVITATION_ACCEPT_OPERATION_TIMEOUT_MS,
        )
        .await?;
        let now_ms = timeout_invitation_stage_with_budget(
            effects.as_ref(),
            &operation_budget,
            "accept_invitation_validate",
            INVITATION_ACCEPT_VALIDATE_STAGE_TIMEOUT_MS,
            async {
                let now_ms = Self::best_effort_current_timestamp_ms(&effects).await;
                self.validate_cached_invitation_for_action(
                    effects.as_ref(),
                    invitation_id,
                    CachedInvitationActionValidation::Accept { now_ms },
                )
                .await?;
                if let Some(invitation) = self
                    .get_invitation_with_storage(effects.as_ref(), invitation_id)
                    .await
                {
                    if matches!(
                        invitation.invitation_type,
                        InvitationType::DeviceEnrollment { .. }
                    ) {
                        enrollment_manifest_admission::load_admitted_baseline(
                            effects.as_ref(),
                            invitation.receiver_id,
                            &invitation,
                        )
                        .await?;
                    }
                }
                Ok(now_ms)
            },
        )
        .await?;

        // Build snapshot and prepare through service
        let outcome = timeout_invitation_stage_with_budget(
            effects.as_ref(),
            &operation_budget,
            "accept_invitation_prepare",
            INVITATION_ACCEPT_PREPARE_STAGE_TIMEOUT_MS,
            async {
                // Stamp before snapshotting so the snapshot is never held
                // across an await in the accept state machine.
                let causal = crate::handlers::shared::stamp_invitation_outcome_causal(
                    effects.as_ref(),
                    self.context.authority.authority_id(),
                    invitation_id,
                )
                .await?;
                let snapshot = self.build_snapshot(effects.as_ref()).await;
                Ok(self
                    .service
                    .prepare_accept_invitation(&snapshot, invitation_id, causal))
            },
        )
        .await?;

        tracing::debug!(
            invitation_id = %invitation_id,
            allowed = %outcome.is_allowed(),
            denied = %outcome.is_denied(),
            "Guard outcome for invitation accept"
        );

        // Accept should not be blocked by best-effort budget/notify side effects.
        timeout_invitation_stage_with_budget(
            effects.as_ref(),
            &operation_budget,
            "accept_invitation_guard_outcome",
            INVITATION_ACCEPT_GUARD_STAGE_TIMEOUT_MS,
            execute_guard_outcome_for_accept(outcome, &self.context.authority, effects.as_ref()),
        )
        .await?;

        // A contact link exists only once the inviter confirms our acceptance;
        // confirmation materializes the contact and settles the invitation.
        if self
            .load_invitation_for_choreography(effects.as_ref(), invitation_id)
            .await
            .is_some_and(|invitation| {
                matches!(invitation.invitation_type, InvitationType::Contact { .. })
                    && invitation.sender_id != self.context.authority.authority_id()
            })
        {
            return self
                .confirm_contact_invitation_acceptance(effects, invitation_id, &operation_budget)
                .await;
        }

        timeout_invitation_stage_with_budget(
            effects.as_ref(),
            &operation_budget,
            "accept_invitation_materialize",
            INVITATION_ACCEPT_MATERIALIZE_STAGE_TIMEOUT_MS,
            self.materialize_accept_invitation_state(effects.clone(), invitation_id, now_ms),
        )
        .await?;

        self.update_imported_invitation_status_if_present(
            effects.as_ref(),
            invitation_id,
            InvitationStatus::Accepted,
            now_ms,
        )
        .await?;
        self.update_created_invitation_status_if_present(
            effects.as_ref(),
            invitation_id,
            InvitationStatus::Accepted,
        )
        .await?;

        // Update cache if we have this invitation
        let _ = self
            .invitation_cache
            .update_invitation(invitation_id, |inv| {
                inv.status = InvitationStatus::Accepted;
            })
            .await;

        let choreography_invitation = self
            .load_invitation_for_choreography(effects.as_ref(), invitation_id)
            .await;

        if let Some(invitation) = choreography_invitation.as_ref() {
            if matches!(
                invitation.invitation_type,
                InvitationType::Contact { .. } | InvitationType::Channel { .. }
            ) {
                tracing::debug!(
                    invitation_id = %invitation_id,
                    invitation_type = ?invitation.invitation_type,
                    "Returning immediately after local invitation acceptance; post-accept notification is best effort"
                );
                return Ok(InvitationResult::new(
                    invitation_id.clone(),
                    InvitationStatus::Accepted,
                ));
            }
        }

        timeout_invitation_stage_with_budget(
            effects.as_ref(),
            &operation_budget,
            "accept_invitation_choreography",
            INVITATION_ACCEPT_CHOREOGRAPHY_STAGE_TIMEOUT_MS,
            self.execute_accept_invitation_follow_up(
                effects.clone(),
                invitation_id,
                choreography_invitation.as_ref(),
            ),
        )
        .await?;

        Ok(InvitationResult::new(
            invitation_id.clone(),
            InvitationStatus::Accepted,
        ))
    }

    async fn materialize_accept_invitation_state(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation_id: &InvitationId,
        accepted_at_ms: u64,
    ) -> AgentResult<()> {
        self.materialize_contact_acceptance_if_needed(
            effects.as_ref(),
            invitation_id,
            accepted_at_ms,
        )
        .await?;
        self.materialize_channel_acceptance_if_needed(effects.as_ref(), invitation_id)
            .await?;
        self.materialize_device_enrollment_acceptance_if_needed(effects.as_ref(), invitation_id)
            .await
    }

    async fn commit_contact_fact_and_record_observation(
        &self,
        effects: &AuraEffectSystem,
        context_id: ContextId,
        fact: &ContactFact,
    ) -> AgentResult<()> {
        effects
            .commit_domain_fact(context_id, fact)
            .await
            .map_err(|source| {
                AgentError::Aura(aura_core::AuraError::Internal {
                    message: "commit required contact fact".into(),
                    source: Some(Arc::new(source)),
                })
            })?;
        self.invitation_cache.record_contact_fact(fact).await;
        Ok(())
    }

    async fn materialize_contact_acceptance_if_needed(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
        accepted_at_ms: u64,
    ) -> AgentResult<()> {
        // Accepting a contact invitation must materialize sender contact state so
        // CONTACTS_SIGNAL converges from facts rather than UI-local mutation.
        if let Some((contact_id, nickname, invitation_code)) = self
            .resolve_contact_invitation(effects, invitation_id)
            .await?
        {
            let context_id = self.context.effect_context.context_id();
            let owner_id = self.context.authority.authority_id();
            let causal = crate::handlers::shared::stamp_contact_causal(
                effects,
                owner_id,
                aura_relational::ContactCausalKey::Add {
                    owner: owner_id,
                    contact: contact_id,
                },
            )
            .await?;
            let fact = ContactFact::Added {
                context_id,
                owner_id: self.context.authority.authority_id(),
                contact_id,
                nickname: nickname.clone(),
                added_at: PhysicalTime {
                    ts_ms: accepted_at_ms,
                    uncertainty: None,
                },
                invitation_code,
                causal,
            };

            tracing::debug!(
                invitation_id = %invitation_id,
                contact_id = %contact_id,
                nickname = %nickname,
                context_id = %context_id,
                "Committing ContactFact::Added for accepted invitation"
            );

            self.commit_contact_fact_and_record_observation(effects, context_id, &fact)
                .await?;

            // Promote LAN-discovered descriptor into the local context so that
            // is_peer_online() / resolve_peer_addr() can find it immediately.
            if let Some(rendezvous) = effects.rendezvous_manager() {
                if let Some(lan_peer) = rendezvous.get_lan_discovered_peer(contact_id).await {
                    let mut desc = lan_peer.descriptor.clone();
                    desc.context_id = context_id;
                    let _ = rendezvous.cache_descriptor(desc).await;
                    tracing::debug!(
                        contact_id = %contact_id,
                        "Promoted LAN descriptor to local context after contact acceptance"
                    );
                }
            }

            tracing::debug!(
                contact_id = %contact_id,
                "ContactFact committed successfully"
            );
        } else {
            tracing::debug!(
                invitation_id = %invitation_id,
                "No contact resolution for invitation (not a contact invitation or already resolved)"
            );
        }

        Ok(())
    }

    async fn materialize_channel_acceptance_if_needed(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
    ) -> AgentResult<()> {
        if let Some(mut channel_invite) = self
            .resolve_channel_invitation(effects, invitation_id)
            .await?
        {
            channel_invite.context_id = self
                .resolve_channel_context_from_chat_facts(effects, &channel_invite)
                .await;

            if let Some(package) = channel_invite.bootstrap.clone() {
                let ChannelBootstrapPackage { bootstrap_id, key } = package;

                if key.len() != 32 {
                    return Err(crate::core::AgentError::invalid(format!(
                        "AMP bootstrap key has invalid length: {}",
                        key.len()
                    )));
                }

                let location = SecureStorageLocation::amp_bootstrap_key(
                    &channel_invite.context_id,
                    &channel_invite.channel_id,
                    &bootstrap_id,
                );

                effects
                    .secure_store(
                        &location,
                        &key,
                        &[
                            SecureStorageCapability::Read,
                            SecureStorageCapability::Write,
                        ],
                    )
                    .await
                    .map_err(|e| {
                        crate::core::AgentError::effects(format!("store AMP bootstrap key: {e}"))
                    })?;

                self.materialize_channel_bootstrap_acceptance(
                    effects,
                    &channel_invite,
                    bootstrap_id,
                )
                .await?;
            }

            self.materialize_channel_invitation_acceptance(effects, &channel_invite)
                .await?;
        }

        Ok(())
    }

    /// Commits durable home membership for a home invitation: the joining
    /// member's `MemberJoined`, plus (on the invitee, which never saw the
    /// inviter's facts) the home's `HomeCreated` with the inviter as creator.
    async fn commit_home_membership(
        &self,
        effects: &AuraEffectSystem,
        invite: &ChannelInviteDetails,
        member: AuthorityId,
        include_home: bool,
    ) -> AgentResult<Option<aura_social::SocialFact>> {
        let now_ms = Self::best_effort_current_timestamp_ms(effects).await;
        let home_id = aura_social::HomeId::from_bytes(*invite.channel_id.as_bytes());
        let mut facts = Vec::new();
        let creation = include_home.then(|| {
            aura_social::SocialFact::home_created_ms(
                home_id,
                invite.context_id,
                now_ms,
                invite.sender_id,
                invite.home_name.clone(),
            )
        });
        if let Some(created) = creation.clone() {
            facts.push(created);
        }
        facts.push(aura_social::SocialFact::member_joined_ms(
            member,
            home_id,
            invite.context_id,
            now_ms,
            member.to_string(),
            invite.invitation_id.to_string(),
        ));
        for fact in facts {
            effects
                .commit_domain_fact(invite.context_id, &fact)
                .await
                .map_err(|error| AgentError::effects(format!("commit home membership: {error}")))?;
        }
        Ok(creation)
    }

    async fn materialize_device_enrollment_acceptance_if_needed(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
    ) -> AgentResult<()> {
        let Some(canonical) = self
            .get_invitation_with_storage(effects, invitation_id)
            .await
        else {
            return Ok(());
        };
        if !matches!(
            canonical.invitation_type,
            InvitationType::DeviceEnrollment { .. }
        ) {
            return Ok(());
        }
        let admitted = enrollment_manifest_admission::load_admitted_baseline(
            effects,
            canonical.receiver_id,
            &canonical,
        )
        .await?;
        crate::runtime::services::enrollment_import::install_admitted_generation(effects, &admitted)
            .await
            .map_err(AgentError::from)
    }

    async fn execute_accept_invitation_follow_up(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation_id: &InvitationId,
        invitation: Option<&Invitation>,
    ) -> AgentResult<()> {
        let Some(invitation) = invitation else {
            tracing::debug!(
                invitation_id = %invitation_id,
                "accept follow-up skipped: invitation not loaded for choreography"
            );
            return Ok(());
        };

        match invitation.invitation_type {
            InvitationType::Contact { .. } => {
                tracing::debug!(
                    invitation_id = %invitation_id,
                    "Skipping synchronous invitation exchange receiver for accepted contact invitation"
                );
            }
            InvitationType::Guardian { .. } => {
                tracing::debug!(invitation_id = %invitation_id, "guardian accept follow-up starting");
                self
                    .execute_guardian_invitation_guardian(effects.clone(), invitation)
                    .await
                    .map_err(|error| match error {
                        AgentError::Timeout(reason) => AgentError::Timeout(format!(
                            "guardian invitation accept follow-up failed for {invitation_id}: {reason}"
                        )),
                        other => other,
                    })?;
            }
            InvitationType::DeviceEnrollment { .. } => {
                let _ = effects;
                tracing::debug!(
                    invitation_id = %invitation_id,
                    "Skipping synchronous device enrollment invitee follow-up; invitation service owns the bounded post-accept task"
                );
            }
            InvitationType::Channel { .. } => {
                self.notify_channel_invitation_acceptance(effects.as_ref(), invitation_id)
                    .await?;
                tracing::debug!(
                    invitation_id = %invitation_id,
                    "Skipping synchronous invitation exchange receiver for accepted channel invitation"
                );
            }
        }

        Ok(())
    }

    pub(crate) async fn notify_channel_invitation_acceptance(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
    ) -> AgentResult<()> {
        InvitationChannelHandler::new(self)
            .notify_channel_invitation_acceptance(effects, invitation_id)
            .await
    }

    /// Home-context journal sync (pull side): ask `peer` for the home
    /// governance and moderation facts of `context_id` this authority lacks.
    pub(crate) async fn request_home_context_sync(
        &self,
        effects: &AuraEffectSystem,
        context_id: ContextId,
        peer: AuthorityId,
    ) -> AgentResult<()> {
        InvitationContactHandler::new(self)
            .request_home_context_sync(effects, context_id, peer)
            .await
    }

    /// Home contexts in this authority's homes view with their sync peers.
    pub(crate) async fn home_context_sync_targets(
        effects: &AuraEffectSystem,
        context_id: ContextId,
    ) -> std::collections::BTreeSet<AuthorityId> {
        InvitationContactHandler::home_context_peers(effects, context_id).await
    }

    /// Process sender-side contact invitation acceptances.
    ///
    /// "Processed" means the acceptance envelope was decoded, validated, and
    /// materialized into the sender's authoritative contact/invitation state.
    pub async fn process_contact_invitation_acceptances(
        &self,
        effects: Arc<AuraEffectSystem>,
    ) -> AgentResult<ProcessedContactInvitationAcceptanceCount> {
        InvitationContactHandler::new(self)
            .process_contact_invitation_acceptances(effects)
            .await
    }

    async fn resolve_contact_invitation(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
    ) -> AgentResult<Option<(AuthorityId, String, Option<String>)>> {
        InvitationContactHandler::new(self)
            .resolve_contact_invitation(effects, invitation_id)
            .await
    }

    async fn resolve_channel_invitation(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
    ) -> AgentResult<Option<ChannelInviteDetails>> {
        InvitationChannelHandler::new(self)
            .resolve_channel_invitation(effects, invitation_id)
            .await
    }

    async fn channel_created_fact_name(
        &self,
        effects: &AuraEffectSystem,
        authority_id: AuthorityId,
        context_id: ContextId,
        channel_id: ChannelId,
    ) -> Option<String> {
        let Ok(envelopes) =
            load_relational_fact_envelopes_by_type(effects, authority_id, CHAT_FACT_TYPE_ID).await
        else {
            return None;
        };

        for envelope in envelopes.into_iter().rev() {
            match ChatFact::from_envelope(&envelope) {
                Some(ChatFact::ChannelCreated {
                    context_id: seen_context,
                    channel_id: seen_channel,
                    name,
                    ..
                }) if seen_context == context_id && seen_channel == channel_id => {
                    return Some(name);
                }
                Some(ChatFact::ChannelUpdated {
                    context_id: seen_context,
                    channel_id: seen_channel,
                    name: Some(name),
                    ..
                }) if seen_context == context_id && seen_channel == channel_id => {
                    return Some(name);
                }
                _ => {}
            }
        }

        None
    }

    async fn resolve_channel_context_from_chat_facts(
        &self,
        effects: &AuraEffectSystem,
        invite: &ChannelInviteDetails,
    ) -> ContextId {
        InvitationChannelHandler::new(self)
            .resolve_channel_context_from_chat_facts(effects, invite)
            .await
    }

    async fn materialize_channel_invitation_acceptance(
        &self,
        effects: &AuraEffectSystem,
        invite: &ChannelInviteDetails,
    ) -> AgentResult<()> {
        InvitationChannelHandler::new(self)
            .materialize_channel_invitation_acceptance(effects, invite)
            .await
    }

    async fn materialize_channel_bootstrap_acceptance(
        &self,
        effects: &AuraEffectSystem,
        invite: &ChannelInviteDetails,
        bootstrap_id: Hash32,
    ) -> AgentResult<()> {
        InvitationChannelHandler::new(self)
            .materialize_channel_bootstrap_acceptance(effects, invite, bootstrap_id)
            .await
    }

    /// Import an invitation from a shareable code into the local cache.
    ///
    /// This is a best-effort, local-only operation used for out-of-band invite
    /// transfer (copy/paste). It does not commit any facts by itself; callers
    /// should accept/decline via the normal guard-chain paths.
    pub async fn import_invitation_code(
        &self,
        effects: &AuraEffectSystem,
        code: &str,
    ) -> AgentResult<Invitation> {
        HandlerUtilities::validate_authority_context(&self.context.authority)?;

        let (shareable, sender_proof, transport_metadata) =
            ShareableInvitation::from_code_with_proof_and_transport(code)
                .map_err(|e| crate::core::AgentError::invalid(format!("{e}")))?;
        let sender_hint_addr = transport_metadata.sender_hint.clone();
        let sender_device_id = transport_metadata.sender_device_id;
        tracing::info!(
            invitation_id = %shareable.invitation_id,
            sender = %shareable.sender_id,
            sender_hint_addr = ?sender_hint_addr,
            sender_device_id = ?sender_device_id,
            "import_invitation_code parsed sender hint"
        );

        tracing::debug!(
            invitation_id = %shareable.invitation_id,
            sender = %shareable.sender_id,
            invitation_type = ?shareable.invitation_type,
            "Importing invite code with context={:?}",
            shareable.context_id
        );

        let invitation_id = shareable.invitation_id.clone();

        // Fast path: already cached.
        if let Some(existing) = self.invitation_cache.get_invitation(&invitation_id).await {
            tracing::debug!(
                invitation_id = %invitation_id,
                status = ?existing.status,
                "Returning existing cached invitation"
            );
            return Ok(existing);
        }

        let now_ms = Self::best_effort_current_timestamp_ms(effects).await;
        let own_id = self.context.authority.authority_id();
        let default_context_id = self.context.effect_context.context_id();
        #[cfg(test)]
        let validated_import = if effects.is_testing() {
            None
        } else {
            Some(
                ValidatedImportedInvitation::verify_code(
                    effects,
                    code,
                    own_id,
                    default_context_id,
                    now_ms,
                )
                .await
                .map_err(|error| AgentError::invalid(error.to_string()))?,
            )
        };
        #[cfg(not(test))]
        let validated_import = ValidatedImportedInvitation::verify_code(
            effects,
            code,
            own_id,
            default_context_id,
            now_ms,
        )
        .await
        .map_err(|error| AgentError::invalid(error.to_string()))?;
        #[cfg(test)]
        let invitation = match &validated_import {
            Some(validated) => validated.invitation().clone(),
            None => unverified_test_invitation(&shareable, own_id, default_context_id, now_ms)?,
        };
        #[cfg(not(test))]
        let invitation = validated_import.invitation().clone();
        if matches!(
            invitation.invitation_type,
            InvitationType::DeviceEnrollment { .. }
        ) {
            enrollment_manifest_admission::load_admitted_baseline(
                effects,
                invitation.receiver_id,
                &invitation,
            )
            .await?;
        }
        let sender_trust = self
            .classify_imported_sender_trust(effects, &shareable, sender_proof.as_ref())
            .await?;

        // Persist the imported invitation with local status so later
        // storage-backed reads do not downgrade accepted/declined state. A
        // record imported earlier (a pasted code, then the sender's delivered
        // envelope) keeps its decision and any acknowledged in-flight
        // acceptance: re-importing must not reset it to Pending.
        {
            let lease = effects.acquire_imported_invitation_decision().await;
            if Self::load_imported_invitation(effects, own_id, &invitation_id, None)
                .await
                .is_none()
            {
                let mut stored =
                    StoredImportedInvitation::pending(shareable.clone(), now_ms, sender_trust);
                stored.sender_proof_key =
                    sender_proof.as_ref().map(|proof| proof.public_key.clone());
                InvitationCacheHandler::persist_imported_invitation_with_decision_lease(
                    effects, own_id, &stored, &lease,
                )
                .await?;
            }
        }
        if let Some(addr) = sender_hint_addr.as_deref() {
            self.cache_verified_peer_descriptor_for_peer(
                effects,
                shareable.sender_id,
                sender_device_id,
                Some(addr),
                now_ms,
            )
            .await;
            let cached_descriptor = if let Some(manager) = effects.rendezvous_manager() {
                manager
                    .get_descriptor(
                        default_context_id_for_authority(shareable.sender_id),
                        shareable.sender_id,
                    )
                    .await
            } else {
                None
            };
            let websocket_hint_count = cached_descriptor
                .as_ref()
                .map(|descriptor| {
                    descriptor
                        .transport_hints
                        .iter()
                        .filter(|hint| matches!(hint, TransportHint::WebSocketDirect { .. }))
                        .count()
                })
                .unwrap_or(0);
            tracing::info!(
                invitation_id = %shareable.invitation_id,
                sender = %shareable.sender_id,
                websocket_hint_count,
                "import_invitation_code cached direct descriptor"
            );
        } else if sender_device_id.is_some() {
            self.cache_verified_peer_descriptor_for_peer(
                effects,
                shareable.sender_id,
                sender_device_id,
                None,
                now_ms,
            )
            .await;
        } else if let Some(manager) = effects.rendezvous_manager() {
            if let Some(peer) = manager.get_lan_discovered_peer(shareable.sender_id).await {
                let _ = manager.cache_descriptor(peer.descriptor.clone()).await;
                let websocket_hint_count = peer
                    .descriptor
                    .transport_hints
                    .iter()
                    .filter(|hint| matches!(hint, TransportHint::WebSocketDirect { .. }))
                    .count();
                tracing::info!(
                    invitation_id = %shareable.invitation_id,
                    sender = %shareable.sender_id,
                    websocket_hint_count,
                    "import_invitation_code cached discovered peer descriptor"
                );
            }
        }
        // Known limitation: imported invitations are cached eagerly and the
        // cache is currently unbounded until a proper TTL/LRU policy lands.
        self.invitation_cache
            .cache_invitation(invitation.clone())
            .await;
        #[cfg(test)]
        let materialization = match validated_import {
            Some(validated) => {
                crate::reactive::app_signal_views::materialize_pending_invitation_signal(
                    &effects.reactive_handler(),
                    own_id,
                    validated,
                )
                .await
            }
            None => {
                crate::reactive::app_signal_views::materialize_unverified_invitation_fixture_signal(
                    &effects.reactive_handler(),
                    own_id,
                    &invitation,
                )
                .await
            }
        };
        #[cfg(not(test))]
        let materialization =
            crate::reactive::app_signal_views::materialize_pending_invitation_signal(
                &effects.reactive_handler(),
                own_id,
                validated_import,
            )
            .await;
        materialization.map_err(AgentError::runtime)?;

        Ok(invitation)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) async fn cache_peer_descriptor_for_peer(
        &self,
        effects: &AuraEffectSystem,
        peer: AuthorityId,
        device_id: Option<DeviceId>,
        addr: Option<&str>,
        now_ms: u64,
    ) {
        let _ = (effects, now_ms);
        tracing::debug!(
            peer = %peer,
            sender_device_id = ?device_id,
            sender_hint = ?addr,
            "Ignoring unauthenticated invitation sender hint for authoritative routing"
        );
    }

    pub(crate) async fn cache_verified_peer_descriptor_for_peer(
        &self,
        effects: &AuraEffectSystem,
        peer: AuthorityId,
        device_id: Option<DeviceId>,
        addr: Option<&str>,
        now_ms: u64,
    ) {
        let Some(manager) = effects.rendezvous_manager() else {
            return;
        };
        let hints = addr
            .map(Self::transport_hints_from_sender_hint)
            .unwrap_or_default();
        if hints.is_empty() {
            return;
        }

        let peer_context = default_context_id_for_authority(peer);
        let local_context = self.context.authority.default_context_id();
        for context_id in [peer_context, local_context] {
            if manager.get_descriptor(context_id, peer).await.is_some() {
                continue;
            }
            let descriptor =
                Self::verified_hint_descriptor(peer, device_id, context_id, hints.clone(), now_ms);
            let _ = manager.cache_descriptor(descriptor).await;
        }
        // Persist the verified hint so a restarted (e.g. reloaded browser)
        // runtime can still reach this peer; descriptors live only in memory.
        if let Some(addr) = addr {
            let record = format!(
                "{peer}\n{}\n{addr}",
                device_id.map(|id| id.to_string()).unwrap_or_default()
            );
            let key = format!(
                "{}{peer}",
                Self::verified_peer_hint_prefix(self.context.authority.authority_id())
            );
            let _ = effects.store(&key, record.into_bytes()).await;
        }
    }

    fn verified_peer_hint_prefix(own: AuthorityId) -> String {
        format!("verified_peer_hints/{own}/")
    }

    /// Re-caches every persisted verified peer hint (see
    /// [`Self::cache_verified_peer_descriptor_for_peer`]) after a restart.
    pub(crate) async fn restore_verified_peer_descriptors(&self, effects: &AuraEffectSystem) {
        let prefix = Self::verified_peer_hint_prefix(self.context.authority.authority_id());
        let Ok(keys) = effects.list_keys(Some(&prefix)).await else {
            return;
        };
        let now_ms = Self::best_effort_current_timestamp_ms(effects).await;
        for key in keys {
            let Ok(Some(bytes)) = effects.retrieve(&key).await else {
                continue;
            };
            let Ok(record) = String::from_utf8(bytes) else {
                continue;
            };
            let mut fields = record.splitn(3, '\n');
            let (Some(peer), Some(device), Some(addr)) =
                (fields.next(), fields.next(), fields.next())
            else {
                continue;
            };
            let Ok(peer) = peer.parse::<AuthorityId>() else {
                continue;
            };
            let device_id = device.parse::<DeviceId>().ok();
            self.cache_verified_peer_descriptor_for_peer(
                effects,
                peer,
                device_id,
                Some(addr),
                now_ms,
            )
            .await;
        }
    }

    /// Parses a sender hint, a comma-separated list of scheme-tagged
    /// addresses, into every transport the sender advertised.
    fn transport_hints_from_sender_hint(hint: &str) -> Vec<TransportHint> {
        hint.split(',')
            .filter_map(Self::transport_hint_from_sender_hint)
            .collect()
    }

    fn transport_hint_from_sender_hint(addr: &str) -> Option<TransportHint> {
        let trimmed = addr.trim();
        if trimmed.is_empty() {
            return None;
        }
        if let Some(addr) = trimmed
            .strip_prefix("ws://")
            .or_else(|| trimmed.strip_prefix("wss://"))
        {
            return TransportHint::websocket_direct(addr).ok();
        }
        let addr = trimmed.strip_prefix("tcp://").unwrap_or(trimmed);
        TransportHint::tcp_direct(addr).ok()
    }

    fn verified_hint_descriptor(
        peer: AuthorityId,
        device_id: Option<DeviceId>,
        context_id: ContextId,
        hints: Vec<TransportHint>,
        now_ms: u64,
    ) -> RendezvousDescriptor {
        let mut psk_material = Vec::new();
        psk_material.extend_from_slice(b"aura.invitation.verified-hint.psk.v1");
        psk_material.extend_from_slice(&peer.to_bytes());
        psk_material.extend_from_slice(&context_id.to_bytes());
        psk_material.extend_from_slice(format!("{hints:?}").as_bytes());
        let mut key_material = Vec::new();
        key_material.extend_from_slice(b"aura.invitation.verified-hint.public-key.v1");
        key_material.extend_from_slice(&peer.to_bytes());
        key_material.extend_from_slice(&context_id.to_bytes());

        RendezvousDescriptor {
            authority_id: peer,
            device_id,
            context_id,
            transport_hints: hints,
            handshake_psk_commitment: hash(&psk_material),
            public_key: hash(&key_material),
            valid_from: now_ms.saturating_sub(1),
            valid_until: now_ms.saturating_add(86_400_000),
            nonce: hash(
                format!("aura.invitation.verified-hint.nonce.v1:{peer}:{context_id}").as_bytes(),
            ),
            nickname_suggestion: None,
        }
    }

    /// Decline an invitation
    pub async fn decline_invitation(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation_id: &InvitationId,
        tasks: &crate::task_registry::TaskGroup,
    ) -> AgentResult<InvitationResult> {
        let admitted = enrollment_manifest_admission::load_admitted_enrollment_for_id(
            effects.as_ref(),
            self.context.authority.authority_id(),
            invitation_id,
        )
        .await
        .map_err(AgentError::EnrollmentManifest)?;
        let enrollment_response = admitted.is_some();
        if let Some(admitted) = admitted {
            device_enrollment::InvitationDeviceEnrollmentHandler::new(self)
                .execute_device_enrollment_invitee_decline(
                    effects.clone(),
                    Arc::new(admitted),
                    tasks,
                )
                .await?;
        }
        self.validate_cached_invitation_for_action(
            effects.as_ref(),
            invitation_id,
            CachedInvitationActionValidation::Decline,
        )
        .await?;

        // Build snapshot and prepare through service
        let causal = crate::handlers::shared::stamp_invitation_outcome_causal(
            effects.as_ref(),
            self.context.authority.authority_id(),
            invitation_id,
        )
        .await?;
        let snapshot = self.build_snapshot(effects.as_ref()).await;
        let outcome = self
            .service
            .prepare_decline_invitation(&snapshot, invitation_id, causal);

        // Execute the outcome
        execute_guard_outcome(outcome, &self.context.authority, effects.as_ref()).await?;

        let now_ms = Self::best_effort_current_timestamp_ms(effects.as_ref()).await;
        self.update_imported_invitation_status_if_present(
            effects.as_ref(),
            invitation_id,
            InvitationStatus::Declined,
            now_ms,
        )
        .await?;
        self.update_created_invitation_status_if_present(
            effects.as_ref(),
            invitation_id,
            InvitationStatus::Declined,
        )
        .await?;

        // Update cache if we have this invitation
        let _ = self
            .invitation_cache
            .update_invitation(invitation_id, |inv| {
                inv.status = InvitationStatus::Declined;
            })
            .await;

        if !enrollment_response {
            if let Some(invitation) = self
                .load_invitation_for_choreography(effects.as_ref(), invitation_id)
                .await
            {
                if matches!(invitation.invitation_type, InvitationType::Channel { .. }) {
                    tracing::debug!(
                        invitation_id = %invitation_id,
                        "Skipping synchronous invitation exchange receiver for declined channel invitation"
                    );
                } else if !matches!(invitation.invitation_type, InvitationType::Guardian { .. }) {
                    if let Err(error) = self
                        .execute_invitation_exchange_receiver(effects.clone(), &invitation, false)
                        .await
                    {
                        tracing::warn!(
                            invitation_id = %invitation_id,
                            error = %error,
                            "decline invitation follow-up exchange failed after local decline"
                        );
                    }
                }
            }
        }

        Ok(InvitationResult::new(
            invitation_id.clone(),
            InvitationStatus::Declined,
        ))
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "sender_invitation_record",
        capability_type = SenderInvitationRecordCapability,
        family = "runtime_helper"
    )]
    pub(crate) async fn created_invitation_required(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation_id: &InvitationId,
    ) -> AgentResult<SenderInvitationRecordCapability> {
        let invitation = required_channel_read::created_required(
            effects.as_ref(),
            self.context.authority.authority_id(),
            invitation_id,
        )
        .await
        .map_err(AgentError::from)?;
        let invitation = required_channel_read::hydrate_created_enrollment_required(
            effects.as_ref(),
            self.context.authority.authority_id(),
            invitation,
        )
        .await
        .map_err(AgentError::from)?;
        Ok(SenderInvitationRecordCapability {
            runtime_owner: effects,
            invitation,
        })
    }

    /// Preserve the original issued selector through required sender hydration.
    /// The raw ceremony ID is consumed only by the initial protected selector.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "sender_invitation_record",
        capability_type = SenderInvitationRecordCapability,
        family = "runtime_helper"
    )]
    pub(crate) async fn created_enrollment_for_ceremony_required(
        &self,
        effects: Arc<AuraEffectSystem>,
        ceremony: &aura_core::CeremonyId,
    ) -> AgentResult<(
        SenderInvitationRecordCapability,
        enrollment_trust::RetainedEnrollmentVmControl,
    )> {
        let selector =
            enrollment_trust::RequiredIssuedEnrollmentSelectorCapability::load(effects, ceremony)
                .await?;
        let record = self
            .created_invitation_for_issued_selector_required(&selector)
            .await?;
        let control =
            enrollment_trust::RetainedEnrollmentVmControl::load_required_sender(&record).await?;
        selector.require_control(&control)?;
        Ok((record, control))
    }

    async fn created_invitation_for_issued_selector_required(
        &self,
        selector: &enrollment_trust::RequiredIssuedEnrollmentSelectorCapability,
    ) -> AgentResult<SenderInvitationRecordCapability> {
        let effects = selector.runtime_owner();
        if self.context.authority.authority_id() != effects.runtime_authority_id() {
            return Err(enrollment_trust::EnrollmentVerifierError::RuntimeOwner.into());
        }
        let invitation = required_channel_read::created_required(
            &effects,
            effects.runtime_authority_id(),
            selector.invitation_id(),
        )
        .await
        .map_err(AgentError::from)?;
        let invitation = required_channel_read::hydrate_created_enrollment_required(
            effects.as_ref(),
            effects.runtime_authority_id(),
            invitation,
        )
        .await
        .map_err(AgentError::from)?;
        Ok(SenderInvitationRecordCapability {
            runtime_owner: effects,
            invitation,
        })
    }

    /// Publish the local cancellation only after the actual enrollment terminal
    /// owner has durably won its first-decision CAS.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "verified_enrollment_cancellation",
        capability_type = AuthorizedInvitationCancellationCapability,
        family = "runtime_helper"
    )]
    pub(crate) async fn publish_verified_enrollment_cancellation(
        &self,
        effects: Arc<AuraEffectSystem>,
        prepared: AuthorizedInvitationCancellationCapability,
        cancelled: crate::runtime::services::ceremony_tracker::VerifiedEnrollmentCancellationCapability,
    ) -> AgentResult<InvitationResult> {
        cancelled
            .require_runtime_owner(&effects)
            .map_err(AgentError::from)?;
        let invitation = prepared.invitation();
        if cancelled.invitation() != &invitation.invitation_id
            || !matches!(&invitation.invitation_type,
                InvitationType::DeviceEnrollment { ceremony_id, .. }
                if ceremony_id == cancelled.ceremony())
        {
            return Err(AgentError::invalid("cancellation evidence binding differs"));
        }
        self.publish_cancelled_invitation(effects, prepared).await
    }

    /// Cancel a non-enrollment invitation (sender only). Enrollment cancellation
    /// requires the stronger actual terminal-CAS token above.
    pub async fn cancel_invitation(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation_id: &InvitationId,
    ) -> AgentResult<InvitationResult> {
        let record = self
            .created_invitation_required(effects, invitation_id)
            .await?;
        self.cancel_required_invitation(record).await
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "sender_invitation_cancellation",
        capability_type = SenderInvitationRecordCapability,
        family = "runtime_helper"
    )]
    pub(crate) async fn cancel_required_invitation(
        &self,
        record: SenderInvitationRecordCapability,
    ) -> AgentResult<InvitationResult> {
        if matches!(
            record.invitation().invitation_type,
            InvitationType::DeviceEnrollment { .. }
        ) {
            return Err(AgentError::invalid(
                "enrollment cancellation requires terminal owner",
            ));
        }
        let effects = record.runtime_owner();
        let prepared = self.prepare_invitation_cancellation(record).await?;
        self.publish_cancelled_invitation(effects, prepared).await
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "issued_enrollment_cancellation",
        capability_type = AuthorizedInvitationCancellationCapability,
        family = "runtime_helper"
    )]
    pub(crate) async fn prepare_enrollment_cancellation(
        &self,
        issued: &enrollment_trust::RetainedEnrollmentVmControl,
        record: SenderInvitationRecordCapability,
    ) -> AgentResult<AuthorizedInvitationCancellationCapability> {
        issued
            .require_runtime_owner(record.runtime_owner.as_ref())
            .map_err(AgentError::from)?;
        if issued.canonical_invitation().invitation_id != record.invitation().invitation_id
            || issued.canonical_invitation().sender_id != record.invitation().sender_id
            || issued.canonical_invitation().context_id != record.invitation().context_id
        {
            return Err(AgentError::invalid(
                "issued cancellation source record differs",
            ));
        }
        self.prepare_invitation_cancellation(record).await
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "prepared_invitation_cancellation",
        capability_type = AuthorizedInvitationCancellationCapability,
        family = "authorizer"
    )]
    pub(crate) async fn prepare_invitation_cancellation(
        &self,
        record: SenderInvitationRecordCapability,
    ) -> AgentResult<AuthorizedInvitationCancellationCapability> {
        let SenderInvitationRecordCapability {
            runtime_owner,
            invitation,
        } = record;

        if invitation.status != InvitationStatus::Pending
            && invitation.status != InvitationStatus::Cancelled
        {
            return Err(AgentError::invalid(
                "only a pending invitation can be cancelled",
            ));
        }
        let snapshot = self
            .cancellation_snapshot_required(runtime_owner.as_ref())
            .await?;
        let causal = crate::handlers::shared::stamp_invitation_outcome_causal(
            runtime_owner.as_ref(),
            self.context.authority.authority_id(),
            &invitation.invitation_id,
        )
        .await?;
        let outcome =
            self.service
                .prepare_cancel_invitation(&snapshot, &invitation.invitation_id, causal);
        if outcome.is_denied() {
            return Err(AgentError::from(aura_core::AuraError::permission_denied(
                aura_invitation::guards::denial_reason(&outcome),
            )));
        }
        Ok(AuthorizedInvitationCancellationCapability {
            runtime_owner,
            invitation,
            outcome,
        })
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "prepared_invitation_cancellation",
        capability_type = AuthorizedInvitationCancellationCapability,
        family = "runtime_helper"
    )]
    async fn publish_cancelled_invitation(
        &self,
        effects: Arc<AuraEffectSystem>,
        prepared: AuthorizedInvitationCancellationCapability,
    ) -> AgentResult<InvitationResult> {
        if !Arc::ptr_eq(&effects, &prepared.runtime_owner) {
            return Err(AgentError::from(aura_core::AuraError::Invalid {
                message: "cancellation publication has another prepared runtime owner".into(),
                source: Some(Arc::new(
                    enrollment_trust::EnrollmentVerifierError::RuntimeOwner,
                )),
            }));
        }
        let AuthorizedInvitationCancellationCapability {
            runtime_owner: _,
            invitation,
            outcome,
        } = prepared;
        if invitation.status == InvitationStatus::Cancelled {
            return Ok(InvitationResult::new(
                invitation.invitation_id,
                InvitationStatus::Cancelled,
            ));
        }
        execute_guard_outcome(outcome, &self.context.authority, effects.as_ref()).await?;
        let mut cancelled = invitation;
        cancelled.status = InvitationStatus::Cancelled;
        // The required record may contain redacted enrollment secrets. Updating
        // this status must not replace the separately retained secret payload.
        let regular = InvitationCacheHandler::redact_device_enrollment_payload(&cancelled);
        let bytes = serde_json::to_vec(&regular).map_err(|source| {
            AgentError::from(aura_core::AuraError::Serialization {
                message: "encode cancelled sender invitation".into(),
                source: Some(Arc::new(source)),
            })
        })?;
        effects
            .store(
                &InvitationCacheHandler::created_invitation_key(
                    self.context.authority.authority_id(),
                    &cancelled.invitation_id,
                ),
                bytes,
            )
            .await
            .map_err(|source| {
                AgentError::from(aura_core::AuraError::Storage {
                    message: "persist cancelled sender invitation".into(),
                    source: Some(Arc::new(source)),
                })
            })?;
        let id = cancelled.invitation_id.clone();
        self.invitation_cache.cache_invitation(cancelled).await;
        Ok(InvitationResult::new(id, InvitationStatus::Cancelled))
    }

    async fn cancellation_snapshot_required(
        &self,
        effects: &AuraEffectSystem,
    ) -> AgentResult<GuardSnapshot> {
        let now = effects.physical_time().await.map_err(|source| {
            AgentError::from(aura_core::AuraError::Internal {
                message: "required cancellation clock".into(),
                source: Some(Arc::new(source)),
            })
        })?;
        let mut capabilities = Vec::new();
        if let Some((token, bridge)) = effects.verified_biscuit_frontier().map_err(|source| {
            AgentError::from(aura_core::AuraError::Internal {
                message: "required cancellation capability frontier".into(),
                source: Some(Arc::new(source)),
            })
        })? {
            for capability in evaluation_candidates_for_invitation_guard() {
                let name: CapabilityName = capability.as_name();
                if bridge
                    .has_capability_with_time(&token, name.as_str(), Some(now.ts_ms / 1_000))
                    .map_err(|source| {
                        AgentError::from(aura_core::AuraError::Internal {
                            message: "required cancellation capability evaluation".into(),
                            source: Some(Arc::new(source)),
                        })
                    })?
                {
                    capabilities.push(name);
                }
            }
        }
        let context = self.context.effect_context.context_id();
        let budget = aura_core::effects::JournalEffects::get_flow_budget(
            effects,
            &context,
            &self.context.authority.authority_id(),
        )
        .await
        .map_err(AgentError::from)?;
        Ok(GuardSnapshot::new(
            self.context.authority.authority_id(),
            context,
            FlowCost::new(u32::try_from(budget.remaining()).unwrap_or(u32::MAX)),
            capabilities,
            u64::from(budget.epoch),
            now.ts_ms,
        ))
    }

    /// List pending invitations (from cache)
    pub async fn list_pending(&self) -> Vec<Invitation> {
        self.invitation_cache
            .list_matching(|inv| inv.status == InvitationStatus::Pending)
            .await
    }

    /// List cached invitations matching a predicate.
    pub async fn list_cached_matching(
        &self,
        predicate: impl Fn(&Invitation) -> bool,
    ) -> Vec<Invitation> {
        self.invitation_cache.list_matching(predicate).await
    }

    /// List invitations from cache plus persisted stores.
    /// Read channel invitations required by canonical participant augmentation.
    /// Required storage, decode, context and time failures are propagated;
    /// enrollment payload restoration is outside this channel-only read.
    pub async fn list_channel_invitations_with_storage_required(
        &self,
        effects: &AuraEffectSystem,
    ) -> Result<Vec<Invitation>, aura_core::AuraError> {
        required_channel_read::list_required(
            effects,
            self.context.authority.authority_id(),
            self.list_cached_matching(|_| true).await,
        )
        .await
    }

    /// Observed-only best-effort listing; not an authoritative absence/readiness API.
    pub async fn list_with_storage(&self, effects: &AuraEffectSystem) -> Vec<Invitation> {
        let mut invitations: HashMap<InvitationId, Invitation> = HashMap::new();
        for invitation in self.list_cached_matching(|_| true).await {
            InvitationCacheHandler::merge_invitation(&mut invitations, invitation);
        }
        let own_id = self.context.authority.authority_id();
        let now_ms = Self::best_effort_current_timestamp_ms(effects).await;

        let created_prefix = InvitationCacheHandler::created_invitation_prefix(own_id);
        if let Ok(keys) = effects.list_keys(Some(&created_prefix)).await {
            for key in keys {
                let Ok(Some(bytes)) = effects.retrieve(&key).await else {
                    continue;
                };
                let Ok(invitation) = serde_json::from_slice::<Invitation>(&bytes) else {
                    continue;
                };
                // Stored device enrollments are redacted; restore the secure payload
                // before caching, or later readers see an empty enrollment.
                let invitation = if matches!(
                    invitation.invitation_type,
                    InvitationType::DeviceEnrollment { .. }
                ) {
                    match InvitationHandler::load_created_invitation(
                        effects,
                        own_id,
                        &invitation.invitation_id,
                    )
                    .await
                    {
                        Some(restored) => restored,
                        None => continue,
                    }
                } else {
                    invitation
                };
                self.invitation_cache
                    .cache_invitation(invitation.clone())
                    .await;
                InvitationCacheHandler::merge_invitation(&mut invitations, invitation);
            }
        }

        let imported_prefix = InvitationCacheHandler::imported_invitation_prefix(own_id);
        if let Ok(keys) = effects.list_keys(Some(&imported_prefix)).await {
            for key in keys {
                let Ok(Some(bytes)) = effects.retrieve(&key).await else {
                    continue;
                };
                let preserved = serde_json::from_slice::<ShareableInvitation>(&bytes)
                    .ok()
                    .and_then(|shareable| invitations.get(&shareable.invitation_id));
                let Some(stored) =
                    InvitationCacheHandler::parse_imported_invitation_bytes(&bytes, preserved)
                else {
                    continue;
                };
                // Stored device enrollments are redacted; restore the secure payload.
                let stored = if matches!(
                    stored.shareable.invitation_type,
                    InvitationType::DeviceEnrollment { .. }
                ) {
                    match InvitationHandler::load_imported_invitation(
                        effects,
                        own_id,
                        &stored.invitation_id,
                        preserved,
                    )
                    .await
                    {
                        Some(restored) => restored,
                        None => continue,
                    }
                } else {
                    stored
                };
                let status = stored.status.clone();
                let created_at = stored.created_at;
                let shareable = stored.shareable;

                let context_id = match &shareable.invitation_type {
                    InvitationType::Channel { .. } => match require_channel_invitation_context(
                        &shareable.invitation_id,
                        shareable.sender_id,
                        shareable.context_id,
                    ) {
                        Ok(context_id) => context_id,
                        Err(error) => {
                            tracing::warn!(
                                invitation_id = %shareable.invitation_id,
                                sender = %shareable.sender_id,
                                error = %error,
                                "Skipping imported channel invitation without authoritative context"
                            );
                            continue;
                        }
                    },
                    _ => self.context.effect_context.context_id(),
                };

                let invitation = Invitation {
                    invitation_id: shareable.invitation_id,
                    context_id,
                    sender_id: shareable.sender_id,
                    receiver_id: imported_invitation_receiver(&shareable.invitation_type, own_id),
                    invitation_type: shareable.invitation_type,
                    status,
                    created_at: if created_at == 0 { now_ms } else { created_at },
                    expires_at: shareable.expires_at,
                    message: shareable.message,
                    receiver_nickname: None,
                };

                let should_cache =
                    invitations
                        .get(&invitation.invitation_id)
                        .map_or(true, |existing| {
                            InvitationCacheHandler::should_replace_invitation(existing, &invitation)
                        });
                if should_cache {
                    self.invitation_cache
                        .cache_invitation(invitation.clone())
                        .await;
                }
                InvitationCacheHandler::merge_invitation(&mut invitations, invitation);
            }
        }

        invitations.into_values().collect()
    }

    /// List pending invitations from cache plus persisted stores.
    ///
    /// This allows runtime components using separate handler instances to
    /// converge on a shared pending invitation view.
    pub async fn list_pending_with_storage(&self, effects: &AuraEffectSystem) -> Vec<Invitation> {
        self.list_with_storage(effects)
            .await
            .into_iter()
            .filter(|inv| inv.status == InvitationStatus::Pending)
            .collect()
    }

    /// Get an invitation by ID (from in-memory cache only)
    pub async fn get_invitation(&self, invitation_id: &InvitationId) -> Option<Invitation> {
        InvitationCacheHandler::new(self)
            .get_invitation(invitation_id)
            .await
    }

    /// Get an invitation by ID, checking both cache and persistent storage
    pub async fn get_invitation_with_storage(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
    ) -> Option<Invitation> {
        InvitationCacheHandler::new(self)
            .get_invitation_with_storage(effects, invitation_id)
            .await
    }
}

// =============================================================================
// Guard Outcome Execution (effect commands)
// =============================================================================

/// Execute a guard outcome's effect commands.
///
/// Takes a `GuardOutcome` from `aura_invitation::InvitationService` and
/// executes each `EffectCommand` using the agent's effect system.
pub async fn execute_guard_outcome(
    outcome: aura_invitation::guards::GuardOutcome,
    authority: &AuthorityContext,
    effects: &AuraEffectSystem,
) -> AgentResult<()> {
    if outcome.is_denied() {
        let reason = aura_invitation::guards::denial_reason(&outcome);
        return Err(AgentError::effects(format!(
            "Guard denied operation: {}",
            reason
        )));
    }

    let local_context_id = authority.default_context_id();
    let charge_peer =
        resolve_charge_peer(
            &outcome.effects,
            authority.authority_id(),
            |command| match command {
                aura_invitation::guards::EffectCommand::NotifyPeer { peer, .. } => Some(*peer),
                aura_invitation::guards::EffectCommand::RecordReceipt { peer, .. } => *peer,
                _ => None,
            },
        );
    let charge_context_id = default_context_id_for_authority(charge_peer);
    let mut pending_receipt: Option<Receipt> = None;

    for command in outcome.effects {
        execute_effect_command(
            command,
            authority,
            local_context_id,
            charge_context_id,
            effects,
            charge_peer,
            &mut pending_receipt,
            false,
        )
        .await?;
    }

    Ok(())
}

pub async fn execute_guard_outcome_for_accept(
    outcome: aura_invitation::guards::GuardOutcome,
    authority: &AuthorityContext,
    effects: &AuraEffectSystem,
) -> AgentResult<()> {
    let execution_plan = aura_invitation::guards::plan_accept_execution(outcome)
        .map_err(|reason| AgentError::effects(format!("Guard denied operation: {reason}")))?;
    tracing::debug!(
        authority = %authority.authority_id(),
        local_effect_count = execution_plan.local_effects.len(),
        deferred_network_effect_count = execution_plan.deferred_network_effects.len(),
        "Prepared invitation accept guard outcome with deferred peer notification side effects"
    );
    execute_invitation_effect_commands(execution_plan.local_effects, authority, effects, false)
        .await?;
    if let Err(error) = execute_invitation_effect_commands(
        execution_plan.deferred_network_effects,
        authority,
        effects,
        true,
    )
    .await
    {
        tracing::warn!(
            authority = %authority.authority_id(),
            error = %error,
            "accept invitation continuing after deferred network side-effect failure"
        );
    }
    Ok(())
}

pub(crate) async fn execute_invitation_effect_commands(
    commands: Vec<aura_invitation::guards::EffectCommand>,
    authority: &AuthorityContext,
    effects: &AuraEffectSystem,
    best_effort_network_failures: bool,
) -> AgentResult<()> {
    let local_context_id = authority.default_context_id();
    let charge_peer =
        resolve_charge_peer(
            &commands,
            authority.authority_id(),
            |command| match command {
                aura_invitation::guards::EffectCommand::NotifyPeer { peer, .. } => Some(*peer),
                aura_invitation::guards::EffectCommand::RecordReceipt { peer, .. } => *peer,
                _ => None,
            },
        );
    let charge_context_id = default_context_id_for_authority(charge_peer);
    let mut pending_receipt: Option<Receipt> = None;

    for command in commands {
        let is_network_side_effect = matches!(
            &command,
            aura_invitation::guards::EffectCommand::ChargeFlowBudget { .. }
                | aura_invitation::guards::EffectCommand::NotifyPeer { .. }
                | aura_invitation::guards::EffectCommand::RecordReceipt { .. }
        );

        let result = if best_effort_network_failures && is_network_side_effect {
            timeout_deferred_network_stage(
                effects,
                "accept_network_side_effect",
                execute_effect_command(
                    command,
                    authority,
                    local_context_id,
                    charge_context_id,
                    effects,
                    charge_peer,
                    &mut pending_receipt,
                    best_effort_network_failures,
                ),
            )
            .await
        } else {
            execute_effect_command(
                command,
                authority,
                local_context_id,
                charge_context_id,
                effects,
                charge_peer,
                &mut pending_receipt,
                best_effort_network_failures,
            )
            .await
        };

        match result {
            Ok(()) => {}
            Err(error) if best_effort_network_failures && is_network_side_effect => {
                tracing::warn!(
                    authority = %authority.authority_id(),
                    context = %charge_context_id,
                    "Invitation side effect continuing after best-effort network failure: {}",
                    error
                );
            }
            Err(error) => return Err(error),
        }
    }

    Ok(())
}

fn execute_effect_command<'a>(
    command: aura_invitation::guards::EffectCommand,
    authority: &'a AuthorityContext,
    local_context_id: ContextId,
    charge_context_id: ContextId,
    effects: &'a AuraEffectSystem,
    charge_peer: AuthorityId,
    pending_receipt: &'a mut Option<Receipt>,
    best_effort_network_failures: bool,
) -> impl Future<Output = AgentResult<()>> + 'a {
    // Keep the entire typed dispatch out of its loop/timeout caller's frame.
    // The same lexical owner retains the command and its mutable receipt.
    Box::pin(execute_effect_command_owned(
        command,
        authority,
        local_context_id,
        charge_context_id,
        effects,
        charge_peer,
        pending_receipt,
        best_effort_network_failures,
    ))
}

async fn execute_effect_command_owned(
    command: aura_invitation::guards::EffectCommand,
    authority: &AuthorityContext,
    local_context_id: ContextId,
    charge_context_id: ContextId,
    effects: &AuraEffectSystem,
    charge_peer: AuthorityId,
    pending_receipt: &mut Option<Receipt>,
    best_effort_network_failures: bool,
) -> AgentResult<()> {
    match command {
        aura_invitation::guards::EffectCommand::JournalAppend { fact } => {
            execute_journal_append(fact, authority, local_context_id, effects).await
        }
        aura_invitation::guards::EffectCommand::ChargeFlowBudget { cost } => {
            *pending_receipt =
                execute_charge_flow_budget(cost, charge_context_id, charge_peer, effects).await?;
            Ok(())
        }
        aura_invitation::guards::EffectCommand::NotifyPeer {
            peer,
            invitation_id,
        } => {
            execute_notify_peer(
                peer,
                invitation_id,
                authority,
                pending_receipt.clone(),
                effects,
                best_effort_network_failures,
            )
            .await
        }
        aura_invitation::guards::EffectCommand::RecordReceipt { operation, peer } => {
            execute_record_receipt(
                operation,
                peer,
                charge_context_id,
                pending_receipt.take(),
                effects,
            )
            .await
        }
    }
}

async fn execute_journal_append(
    fact: InvitationFact,
    authority: &AuthorityContext,
    local_context_id: ContextId,
    effects: &AuraEffectSystem,
) -> AgentResult<()> {
    // A fact that names its context is journaled there: the required invitation
    // projection rejects a payload context that differs from the journal
    // context, and that rejection stops the reactive pipeline. Context-free
    // facts stay in the local default context where their readers look.
    HandlerUtilities::append_domain_fact(
        authority,
        effects,
        fact.context_id_opt().unwrap_or(local_context_id),
        &fact,
    )
    .await
}

async fn execute_charge_flow_budget(
    cost: aura_core::FlowCost,
    context_id: ContextId,
    peer: AuthorityId,
    effects: &AuraEffectSystem,
) -> AgentResult<Option<Receipt>> {
    emit_browser_harness_debug_event("invite_charge_begin", &format!("{context_id}:{peer}"));
    // Deterministic modes charge through the same flow-budget path as
    // production so receipt routing (context/src/dst) is validated in-process.
    let receipt = effects
        .charge_flow(&context_id, &peer, cost)
        .await
        .map_err(|e| {
            emit_browser_harness_debug_event("invite_charge_err", &e.to_string());
            AgentError::effects(format!("Failed to charge invitation flow: {e}"))
        })?;
    emit_browser_harness_debug_event("invite_charge_ok", "");
    Ok(Some(receipt))
}

async fn seed_peer_descriptor_for_authority_context(
    authority: &AuthorityContext,
    effects: &AuraEffectSystem,
    peer: AuthorityId,
) {
    let Some(rendezvous_manager) = effects.rendezvous_manager() else {
        return;
    };

    let authority_context = default_context_id_for_authority(peer);
    if rendezvous_manager
        .get_descriptor(authority_context, peer)
        .await
        .is_some()
    {
        return;
    }

    let local_context_id = authority.default_context_id();
    let existing = rendezvous_manager
        .get_descriptor(local_context_id, peer)
        .await;
    let discovered = rendezvous_manager
        .get_lan_discovered_peer(peer)
        .await
        .map(|peer| peer.descriptor);
    let descriptor =
        match (existing, discovered) {
            (Some(existing), Some(discovered))
                if discovered.transport_hints.iter().any(|hint| {
                    matches!(hint, aura_rendezvous::TransportHint::TcpDirect { .. })
                }) && !existing.transport_hints.iter().any(|hint| {
                    matches!(hint, aura_rendezvous::TransportHint::TcpDirect { .. })
                }) =>
            {
                Some(discovered)
            }
            (Some(existing), _) => Some(existing),
            (None, Some(discovered)) => Some(discovered),
            (None, None) => None,
        };

    let Some(mut descriptor) = descriptor else {
        return;
    };

    descriptor.context_id = authority_context;
    let _ = rendezvous_manager.cache_descriptor(descriptor).await;
}

async fn signed_invitation_code_for_notify(
    effects: &AuraEffectSystem,
    invitation: &Invitation,
) -> AgentResult<String> {
    let transport_metadata = ShareableInvitationTransportMetadata {
        sender_hint: effects.lan_transport().and_then(|transport| {
            transport
                .websocket_addrs()
                .first()
                .map(|addr| {
                    if addr.starts_with("ws://") || addr.starts_with("wss://") {
                        addr.clone()
                    } else {
                        format!("ws://{addr}")
                    }
                })
                .or_else(|| {
                    transport
                        .advertised_addrs()
                        .first()
                        .map(|addr| format!("tcp://{addr}"))
                })
        }),
        sender_device_id: Some(effects.device_id()),
    };

    InvitationServiceApi::export_signed_invitation_with_transport(
        effects,
        invitation,
        &transport_metadata,
        effects.is_testing(),
    )
    .await
}

async fn execute_notify_peer(
    peer: AuthorityId,
    invitation_id: InvitationId,
    authority: &AuthorityContext,
    receipt: Option<Receipt>,
    effects: &AuraEffectSystem,
    best_effort_network_failures: bool,
) -> AgentResult<()> {
    emit_browser_harness_debug_event("invite_notify_begin", &peer.to_string());
    // Use explicit test mode, not `is_testing()`: simulation runs should still
    // exercise transport delivery on the shared deterministic network.
    if effects.is_test_mode() {
        emit_browser_harness_debug_event("invite_notify_test_mode", "");
        return Ok(());
    }

    if peer == authority.authority_id() {
        // Self-addressed invitations are intended for out-of-band sharing.
        // Skip network notify when inviting ourselves.
        emit_browser_harness_debug_event("invite_notify_self", "");
        return Ok(());
    }

    seed_peer_descriptor_for_authority_context(authority, effects, peer).await;

    let authority_id = authority.authority_id();
    let (code, invitation_context) = if let Some(invitation) =
        InvitationHandler::load_created_invitation(effects, authority_id, &invitation_id).await
    {
        (
            signed_invitation_code_for_notify(effects, &invitation).await?,
            invitation.context_id,
        )
    } else {
        let envelopes =
            load_relational_fact_envelopes_by_type(effects, authority_id, INVITATION_FACT_TYPE_ID)
                .await
                .map_err(|_| {
                    AgentError::context(format!("Invitation not found for notify: {invitation_id}"))
                })?;

        let mut shareable: Option<(ShareableInvitation, ContextId)> = None;
        for envelope in &envelopes {
            let Some(inv_fact) = InvitationFact::from_envelope(envelope) else {
                continue;
            };

            let InvitationFact::Sent {
                invitation_id: seen_id,
                sender_id,
                context_id,
                invitation_type,
                expires_at,
                message,
                ..
            } = inv_fact
            else {
                continue;
            };

            if seen_id != invitation_id {
                continue;
            }

            shareable = Some((
                ShareableInvitation {
                    version: ShareableInvitation::CURRENT_VERSION,
                    invitation_id: invitation_id.clone(),
                    sender_id,
                    context_id: Some(context_id),
                    invitation_type,
                    expires_at: expires_at.map(|time| time.ts_ms),
                    message,
                },
                context_id,
            ));
            break;
        }

        let (shareable, _context_id) = shareable.ok_or_else(|| {
            AgentError::context(format!("Invitation not found for notify: {invitation_id}"))
        })?;

        let _ = shareable;
        return Err(AgentError::invalid(
            ShareableInvitationError::MissingSenderProof.to_string(),
        ));
    };
    let mut metadata = HashMap::new();
    metadata.insert(
        "content-type".to_string(),
        "application/aura-invitation".to_string(),
    );
    metadata.insert("invitation-id".to_string(), invitation_id.to_string());
    metadata.insert(
        "invitation-context".to_string(),
        invitation_context.to_string(),
    );
    tracing::info!(
        destination = %peer,
        invitation_context = %invitation_context,
        code_has_context_field = code.contains("\"context_id\""),
        "Sending invitation envelope"
    );
    emit_browser_harness_debug_event("invite_notify_send", &peer.to_string());

    // The invitation establishes or extends semantic access to `invitation_context`,
    // but the transport envelope itself must ride over the receiver's already-
    // materialized authority-scoped peer path rather than assuming the invitee is
    // already routable on the invitation context itself.
    let delivery_context = default_context_id_for_authority(peer);

    let transport_receipt = receipt.and_then(|receipt| {
        if receipt.ctx == delivery_context {
            Some(transport_receipt_from_flow(receipt))
        } else {
            tracing::debug!(
                invitation_id = %invitation_id,
                peer = %peer,
                invitation_context = %invitation_context,
                delivery_context = %delivery_context,
                receipt_context = %receipt.ctx,
                "Dropping invitation transport receipt because delivery uses the authority-scoped peer context"
            );
            None
        }
    });

    let mut envelope = TransportEnvelope {
        destination: peer,
        source: authority.authority_id(),
        context: delivery_context,
        payload: code.into_bytes(),
        metadata,
        receipt: transport_receipt,
    };
    attach_invitation_test_receipt_if_needed(effects, &mut envelope);

    if best_effort_network_failures {
        if let Err(error) =
            attempt_network_send_envelope(effects, "notify peer with invitation failed", envelope)
                .await
        {
            emit_browser_harness_debug_event("invite_notify_error", &error.to_string());
            return Err(error);
        }
    } else if let Err(error) = send_guarded_transport_envelope(effects, envelope).await {
        emit_browser_harness_debug_event("invite_notify_error", &error.to_string());
        return Err(AgentError::effects(format!(
            "Failed to notify peer with invitation: {error}"
        )));
    }
    emit_browser_harness_debug_event("invite_notify_ok", &peer.to_string());

    Ok(())
}

fn contact_invitation_acceptance_transcript(
    invitation: &Invitation,
    acceptor_id: AuthorityId,
    nickname_suggestion: Option<String>,
) -> ContactInvitationAcceptanceTranscript<'_> {
    ContactInvitationAcceptanceTranscript {
        invitation,
        acceptor_id,
        nickname_suggestion,
    }
}

/// Best-effort read of this account's nickname for invitation acceptances.
async fn local_account_nickname_suggestion(effects: &AuraEffectSystem) -> Option<String> {
    use aura_core::effects::StorageCoreEffects;
    let bytes = effects.retrieve("account.json").await.ok().flatten()?;
    serde_json::from_slice::<aura_app::views::account::AccountConfig>(&bytes)
        .ok()?
        .nickname_suggestion
        .filter(|name| !name.trim().is_empty())
}

fn channel_invitation_acceptance_transcript(
    invitation: &Invitation,
    acceptor_id: AuthorityId,
    context_id: ContextId,
    channel_id: ChannelId,
    channel_name: Option<String>,
) -> ChannelInvitationAcceptanceTranscript<'_> {
    ChannelInvitationAcceptanceTranscript {
        invitation,
        acceptor_id,
        context_id,
        channel_id,
        channel_name,
    }
}

/// Receiver of an imported invitation: the importing authority, except a device
/// enrollment, which names the authority it invited (the new device re-imports
/// and rehydrates the code after its runtime switches to the subject authority).
pub(super) fn imported_invitation_receiver(
    invitation_type: &InvitationType,
    own_id: AuthorityId,
) -> AuthorityId {
    match invitation_type {
        InvitationType::DeviceEnrollment {
            invitee_authority: Some(invitee),
            ..
        } => *invitee,
        _ => own_id,
    }
}

/// Unit-fixture conversion for unsigned codes. This is compiled only into the
/// agent's own tests and cannot mint the domain validated-import token.
#[cfg(test)]
fn unverified_test_invitation(
    shareable: &ShareableInvitation,
    own_id: AuthorityId,
    default_context_id: ContextId,
    now_ms: u64,
) -> AgentResult<Invitation> {
    if shareable
        .expires_at
        .is_some_and(|expires_at| now_ms > expires_at)
    {
        return Err(AgentError::invalid("invite code expired"));
    }
    let context_id = if matches!(shareable.invitation_type, InvitationType::Channel { .. }) {
        require_channel_invitation_context(
            &shareable.invitation_id,
            shareable.sender_id,
            shareable.context_id,
        )?
    } else {
        default_context_id
    };
    Ok(Invitation {
        invitation_id: shareable.invitation_id.clone(),
        context_id,
        sender_id: shareable.sender_id,
        receiver_id: imported_invitation_receiver(&shareable.invitation_type, own_id),
        invitation_type: shareable.invitation_type.clone(),
        status: InvitationStatus::Pending,
        created_at: now_ms,
        expires_at: shareable.expires_at,
        message: shareable.message.clone(),
        receiver_nickname: None,
    })
}
async fn sign_invitation_acceptance_transcript<T>(
    effects: &AuraEffectSystem,
    authority: AuthorityId,
    transcript: &T,
) -> AgentResult<ThresholdSignature>
where
    T: SecurityTranscript + ?Sized,
{
    let payload = transcript.transcript_bytes().map_err(|source| {
        AgentError::from(aura_core::AuraError::Serialization {
            message: "encode invitation acceptance transcript".into(),
            source: Some(Arc::new(source)),
        })
    })?;
    effects
        .sign(SigningContext {
            authority,
            operation: SignableOperation::Message {
                domain: T::DOMAIN_SEPARATOR.to_string(),
                payload,
            },
            approval_context: ApprovalContext::SelfOperation,
        })
        .await
        .map_err(AgentError::from)
}

async fn verify_invitation_acceptance_signature<T>(
    effects: &AuraEffectSystem,
    authority: AuthorityId,
    transcript: &T,
    signature: &ThresholdSignature,
) -> AgentResult<()>
where
    T: SecurityTranscript + ?Sized,
{
    if signature.signature.is_empty() {
        return Err(AgentError::invalid(
            "invitation acceptance signature must be non-empty".to_string(),
        ));
    }
    if signature.public_key_package.is_empty() {
        return Err(AgentError::invalid(
            "invitation acceptance public key package must be present".to_string(),
        ));
    }

    let mode = if signature.is_single_signer() {
        SigningMode::SingleSigner
    } else {
        SigningMode::Threshold
    };
    let payload = transcript.transcript_bytes().map_err(|source| {
        AgentError::from(aura_core::AuraError::Serialization {
            message: "encode invitation acceptance verification transcript".into(),
            source: Some(Arc::new(source)),
        })
    })?;
    let verification_message = threshold_signing_context_transcript_bytes(
        &SigningContext {
            authority,
            operation: SignableOperation::Message {
                domain: T::DOMAIN_SEPARATOR.to_string(),
                payload,
            },
            approval_context: ApprovalContext::SelfOperation,
        },
        signature.epoch,
    )
    .map_err(|source| {
        AgentError::from(aura_core::AuraError::Serialization {
            message: "encode invitation threshold verification context".into(),
            source: Some(Arc::new(source)),
        })
    })?;
    let verified = effects
        .verify_signature(
            &verification_message,
            signature.signature_bytes(),
            signature.public_key_bytes(),
            mode,
        )
        .await
        .map_err(AgentError::from)?;
    if !verified {
        return Err(AgentError::invalid(
            "invitation acceptance signature verification failed".to_string(),
        ));
    }
    Ok(())
}

fn transport_receipt_from_flow(receipt: Receipt) -> TransportReceipt {
    TransportReceipt {
        context: receipt.ctx,
        src: receipt.src,
        dst: receipt.dst,
        epoch: receipt.epoch.value(),
        cost: receipt.cost.value(),
        nonce: receipt.nonce.value(),
        prev: receipt.prev.0,
        sig: receipt.sig.into_bytes(),
    }
}

fn attach_invitation_test_receipt_if_needed(
    effects: &AuraEffectSystem,
    envelope: &mut TransportEnvelope,
) {
    crate::runtime::receipt_model::attach_test_transport_receipt_if_needed(
        effects.is_testing(),
        envelope,
    );
}

async fn execute_record_receipt(
    operation: InvitationOperation,
    peer: Option<AuthorityId>,
    context_id: ContextId,
    receipt: Option<Receipt>,
    effects: &AuraEffectSystem,
) -> AgentResult<()> {
    // Deterministic testing/simulation modes do not persist transport receipts.
    if effects.is_testing() {
        return Ok(());
    }

    let Some(receipt) = receipt else {
        tracing::debug!(
            operation = %operation,
            peer = ?peer,
            context = %context_id,
            "Invitation receipt recording skipped (no receipt available)"
        );
        return Ok(());
    };

    let peer_id = peer.unwrap_or(receipt.dst);
    let operation_key = match operation {
        InvitationOperation::SendInvitation => "send_invitation",
        InvitationOperation::AcceptInvitation => "accept_invitation",
        InvitationOperation::DeclineInvitation => "decline_invitation",
        InvitationOperation::CancelInvitation => "cancel_invitation",
        InvitationOperation::Ceremony => "invitation_ceremony",
    };
    let key = format!(
        "invitation/receipts/{}/{}/{}/{}",
        receipt.ctx, peer_id, operation_key, receipt.nonce
    );
    let bytes = serde_json::to_vec(&receipt)
        .map_err(|e| AgentError::effects(format!("Failed to serialize invitation receipt: {e}")))?;
    effects
        .store(&key, bytes)
        .await
        .map_err(|e| AgentError::effects(format!("Failed to store invitation receipt: {e}")))?;
    Ok(())
}

#[cfg(test)]
#[test]
fn unpolled_invitation_command_dispatch_caller_frame_is_bounded() {
    fn frame_bytes<A, F: Future>(_: impl FnOnce(A) -> F) -> usize {
        std::mem::size_of::<F>()
    }
    // Infer the real production dispatcher type without constructing any
    // command, owner, receipt, effect handler or future.
    let bytes = frame_bytes(
        |(command, authority, context, charge_context, effects, peer, receipt): (
            aura_invitation::guards::EffectCommand,
            &'static AuthorityContext,
            ContextId,
            ContextId,
            &'static AuraEffectSystem,
            AuthorityId,
            &'static mut Option<Receipt>,
        )| {
            execute_effect_command(
                command,
                authority,
                context,
                charge_context,
                effects,
                peer,
                receipt,
                false,
            )
        },
    );
    assert!(bytes <= 16 * 1024,
        "invitation command dispatch caller frame is {bytes} bytes; bounded lexical delegation is required");
}

impl InvitationHandler {
    /// Pure preview under the actual borrowed reservation. Enrollment has no
    /// context lookup: the handler's original effect context is authoritative.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "ReservedInvitationIssuance",
        family = "runtime_helper"
    )]
    pub(crate) fn preview_reserved_device_enrollment(
        &self,
        effects: &AuraEffectSystem,
        reserved: &ReservedInvitationIssuance,
        invitation_type: InvitationType,
    ) -> AgentResult<Invitation> {
        HandlerUtilities::validate_authority_context(&self.context.authority)?;
        let sender = self.context.authority.authority_id();
        let InvitationType::DeviceEnrollment {
            subject_authority,
            invitee_authority: Some(receiver),
            initiator_device_id,
            ..
        } = &invitation_type
        else {
            return Err(AgentError::invalid(
                "owned preview requires addressed device enrollment",
            ));
        };
        if !reserved.owns_effects(effects)
            || reserved.issuer_binding() != (sender, effects.device_id())
            || *subject_authority != sender
            || *initiator_device_id != effects.device_id()
        {
            return Err(AgentError::invalid(
                "owned enrollment preview reservation binding changed",
            ));
        }
        Ok(Invitation {
            invitation_id: reserved.invitation_id().clone(),
            context_id: self.context.effect_context.context_id(),
            sender_id: sender,
            receiver_id: *receiver,
            invitation_type,
            status: InvitationStatus::Pending,
            created_at: reserved.created_at_ms(),
            expires_at: None,
            message: None,
            receiver_nickname: None,
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    include!("invitation/tests.rs");
    include!("invitation/distributed_tests.rs");
}
