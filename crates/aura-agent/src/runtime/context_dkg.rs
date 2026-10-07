//! Context DKG ceremony runtime (work/8.md Tasks 55/59).
//!
//! Runs one participant's [`ContextDkgSession`] over device-addressed
//! transport envelopes: round-one packages are broadcast to the other
//! participants' devices, round-two packages are sealed to the recipient
//! device's Ed25519 key with `device_sealed`, and every received message must
//! come from the participant it claims (the envelope source authority is
//! authenticated by the transport receipt). The finished key package is kept
//! in secure storage and the public key package (the verifying shares other
//! members check threshold-PRF partials against, docs/100 §7.5) in storage.
//!
//! Callers choose the participants and supply each one's verified device key;
//! this module does not decide whom to trust.

// The ceremony API is consumed by AMP runtime key wiring (Work 10 Task 12,
// agreed split 2026-10-04); until that lands only tests call it.
#![allow(dead_code)]

use super::AuraEffectSystem;
use aura_consensus::dkg::context_session::{
    ContextDkgMessage, ContextDkgOutput, ContextDkgSession, DkgSealer,
};
use aura_consensus::dkg::DkgConfig;
use aura_core::effects::transport::TransportEnvelope;
use aura_core::types::identifiers::{AuthorityId, DeviceId};
use aura_core::AuraError;
use aura_sync::protocols::device_sealed::{open_for_device, seal_for_device, DeviceSealedPayload};
use rand::SeedableRng;
use std::collections::BTreeMap;

/// Content type of context DKG messages.
pub(crate) const CONTEXT_DKG_CONTENT_TYPE: &str = "application/aura-context-dkg";
const CONTEXT_DKG_SEAL_PURPOSE: &str = "aura.context-dkg.round2.v1";
const RECEIVE_POLL_MS: u64 = 50;

/// A DKG participant: its authority and the device running the ceremony.
#[derive(Debug, Clone)]
pub(crate) struct ContextDkgPeer {
    pub authority: AuthorityId,
    pub device: DeviceId,
    /// The device's verified Ed25519 public key (round-two packages are
    /// sealed to it).
    pub device_public_key: Vec<u8>,
}

/// This device's key material for opening round-two packages.
pub(crate) struct LocalDeviceKeys {
    pub public_key: Vec<u8>,
    pub key_agreement_secret: aura_core::secrets::PrivateKeyBytes,
}

struct DeviceSealer<'a> {
    effects: &'a AuraEffectSystem,
    me: AuthorityId,
    my_device: DeviceId,
    local: &'a LocalDeviceKeys,
    peers: &'a BTreeMap<AuthorityId, ContextDkgPeer>,
}

#[async_trait::async_trait]
impl DkgSealer for DeviceSealer<'_> {
    async fn seal(&self, recipient: AuthorityId, plaintext: &[u8]) -> aura_core::Result<Vec<u8>> {
        let peer = self
            .peers
            .get(&recipient)
            .ok_or_else(|| AuraError::invalid("no device key for DKG recipient"))?;
        let sealed = seal_for_device(
            self.effects,
            CONTEXT_DKG_SEAL_PURPOSE,
            recipient,
            peer.device,
            &peer.device_public_key,
            plaintext,
        )
        .await?;
        aura_core::util::serialization::to_vec(&sealed)
            .map_err(|error| AuraError::serialization(error.to_string()))
    }

    async fn open(&self, _sender: AuthorityId, sealed: &[u8]) -> aura_core::Result<Vec<u8>> {
        let sealed: DeviceSealedPayload = aura_core::util::serialization::from_slice(sealed)
            .map_err(|error| AuraError::serialization(error.to_string()))?;
        open_for_device(
            self.effects,
            CONTEXT_DKG_SEAL_PURPOSE,
            self.me,
            self.my_device,
            &self.local.public_key,
            <&[u8; 32]>::try_from(self.local.key_agreement_secret.expose_private_key())
                .map_err(|_| AuraError::crypto("device key-agreement secret must be 32 bytes"))?,
            &sealed,
        )
        .await
    }
}

