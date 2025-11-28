//! AMP transport integration
//!
//! Glue between AMP ratchet helpers, guard chain, and journal operations.
//! Uses centralized telemetry for structured logging and metrics collection.

use super::telemetry::{
    create_window_validation_result, AmpFlowTelemetry, AmpMetrics, AmpReceiveTelemetry,
    AmpSendTelemetry, AMP_TELEMETRY,
};
use crate::amp::{get_channel_state, AmpJournalEffects};
use crate::consensus::finalize_amp_bump_with_journal_default;
use crate::guards::effect_system_trait::GuardContextProvider;
use crate::guards::GuardEffects;
use aura_core::effects::NetworkEffects;
use aura_core::frost::{PublicKeyPackage, Share};
use aura_core::identifiers::{ChannelId, ContextId};
use aura_core::{AuraError, Result};
use aura_journal::{fact::ProposedChannelEpochBump, ChannelEpochState};
use aura_transport::amp::{
    derive_for_recv, derive_for_send, AmpError, AmpHeader, AmpRatchetState, RatchetDerivation,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tracing::instrument;
fn map_amp_error(err: AmpError) -> AuraError {
    AuraError::invalid(format!("AMP ratchet error: {}", err))
}

/// Simple wire format placeholder for AMP messages (header + opaque payload).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmpMessage {
    pub header: AmpHeader,
    pub payload: Vec<u8>,
}

impl AmpMessage {
    pub fn new(header: AmpHeader, payload: Vec<u8>) -> Self {
        Self { header, payload }
    }

    pub fn receipt(&self) -> AmpReceipt {
        AmpReceipt {
            context: self.header.context,
            channel: self.header.channel,
            chan_epoch: self.header.chan_epoch,
            ratchet_gen: self.header.ratchet_gen,
        }
    }
}

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

/// Derive nonce from AMP header using centralized crypto utilities.
///
/// This function delegates to `aura_core::crypto::amp::derive_nonce_from_ratchet`
/// to ensure consistent nonce derivation across the codebase.
fn nonce_from_header(header: &AmpHeader) -> [u8; 12] {
    aura_core::crypto::amp::derive_nonce_from_ratchet(header.ratchet_gen, header.chan_epoch)
}

/// Create a configured guard chain for AMP send operations.
///
/// This helper centralizes guard chain construction for AMP sends, avoiding
/// repetition and lifetime/capture issues. It configures:
/// - Capability requirement: "amp:send"
/// - Flow cost: Configurable (default: 1 for minimal overhead)
/// - Leakage budget: Standard AMP metadata leakage
/// - Journal coupling: Atomic fact commit with send operation
///
/// # Arguments
///
/// * `context` - Context identifier for the channel
/// * `peer` - Destination authority ID
/// * `flow_cost` - Optional flow cost (defaults to 1 if None)
///
/// # Returns
///
/// A configured `SendGuardChain` ready for evaluation
///
/// # Example
///
/// ```ignore
/// let guard = build_amp_send_guard(context, peer, None);
/// let result = guard.evaluate_with_coupling(effects).await?;
/// ```
fn build_amp_send_guard(
    context: aura_core::identifiers::ContextId,
    peer: aura_core::AuthorityId,
    flow_cost: Option<u32>,
) -> crate::guards::send_guard::SendGuardChain {
    use crate::guards::journal_coupler::JournalCoupler;
    use crate::guards::send_guard::SendGuardChain;

    let cost = flow_cost.unwrap_or(1);
    SendGuardChain::new("amp:send".to_string(), context, peer, cost)
        .with_operation_id("amp_send")
        .with_journal_coupler(JournalCoupler::new())
}

/// Reduce-before-send, validate window/epoch, and derive header/key/next_gen.
pub async fn prepare_send<E: AmpJournalEffects>(
    effects: &E,
    context: ContextId,
    channel: ChannelId,
) -> Result<RatchetDerivation> {
    let state = get_channel_state(effects, context, channel).await?;
    let ratchet_state = ratchet_from_epoch_state(&state);
    derive_for_send(context, channel, &ratchet_state, state.current_gen).map_err(map_amp_error)
}

