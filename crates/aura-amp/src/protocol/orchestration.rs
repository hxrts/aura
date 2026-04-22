//! AMP transport integration
//!
//! Glue between AMP ratchet helpers, guard chain, and journal operations.
//! Telemetry is handled via tracing spans for timing and structured logging.

use super::telemetry::{create_window_validation_result, WindowValidationResult, AMP_TELEMETRY};
use crate::config::AmpRuntimeConfig;
use crate::consensus::finalize_amp_bump_with_journal_default;
use crate::core::{nonce_from_header, ratchet_from_epoch_state, send_ratchet_from_epoch_state};
use crate::get_channel_state;
use crate::wire::{deserialize_message, serialize_message, AmpMessage};
use crate::{AmpEvidenceEffects, AmpJournalEffects};
use aura_core::effects::amp::{AmpCiphertext, AmpHeader};
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::effects::{
    CryptoEffects, NetworkEffects, RandomEffects, SecureStorageCapability, SecureStorageEffects,
    SecureStorageLocation,
};
use aura_core::frost::{PublicKeyPackage, Share};
use aura_core::threshold::{policy_for, AgreementMode, CeremonyFlow};
use aura_core::types::identifiers::{ChannelId, ContextId};
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

const AMP_SEND_COUPLING_OPERATION: &str = "send_coupling";

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

    create_send_guard_op(
        GuardOperation::AmpSend,
        context,
        peer,
        config.default_flow_cost,
    )
    .with_operation_id(GuardOperationId::AmpSend)
    .with_journal_coupler(amp_send_journal_coupler())
}