async fn send(
    effects: &AuraEffectSystem,
    peers: &BTreeMap<AuthorityId, ContextDkgPeer>,
    to: &[AuthorityId],
    message: &ContextDkgMessage,
) -> Result<(), AuraError> {
    let bytes = aura_core::util::serialization::to_vec(message)
        .map_err(|error| AuraError::serialization(error.to_string()))?;
    for recipient in to {
        let peer = peers
            .get(recipient)
            .ok_or_else(|| AuraError::invalid("DKG recipient is not a participant"))?;
        effects
            .send_device_payload(peer.device.uuid(), CONTEXT_DKG_CONTENT_TYPE, bytes.clone())
            .await
            .map_err(|error| AuraError::network(error.to_string()))?;
    }
    Ok(())
}

fn is_dkg_envelope(
    envelope: &TransportEnvelope,
    peers: &BTreeMap<AuthorityId, ContextDkgPeer>,
) -> bool {
    envelope.metadata.get("content-type").map(String::as_str) == Some(CONTEXT_DKG_CONTENT_TYPE)
        && peers.contains_key(&envelope.source)
}

/// Run this device's side of a context DKG to completion (or until `max_polls`
/// receive polls pass with the ceremony unfinished).
pub(crate) async fn run_context_dkg(
    effects: &AuraEffectSystem,
    config: DkgConfig,
    peers: &[ContextDkgPeer],
    local: &LocalDeviceKeys,
    max_polls: u32,
) -> Result<ContextDkgOutput, AuraError> {
    use aura_core::effects::RandomCoreEffects;
    let me = aura_guards::GuardContextProvider::authority_id(effects);
    let peers: BTreeMap<AuthorityId, ContextDkgPeer> = peers
        .iter()
        .filter(|peer| peer.authority != me)
        .map(|peer| (peer.authority, peer.clone()))
        .collect();
    let sealer = DeviceSealer {
        effects,
        me,
        my_device: effects.device_id(),
        local,
        peers: &peers,
    };
    let mut rng = rand::rngs::StdRng::from_seed(effects.random_bytes_32().await);
    let (mut session, broadcast) = ContextDkgSession::start(config, me, &mut rng)?;
    send(effects, &peers, &broadcast.to, &broadcast.message).await?;

    for _ in 0..max_polls {
        while let Ok(envelope) = effects.take_inbound_envelope(|env| is_dkg_envelope(env, &peers)) {
            let message: ContextDkgMessage =
                aura_core::util::serialization::from_slice(&envelope.payload)
                    .map_err(|error| AuraError::serialization(error.to_string()))?;
            // The transport authenticated the envelope's source; a message
            // claiming another participant is an injection attempt.
            if message.from != envelope.source {
                tracing::warn!(
                    source = %envelope.source,
                    claimed = %message.from,
                    "dropping context DKG message whose sender does not match its envelope"
                );
                continue;
            }
            let (outgoing, output) = session.receive(message, &sealer).await?;
            for out in outgoing {
                send(effects, &peers, &out.to, &out.message).await?;
            }
            if let Some(output) = output {
                return Ok(output);
            }
        }
        aura_core::effects::time::PhysicalTimeEffects::sleep_ms(effects, RECEIVE_POLL_MS)
            .await
            .map_err(|error| AuraError::internal(error.to_string()))?;
    }
    Err(AuraError::internal(
        "context DKG did not complete before its deadline",
    ))
}

fn key_package_location(
    context: aura_core::types::identifiers::ContextId,
    epoch: u64,
) -> aura_core::effects::SecureStorageLocation {
    aura_core::effects::SecureStorageLocation::with_sub_key(
        "context_dkg",
        format!("{context}:{epoch}"),
        "key_package",
    )
}

fn public_package_key(context: aura_core::types::identifiers::ContextId, epoch: u64) -> String {
    format!("context_dkg/{context}/{epoch}/public_key_package")
}

fn roster_key(context: aura_core::types::identifiers::ContextId, epoch: u64) -> String {
    format!("context_dkg/{context}/{epoch}/roster")
}