/// Validate an incoming AMP header against reduced state and derive recv keys.
pub async fn validate_header<E: AmpJournalEffects>(
    effects: &E,
    context: ContextId,
    header: AmpHeader,
) -> Result<(RatchetDerivation, (u64, u64))> {
    let state = get_channel_state(effects, context, header.channel).await?;
    let ratchet_state = ratchet_from_epoch_state(&state);
    let (min_gen, max_gen) = aura_transport::amp::window_bounds(
        ratchet_state.last_checkpoint_gen,
        ratchet_state.skip_window,
    );
    derive_for_recv(&ratchet_state, header)
        .map(|deriv| (deriv, (min_gen, max_gen)))
        .map_err(map_amp_error)
}

fn ratchet_from_epoch_state(state: &ChannelEpochState) -> AmpRatchetState {
    AmpRatchetState {
        chan_epoch: state.chan_epoch,
        last_checkpoint_gen: state.last_checkpoint_gen,
        skip_window: state.skip_window as u64,
        pending_epoch: state.pending_bump.as_ref().map(|p| p.new_epoch),
    }
}

/// Insert a proposed bump as a fact.
pub async fn emit_proposed_bump<E: AmpJournalEffects>(
    effects: &E,
    proposal: ProposedChannelEpochBump,
) -> Result<()> {
    effects
        .insert_relational_fact(
            aura_journal::fact::RelationalFact::AmpProposedChannelEpochBump(proposal),
        )
        .await
}

/// Finalize a pending bump via consensus and insert committed fact using default witness policy.
pub async fn commit_bump_with_consensus<E: AmpJournalEffects>(
    effects: &E,
    prestate: &aura_core::Prestate,
    proposal: &ProposedChannelEpochBump,
    key_packages: HashMap<aura_core::AuthorityId, Share>,
    group_public_key: PublicKeyPackage,
) -> Result<()> {
    finalize_amp_bump_with_journal_default(
        effects,
        prestate,
        proposal,
        key_packages,
        group_public_key,
        aura_core::epochs::Epoch::from(proposal.new_epoch),
    )
    .await?;
    Ok(())
}

