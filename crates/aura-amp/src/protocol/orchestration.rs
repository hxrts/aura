//! AMP transport integration
//!
//! Glue between AMP ratchet helpers, guard chain, and journal operations.
//! Telemetry is handled via tracing spans for timing and structured logging.

use super::telemetry::{create_window_validation_result, WindowValidationResult, AMP_TELEMETRY};
use crate::config::AmpRuntimeConfig;
use crate::consensus::finalize_amp_bump_with_journal_default;
use crate::core::{nonce_from_header, ratchet_from_epoch_state, send_ratchet_from_epoch_state};
use crate::wire::{deserialize_message, serialize_message, AmpMessage, AMP_WIRE_SCHEMA_VERSION};
use crate::{get_channel_state, list_channel_participants};
use crate::{AmpEvidenceEffects, AmpJournalEffects};
use aura_core::effects::amp::{AmpCiphertext, AmpHeader};
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::effects::{
    CryptoEffects, NetworkEffects, RandomEffects, SecureStorageCapability, SecureStorageEffects,
    SecureStorageLocation,
};
use aura_core::frost::{PublicKeyPackage, Share};
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

fn amp_recipients(participants: &[AuthorityId], sender: AuthorityId) -> Vec<AuthorityId> {
    let mut recipients: Vec<AuthorityId> = participants
        .iter()
        .copied()
        .filter(|participant| *participant != sender)
        .collect();
    recipients.sort_unstable_by_key(|participant| participant.to_bytes());
    recipients
}

fn amp_additional_data(
    header: &AmpHeader,
    sender: AuthorityId,
    recipients: &[AuthorityId],
) -> Vec<u8> {
    let mut aad = Vec::with_capacity(2 + 32 + 32 + 8 + 8 + 32 + (recipients.len() * 32));
    aad.extend_from_slice(&AMP_WIRE_SCHEMA_VERSION.to_le_bytes());
    aad.extend_from_slice(header.context.as_bytes());
    aad.extend_from_slice(header.channel.as_bytes());
    aad.extend_from_slice(&header.chan_epoch.to_le_bytes());
    aad.extend_from_slice(&header.ratchet_gen.to_le_bytes());
    aad.extend_from_slice(&sender.to_bytes());
    for recipient in recipients {
        aad.extend_from_slice(&recipient.to_bytes());
    }
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

async fn derive_channel_message_key<E: SecureStorageEffects>(
    effects: &E,
    context: ContextId,
    channel: ChannelId,
    state: &ChannelEpochState,
    header: &AmpHeader,
) -> Result<aura_core::Hash32> {
    let bootstrap = state.bootstrap.as_ref().ok_or_else(|| {
        AuraError::invalid(format!(
            "AMP channel {} missing bootstrap key material for epoch {}",
            channel, header.chan_epoch
        ))
    })?;
    derive_bootstrap_message_key(effects, context, channel, bootstrap.bootstrap_id, header).await
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
        + aura_core::PhysicalTimeEffects
        + aura_core::TimeEffects,
{
    let payload_size = payload.len();

    // Phase 1: Prepare send (journal reduction and ratchet derivation)
    let (state, deriv) = prepare_send(effects, context, channel)
        .await
        .map_err(|e| return_send_failure(context, channel, e))?;
    let header = deriv.header;

    // Phase 2: AEAD encryption
    let participants = list_channel_participants(effects, context, channel)
        .await
        .map_err(|e| return_send_failure(context, channel, e))?;
    let recipients = amp_recipients(&participants, sender);
    let aad = amp_additional_data(&header, sender, &recipients);
    let message_key = derive_channel_message_key(effects, context, channel, &state, &header)
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
    let participants =
        list_channel_participants(effects, context, transport_header.channel).await?;
    let recipients = amp_recipients(&participants, sender);
    let aad = amp_additional_data(&transport_header, sender, &recipients);
    let message_key = derive_channel_message_key(
        effects,
        context,
        transport_header.channel,
        &state,
        &transport_header,
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
            chan_epoch: 1,
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
            chan_epoch: 1,
            ratchet_gen: 7,
        };
        let location = SecureStorageLocation::amp_bootstrap_key(&context, &channel, &bootstrap_id);
        storage
            .secure_store(&location, &bootstrap_key, &[SecureStorageCapability::Write])
            .await
            .unwrap();

        let recipients = amp_recipients(&[sender, recipient], sender);
        let aad = amp_additional_data(&header, sender, &recipients);
        let key = derive_channel_message_key(&storage, context, channel, &state, &header)
            .await
            .unwrap();
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

        let wrong_aad = amp_additional_data(&header, recipient, &recipients);
        assert!(crypto
            .aes_gcm_decrypt_with_aad(&ciphertext, &key.0, &nonce, &wrong_aad)
            .await
            .is_err());
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