fn amp_send_journal_coupler() -> aura_guards::journal::JournalCoupler {
    use aura_guards::journal::JournalCouplerBuilder;
    use aura_mpst::journal::JournalAnnotation;

    JournalCouplerBuilder::new()
        .with_annotation(
            GuardOperationId::Custom(AMP_SEND_COUPLING_OPERATION.to_string()),
            JournalAnnotation::add_facts("AMP send authorization and flow receipt"),
        )
        .build()
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

async fn derive_epoch_message_key<E: SecureStorageEffects>(
    effects: &E,
    context: ContextId,
    channel: ChannelId,
    header: &AmpHeader,
) -> Result<aura_core::Hash32> {
    let location = SecureStorageLocation::amp_epoch_key(&context, &channel, header.chan_epoch);
    let key_bytes = effects
        .secure_retrieve(&location, &[SecureStorageCapability::Read])
        .await
        .map_err(|e| {
            AuraError::crypto(format!(
                "Failed to read AMP channel epoch key for {context}/{channel}/{}: {e}",
                header.chan_epoch
            ))
        })?;

    if key_bytes.len() != 32 {
        return Err(AuraError::invalid(format!(
            "AMP channel epoch key has invalid length: {}",
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
    .map_err(|e| AuraError::crypto(format!("AMP epoch KDF failed: {e}")))
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
            .map_err(|e| return_send_failure(context, channel, e))?,
            None => derive_epoch_message_key(effects, context, channel, &header)
                .await
                .map_err(|e| return_send_failure(context, channel, e))?,
        }
    } else {
        derive_epoch_message_key(effects, context, channel, &header)
            .await
            .map_err(|e| return_send_failure(context, channel, e))?
    };
    let key = message_key.0;
    let nonce = nonce_from_header(&header);
    let sealed = effects
        .aes_gcm_encrypt(&payload, &key, &nonce)
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
pub async fn amp_recv<E>(effects: &E, context: ContextId, bytes: Vec<u8>) -> Result<AmpMessage>
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
                return_receive_failure(
                    context,
                    Some(&transport_header),
                    Some(&window_validation),
                    e,
                )
            })?,
            None => derive_epoch_message_key(
                effects,
                context,
                transport_header.channel,
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
            })?,
        }
    } else {
        derive_epoch_message_key(
            effects,
            context,
            transport_header.channel,
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
        })?
    };
    let key = message_key.0;
    let nonce = nonce_from_header(&transport_header);
    let opened = effects
        .aes_gcm_decrypt(&wire.payload, &key, &nonce)
        .await
        .map_err(|e| {
            return_receive_failure(
                context,
                Some(&transport_header),
                Some(&window_validation),
                AuraError::crypto(format!("AMP open failed: {e}")),
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

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::effects::CryptoExtendedEffects;
    use aura_effects::crypto::RealCryptoHandler;
    use aura_transport::amp::AmpRatchetState;
    use futures::lock::Mutex;
    use std::sync::Arc;

    #[tokio::test]
    async fn old_public_header_key_cannot_decrypt_epoch_ciphertext() {
        let context = ContextId::new_from_entropy([7u8; 32]);
        let channel = ChannelId::from_bytes([8u8; 32]);
        let state = AmpRatchetState {
            chan_epoch: 3,
            last_checkpoint_gen: 0,
            skip_window: 8,
            pending_epoch: None,
        };
        let deriv = derive_for_send(context, channel, &state, 4)
            .expect("ratchet derivation should succeed");
        let header = deriv.header;
        let master_key = aura_core::Hash32([0x42u8; 32]);
        let epoch_key = aura_core::crypto::amp::derive_message_key(
            &master_key,
            context.as_bytes(),
            channel.as_bytes(),
            header.chan_epoch,
            header.ratchet_gen,
        )
        .expect("secret epoch key derivation should succeed");
        let old_public_key: [u8; 32] = (deriv.message_id.0).0;
        let nonce = nonce_from_header(&header);
        let crypto = RealCryptoHandler::new();
        let ciphertext = crypto
            .aes_gcm_encrypt(b"confidential", &epoch_key.0, &nonce)
            .await
            .expect("encryption should succeed");

        let old_key_result = crypto
            .aes_gcm_decrypt(&ciphertext, &old_public_key, &nonce)
            .await;

        assert!(old_key_result.is_err());
    }

    #[derive(Clone, Default)]
    struct RecordingSendEffects {
        events: Arc<Mutex<Vec<&'static str>>>,
    }

    #[async_trait::async_trait]
    impl aura_core::JournalEffects for RecordingSendEffects {
        async fn merge_facts(
            &self,
            mut target: aura_core::Journal,
            delta: aura_core::Journal,
        ) -> std::result::Result<aura_core::Journal, aura_core::AuraError> {
            target.merge_facts(delta.facts);
            Ok(target)
        }

        async fn refine_caps(
            &self,
            target: aura_core::Journal,
            _refinement: aura_core::Journal,
        ) -> std::result::Result<aura_core::Journal, aura_core::AuraError> {
            Ok(target)
        }

        async fn get_journal(
            &self,
        ) -> std::result::Result<aura_core::Journal, aura_core::AuraError> {
            Ok(aura_core::Journal::new())
        }

        async fn persist_journal(
            &self,
            _journal: &aura_core::Journal,
        ) -> std::result::Result<(), aura_core::AuraError> {
            self.events.lock().await.push("persist");
            Ok(())
        }

        async fn get_flow_budget(
            &self,
            _context: &ContextId,
            _peer: &aura_core::AuthorityId,
        ) -> std::result::Result<aura_core::FlowBudget, aura_core::AuraError> {
            Ok(aura_core::FlowBudget::new(1_000, aura_core::Epoch::new(0)))
        }

        async fn update_flow_budget(
            &self,
            _context: &ContextId,
            _peer: &aura_core::AuthorityId,
            budget: &aura_core::FlowBudget,
        ) -> std::result::Result<aura_core::FlowBudget, aura_core::AuraError> {
            Ok(*budget)
        }

        async fn charge_flow_budget(
            &self,
            _context: &ContextId,
            _peer: &aura_core::AuthorityId,
            _cost: aura_core::FlowCost,
        ) -> std::result::Result<aura_core::FlowBudget, aura_core::AuraError> {
            Ok(aura_core::FlowBudget::new(1_000, aura_core::Epoch::new(0)))
        }
    }

    #[async_trait::async_trait]
    impl aura_core::PhysicalTimeEffects for RecordingSendEffects {
        async fn physical_time(
            &self,
        ) -> std::result::Result<aura_core::PhysicalTime, aura_core::effects::time::TimeError>
        {
            Ok(aura_core::PhysicalTime::exact(1_000))
        }

        async fn sleep_ms(
            &self,
            _duration_ms: u64,
        ) -> std::result::Result<(), aura_core::effects::time::TimeError> {
            Ok(())
        }
    }

    impl aura_core::TimeEffects for RecordingSendEffects {}

    #[async_trait::async_trait]
    impl aura_core::effects::NetworkCoreEffects for RecordingSendEffects {
        async fn send_to_peer(
            &self,
            _peer_id: uuid::Uuid,
            _message: Vec<u8>,
        ) -> std::result::Result<(), aura_core::effects::NetworkError> {
            Ok(())
        }

        async fn broadcast(
            &self,
            _message: Vec<u8>,
        ) -> std::result::Result<(), aura_core::effects::NetworkError> {
            self.events.lock().await.push("broadcast");
            Ok(())
        }

        async fn receive(
            &self,
        ) -> std::result::Result<(uuid::Uuid, Vec<u8>), aura_core::effects::NetworkError> {
            Err(aura_core::effects::NetworkError::NoMessage)
        }
    }

    #[tokio::test]
    async fn amp_send_coupler_persists_required_annotation_before_broadcast() {
        let effects = RecordingSendEffects::default();
        let coupler = amp_send_journal_coupler();

        let metrics = coupler
            .couple_with_send(&effects, &None)
            .await
            .expect("AMP send coupler should persist annotation");
        aura_core::effects::NetworkCoreEffects::broadcast(&effects, vec![1, 2, 3])
            .await
            .expect("broadcast should be recorded");

        assert_eq!(metrics.operations_applied, 1);
        assert_eq!(*effects.events.lock().await, vec!["persist", "broadcast"]);
    }
}