/// High-level send path: reduce, derive header/key, and broadcast.
///
/// Implements AMP specification section 7.2: performs guard chain to enforce authorization and flow budgets.
/// Uses centralized telemetry for structured observability.
/// Timing is captured via the tracing span (subscriber handles actual timing).
#[instrument(skip(effects, payload), fields(context = %context, channel = %channel))]
pub async fn amp_send<E>(
    effects: &mut E,
    context: ContextId,
    channel: ChannelId,
    payload: Vec<u8>,
) -> Result<AmpHeader>
where
    E: AmpJournalEffects
        + NetworkEffects
        + GuardEffects
        + GuardContextProvider
        + crate::effects::CryptoEffects
        + aura_core::PhysicalTimeEffects
        + aura_core::TimeEffects,
{
    // Timing captured by tracing span, not explicit measurement
    let payload_size = payload.len();

    // Phase 1: Prepare send (journal reduction and ratchet derivation)
    let deriv = match prepare_send(effects, context, channel).await {
        Ok(deriv) => deriv,
        Err(error) => {
            AMP_TELEMETRY.log_send_failure(context, channel, &error, None);
            return Err(error);
        }
    };
    let header = deriv.header;

    // Phase 2: AEAD encryption (timing captured by tracing span)
    let key = deriv.message_key.0;
    let nonce = nonce_from_header(&header);
    let sealed = match effects.aes_gcm_encrypt(&payload, &key, &nonce).await {
        Ok(sealed) => sealed,
        Err(e) => {
            let error = AuraError::crypto(format!("AMP seal failed: {}", e));
            AMP_TELEMETRY.log_send_failure(
                context,
                channel,
                &error,
                Some(AmpMetrics {
                    duration: Duration::ZERO, // Captured by tracing span
                    crypto_time: Some(Duration::ZERO),
                    guard_time: None,
                    journal_time: Some(Duration::ZERO),
                    bytes_processed: payload_size,
                    flow_charged: 0,
                }),
            );
            return Err(error);
        }
    };

    let msg = AmpMessage::new(header, sealed.clone());
    let bytes = match serde_json::to_vec(&msg) {
        Ok(bytes) => bytes,
        Err(e) => {
            let error = AuraError::serialization(e.to_string());
            AMP_TELEMETRY.log_send_failure(
                context,
                channel,
                &error,
                Some(AmpMetrics {
                    duration: Duration::ZERO, // Captured by tracing span
                    crypto_time: Some(Duration::ZERO),
                    guard_time: None,
                    journal_time: Some(Duration::ZERO),
                    bytes_processed: payload_size,
                    flow_charged: 0,
                }),
            );
            return Err(error);
        }
    };

    // Phase 3: Guard chain execution (authorization and flow budget) - timing captured by tracing span
    let peer = crate::guards::effect_system_trait::GuardContextProvider::authority_id(effects);
    let flow_cost = 1u32; // Minimal cost for AMP message
    let guard_chain = build_amp_send_guard(context, peer, Some(flow_cost));

    let guard_result = match guard_chain.evaluate_with_coupling(effects).await {
        Ok(result) => result,
        Err(error) => {
            AMP_TELEMETRY.log_send_failure(
                context,
                channel,
                &error,
                Some(AmpMetrics {
                    duration: Duration::ZERO, // Captured by tracing span
                    crypto_time: Some(Duration::ZERO),
                    guard_time: Some(Duration::ZERO),
                    journal_time: Some(Duration::ZERO),
                    bytes_processed: payload_size,
                    flow_charged: 0,
                }),
            );
            return Err(error);
        }
    };

    if !guard_result.authorized {
        let error = AuraError::permission_denied(
            guard_result
                .denial_reason
                .unwrap_or_else(|| "AMP send unauthorized".to_string()),
        );
        AMP_TELEMETRY.log_send_failure(
            context,
            channel,
            &error,
            Some(AmpMetrics {
                duration: Duration::ZERO, // Captured by tracing span
                crypto_time: Some(Duration::ZERO),
                guard_time: Some(Duration::ZERO),
                journal_time: Some(Duration::ZERO),
                bytes_processed: payload_size,
                flow_charged: flow_cost,
            }),
        );
        return Err(error);
    }

    // Log flow charge telemetry
    if let Some(receipt) = &guard_result.receipt {
        AMP_TELEMETRY.log_flow_charge(AmpFlowTelemetry {
            context,
            peer,
            operation: "amp_send",
            cost: flow_cost,
            receipt: Some(receipt.clone()),
            budget_remaining: guard_result.receipt.as_ref().map(|_| 0),
            charge_duration: Duration::ZERO, // Captured by tracing span
        });
    }

    // Phase 4: Network broadcast
    if let Err(e) = effects.broadcast(bytes).await {
        let error = AuraError::network(e.to_string());
        AMP_TELEMETRY.log_send_failure(
            context,
            channel,
            &error,
            Some(AmpMetrics {
                duration: Duration::ZERO, // Captured by tracing span
                crypto_time: Some(Duration::ZERO),
                guard_time: Some(Duration::ZERO),
                journal_time: Some(Duration::ZERO),
                bytes_processed: payload_size,
                flow_charged: flow_cost,
            }),
        );
        return Err(error);
    }

    // Success: Log comprehensive telemetry (timing captured by tracing span)
    AMP_TELEMETRY.log_send_success(AmpSendTelemetry {
        context,
        channel,
        header,
        payload_size,
        encrypted_size: sealed.len(),
        flow_charge: flow_cost,
        receipt: guard_result.receipt,
        metrics: AmpMetrics {
            duration: Duration::ZERO, // Captured by tracing span
            crypto_time: Some(Duration::ZERO),
            guard_time: Some(Duration::ZERO),
            journal_time: Some(Duration::ZERO),
            bytes_processed: payload_size,
            flow_charged: flow_cost,
        },
    });

    Ok(deriv.header)
}