/// The participants of a finished context DKG, in identifier order (the
/// participant at position `i` holds FROST identifier `i + 1`), and the
/// number of partials that combine to a PRF output.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct ContextKeyRoster {
    pub participants: Vec<AuthorityId>,
    pub threshold: u16,
}

impl ContextKeyRoster {
    fn identifier(&self, authority: AuthorityId) -> Option<u16> {
        let position = self.participants.iter().position(|p| *p == authority)?;
        u16::try_from(position + 1).ok()
    }
}

/// Persist a finished context DKG: the key package (this device's share) in
/// secure storage, the public key package and roster in storage.
pub(crate) async fn store_context_dkg_output(
    effects: &AuraEffectSystem,
    context: aura_core::types::identifiers::ContextId,
    config: &DkgConfig,
    output: &ContextDkgOutput,
) -> Result<(), AuraError> {
    use aura_core::effects::{SecureStorageCapability, SecureStorageEffects, StorageCoreEffects};
    let epoch = config.epoch;
    let key_package = output
        .key_package
        .serialize()
        .map_err(|error| AuraError::serialization(error.to_string()))?;
    effects
        .secure_store(
            &key_package_location(context, epoch),
            &key_package,
            &[
                SecureStorageCapability::Read,
                SecureStorageCapability::Write,
            ],
        )
        .await
        .map_err(|error| AuraError::storage(format!("store context DKG share: {error}")))?;
    let public = output
        .public_key_package
        .serialize()
        .map_err(|error| AuraError::serialization(error.to_string()))?;
    effects
        .store(&public_package_key(context, epoch), public)
        .await
        .map_err(|error| {
            AuraError::storage(format!("store context DKG public package: {error}"))
        })?;
    let roster = ContextKeyRoster {
        participants: config.participants.clone(),
        threshold: config.threshold,
    };
    let roster = aura_core::util::serialization::to_vec(&roster)
        .map_err(|error| AuraError::serialization(error.to_string()))?;
    effects
        .store(&roster_key(context, epoch), roster)
        .await
        .map_err(|error| AuraError::storage(format!("store context DKG roster: {error}")))
}

/// This device's threshold-PRF partial for `input` from the stored context
/// DKG share (docs/100 §7.5); `nonce` comes from the caller's random effect.
pub(crate) async fn context_prf_partial(
    effects: &AuraEffectSystem,
    context: aura_core::types::identifiers::ContextId,
    epoch: u64,
    input: &[u8],
    nonce: &[u8; 64],
) -> Result<aura_core::crypto::threshold_prf::PartialEvaluation, AuraError> {
    use aura_core::effects::{SecureStorageCapability, SecureStorageEffects};
    let bytes = effects
        .secure_retrieve(
            &key_package_location(context, epoch),
            &[SecureStorageCapability::Read],
        )
        .await
        .map_err(|error| AuraError::storage(format!("load context DKG share: {error}")))?;
    let key_package = frost_ed25519::keys::KeyPackage::deserialize(&bytes)
        .map_err(|error| AuraError::serialization(error.to_string()))?;
    let participant = u16::from_le_bytes(
        key_package.identifier().serialize()[..2]
            .try_into()
            .map_err(|_| AuraError::invalid("context DKG identifier"))?,
    );
    let share = key_package.signing_share().serialize();
    aura_core::crypto::threshold_prf::evaluate_partial(participant, &share, input, nonce)
        .map_err(|error| AuraError::crypto(error.to_string()))
}

/// Content type of channel base-key partial evaluations.
pub(crate) const CHANNEL_KEY_PARTIAL_CONTENT_TYPE: &str =
    "application/aura-amp-channel-key-partial";

/// One member's threshold-PRF partial for a channel base key.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ChannelKeyPartial {
    context: aura_core::types::identifiers::ContextId,
    dkg_epoch: u64,
    channel: aura_core::types::identifiers::ChannelId,
    chan_epoch: u64,
    from: AuthorityId,
    partial: aura_core::crypto::threshold_prf::PartialEvaluation,
}

