//! AMP transport integration
//!
//! Glue between AMP ratchet helpers, guard chain, and journal operations.
//! Telemetry is handled via tracing spans for timing and structured logging.

use super::telemetry::{create_window_validation_result, WindowValidationResult, AMP_TELEMETRY};
use crate::config::AmpRuntimeConfig;
use crate::consensus::finalize_amp_bump_with_journal_default;
use crate::core::{nonce_from_header, ratchet_from_epoch_state};
use crate::get_channel_state;
use crate::wire::{deserialize_message, serialize_message, AmpMessage};
use crate::{AmpEvidenceEffects, AmpJournalEffects};
use aura_core::effects::amp::{AmpCiphertext, AmpHeader as CoreAmpHeader};
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::effects::{
    CryptoEffects, NetworkEffects, RandomEffects, SecureStorageCapability, SecureStorageEffects,
    SecureStorageLocation,
};
use aura_core::frost::{PublicKeyPackage, Share};
use aura_core::identifiers::{ChannelId, ContextId};
use aura_core::threshold::{policy_for, AgreementMode, CeremonyFlow};
use aura_core::{AuraError, Result};
use aura_guards::traits::GuardContextProvider;
use aura_guards::{GuardEffects, GuardOperation, GuardOperationId};
use aura_journal::fact::{
    DkgTranscriptCommit, FactContent, ProposedChannelEpochBump, RelationalFact,
};
use aura_journal::ChannelEpochState;
use aura_transport::amp::{
    derive_for_recv, derive_for_send, AmpError, AmpHeader as TransportAmpHeader, RatchetDerivation,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::instrument;

// ============================================================================
// Header Conversions
// ============================================================================
// Note: Can't use From/Into due to orphan rules (both types are foreign).

#[inline]
fn to_core_header(h: TransportAmpHeader) -> CoreAmpHeader {
    CoreAmpHeader {
        context: h.context,
        channel: h.channel,
        chan_epoch: h.chan_epoch,
        ratchet_gen: h.ratchet_gen,
    }
}

#[inline]
fn to_transport_header(h: &CoreAmpHeader) -> TransportAmpHeader {
    TransportAmpHeader {
        context: h.context,
        channel: h.channel,
        chan_epoch: h.chan_epoch,
        ratchet_gen: h.ratchet_gen,
    }
}

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
    header: &TransportAmpHeader,
) -> Result<aura_core::Hash32> {
    let location = SecureStorageLocation::amp_bootstrap_key(&context, &channel, &bootstrap_id);
    let key_bytes = effects
        .secure_retrieve(&location, &[SecureStorageCapability::Read])
        .await
        .map_err(|e| AuraError::internal(format!("Failed to read AMP bootstrap key: {e}")))?;

    if key_bytes.len() != 32 {
        return Err(AuraError::invalid(format!(
            "AMP bootstrap key has invalid length: {}",
            key_bytes.len()
        )));
    }

    let mut key = [0u8; 32];
    key.copy_from_slice(&key_bytes);
    let master_key = aura_core::Hash32::new(key);

    aura_core::crypto::amp::derive_message_key(
        &master_key,
        context.as_bytes(),
        channel.as_bytes(),
        header.chan_epoch,
        header.ratchet_gen,
    )
    .map_err(|e| AuraError::crypto(format!("AMP bootstrap KDF failed: {e}")))
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
    let ratchet_state = ratchet_from_epoch_state(&state);
    let deriv = derive_for_send(context, channel, &ratchet_state, state.current_gen)
        .map_err(map_amp_error)?;
    Ok((state, deriv))
}