/// High-level recv path: decode, validate header/window, and surface payload + derivation.
/// Uses centralized telemetry for structured observability including window validation.
/// Timing is captured via the tracing span (subscriber handles actual timing).
#[instrument(skip(effects, bytes), fields(context = %context))]
pub async fn amp_recv<E>(effects: &E, context: ContextId, bytes: Vec<u8>) -> Result<AmpMessage>
where
    E: AmpJournalEffects + crate::effects::CryptoEffects,
{
    // Timing captured by tracing span, not explicit measurement
    let wire_size = bytes.len();

    // Phase 1: Deserialize wire message
    let wire: AmpMessage = match serde_json::from_slice(&bytes) {
        Ok(wire) => wire,
        Err(e) => {
            let error = AuraError::serialization(e.to_string());
            AMP_TELEMETRY.log_receive_failure(
                context,
                None,
                None,
                &error,
                Some(AmpMetrics {
                    duration: Duration::ZERO, // Captured by tracing span
                    crypto_time: None,
                    guard_time: None,
                    journal_time: Some(Duration::ZERO),
                    bytes_processed: wire_size,
                    flow_charged: 0,
                }),
            );
            return Err(error);
        }
    };

    // Phase 2: Context validation
    if wire.header.context != context {
        let error = AuraError::invalid("AMP context mismatch");
        AMP_TELEMETRY.log_receive_failure(
            context,
            Some(wire.header),
            None,
            &error,
            Some(AmpMetrics {
                duration: Duration::ZERO, // Captured by tracing span
                crypto_time: None,
                guard_time: None,
                journal_time: Some(Duration::ZERO),
                bytes_processed: wire_size,
                flow_charged: 0,
            }),
        );
        return Err(error);
    }

    // Phase 3: Window/epoch validation and ratchet derivation
    let (deriv, window_validation) = match validate_header(effects, context, wire.header).await {
        Ok((deriv, bounds)) => {
            // Create successful window validation result
            let window_validation = create_window_validation_result(
                true, // epoch_valid (if we got here, validation passed)
                true, // generation_valid
                bounds,
                wire.header.ratchet_gen,
                None, // no error
            );
            (deriv, window_validation)
        }
        Err(error) => {
            // Create basic window validation result for failed validation
            let error_str = error.to_string().to_lowercase();
            let (epoch_valid, generation_valid) = if error_str.contains("epoch") {
                (false, true) // epoch mismatch, generation might be valid
            } else if error_str.contains("generation") || error_str.contains("window") {
                (true, false) // generation out of window
            } else {
                (false, false) // unknown validation failure
            };

            let window_validation = create_window_validation_result(
                epoch_valid,
                generation_valid,
                (0, 0), // window bounds unavailable on validation failure
                wire.header.ratchet_gen,
                None, // amp_error not available from AuraError
            );

            AMP_TELEMETRY.log_receive_failure(
                context,
                Some(wire.header),
                Some(window_validation),
                &error,
                Some(AmpMetrics {
                    duration: Duration::ZERO, // Captured by tracing span
                    crypto_time: None,
                    guard_time: None,
                    journal_time: Some(Duration::ZERO),
                    bytes_processed: wire_size,
                    flow_charged: 0,
                }),
            );
            return Err(error);
        }
    };

    // Phase 4: AEAD decryption (timing captured by tracing span)
    let key = deriv.message_key.0;
    let nonce = nonce_from_header(&wire.header);
    let opened = match effects.aes_gcm_decrypt(&wire.payload, &key, &nonce).await {
        Ok(opened) => opened,
        Err(e) => {
            let error = AuraError::crypto(format!("AMP open failed: {}", e));
            AMP_TELEMETRY.log_receive_failure(
                context,
                Some(wire.header),
                Some(window_validation), // window_validation from successful header validation
                &error,
                Some(AmpMetrics {
                    duration: Duration::ZERO, // Captured by tracing span
                    crypto_time: Some(Duration::ZERO),
                    guard_time: None,
                    journal_time: Some(Duration::ZERO),
                    bytes_processed: wire_size,
                    flow_charged: 0,
                }),
            );
            return Err(error);
        }
    };

    // Success: Log comprehensive telemetry (timing captured by tracing span)
    AMP_TELEMETRY.log_receive_success(AmpReceiveTelemetry {
        context,
        header: wire.header,
        wire_size,
        decrypted_size: opened.len(),
        window_validation, // from successful validation
        metrics: AmpMetrics {
            duration: Duration::ZERO, // Captured by tracing span
            crypto_time: Some(Duration::ZERO),
            guard_time: None,
            journal_time: Some(Duration::ZERO),
            bytes_processed: wire_size,
            flow_charged: 0,
        },
    });

    Ok(AmpMessage::new(wire.header, opened))
}

/// Receive + decrypt and surface a receipt alongside the payload.
pub async fn amp_recv_with_receipt<E>(
    effects: &E,
    context: ContextId,
    bytes: Vec<u8>,
) -> Result<AmpDelivery>
where
    E: AmpJournalEffects + crate::effects::CryptoEffects,
{
    let msg = amp_recv(effects, context, bytes).await?;
    Ok(AmpDelivery {
        receipt: msg.receipt(),
        payload: msg.payload,
    })
}
