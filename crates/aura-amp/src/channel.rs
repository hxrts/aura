//! AMP channel lifecycle coordinator (Layer 4)
//!
//! Provides an implementation of `aura_core::effects::AmpChannelEffects` that
//! persists AMP channel facts to the context journal via `AmpJournalEffects`.
//! Encryption uses a deterministic stream mask derived from channel header +
//! sender to keep Layer 4 transport non-plaintext without introducing a
//! ratchet/AEAD dependency here. This is sufficient for simulation/demo;
//! production AEAD can replace the mask while preserving the interface.

use aura_core::effects::amp::{
    AmpChannelEffects, AmpChannelError, AmpCiphertext, AmpHeader, ChannelCloseParams,
    ChannelCreateParams, ChannelJoinParams, ChannelLeaveParams, ChannelSendParams,
};
use aura_core::effects::random::RandomExtendedEffects;
use aura_core::hash::hash;
use aura_core::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_core::threshold::{policy_for, AgreementMode, CeremonyFlow};
use aura_core::time::{OrderTime, TimeStamp};
use aura_core::Hash32;
use aura_journal::fact::{
    ChannelBumpReason, ChannelCheckpoint, ChannelPolicy, ProposedChannelEpochBump, RelationalFact,
};
use aura_journal::DomainFact;
use aura_macros::DomainFact;
use serde::{Deserialize, Serialize};

use crate::{config::AmpRuntimeConfig, get_channel_state, AmpJournalEffects};

/// Simple coordinator that writes AMP channel facts into the context journal.
pub struct AmpChannelCoordinator<E> {
    effects: E,
}

impl<E> AmpChannelCoordinator<E> {
    pub fn new(effects: E) -> Self {
        Self { effects }
    }
}

#[async_trait::async_trait]
impl<E> AmpChannelEffects for AmpChannelCoordinator<E>
where
    E: AmpJournalEffects + RandomExtendedEffects + Send + Sync,
{
    async fn create_channel(
        &self,
        params: ChannelCreateParams,
    ) -> std::result::Result<ChannelId, AmpChannelError> {
        let policy = policy_for(CeremonyFlow::AmpBootstrap);
        if !policy.allows_mode(AgreementMode::Provisional) {
            return Err(AmpChannelError::InvalidState(
                "AMP bootstrap policy does not allow provisional channels".to_string(),
            ));
        }
        let channel = if let Some(id) = params.channel {
            id
        } else {
            let order = self
                .effects
                .order_time()
                .await
                .map_err(|e| AmpChannelError::Internal(e.to_string()))?;
            aura_core::identifiers::ChannelId::from_bytes(order.0)
        };

        let config = AmpRuntimeConfig::default();
        let window = params.skip_window.unwrap_or(config.default_skip_window);

        let checkpoint = ChannelCheckpoint {
            context: params.context,
            channel,
            chan_epoch: 0,
            base_gen: 0,
            window,
            ck_commitment: Hash32::default(),
            skip_window_override: Some(window),
        };

        self.effects
            .insert_relational_fact(RelationalFact::Protocol(
                aura_journal::ProtocolRelationalFact::AmpChannelCheckpoint(checkpoint),
            ))
            .await
            .map_err(map_err)?;

        if params.topic.is_some() || params.skip_window.is_some() {
            let policy = ChannelPolicy {
                context: params.context,
                channel,
                skip_window: params.skip_window.or(Some(window)),
            };
            self.effects
                .insert_relational_fact(RelationalFact::Protocol(
                    aura_journal::ProtocolRelationalFact::AmpChannelPolicy(policy),
                ))
                .await
                .map_err(map_err)?;
        }

        Ok(channel)
    }

    async fn close_channel(
        &self,
        params: ChannelCloseParams,
    ) -> std::result::Result<(), AmpChannelError> {
        let state = get_channel_state(&self.effects, params.context, params.channel)
            .await
            .map_err(map_err)?;

        let policy = policy_for(CeremonyFlow::AmpEpochBump);
        if !policy.allows_mode(AgreementMode::Provisional) {
            return Err(AmpChannelError::InvalidState(
                "AMP epoch bump policy does not allow provisional mode".to_string(),
            ));
        }

        let bump_nonce = self.effects.random_uuid().await.as_bytes().to_vec();
        let bump_id = aura_core::Hash32(hash(&bump_nonce));
        let proposal = ProposedChannelEpochBump {
            context: params.context,
            channel: params.channel,
            parent_epoch: state.chan_epoch,
            new_epoch: state.chan_epoch + 1,
            bump_id,
            reason: ChannelBumpReason::Routine,
        };

        self.effects
            .insert_relational_fact(RelationalFact::Protocol(
                aura_journal::ProtocolRelationalFact::AmpProposedChannelEpochBump(proposal),
            ))
            .await
            .map_err(map_err)?;

        let policy = ChannelPolicy {
            context: params.context,
            channel: params.channel,
            skip_window: Some(0),
        };

        self.effects
            .insert_relational_fact(RelationalFact::Protocol(
                aura_journal::ProtocolRelationalFact::AmpChannelPolicy(policy),
            ))
            .await
            .map_err(map_err)?;

        Ok(())
    }

    async fn join_channel(
        &self,
        params: ChannelJoinParams,
    ) -> std::result::Result<(), AmpChannelError> {
        // Verify the channel exists by getting its state
        let _state = get_channel_state(&self.effects, params.context, params.channel)
            .await
            .map_err(map_err)?;

        let timestamp = ChannelMembershipFact::random_timestamp(&self.effects).await;
        let membership = ChannelMembershipFact::new(
            params.context,
            params.channel,
            params.participant,
            ChannelParticipantEvent::Joined,
            timestamp,
        );
        self.effects
            .insert_relational_fact(membership.to_generic())
            .await
            .map_err(map_err)?;

        tracing::debug!(
            "Participant {:?} joined channel {:?} in context {:?}",
            params.participant,
            params.channel,
            params.context
        );

        Ok(())
    }

    async fn leave_channel(
        &self,
        params: ChannelLeaveParams,
    ) -> std::result::Result<(), AmpChannelError> {
        // Verify the channel exists by getting its state
        let _state = get_channel_state(&self.effects, params.context, params.channel)
            .await
            .map_err(map_err)?;

        let timestamp = ChannelMembershipFact::random_timestamp(&self.effects).await;
        let membership = ChannelMembershipFact::new(
            params.context,
            params.channel,
            params.participant,
            ChannelParticipantEvent::Left,
            timestamp,
        );
        self.effects
            .insert_relational_fact(membership.to_generic())
            .await
            .map_err(map_err)?;

        tracing::debug!(
            "Participant {:?} left channel {:?} in context {:?}",
            params.participant,
            params.channel,
            params.context
        );

        Ok(())
    }

    async fn send_message(
        &self,
        params: ChannelSendParams,
    ) -> std::result::Result<AmpCiphertext, AmpChannelError> {
        let state = get_channel_state(&self.effects, params.context, params.channel)
            .await
            .map_err(map_err)?;

        let header = AmpHeader {
            context: params.context,
            channel: params.channel,
            chan_epoch: state.chan_epoch,
            ratchet_gen: state.current_gen,
        };

        let ciphertext = mask_ciphertext(&header, &params.sender, &params.plaintext);

        Ok(AmpCiphertext { header, ciphertext })
    }
}