async fn load_roster(
    effects: &AuraEffectSystem,
    context: aura_core::types::identifiers::ContextId,
    dkg_epoch: u64,
) -> Result<(ContextKeyRoster, frost_ed25519::keys::PublicKeyPackage), AuraError> {
    use aura_core::effects::StorageCoreEffects;
    let read = |what: &'static str, bytes: Option<Vec<u8>>| {
        bytes.ok_or_else(|| AuraError::not_found(format!("context DKG {what} for {context}")))
    };
    let roster = effects
        .retrieve(&roster_key(context, dkg_epoch))
        .await
        .map_err(|error| AuraError::storage(format!("load context DKG roster: {error}")))?;
    let roster: ContextKeyRoster =
        aura_core::util::serialization::from_slice(&read("roster", roster)?)
            .map_err(|error| AuraError::serialization(error.to_string()))?;
    let public = effects
        .retrieve(&public_package_key(context, dkg_epoch))
        .await
        .map_err(|error| AuraError::storage(format!("load context DKG public package: {error}")))?;
    let public =
        frost_ed25519::keys::PublicKeyPackage::deserialize(&read("public package", public)?)
            .map_err(|error| AuraError::serialization(error.to_string()))?;
    Ok((roster, public))
}

/// Check a received partial: it comes from a roster member other than us,
/// under that member's FROST identifier, with a proof against its verifying
/// share.
fn verify_channel_key_partial(
    roster: &ContextKeyRoster,
    public: &frost_ed25519::keys::PublicKeyPackage,
    input: &[u8],
    message: &ChannelKeyPartial,
) -> Result<(), AuraError> {
    let expected = roster
        .identifier(message.from)
        .ok_or_else(|| AuraError::permission_denied("channel key partial from a non-member"))?;
    if message.partial.participant != expected {
        return Err(AuraError::permission_denied(
            "channel key partial claims another member's identifier",
        ));
    }
    let identifier = frost_ed25519::Identifier::try_from(expected)
        .map_err(|error| AuraError::invalid(error.to_string()))?;
    let verifying = public
        .verifying_shares()
        .get(&identifier)
        .ok_or_else(|| AuraError::invalid("no verifying share for channel key partial"))?
        .serialize();
    aura_core::crypto::threshold_prf::verify_partial(&verifying, input, &message.partial)
        .map_err(|error| AuraError::crypto(error.to_string()))
}

