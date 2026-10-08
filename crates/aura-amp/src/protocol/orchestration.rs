//! AMP transport integration
//!
//! Glue between AMP ratchet helpers, guard chain, and journal operations.
//! Telemetry is handled via tracing spans for timing and structured logging.

use super::telemetry::{create_window_validation_result, WindowValidationResult, AMP_TELEMETRY};
use crate::config::AmpRuntimeConfig;
use crate::consensus::finalize_amp_bump_with_journal_default;
use crate::core::{nonce_from_header, ratchet_from_epoch_state, send_ratchet_from_epoch_state};
use crate::get_channel_state;
use crate::wire::{deserialize_message, serialize_message, AmpMessage, AMP_WIRE_SCHEMA_VERSION};
use crate::{AmpEvidenceEffects, AmpJournalEffects};
use aura_core::effects::amp::{AmpCiphertext, AmpHeader};
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::effects::{
    CryptoEffects, NetworkEffects, RandomEffects, SecureStorageCapability, SecureStorageEffects,
    SecureStorageLocation,
};
use aura_core::frost::{PublicKeyPackage, Share};
use aura_core::ownership::sequence::{Admission, DurableSequenceOwner, SequenceStore};
use aura_core::threshold::{policy_for, AgreementMode, CeremonyFlow};
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_core::{AuraError, Result};
use aura_guards::traits::GuardContextProvider;
use aura_guards::{GuardEffects, GuardOperation, GuardOperationId};
use aura_journal::fact::{
    DkgTranscriptCommit, FactContent, ProposedChannelEpochBump, RelationalFact,
};
use aura_journal::ChannelEpochState;
use aura_transport::amp::{derive_for_recv, derive_for_send, AmpError, RatchetDerivation};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::instrument;

// AmpHeader is now unified — defined in aura-core, re-exported by aura-transport.
// No conversion functions needed.

// ============================================================================
// Types
// ============================================================================

/// Minimal receipt metadata mirroring the AMP header for auditability.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AmpReceipt {
    pub context: ContextId,
    pub channel: ChannelId,
    pub chan_epoch: u64,
    pub ratchet_gen: u64,
}

/// Convenience type pairing a receipt with decrypted payload.
#[derive(Debug, Clone)]
pub struct AmpDelivery {
    pub receipt: AmpReceipt,
    pub payload: Vec<u8>,
}