/// Event types for channel membership facts.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ChannelParticipantEvent {
    /// Participant joined the channel.
    Joined,
    /// Participant left the channel.
    Left,
}

/// Domain fact that records AMP channel membership events.
#[derive(Debug, Clone, Serialize, Deserialize, DomainFact)]
#[domain_fact(
    type_id = "amp-channel-membership",
    schema_version = 1,
    context = "context"
)]
pub struct ChannelMembershipFact {
    #[serde(default = "channel_membership_schema_version")]
    schema_version: u16,
    context: ContextId,
    channel: ChannelId,
    participant: AuthorityId,
    event: ChannelParticipantEvent,
    timestamp: TimeStamp,
}

impl ChannelMembershipFact {
    pub fn new(
        context: ContextId,
        channel: ChannelId,
        participant: AuthorityId,
        event: ChannelParticipantEvent,
        timestamp: TimeStamp,
    ) -> Self {
        Self {
            schema_version: channel_membership_schema_version(),
            context,
            channel,
            participant,
            event,
            timestamp,
        }
    }

    pub async fn random_timestamp<A: AmpJournalEffects>(effects: &A) -> TimeStamp {
        match effects.order_time().await {
            Ok(order) => TimeStamp::OrderClock(order),
            Err(err) => {
                tracing::warn!("order_time unavailable: {err}; falling back to zero order");
                TimeStamp::OrderClock(OrderTime([0u8; 32]))
            }
        }
    }
}

fn channel_membership_schema_version() -> u16 {
    1
}

fn map_err(e: aura_core::AuraError) -> AmpChannelError {
    match e {
        aura_core::AuraError::NotFound { message: _ } => AmpChannelError::NotFound,
        aura_core::AuraError::Storage { message } => AmpChannelError::Storage(message),
        aura_core::AuraError::PermissionDenied { message: _ } => AmpChannelError::Unauthorized,
        aura_core::AuraError::Crypto { message } => AmpChannelError::Crypto(message),
        other => AmpChannelError::Internal(other.to_string()),
    }
}

// ============================================================================
// Demo Encryption (feature-gated)
// ============================================================================

/// Derive a deterministic keystream from header + sender and XOR-mask the payload.
///
/// # Security Warning
///
/// This is a **demo-only** encryption scheme using XOR with a hash-derived keystream.
/// It provides NO real security guarantees and MUST NOT be used in production.
/// Production deployments should disable the `demo-crypto` feature and use
/// proper AEAD encryption via `CryptoEffects`.
#[cfg(feature = "demo-crypto")]
fn mask_ciphertext(header: &AmpHeader, sender: &AuthorityId, plaintext: &[u8]) -> Vec<u8> {
    let mut key_material = Vec::new();
    key_material.extend_from_slice(header.channel.as_bytes());
    key_material.extend_from_slice(&header.chan_epoch.to_le_bytes());
    key_material.extend_from_slice(&header.ratchet_gen.to_le_bytes());
    key_material.extend_from_slice(sender.0.as_bytes());

    let mut keystream = Vec::with_capacity(plaintext.len());
    let mut counter: u64 = 0;
    while keystream.len() < plaintext.len() {
        let mut block_input = key_material.clone();
        block_input.extend_from_slice(&counter.to_le_bytes());
        let block = hash(&block_input);
        keystream.extend_from_slice(&block);
        counter += 1;
    }

    plaintext
        .iter()
        .zip(keystream)
        .map(|(p, k)| p ^ k)
        .collect()
}

/// Placeholder when demo-crypto is disabled - panics to prevent accidental use.
#[cfg(not(feature = "demo-crypto"))]
fn mask_ciphertext(_header: &AmpHeader, _sender: &AuthorityId, _plaintext: &[u8]) -> Vec<u8> {
    panic!(
        "Demo encryption is disabled. Enable the `demo-crypto` feature for testing, \
         or use `amp_send` with proper AEAD encryption for production."
    );
}