/// Derive the base key of `channel` at `chan_epoch` (>= 1) from the context
/// threshold PRF (docs/112 §4): send this member's verified partial to the
/// other roster members' devices, combine our partial with the first
/// verified partials received until the roster threshold is met, and store
/// the key at [`SecureStorageLocation::amp_channel_base_key`]. Every member
/// of the context runs this for the same `(channel, chan_epoch)`; none of
/// the inputs other than the members' shares is secret.
///
/// [`SecureStorageLocation::amp_channel_base_key`]: aura_core::effects::SecureStorageLocation::amp_channel_base_key
pub(crate) async fn derive_channel_base_key(
    effects: &AuraEffectSystem,
    context: aura_core::types::identifiers::ContextId,
    dkg_epoch: u64,
    channel: aura_core::types::identifiers::ChannelId,
    chan_epoch: u64,
    peers: &[ContextDkgPeer],
    max_polls: u32,
) -> Result<[u8; 32], AuraError> {
    use aura_core::crypto::threshold_prf;
    use aura_core::effects::{
        RandomCoreEffects, SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
    };
    if chan_epoch == 0 {
        return Err(AuraError::invalid(
            "epoch 0 uses the bootstrap key, not the context PRF",
        ));
    }
    let me = aura_guards::GuardContextProvider::authority_id(effects);
    let (roster, public) = load_roster(effects, context, dkg_epoch).await?;
    if roster.identifier(me).is_none() {
        return Err(AuraError::permission_denied(
            "this authority holds no share of the context key",
        ));
    }
    let input = threshold_prf::channel_base_key_input(&context, &channel, chan_epoch);
    let mut nonce = [0u8; 64];
    nonce[..32].copy_from_slice(&effects.random_bytes_32().await);
    nonce[32..].copy_from_slice(&effects.random_bytes_32().await);
    let own = context_prf_partial(effects, context, dkg_epoch, &input, &nonce).await?;

    let message = ChannelKeyPartial {
        context,
        dkg_epoch,
        channel,
        chan_epoch,
        from: me,
        partial: own,
    };
    let bytes = aura_core::util::serialization::to_vec(&message)
        .map_err(|error| AuraError::serialization(error.to_string()))?;
    let peers: BTreeMap<AuthorityId, &ContextDkgPeer> = peers
        .iter()
        .filter(|peer| peer.authority != me && roster.participants.contains(&peer.authority))
        .map(|peer| (peer.authority, peer))
        .collect();
    for peer in peers.values() {
        effects
            .send_device_payload(
                peer.device.uuid(),
                CHANNEL_KEY_PARTIAL_CONTENT_TYPE,
                bytes.clone(),
            )
            .await
            .map_err(|error| AuraError::network(error.to_string()))?;
    }

    let threshold = usize::from(roster.threshold);
    let mut partials = BTreeMap::from([(me, own)]);
    let is_ours = |envelope: &TransportEnvelope| {
        envelope.metadata.get("content-type").map(String::as_str)
            == Some(CHANNEL_KEY_PARTIAL_CONTENT_TYPE)
            && aura_core::util::serialization::from_slice::<ChannelKeyPartial>(&envelope.payload)
                .is_ok_and(|m| {
                    m.context == context
                        && m.dkg_epoch == dkg_epoch
                        && m.channel == channel
                        && m.chan_epoch == chan_epoch
                })
    };
    for _ in 0..max_polls {
        while partials.len() < threshold {
            let Ok(envelope) = effects.take_inbound_envelope(is_ours) else {
                break;
            };
            let Ok(received) =
                aura_core::util::serialization::from_slice::<ChannelKeyPartial>(&envelope.payload)
            else {
                continue;
            };
            // The transport authenticated the envelope's source; a partial
            // claiming another member is an injection attempt.
            if received.from != envelope.source {
                tracing::warn!(
                    source = %envelope.source,
                    claimed = %received.from,
                    "dropping channel key partial whose sender does not match its envelope"
                );
                continue;
            }
            if let Err(error) = verify_channel_key_partial(&roster, &public, &input, &received) {
                tracing::warn!(from = %received.from, %error, "dropping invalid channel key partial");
                continue;
            }
            partials.entry(received.from).or_insert(received.partial);
        }
        if partials.len() >= threshold {
            let chosen: Vec<_> = partials.values().copied().collect();
            let key = threshold_prf::combine(threshold, &input, &chosen)
                .map_err(|error| AuraError::crypto(error.to_string()))?;
            effects
                .secure_store(
                    &SecureStorageLocation::amp_channel_base_key(&context, &channel, chan_epoch),
                    &key,
                    &[
                        SecureStorageCapability::Read,
                        SecureStorageCapability::Write,
                    ],
                )
                .await
                .map_err(|error| AuraError::storage(format!("store channel base key: {error}")))?;
            return Ok(key);
        }
        aura_core::effects::time::PhysicalTimeEffects::sleep_ms(effects, RECEIVE_POLL_MS)
            .await
            .map_err(|error| AuraError::internal(error.to_string()))?;
    }
    Err(AuraError::internal(
        "channel base key partials did not reach the threshold before the deadline",
    ))
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::core::AgentConfig;
    use aura_core::effects::{CryptoCoreEffects, CryptoExtendedEffects};
    use aura_core::Hash32;

    async fn member(
        shared: &crate::SharedTransport,
        seed: u8,
    ) -> (AuraEffectSystem, ContextDkgPeer, LocalDeviceKeys) {
        let authority = AuthorityId::new_from_entropy([seed; 32]);
        let config = AgentConfig {
            device_id: DeviceId::new_from_entropy([seed.wrapping_add(100); 32]),
            ..AgentConfig::default()
        };
        let effects =
            AuraEffectSystem::simulation_for_named_test_with_shared_transport_for_authority(
                &config,
                &format!("context-dkg-{seed}"),
                authority,
                shared.clone(),
            )
            .expect("effects");
        let (private_key, public_key) = effects.ed25519_generate_keypair().await.expect("keys");
        let secret = effects
            .convert_ed25519_to_x25519_private(&private_key)
            .await
            .expect("x25519");
        let peer = ContextDkgPeer {
            authority,
            device: effects.device_id(),
            device_public_key: public_key.clone(),
        };
        (
            effects,
            peer,
            LocalDeviceKeys {
                public_key,
                key_agreement_secret: aura_core::secrets::PrivateKeyBytes::import_from_slice(
                    &secret,
                ),
            },
        )
    }

    type Member = (AuraEffectSystem, ContextDkgPeer, LocalDeviceKeys);

    fn dkg_config(epoch: u64, peers: &[ContextDkgPeer]) -> DkgConfig {
        DkgConfig {
            epoch,
            threshold: 2,
            max_signers: u16::try_from(peers.len()).expect("small"),
            membership_hash: Hash32::default(),
            cutoff: 0,
            prestate_hash: Hash32::default(),
            operation_hash: Hash32::default(),
            participants: peers.iter().map(|peer| peer.authority).collect(),
        }
    }

    /// Run a context DKG among `members` and store each member's output.
    async fn establish_context_key(
        members: &[Member],
        context: aura_core::types::identifiers::ContextId,
        epoch: u64,
    ) -> Vec<ContextDkgOutput> {
        let peers: Vec<ContextDkgPeer> = members.iter().map(|(_, peer, _)| peer.clone()).collect();
        let config = dkg_config(epoch, &peers);
        let outputs: Vec<ContextDkgOutput> =
            futures::future::join_all(members.iter().map(|(effects, _, local)| {
                run_context_dkg(effects, config.clone(), &peers, local, 400)
            }))
            .await
            .into_iter()
            .map(|output| output.expect("dkg completes"))
            .collect();
        for ((effects, _, _), output) in members.iter().zip(&outputs) {
            store_context_dkg_output(effects, context, &config, output)
                .await
                .expect("stored");
        }
        outputs
    }

    /// Every member derives `(channel, chan_epoch)`'s base key concurrently.
    async fn derive_all(
        members: &[Member],
        context: aura_core::types::identifiers::ContextId,
        dkg_epoch: u64,
        channel: aura_core::types::identifiers::ChannelId,
        chan_epoch: u64,
    ) -> Vec<[u8; 32]> {
        let peers: Vec<ContextDkgPeer> = members.iter().map(|(_, peer, _)| peer.clone()).collect();
        futures::future::join_all(members.iter().map(|(effects, _, _)| {
            derive_channel_base_key(
                effects, context, dkg_epoch, channel, chan_epoch, &peers, 400,
            )
        }))
        .await
        .into_iter()
        .map(|key| key.expect("base key derived"))
        .collect()
    }

    // Three authorities run the context DKG over the shared transport, with
    // round-two packages sealed to each recipient device, and end with the
    // same group key; any two members' partials combine to the same PRF
    // output.
    #[tokio::test(start_paused = true)]
    async fn members_of_a_context_complete_a_dkg_over_transport() {
        let shared = crate::SharedTransport::new();
        let mut members = Vec::new();
        for seed in [61u8, 62, 63] {
            members.push(member(&shared, seed).await);
        }
        let context = aura_core::types::identifiers::ContextId::new_from_entropy([64; 32]);
        let outputs = establish_context_key(&members, context, 1).await;
        let group = outputs[0].public_key_package.verifying_key();
        assert!(outputs
            .iter()
            .all(|output| output.public_key_package.verifying_key() == group));

        let input = aura_core::crypto::threshold_prf::channel_base_key_input(
            &context,
            &aura_core::types::identifiers::ChannelId::from_bytes([65; 32]),
            1,
        );
        let mut partials = Vec::new();
        for (index, ((effects, _, _), output)) in members.iter().zip(&outputs).enumerate() {
            let partial = context_prf_partial(effects, context, 1, &input, &[index as u8 + 1; 64])
                .await
                .expect("partial");
            let verifying = output.public_key_package.verifying_shares()
                [output.key_package.identifier()]
            .serialize();
            aura_core::crypto::threshold_prf::verify_partial(&verifying, &input, &partial)
                .expect("partial verifies against the public package");
            partials.push(partial);
        }
        let combine = |pair: [usize; 2]| {
            aura_core::crypto::threshold_prf::combine(
                2,
                &input,
                &[partials[pair[0]], partials[pair[1]]],
            )
            .expect("combine")
        };
        assert_eq!(combine([0, 1]), combine([1, 2]));
    }

    // Tasks 31/55: members of a context derive the same epoch >= 1 channel
    // base key by exchanging verified partials; the key differs per channel,
    // per channel epoch and per context-key (DKG) epoch; a non-member that
    // knows every public input (context, channel, epoch, the public key
    // package) derives nothing, and its forged partials are rejected.
    #[tokio::test(start_paused = true)]
    async fn members_derive_channel_base_keys_that_non_members_cannot() {
        use aura_core::effects::{SecureStorageCapability, SecureStorageEffects};
        use aura_core::types::identifiers::{ChannelId, ContextId};
        let shared = crate::SharedTransport::new();
        let mut members = Vec::new();
        for seed in [71u8, 72, 73] {
            members.push(member(&shared, seed).await);
        }
        let context = ContextId::new_from_entropy([74; 32]);
        let channel = ChannelId::from_bytes([75; 32]);
        let other_channel = ChannelId::from_bytes([76; 32]);
        let outputs = establish_context_key(&members, context, 1).await;

        let keys = derive_all(&members, context, 1, channel, 1).await;
        assert!(keys.iter().all(|key| *key == keys[0]), "members agree");
        let stored = members[2]
            .0
            .secure_retrieve(
                &aura_core::effects::SecureStorageLocation::amp_channel_base_key(
                    &context, &channel, 1,
                ),
                &[SecureStorageCapability::Read],
            )
            .await
            .expect("base key cached");
        assert_eq!(stored, keys[0]);

        let next_epoch = derive_all(&members, context, 1, channel, 2).await[0];
        let other = derive_all(&members, context, 1, other_channel, 1).await[0];
        assert_ne!(next_epoch, keys[0], "keys change across channel epochs");
        assert_ne!(other, keys[0], "keys differ per channel");

        // A re-run DKG (authority epoch rotation) gives new keys.
        establish_context_key(&members, context, 2).await;
        let rotated = derive_all(&members, context, 2, channel, 1).await[0];
        assert_ne!(rotated, keys[0], "keys change across context-key epochs");

        // A non-member holds no roster or share and cannot derive.
        let (outsider, _, _) = member(&shared, 79).await;
        let peers: Vec<ContextDkgPeer> = members.iter().map(|(_, peer, _)| peer.clone()).collect();
        assert!(
            derive_channel_base_key(&outsider, context, 1, channel, 1, &peers, 4)
                .await
                .is_err(),
            "a non-member derives no channel key"
        );

        // A partial from a share the outsider made up does not verify, nor
        // does a member's genuine partial replayed under another member.
        let roster = ContextKeyRoster {
            participants: peers.iter().map(|peer| peer.authority).collect(),
            threshold: 2,
        };
        let public = &outputs[0].public_key_package;
        let input = aura_core::crypto::threshold_prf::channel_base_key_input(&context, &channel, 1);
        let forged =
            aura_core::crypto::threshold_prf::evaluate_partial(1, &[7u8; 32], &input, &[8u8; 64])
                .expect("partial");
        let forged = ChannelKeyPartial {
            context,
            dkg_epoch: 1,
            channel,
            chan_epoch: 1,
            from: peers[0].authority,
            partial: forged,
        };
        assert!(verify_channel_key_partial(&roster, public, &input, &forged).is_err());
        let genuine = context_prf_partial(&members[0].0, context, 1, &input, &[9u8; 64])
            .await
            .expect("partial");
        let replayed = ChannelKeyPartial {
            from: peers[1].authority,
            partial: genuine,
            ..forged.clone()
        };
        assert!(verify_channel_key_partial(&roster, public, &input, &replayed).is_err());
        let accepted = ChannelKeyPartial {
            from: peers[0].authority,
            partial: genuine,
            ..forged
        };
        verify_channel_key_partial(&roster, public, &input, &accepted).expect("genuine partial");
    }
}
