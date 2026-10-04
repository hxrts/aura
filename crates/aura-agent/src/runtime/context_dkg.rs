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
    pub key_agreement_secret: [u8; 32],
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
            &self.local.key_agreement_secret,
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

/// Persist a finished context DKG: the key package (this device's share) in
/// secure storage, the public key package in storage.
pub(crate) async fn store_context_dkg_output(
    effects: &AuraEffectSystem,
    context: aura_core::types::identifiers::ContextId,
    epoch: u64,
    output: &ContextDkgOutput,
) -> Result<(), AuraError> {
    use aura_core::effects::{SecureStorageCapability, SecureStorageEffects, StorageCoreEffects};
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
        .map_err(|error| AuraError::storage(format!("store context DKG public package: {error}")))
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
                key_agreement_secret: secret,
            },
        )
    }

    // Three authorities run the context DKG over the shared transport, with
    // round-two packages sealed to each recipient device, and end with the
    // same group key.
    #[tokio::test]
    async fn members_of_a_context_complete_a_dkg_over_transport() {
        let shared = crate::SharedTransport::new();
        let mut members = Vec::new();
        for seed in [61u8, 62, 63] {
            members.push(member(&shared, seed).await);
        }
        let peers: Vec<ContextDkgPeer> = members.iter().map(|(_, peer, _)| peer.clone()).collect();
        let config = DkgConfig {
            epoch: 1,
            threshold: 2,
            max_signers: 3,
            membership_hash: Hash32::default(),
            cutoff: 0,
            prestate_hash: Hash32::default(),
            operation_hash: Hash32::default(),
            participants: peers.iter().map(|peer| peer.authority).collect(),
        };
        let outputs = futures::future::join_all(members.iter().map(|(effects, _, local)| {
            run_context_dkg(effects, config.clone(), &peers, local, 400)
        }))
        .await;
        let outputs: Vec<ContextDkgOutput> = outputs
            .into_iter()
            .map(|output| output.expect("dkg completes"))
            .collect();
        let group = outputs[0].public_key_package.verifying_key();
        assert!(outputs
            .iter()
            .all(|output| output.public_key_package.verifying_key() == group));

        // Each member stores its share; partials from storage verify against
        // the public package and any two members derive the same channel key.
        let context = aura_core::types::identifiers::ContextId::new_from_entropy([64; 32]);
        for ((effects, _, _), output) in members.iter().zip(&outputs) {
            store_context_dkg_output(effects, context, 1, output)
                .await
                .expect("stored");
        }
        let input =
            aura_core::crypto::threshold_prf::channel_base_key_input(&[64; 32], &[65; 32], 1);
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
}