impl AmpMessage {
    pub fn receipt(&self) -> AmpReceipt {
        AmpReceipt {
            context: self.header.context,
            channel: self.header.channel,
            chan_epoch: self.header.chan_epoch,
            ratchet_gen: self.header.ratchet_gen,
        }
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn map_amp_error(err: AmpError) -> AuraError {
    AuraError::invalid(format!("AMP ratchet error: {err}"))
}

/// AEAD additional data: wire schema, header, and sender.
///
/// Recipients are deliberately excluded: each side derived them from its local
/// channel-membership view, so any membership skew between sender and receiver
/// made messages undecryptable. Audience is enforced by channel membership and
/// bootstrap key distribution.
fn amp_additional_data(header: &AmpHeader, sender: AuthorityId) -> Vec<u8> {
    let mut aad = Vec::with_capacity(2 + 32 + 32 + 8 + 8 + 32);
    aad.extend_from_slice(&AMP_WIRE_SCHEMA_VERSION.to_le_bytes());
    aad.extend_from_slice(header.context.as_bytes());
    aad.extend_from_slice(header.channel.as_bytes());
    aad.extend_from_slice(&header.chan_epoch.to_le_bytes());
    aad.extend_from_slice(&header.ratchet_gen.to_le_bytes());
    aad.extend_from_slice(&sender.to_bytes());
    aad
}

fn amp_replay_marker_location(
    namespace: &str,
    header: &AmpHeader,
    sender: AuthorityId,
) -> SecureStorageLocation {
    let mut scope = Vec::with_capacity(32 + 32 + 32 + 8);
    scope.extend_from_slice(header.context.as_bytes());
    scope.extend_from_slice(header.channel.as_bytes());
    scope.extend_from_slice(&sender.to_bytes());
    scope.extend_from_slice(&header.chan_epoch.to_le_bytes());
    let scope_hash = aura_core::Hash32::from_bytes(&scope).to_hex();
    SecureStorageLocation::with_sub_key(namespace, scope_hash, header.ratchet_gen.to_string())
}

async fn record_amp_replay_marker<E: SecureStorageEffects>(
    effects: &E,
    namespace: &str,
    header: &AmpHeader,
    sender: AuthorityId,
) -> Result<()> {
    let location = amp_replay_marker_location(namespace, header, sender);
    if effects
        .secure_exists(&location)
        .await
        .map_err(|e| AuraError::internal(format!("AMP replay marker lookup failed: {e}")))?
    {
        return Err(AuraError::invalid(format!(
            "AMP replay detected for channel {} epoch {} generation {}",
            header.channel, header.chan_epoch, header.ratchet_gen
        )));
    }
    effects
        .secure_store(&location, &[1], &[SecureStorageCapability::Write])
        .await
        .map_err(|e| AuraError::internal(format!("AMP replay marker store failed: {e}")))?;
    Ok(())
}

/// Secure-storage location of a sender's next unused ratchet generation for
/// one channel epoch.
fn amp_send_cursor_location(
    context: ContextId,
    channel: ChannelId,
    sender: AuthorityId,
    chan_epoch: u64,
) -> SecureStorageLocation {
    let mut scope = Vec::with_capacity(32 + 32 + 16 + 8);
    scope.extend_from_slice(context.as_bytes());
    scope.extend_from_slice(channel.as_bytes());
    scope.extend_from_slice(&sender.to_bytes());
    scope.extend_from_slice(&chan_epoch.to_le_bytes());
    let scope_hash = aura_core::Hash32::from_bytes(&scope).to_hex();
    SecureStorageLocation::with_sub_key("amp_send_cursor", scope_hash, "next")
}

/// Sequence domain of one sender's AMP ratchet generations. The AES-GCM
/// nonce derives from (epoch, generation), so a generation must never repeat.
pub struct AmpSendGeneration;

/// The persisted send cursor for one (context, channel, sender, epoch).
struct AmpSendCursorStore<'a, E> {
    effects: &'a E,
    location: SecureStorageLocation,
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl<E: SecureStorageEffects + Sync> SequenceStore for AmpSendCursorStore<'_, E> {
    fn identity(&self) -> aura_core::Hash32 {
        aura_core::Hash32::from_bytes(self.location.full_path().as_bytes())
    }

    async fn load(&self) -> Result<Option<u64>> {
        let exists = self
            .effects
            .secure_exists(&self.location)
            .await
            .map_err(|e| AuraError::internal(format!("AMP send cursor lookup failed: {e}")))?;
        if !exists {
            return Ok(None);
        }
        let bytes = self
            .effects
            .secure_retrieve(&self.location, &[SecureStorageCapability::Read])
            .await
            .map_err(|e| AuraError::internal(format!("AMP send cursor read failed: {e}")))?;
        <[u8; 8]>::try_from(bytes.as_slice())
            .map(|bytes| Some(u64::from_le_bytes(bytes)))
            .map_err(|_| AuraError::invalid("AMP send cursor has invalid length"))
    }

    async fn persist(&self, next: u64) -> Result<()> {
        self.effects
            .secure_store(
                &self.location,
                &next.to_le_bytes(),
                &[SecureStorageCapability::Write],
            )
            .await
            .map_err(|e| AuraError::internal(format!("AMP send cursor store failed: {e}")))
    }
}

type SendCursorOwner = DurableSequenceOwner<AmpSendGeneration>;

/// Runtime-owned registry of AMP send-generation owners, one per
/// (context, channel, sender, epoch) cursor. A reservation holds that
/// cursor's owner lock across read, persist and mint, so concurrent sends
/// never share a generation (and so never share an AES-GCM nonce).
#[derive(Default)]
pub struct AmpSendSequencer {
    owners: futures::lock::Mutex<HashMap<aura_core::Hash32, std::sync::Arc<SendCursorOwner>>>,
}

impl AmpSendSequencer {
    /// Reserve `sender`'s next unused generation, never below the reduced
    /// checkpoint generation, persisting the advanced cursor first.
    pub async fn reserve<E: SecureStorageEffects + Sync>(
        &self,
        effects: &E,
        state: &ChannelEpochState,
        ratchet_state: &aura_transport::amp::AmpRatchetState,
        context: ContextId,
        channel: ChannelId,
        sender: AuthorityId,
    ) -> Result<Admission<AmpSendGeneration>> {
        let store = AmpSendCursorStore {
            effects,
            location: amp_send_cursor_location(context, channel, sender, ratchet_state.chan_epoch),
        };
        let owner = self
            .owners
            .lock()
            .await
            .entry(store.identity())
            .or_default()
            .clone();
        let floor = state.current_gen.max(ratchet_state.last_checkpoint_gen);
        owner.reserve(&store, floor).await
    }
}

/// Access to the runtime's [`AmpSendSequencer`].
pub trait AmpSendSequencerProvider {
    /// The runtime's send-generation owners.
    fn amp_send_sequencer(&self) -> &AmpSendSequencer;
}

fn return_send_failure(context: ContextId, channel: ChannelId, error: AuraError) -> AuraError {
    AMP_TELEMETRY.log_send_failure(context, channel, &error);
    error
}

fn return_receive_failure(
    context: ContextId,
    header: Option<&AmpHeader>,
    validation: Option<&WindowValidationResult>,
    error: AuraError,
) -> AuraError {
    AMP_TELEMETRY.log_receive_failure(context, header, validation, &error);
    error
}

/// Build guard chain for AMP send operations.
fn build_amp_send_guard(
    context: ContextId,
    peer: aura_core::AuthorityId,
    config: &AmpRuntimeConfig,
) -> aura_guards::chain::SendGuardChain {
    use aura_guards::chain::create_send_guard_op;
    use aura_guards::journal::JournalCoupler;

    create_send_guard_op(
        GuardOperation::AmpSend,
        context,
        peer,
        config.default_flow_cost,
    )
    .with_operation_id(GuardOperationId::AmpSend)
    .with_journal_coupler(JournalCoupler::new())
}

fn latest_transcript_ref_from_context(
    journal: &aura_journal::FactJournal,
    context: ContextId,
) -> Option<aura_core::Hash32> {
    let mut latest: Option<DkgTranscriptCommit> = None;

    for fact in journal.facts.iter() {
        if let FactContent::Relational(RelationalFact::Protocol(
            aura_journal::ProtocolRelationalFact::DkgTranscriptCommit(commit),
        )) = &fact.content
        {
            if commit.context == context {
                latest = Some(commit.clone());
            }
        }
    }

    latest.and_then(|commit| commit.blob_ref.or(Some(commit.transcript_hash)))
}

async fn derive_bootstrap_message_key<E: SecureStorageEffects>(
    effects: &E,
    context: ContextId,
    channel: ChannelId,
    bootstrap_id: aura_core::Hash32,
    header: &AmpHeader,
    sender: AuthorityId,
) -> Result<aura_core::Hash32> {
    let location = SecureStorageLocation::amp_bootstrap_key(&context, &channel, &bootstrap_id);
    let key_bytes = effects
        .secure_retrieve(&location, &[SecureStorageCapability::Read])
        .await
        .map_err(|e| AuraError::internal(format!("Failed to read AMP bootstrap key: {e}")))?;
    message_key_from_master(&key_bytes, context, channel, header, sender)
}

/// The message key under `master` (an epoch's channel key), bound to the
/// sender: generations are per-sender and the AEAD nonce comes from the
/// header, so two senders sharing a generation must never share a key.
fn message_key_from_master(
    master: &[u8],
    context: ContextId,
    channel: ChannelId,
    header: &AmpHeader,
    sender: AuthorityId,
) -> Result<aura_core::Hash32> {
    if master.len() != 32 {
        return Err(AuraError::invalid(format!(
            "AMP channel key has invalid length: {}",
            master.len()
        )));
    }

    let mut key = [0u8; 32];
    key.copy_from_slice(master);
    let master_key = aura_core::Hash32::new(key);

    let mut key_context = Vec::with_capacity(32 + 16);
    key_context.extend_from_slice(context.as_bytes());
    key_context.extend_from_slice(&sender.to_bytes());

    aura_core::crypto::amp::derive_message_key(
        &master_key,
        &key_context,
        channel.as_bytes(),
        header.chan_epoch,
        header.ratchet_gen,
    )
    .map_err(|e| AuraError::crypto(format!("AMP message KDF failed: {e}")))
}

/// The message key of `header`: epoch 0 uses the bootstrap dealer key;
/// every later epoch uses that epoch's channel key from the members' key
/// ceremony (docs/112 §4), which only members of that epoch hold.
async fn derive_channel_message_key<E: SecureStorageEffects>(
    effects: &E,
    context: ContextId,
    channel: ChannelId,
    state: &ChannelEpochState,
    header: &AmpHeader,
    sender: AuthorityId,
) -> Result<aura_core::Hash32> {
    if header.chan_epoch >= 1 {
        let location =
            SecureStorageLocation::amp_channel_base_key(&context, &channel, header.chan_epoch);
        let key = effects
            .secure_retrieve(&location, &[SecureStorageCapability::Read])
            .await
            .map_err(|e| AuraError::PermissionDenied {
                message: format!(
                    "no channel key for epoch {} of {channel}",
                    header.chan_epoch
                ),
                source: Some(std::sync::Arc::new(e)),
            })?;
        return message_key_from_master(&key, context, channel, header, sender);
    }
    let bootstrap = state.bootstrap.as_ref().ok_or_else(|| {
        AuraError::invalid(format!(
            "AMP channel {} missing bootstrap key material for epoch {}",
            channel, header.chan_epoch
        ))
    })?;
    derive_bootstrap_message_key(
        effects,
        context,
        channel,
        bootstrap.bootstrap_id,
        header,
        sender,
    )
    .await
}

// ============================================================================
// Low-level Operations
// ============================================================================

/// Reduce-before-send, validate window/epoch, and derive header/key/next_gen.
pub async fn prepare_send<E: AmpJournalEffects>(
    effects: &E,
    context: ContextId,
    channel: ChannelId,
) -> Result<(ChannelEpochState, RatchetDerivation)> {
    let state = get_channel_state(effects, context, channel).await?;
    let ratchet_state = send_ratchet_from_epoch_state(&state);
    let deriv = derive_for_send(context, channel, &ratchet_state, state.current_gen)
        .map_err(map_amp_error)?;
    Ok((state, deriv))
}

/// Validate an incoming AMP header against reduced state and derive recv keys.
pub fn validate_header(
    state: &ChannelEpochState,
    header: AmpHeader,
) -> Result<(RatchetDerivation, (u64, u64))> {
    // An earlier epoch: a member that holds that epoch's key still reads its
    // messages (history); the key lookup decides, and replay markers guard
    // duplicates. Its generation window is anchored at the header itself.
    if header.chan_epoch < state.chan_epoch {
        let historical = aura_transport::amp::AmpRatchetState {
            chan_epoch: header.chan_epoch,
            last_checkpoint_gen: header.ratchet_gen,
            skip_window: u64::from(state.skip_window),
            pending_epoch: None,
        };
        let bounds = aura_transport::amp::window_bounds(header.ratchet_gen, historical.skip_window);
        return derive_for_recv(&historical, header)
            .map(|deriv| (deriv, bounds))
            .map_err(map_amp_error);
    }
    let ratchet_state = ratchet_from_epoch_state(state);
    let bounds = aura_transport::amp::window_bounds(
        ratchet_state.last_checkpoint_gen,
        ratchet_state.skip_window,
    );
    derive_for_recv(&ratchet_state, header)
        .map(|deriv| (deriv, bounds))
        .map_err(map_amp_error)
}

/// Insert a proposed bump as a fact (A1: provisional).
pub async fn emit_proposed_bump<E: AmpJournalEffects>(
    effects: &E,
    proposal: ProposedChannelEpochBump,
) -> Result<()> {
    let policy = policy_for(CeremonyFlow::AmpEpochBump);
    if !policy.allows_mode(AgreementMode::Provisional) {
        return Err(AuraError::invalid(
            "AMP epoch bump policy does not allow provisional mode",
        ));
    }
    effects
        .insert_relational_fact(aura_journal::fact::RelationalFact::Protocol(
            aura_journal::ProtocolRelationalFact::AmpProposedChannelEpochBump(proposal),
        ))
        .await
}

/// Insert a convergence certificate for a soft-safe bump (A2).
pub async fn emit_soft_safe_bump<E: AmpJournalEffects>(
    effects: &E,
    cert: aura_core::threshold::ConvergenceCert,
) -> Result<()> {
    let policy = policy_for(CeremonyFlow::AmpEpochBump);
    if !policy.allows_mode(AgreementMode::CoordinatorSoftSafe) {
        return Err(AuraError::invalid(
            "AMP epoch bump policy does not allow soft-safe mode",
        ));
    }
    effects
        .insert_relational_fact(aura_journal::fact::RelationalFact::Protocol(
            aura_journal::ProtocolRelationalFact::ConvergenceCert(cert),
        ))
        .await
}

/// Finalize a pending bump via consensus and insert committed fact (A3).
pub async fn commit_bump_with_consensus<
    E: AmpJournalEffects + AmpEvidenceEffects + RandomEffects + PhysicalTimeEffects,
>(
    effects: &E,
    prestate: &aura_core::Prestate,
    proposal: &ProposedChannelEpochBump,
    key_packages: HashMap<aura_core::AuthorityId, Share>,
    group_public_key: PublicKeyPackage,
    transcript_ref: Option<aura_core::Hash32>,
) -> Result<()> {
    let policy = policy_for(CeremonyFlow::AmpEpochBump);
    if !policy.allows_mode(AgreementMode::ConsensusFinalized) {
        return Err(AuraError::invalid(
            "AMP epoch bump policy does not allow consensus finalization",
        ));
    }
    let resolved_transcript = match transcript_ref {
        Some(value) => Some(value),
        None => {
            let journal = effects.fetch_context_journal(proposal.context).await?;
            latest_transcript_ref_from_context(&journal, proposal.context)
        }
    };

    finalize_amp_bump_with_journal_default(
        effects,
        prestate,
        proposal,
        key_packages,
        group_public_key,
        aura_core::types::Epoch::from(proposal.new_epoch),
        resolved_transcript,
        effects,
        effects,
    )
    .await?;
    Ok(())
}

// ============================================================================
// High-level Send/Recv
// ============================================================================

/// High-level send path: reduce, derive header/key, encrypt, guard, and broadcast.
///
/// Returns the AMP ciphertext (header + sealed payload) for local persistence.
#[instrument(skip(effects, payload, config), fields(context = %context, channel = %channel))]
pub async fn amp_send<E>(
    effects: &E,
    context: ContextId,
    channel: ChannelId,
    sender: AuthorityId,
    payload: Vec<u8>,
    config: &AmpRuntimeConfig,
) -> Result<AmpCiphertext>
where
    E: AmpJournalEffects
        + NetworkEffects
        + GuardEffects
        + GuardContextProvider
        + CryptoEffects
        + SecureStorageEffects
        + AmpSendSequencerProvider
        + Sync
        + aura_core::PhysicalTimeEffects
        + aura_core::TimeEffects,
{
    let payload_size = payload.len();

    // Phase 1: Prepare send (journal reduction and ratchet derivation)
    let (state, _) = prepare_send(effects, context, channel)
        .await
        .map_err(|e| return_send_failure(context, channel, e))?;
    if !crate::core::sender_allowed_by_epoch_state(&state, sender)
        || !crate::journal::sender_allowed_by_channel_membership(effects, context, channel, sender)
            .await?
    {
        return Err(AuraError::permission_denied(
            "sender is excluded by canonical AMP membership",
        ));
    }
    let ratchet_state = send_ratchet_from_epoch_state(&state);
    // The owner persists the advanced cursor before admitting the
    // generation, so it is never reused, even if this send later fails.
    let ratchet_gen = effects
        .amp_send_sequencer()
        .reserve(effects, &state, &ratchet_state, context, channel, sender)
        .await
        .map_err(|e| return_send_failure(context, channel, e))?
        .consume();
    let deriv = derive_for_send(context, channel, &ratchet_state, ratchet_gen)
        .map_err(|e| return_send_failure(context, channel, map_amp_error(e)))?;
    let header = deriv.header;

    // Phase 2: AEAD encryption
    let aad = amp_additional_data(&header, sender);
    let message_key =
        derive_channel_message_key(effects, context, channel, &state, &header, sender)
            .await
            .map_err(|e| return_send_failure(context, channel, e))?;
    record_amp_replay_marker(effects, "amp_send_replay_markers", &header, sender)
        .await
        .map_err(|e| return_send_failure(context, channel, e))?;
    let key = message_key.0;
    let nonce = nonce_from_header(&header);
    let sealed = effects
        .aes_gcm_encrypt_with_aad(&payload, &key, &nonce, &aad)
        .await
        .map_err(|e| {
            return_send_failure(
                context,
                channel,
                AuraError::crypto(format!("AMP seal failed: {e}")),
            )
        })?;

    // Phase 3: Serialize
    let core_header = header;
    let msg = AmpMessage::new(core_header, sealed.clone());
    let bytes = serialize_message(&msg).map_err(|e| return_send_failure(context, channel, e))?;

    // Phase 4: Guard chain execution
    let peer = GuardContextProvider::authority_id(effects);
    let guard_chain = build_amp_send_guard(context, peer, config);
    let guard_result = guard_chain
        .evaluate_with_coupling(effects)
        .await
        .map_err(|e| return_send_failure(context, channel, e))?;

    if !guard_result.authorized {
        let err = return_send_failure(
            context,
            channel,
            AuraError::permission_denied(
                guard_result
                    .denial_reason
                    .unwrap_or_else(|| "AMP send unauthorized".to_string()),
            ),
        );
        return Err(err);
    }

    // Log flow charge
    if let Some(receipt) = &guard_result.receipt {
        AMP_TELEMETRY.log_flow_charge(
            context,
            peer,
            "amp_send",
            config.default_flow_cost.value(),
            Some(receipt),
        );
    }

    // Phase 5: Network broadcast
    effects
        .broadcast(bytes)
        .await
        .map_err(|e| return_send_failure(context, channel, AuraError::network(e.to_string())))?;

    // Success telemetry
    AMP_TELEMETRY.log_send_success(
        context,
        channel,
        &header,
        payload_size,
        sealed.len(),
        config.default_flow_cost.value(),
        guard_result.receipt.as_ref(),
    );

    Ok(AmpCiphertext {
        header: core_header,
        ciphertext: sealed,
    })
}

/// High-level recv path: decode, validate header/window, and decrypt.
#[instrument(skip(effects, bytes), fields(context = %context))]
pub async fn amp_recv<E>(
    effects: &E,
    context: ContextId,
    sender: AuthorityId,
    bytes: Vec<u8>,
) -> Result<AmpMessage>
where
    E: AmpJournalEffects + CryptoEffects + SecureStorageEffects,
{
    amp_open(effects, context, sender, bytes, true).await
}

/// Open a message that is already committed to the local journal.
///
/// Projections re-read committed messages (for example after a restart); those
/// reads are not new arrivals, so they skip the persistent replay marker that
/// `amp_recv` records for transport ingress.
pub async fn amp_open_committed<E>(
    effects: &E,
    context: ContextId,
    sender: AuthorityId,
    bytes: Vec<u8>,
) -> Result<AmpMessage>
where
    E: AmpJournalEffects + CryptoEffects + SecureStorageEffects,
{
    amp_open(effects, context, sender, bytes, false).await
}

async fn amp_open<E>(
    effects: &E,
    context: ContextId,
    sender: AuthorityId,
    bytes: Vec<u8>,
    record_replay: bool,
) -> Result<AmpMessage>
where
    E: AmpJournalEffects + CryptoEffects + SecureStorageEffects,
{
    let wire_size = bytes.len();

    // Phase 1: Deserialize
    let wire: AmpMessage =
        deserialize_message(&bytes).map_err(|e| return_receive_failure(context, None, None, e))?;

    // Phase 2: Context validation
    if wire.header.context != context {
        let err = return_receive_failure(
            context,
            Some(&wire.header),
            None,
            AuraError::invalid("AMP context mismatch"),
        );
        let transport_header = wire.header;
        return Err(err);
    }

    // Phase 3: Window/epoch validation
    let transport_header = wire.header;
    let state = get_channel_state(effects, context, transport_header.channel).await?;
    let (deriv, window_validation) =
        validate_and_build_result(&state, transport_header).map_err(|(e, validation)| {
            return_receive_failure(context, Some(&transport_header), validation.as_ref(), e)
        })?;

    // Phase 4: AEAD decryption
    let aad = amp_additional_data(&transport_header, sender);
    let message_key = derive_channel_message_key(
        effects,
        context,
        transport_header.channel,
        &state,
        &transport_header,
        sender,
    )
    .await
    .map_err(|e| {
        return_receive_failure(
            context,
            Some(&transport_header),
            Some(&window_validation),
            e,
        )
    })?;
    let key = message_key.0;
    let nonce = nonce_from_header(&transport_header);
    let opened = effects
        .aes_gcm_decrypt_with_aad(&wire.payload, &key, &nonce, &aad)
        .await
        .map_err(|e| {
            return_receive_failure(
                context,
                Some(&transport_header),
                Some(&window_validation),
                AuraError::crypto(format!("AMP open failed: {e}")),
            )
        })?;
    if record_replay {
        record_amp_replay_marker(
            effects,
            "amp_recv_replay_markers",
            &transport_header,
            sender,
        )
        .await
        .map_err(|e| {
            return_receive_failure(
                context,
                Some(&transport_header),
                Some(&window_validation),
                e,
            )
        })?;
    }

    // Success telemetry
    AMP_TELEMETRY.log_receive_success(
        context,
        &transport_header,
        wire_size,
        opened.len(),
        &window_validation,
    );

    Ok(AmpMessage::new(transport_header, opened))
}

/// Helper to validate header and build window validation result.
fn validate_and_build_result(
    state: &ChannelEpochState,
    header: AmpHeader,
) -> std::result::Result<
    (RatchetDerivation, WindowValidationResult),
    (AuraError, Option<WindowValidationResult>),
> {
    match validate_header(state, header) {
        Ok((deriv, bounds)) => {
            let validation =
                create_window_validation_result(true, true, bounds, header.ratchet_gen, None);
            Ok((deriv, validation))
        }
        Err(error) => {
            let error_str = error.to_string().to_lowercase();
            let (epoch_valid, generation_valid) = if error_str.contains("epoch") {
                (false, true)
            } else if error_str.contains("generation") || error_str.contains("window") {
                (true, false)
            } else {
                (false, false)
            };

            let validation = create_window_validation_result(
                epoch_valid,
                generation_valid,
                (0, 0),
                header.ratchet_gen,
                None,
            );
            Err((error, Some(validation)))
        }
    }
}

/// Receive + decrypt and surface a receipt alongside the payload.
pub async fn amp_recv_with_receipt<E>(
    effects: &E,
    context: ContextId,
    sender: AuthorityId,
    bytes: Vec<u8>,
) -> Result<AmpDelivery>
where
    E: AmpJournalEffects + CryptoEffects + SecureStorageEffects,
{
    let msg = amp_recv(effects, context, sender, bytes).await?;
    Ok(AmpDelivery {
        receipt: msg.receipt(),
        payload: msg.payload,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::effects::{
        CryptoExtendedEffects, SecureStorageCapability, SecureStorageEffects,
    };
    use aura_core::time::PhysicalTime;
    use aura_effects::{ProductionSecureStorageHandler, RealCryptoHandler};
    use aura_journal::fact::ChannelBootstrap;
    use uuid::Uuid;

    fn test_paths(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "aura-amp-orchestration-{label}-{}-{}",
            std::process::id(),
            Uuid::new_v4()
        ))
    }

    fn bootstrap_state(
        context: ContextId,
        channel: ChannelId,
        dealer: AuthorityId,
        recipients: Vec<AuthorityId>,
        bootstrap_id: aura_core::Hash32,
    ) -> ChannelEpochState {
        ChannelEpochState {
            canonical_checkpoint: None,
            chan_epoch: 0,
            pending_bump: None,
            bootstrap: Some(ChannelBootstrap {
                context,
                channel,
                bootstrap_id,
                dealer,
                recipients,
                created_at: PhysicalTime::exact(1),
                expires_at: None,
            }),
            last_checkpoint_gen: 0,
            current_gen: 1,
            skip_window: 64,
            transition: None,
        }
    }

    /// Regression (work/8.md task 4): consecutive sends must use fresh
    /// generations; reusing the reduced checkpoint generation made every
    /// second message fail as a replay.
    #[tokio::test]
    async fn send_cursor_advances_per_sender() {
        let storage = ProductionSecureStorageHandler::filesystem_fallback_for_non_production(
            test_paths("cursor"),
        );
        let context = ContextId::new_from_entropy([5; 32]);
        let channel = ChannelId::from_bytes([6; 32]);
        let alice = AuthorityId::new_from_entropy([7; 32]);
        let bob = AuthorityId::new_from_entropy([8; 32]);
        let state = bootstrap_state(
            context,
            channel,
            alice,
            vec![bob],
            aura_core::Hash32::from_bytes(&[1; 32]),
        );
        let ratchet = send_ratchet_from_epoch_state(&state);

        let sequencer = AmpSendSequencer::default();
        let mut alice_gens = Vec::new();
        for _ in 0..3 {
            let gen = sequencer
                .reserve(&storage, &state, &ratchet, context, channel, alice)
                .await
                .unwrap()
                .consume();
            alice_gens.push(gen);
        }
        assert_eq!(alice_gens, vec![1, 2, 3]);

        // Bob has an independent cursor starting at the reduced generation.
        let bob_gen = sequencer
            .reserve(&storage, &state, &ratchet, context, channel, bob)
            .await
            .unwrap()
            .consume();
        assert_eq!(bob_gen, 1);

        // A restarted runtime resumes from the persisted cursor.
        let restarted = AmpSendSequencer::default();
        let gen = restarted
            .reserve(&storage, &state, &ratchet, context, channel, alice)
            .await
            .unwrap()
            .consume();
        assert_eq!(gen, 4);
    }

    /// Regression (work/8.md task 172): concurrent sends from one sender
    /// never share a generation, so they never share an AES-GCM nonce. The
    /// interleaving is driven deterministically on one task: every
    /// reservation is polled to its first await before any completes.
    #[tokio::test]
    async fn concurrent_send_reservations_never_share_a_generation() {
        let storage = ProductionSecureStorageHandler::filesystem_fallback_for_non_production(
            test_paths("concurrent"),
        );
        let context = ContextId::new_from_entropy([31; 32]);
        let channel = ChannelId::from_bytes([32; 32]);
        let sender = AuthorityId::new_from_entropy([33; 32]);
        let state = bootstrap_state(
            context,
            channel,
            sender,
            vec![AuthorityId::new_from_entropy([34; 32])],
            aura_core::Hash32::from_bytes(&[2; 32]),
        );
        let ratchet = send_ratchet_from_epoch_state(&state);
        let sequencer = AmpSendSequencer::default();
        let reservations = (0..16).map(|_| async {
            sequencer
                .reserve(&storage, &state, &ratchet, context, channel, sender)
                .await
                .unwrap()
                .consume()
        });
        let mut gens = futures::future::join_all(reservations).await;
        gens.sort_unstable();
        assert_eq!(gens, (1..=16).collect::<Vec<_>>());
    }

    /// Regression (work/8.md task 4): one sender's consecutive messages use
    /// fresh generations end to end, and the recipient opens them in order
    /// while rejecting a replay.
    #[tokio::test]
    async fn consecutive_sends_from_one_sender_are_received_in_order() {
        let storage = ProductionSecureStorageHandler::filesystem_fallback_for_non_production(
            test_paths("in-order"),
        );
        let crypto = RealCryptoHandler::for_simulation_seed([0xB7; 32]);
        let context = ContextId::new_from_entropy([21; 32]);
        let channel = ChannelId::from_bytes([22; 32]);
        let sender = AuthorityId::new_from_entropy([23; 32]);
        let recipient = AuthorityId::new_from_entropy([24; 32]);
        let bootstrap_key = [25u8; 32];
        let bootstrap_id = aura_core::Hash32::from_bytes(&bootstrap_key);
        let state = bootstrap_state(context, channel, sender, vec![recipient], bootstrap_id);
        storage
            .secure_store(
                &SecureStorageLocation::amp_bootstrap_key(&context, &channel, &bootstrap_id),
                &bootstrap_key,
                &[SecureStorageCapability::Write],
            )
            .await
            .unwrap();
        let ratchet = send_ratchet_from_epoch_state(&state);

        let sequencer = AmpSendSequencer::default();
        let mut sent = Vec::new();
        for index in 0..5u8 {
            let gen = sequencer
                .reserve(&storage, &state, &ratchet, context, channel, sender)
                .await
                .unwrap()
                .consume();
            let header = AmpHeader {
                context,
                channel,
                chan_epoch: state.chan_epoch,
                ratchet_gen: gen,
            };
            let key =
                derive_channel_message_key(&storage, context, channel, &state, &header, sender)
                    .await
                    .unwrap();
            let plaintext = vec![index; 4];
            let ciphertext = crypto
                .aes_gcm_encrypt_with_aad(
                    &plaintext,
                    &key.0,
                    &nonce_from_header(&header),
                    &amp_additional_data(&header, sender),
                )
                .await
                .unwrap();
            sent.push((header, ciphertext));
        }

        let mut received = Vec::new();
        for (header, ciphertext) in &sent {
            validate_header(&state, *header).unwrap();
            record_amp_replay_marker(&storage, "amp_recv_replay_markers", header, sender)
                .await
                .unwrap();
            let key =
                derive_channel_message_key(&storage, context, channel, &state, header, sender)
                    .await
                    .unwrap();
            let opened = crypto
                .aes_gcm_decrypt_with_aad(
                    ciphertext,
                    &key.0,
                    &nonce_from_header(header),
                    &amp_additional_data(header, sender),
                )
                .await
                .unwrap();
            received.push(opened[0]);
        }
        assert_eq!(received, vec![0, 1, 2, 3, 4]);
        assert!(
            record_amp_replay_marker(&storage, "amp_recv_replay_markers", &sent[2].0, sender)
                .await
                .is_err(),
            "a replayed message must be rejected"
        );
    }

    #[tokio::test]
    async fn amp_ciphertext_requires_channel_key_and_aad_integrity() {
        let storage_dir = test_paths("aead");
        let storage =
            ProductionSecureStorageHandler::filesystem_fallback_for_non_production(storage_dir);
        let crypto = RealCryptoHandler::for_simulation_seed([0xA4; 32]);
        let context = ContextId::new_from_entropy([1; 32]);
        let channel = ChannelId::from_bytes([2; 32]);
        let sender = AuthorityId::new_from_entropy([3; 32]);
        let recipient = AuthorityId::new_from_entropy([4; 32]);
        let bootstrap_key = [9u8; 32];
        let bootstrap_id = aura_core::Hash32::from_bytes(&bootstrap_key);
        let state = bootstrap_state(context, channel, sender, vec![recipient], bootstrap_id);
        let header = AmpHeader {
            context,
            channel,
            chan_epoch: 0,
            ratchet_gen: 7,
        };
        let location = SecureStorageLocation::amp_bootstrap_key(&context, &channel, &bootstrap_id);
        storage
            .secure_store(&location, &bootstrap_key, &[SecureStorageCapability::Write])
            .await
            .unwrap();

        let aad = amp_additional_data(&header, sender);
        let key = derive_channel_message_key(&storage, context, channel, &state, &header, sender)
            .await
            .unwrap();
        let other_sender_key =
            derive_channel_message_key(&storage, context, channel, &state, &header, recipient)
                .await
                .unwrap();
        assert_ne!(
            key, other_sender_key,
            "senders sharing a generation must not share a message key"
        );
        let nonce = nonce_from_header(&header);
        let plaintext = b"amp-secret-payload";
        let ciphertext = crypto
            .aes_gcm_encrypt_with_aad(plaintext, &key.0, &nonce, &aad)
            .await
            .unwrap();

        let opened = crypto
            .aes_gcm_decrypt_with_aad(&ciphertext, &key.0, &nonce, &aad)
            .await
            .unwrap();
        assert_eq!(opened, plaintext);

        let wrong_key = [0x55; 32];
        assert!(crypto
            .aes_gcm_decrypt_with_aad(&ciphertext, &wrong_key, &nonce, &aad)
            .await
            .is_err());

        let mut tampered = ciphertext.clone();
        tampered[0] ^= 0x80;
        assert!(crypto
            .aes_gcm_decrypt_with_aad(&tampered, &key.0, &nonce, &aad)
            .await
            .is_err());

        let wrong_aad = amp_additional_data(&header, recipient);
        assert!(crypto
            .aes_gcm_decrypt_with_aad(&ciphertext, &key.0, &nonce, &wrong_aad)
            .await
            .is_err());
    }

    /// Task 164: an epoch >= 1 message key comes from that epoch's channel
    /// key, which only its members hold; the bootstrap key never opens it,
    /// and a member still validates an earlier epoch's header (history).
    #[tokio::test]
    async fn later_epochs_need_their_epoch_key_and_earlier_epochs_stay_readable() {
        let storage = ProductionSecureStorageHandler::filesystem_fallback_for_non_production(
            test_paths("epoch-keys"),
        );
        let context = ContextId::new_from_entropy([31; 32]);
        let channel = ChannelId::from_bytes([32; 32]);
        let sender = AuthorityId::new_from_entropy([33; 32]);
        let bootstrap_key = [34u8; 32];
        let bootstrap_id = aura_core::Hash32::from_bytes(&bootstrap_key);
        let mut state = bootstrap_state(context, channel, sender, vec![], bootstrap_id);
        storage
            .secure_store(
                &SecureStorageLocation::amp_bootstrap_key(&context, &channel, &bootstrap_id),
                &bootstrap_key,
                &[SecureStorageCapability::Write],
            )
            .await
            .unwrap();
        let header = |chan_epoch| AmpHeader {
            context,
            channel,
            chan_epoch,
            ratchet_gen: 3,
        };
        assert!(
            derive_channel_message_key(&storage, context, channel, &state, &header(1), sender)
                .await
                .is_err(),
            "without the epoch-1 key, the bootstrap key does not open epoch 1"
        );
        storage
            .secure_store(
                &SecureStorageLocation::amp_channel_base_key(&context, &channel, 1),
                &[35u8; 32],
                &[SecureStorageCapability::Write],
            )
            .await
            .unwrap();
        let epoch_one =
            derive_channel_message_key(&storage, context, channel, &state, &header(1), sender)
                .await
                .unwrap();
        let epoch_zero =
            derive_channel_message_key(&storage, context, channel, &state, &header(0), sender)
                .await
                .unwrap();
        assert_ne!(epoch_one, epoch_zero);

        state.chan_epoch = 2;
        assert!(
            validate_header(&state, header(1)).is_ok(),
            "an earlier epoch's header stays valid for a member holding its key"
        );
        assert!(validate_header(&state, header(3)).is_err());
    }

    #[tokio::test]
    async fn amp_replay_marker_rejects_duplicate_epoch_generation() {
        let storage_dir = test_paths("replay");
        let storage =
            ProductionSecureStorageHandler::filesystem_fallback_for_non_production(storage_dir);
        let header = AmpHeader {
            context: ContextId::new_from_entropy([7; 32]),
            channel: ChannelId::from_bytes([8; 32]),
            chan_epoch: 2,
            ratchet_gen: 11,
        };
        let sender = AuthorityId::new_from_entropy([9; 32]);

        record_amp_replay_marker(&storage, "amp_recv_replay_markers", &header, sender)
            .await
            .unwrap();
        let err = record_amp_replay_marker(&storage, "amp_recv_replay_markers", &header, sender)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("AMP replay detected"));
    }
}