/// Validate an incoming AMP header against reduced state and derive recv keys.
pub fn validate_header(
    state: &ChannelEpochState,
    header: TransportAmpHeader,
) -> Result<(RatchetDerivation, (u64, u64))> {
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
        + aura_core::PhysicalTimeEffects
        + aura_core::TimeEffects,
{
    let payload_size = payload.len();

    // Phase 1: Prepare send (journal reduction and ratchet derivation)
    let (state, deriv) = prepare_send(effects, context, channel).await.map_err(|e| {
        AMP_TELEMETRY.log_send_failure(context, channel, &e);
        e
    })?;
    let header = deriv.header;

    // Phase 2: AEAD encryption
    let message_key = if header.chan_epoch == 0 {
        match state.bootstrap.as_ref() {
            Some(bootstrap) => derive_bootstrap_message_key(
                effects,
                context,
                channel,
                bootstrap.bootstrap_id,
                &header,
            )
            .await
            .map_err(|e| {
                AMP_TELEMETRY.log_send_failure(context, channel, &e);
                e
            })?,
            None => deriv.message_key,
        }
    } else {
        deriv.message_key
    };
    let key = message_key.0;
    let nonce = nonce_from_header(&header);
    let sealed = effects
        .aes_gcm_encrypt(&payload, &key, &nonce)
        .await
        .map_err(|e| {
            let err = AuraError::crypto(format!("AMP seal failed: {e}"));
            AMP_TELEMETRY.log_send_failure(context, channel, &err);
            err
        })?;

    // Phase 3: Serialize
    let core_header = to_core_header(header);
    let msg = AmpMessage::new(core_header.clone(), sealed.clone());
    let bytes = serialize_message(&msg).map_err(|e| {
        AMP_TELEMETRY.log_send_failure(context, channel, &e);
        e
    })?;

    // Phase 4: Guard chain execution
    let peer = GuardContextProvider::authority_id(effects);
    let guard_chain = build_amp_send_guard(context, peer, config);
    let guard_result = guard_chain
        .evaluate_with_coupling(effects)
        .await
        .map_err(|e| {
            AMP_TELEMETRY.log_send_failure(context, channel, &e);
            e
        })?;

    if !guard_result.authorized {
        let err = AuraError::permission_denied(
            guard_result
                .denial_reason
                .unwrap_or_else(|| "AMP send unauthorized".to_string()),
        );
        AMP_TELEMETRY.log_send_failure(context, channel, &err);
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
    effects.broadcast(bytes).await.map_err(|e| {
        let err = AuraError::network(e.to_string());
        AMP_TELEMETRY.log_send_failure(context, channel, &err);
        err
    })?;

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
pub async fn amp_recv<E>(effects: &E, context: ContextId, bytes: Vec<u8>) -> Result<AmpMessage>
where
    E: AmpJournalEffects + CryptoEffects + SecureStorageEffects,
{
    let wire_size = bytes.len();

    // Phase 1: Deserialize
    let wire: AmpMessage = deserialize_message(&bytes).map_err(|e| {
        AMP_TELEMETRY.log_receive_failure(context, None, None, &e);
        e
    })?;

    // Phase 2: Context validation
    if wire.header.context != context {
        let err = AuraError::invalid("AMP context mismatch");
        let transport_header = to_transport_header(&wire.header);
        AMP_TELEMETRY.log_receive_failure(context, Some(&transport_header), None, &err);
        return Err(err);
    }

    // Phase 3: Window/epoch validation
    let transport_header = to_transport_header(&wire.header);
    let state = get_channel_state(effects, context, transport_header.channel).await?;
    let (deriv, window_validation) =
        validate_and_build_result(&state, transport_header).map_err(|(e, validation)| {
            AMP_TELEMETRY.log_receive_failure(
                context,
                Some(&transport_header),
                validation.as_ref(),
                &e,
            );
            e
        })?;

    // Phase 4: AEAD decryption
    let message_key = if transport_header.chan_epoch == 0 {
        match state.bootstrap.as_ref() {
            Some(bootstrap) => derive_bootstrap_message_key(
                effects,
                context,
                transport_header.channel,
                bootstrap.bootstrap_id,
                &transport_header,
            )
            .await
            .map_err(|e| {
                AMP_TELEMETRY.log_receive_failure(
                    context,
                    Some(&transport_header),
                    Some(&window_validation),
                    &e,
                );
                e
            })?,
            None => deriv.message_key,
        }
    } else {
        deriv.message_key
    };
    let key = message_key.0;
    let nonce = nonce_from_header(&transport_header);
    let opened = effects
        .aes_gcm_decrypt(&wire.payload, &key, &nonce)
        .await
        .map_err(|e| {
            let err = AuraError::crypto(format!("AMP open failed: {e}"));
            AMP_TELEMETRY.log_receive_failure(
                context,
                Some(&transport_header),
                Some(&window_validation),
                &err,
            );
            err
        })?;

    // Success telemetry
    AMP_TELEMETRY.log_receive_success(
        context,
        &transport_header,
        wire_size,
        opened.len(),
        &window_validation,
    );

    Ok(AmpMessage::new(to_core_header(transport_header), opened))
}

/// Helper to validate header and build window validation result.
fn validate_and_build_result(
    state: &ChannelEpochState,
    header: TransportAmpHeader,
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
    bytes: Vec<u8>,
) -> Result<AmpDelivery>
where
    E: AmpJournalEffects + CryptoEffects + SecureStorageEffects,
{
    let msg = amp_recv(effects, context, bytes).await?;
    Ok(AmpDelivery {
        receipt: msg.receipt(),
        payload: msg.payload,
    })
}
