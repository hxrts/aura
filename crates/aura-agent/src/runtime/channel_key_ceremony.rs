//! Channel key ceremony over transport (work/8.md Task 164 step 1).
//!
//! When a channel's membership changes, its members run a DKG for the new
//! key epoch (`context_dkg`): the roster is exactly the channel's members at
//! that epoch, and the finished shares derive the epoch's channel base key
//! through the threshold PRF. A late joiner therefore holds keys only for
//! epochs at or after its join, and a departed member none after it left.
//!
//! The coordinator (the author of the membership change) sends each other
//! participant a [`ChannelKeyInvite`]; every participant, the coordinator
//! included, runs the same ceremony: resolve each participant's device key
//! from this device's verified device-key directory, run the DKG with
//! round-two packages sealed to those device keys, persist the share and
//! public package, and derive the epoch's base key. A participant joins only
//! if the coordinator has standing in the channel and the participant is on
//! the roster; nothing in the invite selects a device key.

use super::context_dkg::{
    derive_channel_base_key, run_context_dkg, store_context_dkg_output, ChannelKeyScope,
    ContextDkgPeer, LocalDeviceKeys,
};
use super::AuraEffectSystem;
use aura_consensus::dkg::DkgConfig;
use aura_core::effects::transport::TransportEnvelope;
use aura_core::effects::{
    CryptoCoreEffects, CryptoExtendedEffects, PhysicalTimeEffects, SecureStorageCapability,
    SecureStorageEffects, SecureStorageLocation, StorageCoreEffects,
};
use aura_core::types::identifiers::{AuthorityId, DeviceId};
use aura_core::{AuraError, Hash32};

/// Content type of channel key ceremony invitations.
pub(crate) const CHANNEL_KEY_INVITE_CONTENT_TYPE: &str = "application/aura-channel-key-invite";
const INVITE_VERSION: u16 = 1;
const MAX_INVITE_BYTES: usize = 16_384;
/// Receive polls a ceremony waits for the other participants.
pub(crate) const CEREMONY_MAX_POLLS: u32 = 1_200;

/// Invitation to run the channel key ceremony of `scope` at `epoch`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChannelKeyInvite {
    pub version: u16,
    pub scope: ChannelKeyScope,
    pub epoch: u64,
    pub threshold: u16,
    /// The roster, in FROST identifier order.
    pub participants: Vec<AuthorityId>,
    pub coordinator: AuthorityId,
    /// This attempt of the ceremony; a retry gets a fresh one.
    pub ceremony: Hash32,
}

impl ChannelKeyInvite {
    /// An invitation for `participants` (deduplicated, sorted); the threshold
    /// is a majority, at least two.
    pub(crate) fn new(
        scope: ChannelKeyScope,
        epoch: u64,
        coordinator: AuthorityId,
        participants: impl IntoIterator<Item = AuthorityId>,
    ) -> Result<Self, AuraError> {
        let participants: Vec<AuthorityId> = participants
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        if participants.len() < 2 || !participants.contains(&coordinator) || epoch == 0 {
            return Err(AuraError::invalid(
                "a channel key ceremony needs two or more members including its coordinator, at epoch >= 1",
            ));
        }
        let n = u16::try_from(participants.len())
            .map_err(|_| AuraError::invalid("channel key roster too large"))?;
        Ok(Self {
            version: INVITE_VERSION,
            scope,
            epoch,
            threshold: (n / 2 + 1).max(2),
            participants,
            coordinator,
            ceremony: Hash32::default(),
        })
    }

    /// This invitation as attempt `ceremony` (a retry needs a fresh one).
    #[must_use]
    pub(crate) fn with_ceremony(mut self, ceremony: Hash32) -> Self {
        self.ceremony = ceremony;
        self
    }

    fn validate(&self) -> Result<(), AuraError> {
        let n = self.participants.len();
        let sorted = self.participants.windows(2).all(|pair| pair[0] < pair[1]);
        if self.version != INVITE_VERSION
            || self.epoch == 0
            || n < 2
            || !sorted
            || !self.participants.contains(&self.coordinator)
            || usize::from(self.threshold) > n
            || self.threshold < 2
        {
            return Err(AuraError::invalid("malformed channel key invite"));
        }
        Ok(())
    }

    fn dkg_config(&self) -> Result<DkgConfig, AuraError> {
        let roster = aura_core::util::serialization::to_vec(&(self.scope, &self.participants))
            .map_err(|error| AuraError::serialization(error.to_string()))?;
        Ok(DkgConfig {
            epoch: self.epoch,
            threshold: self.threshold,
            max_signers: u16::try_from(self.participants.len())
                .map_err(|_| AuraError::invalid("channel key roster too large"))?,
            membership_hash: Hash32::from_bytes(&roster),
            cutoff: 0,
            prestate_hash: Hash32::default(),
            operation_hash: self.ceremony,
            participants: self.participants.clone(),
        })
    }
}

// ---------------------------------------------------------------------------
// Device keys
// ---------------------------------------------------------------------------

/// A device key this device holds as verified for `authority`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct VerifiedDeviceKey {
    pub authority: AuthorityId,
    pub device: DeviceId,
    pub public_key: Vec<u8>,
}

fn device_ceremony_key_location(device: DeviceId) -> SecureStorageLocation {
    SecureStorageLocation::with_sub_key("channel_key_device", device.to_string(), "ed25519")
}

fn directory_key(authority: AuthorityId) -> String {
    format!("device_key_directory/{authority}")
}

/// This device's ceremony key pair (created on first use, kept in secure
/// storage); round-two DKG packages are sealed to its public half.
pub(crate) async fn local_ceremony_keys(
    effects: &AuraEffectSystem,
) -> Result<LocalDeviceKeys, AuraError> {
    let location = device_ceremony_key_location(effects.device_id());
    let caps = [
        SecureStorageCapability::Read,
        SecureStorageCapability::Write,
    ];
    let private = if effects.secure_exists(&location).await? {
        effects.secure_retrieve(&location, &caps).await?
    } else {
        let (private, _) = effects.ed25519_generate_keypair().await?;
        effects.secure_store(&location, &private, &caps).await?;
        private
    };
    let public_key = effects.ed25519_public_key(&private).await?;
    let secret = effects.convert_ed25519_to_x25519_private(&private).await?;
    Ok(LocalDeviceKeys {
        public_key,
        key_agreement_secret: aura_core::secrets::PrivateKeyBytes::import_from_slice(&secret),
    })
}

/// This device's own directory entry (what peers must hold as verified).
pub(crate) async fn own_device_key(
    effects: &AuraEffectSystem,
) -> Result<VerifiedDeviceKey, AuraError> {
    Ok(VerifiedDeviceKey {
        authority: aura_guards::GuardContextProvider::authority_id(effects),
        device: effects.device_id(),
        public_key: local_ceremony_keys(effects).await?.public_key,
    })
}

/// The verified device key held for `authority`.
pub(crate) async fn verified_device_key(
    effects: &AuraEffectSystem,
    authority: AuthorityId,
) -> Result<Option<VerifiedDeviceKey>, AuraError> {
    if authority == aura_guards::GuardContextProvider::authority_id(effects) {
        return own_device_key(effects).await.map(Some);
    }
    let Some(bytes) = effects.retrieve(&directory_key(authority)).await? else {
        return Ok(None);
    };
    let entry: VerifiedDeviceKey = aura_core::util::serialization::from_slice(&bytes)
        .map_err(|error| AuraError::serialization(error.to_string()))?;
    Ok((entry.authority == authority).then_some(entry))
}

/// Record a device key as verified for its authority. Only the verified
/// device-key exchange may call this (Task 164 step 3).
pub(crate) async fn record_verified_device_key(
    effects: &AuraEffectSystem,
    entry: &VerifiedDeviceKey,
) -> Result<(), AuraError> {
    if entry.public_key.len() != 32 {
        return Err(AuraError::invalid("device key must be 32 bytes"));
    }
    let bytes = aura_core::util::serialization::to_vec(entry)
        .map_err(|error| AuraError::serialization(error.to_string()))?;
    effects
        .store(&directory_key(entry.authority), bytes)
        .await
        .map_err(|error| AuraError::storage(error.to_string()))
}

// ---------------------------------------------------------------------------
// Ceremony
// ---------------------------------------------------------------------------

async fn ceremony_peers(
    effects: &AuraEffectSystem,
    invite: &ChannelKeyInvite,
    max_polls: u32,
) -> Result<Vec<ContextDkgPeer>, AuraError> {
    // Fetch any member's device key not yet held, over the authenticated
    // transport, from members with standing in this channel.
    super::device_key_exchange::ensure_device_keys(
        effects,
        &invite.participants,
        Some(invite.scope),
        |peer, scope| super::device_key_exchange::runtime_known_peer(effects, peer, scope),
        max_polls,
    )
    .await?;
    let mut peers = Vec::with_capacity(invite.participants.len());
    for authority in &invite.participants {
        let entry = verified_device_key(effects, *authority)
            .await?
            .ok_or_else(|| {
                AuraError::permission_denied(format!(
                    "no verified device key for channel key participant {authority}"
                ))
            })?;
        peers.push(ContextDkgPeer {
            authority: entry.authority,
            device: entry.device,
            device_public_key: entry.public_key,
        });
    }
    Ok(peers)
}

/// Run this device's side of the ceremony named by `invite`: the DKG, then
/// the epoch's channel base key, which every participant ends up holding.
pub(crate) async fn run_channel_key_ceremony(
    effects: &AuraEffectSystem,
    invite: &ChannelKeyInvite,
    max_polls: u32,
) -> Result<[u8; 32], AuraError> {
    invite.validate()?;
    let peers = ceremony_peers(effects, invite, max_polls).await?;
    let local = local_ceremony_keys(effects).await?;
    let config = invite.dkg_config()?;
    let output = run_context_dkg(
        effects,
        invite.scope,
        config.clone(),
        &peers,
        &local,
        max_polls,
    )
    .await?;
    store_context_dkg_output(effects, invite.scope, &config, &output).await?;
    derive_channel_base_key(
        effects,
        invite.scope,
        invite.epoch,
        invite.epoch,
        &peers,
        max_polls,
    )
    .await
}

/// Coordinate the ceremony: invite every other participant, then run it.
pub(crate) async fn coordinate_channel_key_ceremony(
    effects: &AuraEffectSystem,
    invite: &ChannelKeyInvite,
    max_polls: u32,
) -> Result<[u8; 32], AuraError> {
    invite.validate()?;
    let me = aura_guards::GuardContextProvider::authority_id(effects);
    if invite.coordinator != me {
        return Err(AuraError::permission_denied(
            "only the coordinator sends channel key invites",
        ));
    }
    let peers = ceremony_peers(effects, invite, max_polls).await?;
    let bytes = aura_core::util::serialization::to_vec(invite)
        .map_err(|error| AuraError::serialization(error.to_string()))?;
    for peer in peers.iter().filter(|peer| peer.authority != me) {
        effects
            .send_device_payload(
                peer.device.uuid(),
                CHANNEL_KEY_INVITE_CONTENT_TYPE,
                bytes.clone(),
            )
            .await
            .map_err(|error| AuraError::network(error.to_string()))?;
    }
    run_channel_key_ceremony(effects, invite, max_polls).await
}

fn is_invite_envelope(envelope: &TransportEnvelope) -> bool {
    envelope.metadata.get("content-type").map(String::as_str)
        == Some(CHANNEL_KEY_INVITE_CONTENT_TYPE)
}

/// Admit one received invite: authenticated by its transport source (the
/// coordinator), naming this authority, from a coordinator `has_standing`
/// accepts for the channel.
fn admit_invite(
    me: AuthorityId,
    envelope: &TransportEnvelope,
) -> Result<ChannelKeyInvite, AuraError> {
    if envelope.payload.len() > MAX_INVITE_BYTES {
        return Err(AuraError::invalid("oversized channel key invite"));
    }
    let invite: ChannelKeyInvite = aura_core::util::serialization::from_slice(&envelope.payload)
        .map_err(|error| AuraError::serialization(error.to_string()))?;
    invite.validate()?;
    if invite.coordinator != envelope.source || !invite.participants.contains(&me) {
        return Err(AuraError::permission_denied(
            "channel key invite is not from its coordinator or does not name this authority",
        ));
    }
    Ok(invite)
}

/// Run the ceremonies other members invited this device to. `has_standing`
/// decides whether the coordinator may change this channel's keys.
pub(crate) async fn process_channel_key_invites<F, Fut>(
    effects: &AuraEffectSystem,
    has_standing: F,
    max_polls: u32,
) -> Result<Vec<(ChannelKeyInvite, Result<[u8; 32], AuraError>)>, AuraError>
where
    F: Fn(ChannelKeyInvite) -> Fut,
    Fut: std::future::Future<Output = Result<bool, AuraError>>,
{
    let me = aura_guards::GuardContextProvider::authority_id(effects);
    let now_ms = effects
        .physical_time()
        .await
        .map_err(|error| AuraError::internal(format!("time error: {error}")))?
        .ts_ms;
    let mut outcomes = Vec::new();
    let mut deferred = Vec::new();
    let mut ready: Vec<(u64, ChannelKeyInvite)> = Vec::new();
    while let Ok(envelope) = effects.take_inbound_envelope(is_invite_envelope) {
        let invite = match admit_invite(me, &envelope) {
            Ok(invite) => invite,
            Err(error) => {
                tracing::warn!(source = %envelope.source, %error, "channel key invite refused");
                continue;
            }
        };
        if !has_standing(invite.clone()).await? {
            // The coordinator's own join may not have synced here yet (a
            // late joiner sees the invite before the roster facts). Hold the
            // invite until standing is observed, for at most the
            // coordinator's attempt window, rather than refusing the only
            // invite (Task 196).
            match hold_for_standing(envelope, now_ms) {
                Some(held) => {
                    tracing::debug!(
                        coordinator = %invite.coordinator,
                        "channel key invite waits for the coordinator's standing"
                    );
                    deferred.push(held);
                }
                None => tracing::warn!(
                    coordinator = %invite.coordinator,
                    "channel key invite from a coordinator without standing refused"
                ),
            }
            continue;
        }
        ready.push((held_since_ms(&envelope).unwrap_or(now_ms), invite));
    }
    // Requeue after draining, so one round never takes a deferred invite twice.
    for envelope in deferred {
        effects.requeue_envelope(envelope);
    }
    // A coordinator re-invites after an attempt that timed out while its
    // standing was unobserved here; of the invites for one channel epoch,
    // run only the most recently received. An older one names a ceremony its
    // coordinator already abandoned.
    ready.sort_by_key(|(held_since, _)| std::cmp::Reverse(*held_since));
    let mut started = std::collections::BTreeSet::new();
    for (_, invite) in ready {
        if !started.insert((invite.scope.context, invite.scope.channel, invite.epoch)) {
            tracing::debug!(
                coordinator = %invite.coordinator,
                epoch = invite.epoch,
                "superseded channel key invite dropped"
            );
            continue;
        }
        let outcome = run_channel_key_ceremony(effects, &invite, max_polls).await;
        outcomes.push((invite, outcome));
    }
    Ok(outcomes)
}

/// Local metadata recording when this member first held an invite while it
/// waited for the coordinator's standing.
const HELD_SINCE_METADATA: &str = "aura-channel-key-held-since-ms";

/// How long an invite may wait for its coordinator's standing before it is
/// refused: the coordinator's attempt window (`CEREMONY_MAX_POLLS` receive
/// polls of 50 ms).
pub(crate) const STANDING_HOLD_MS: u64 = CEREMONY_MAX_POLLS as u64 * 50;

/// Hold `envelope` (from a peer whose standing this member has not yet
/// observed) for a later round, or `None` once it has waited out the
/// coordinator's attempt window and is refused.
pub(crate) fn hold_for_standing(
    mut envelope: TransportEnvelope,
    now_ms: u64,
) -> Option<TransportEnvelope> {
    let held_since = held_since_ms(&envelope).unwrap_or(now_ms);
    if now_ms.saturating_sub(held_since) >= STANDING_HOLD_MS {
        return None;
    }
    envelope
        .metadata
        .insert(HELD_SINCE_METADATA.to_string(), held_since.to_string());
    Some(envelope)
}

fn held_since_ms(envelope: &TransportEnvelope) -> Option<u64> {
    envelope
        .metadata
        .get(HELD_SINCE_METADATA)
        .and_then(|ms| ms.parse().ok())
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::core::AgentConfig;
    use aura_core::effects::SecureStorageEffects;
    use aura_core::types::identifiers::{ChannelId, ContextId};

    fn runtime(shared: &crate::SharedTransport, seed: u8) -> AuraEffectSystem {
        let config = AgentConfig {
            device_id: DeviceId::new_from_entropy([seed.wrapping_add(100); 32]),
            ..AgentConfig::default()
        };
        AuraEffectSystem::simulation_for_named_test_with_shared_transport_for_authority(
            &config,
            &format!("channel-key-ceremony-{seed}"),
            AuthorityId::new_from_entropy([seed; 32]),
            shared.clone(),
        )
        .expect("effects")
    }

    /// Every member records every other member's device key as verified
    /// (stands in for the step-3 exchange).
    async fn exchange_device_keys(members: &[&AuraEffectSystem]) {
        for owner in members {
            let entry = own_device_key(owner).await.expect("own key");
            for other in members {
                if aura_guards::GuardContextProvider::authority_id(*other) != entry.authority {
                    record_verified_device_key(other, &entry)
                        .await
                        .expect("record");
                }
            }
        }
    }

    fn scope() -> ChannelKeyScope {
        ChannelKeyScope {
            context: ContextId::new_from_entropy([91; 32]),
            channel: ChannelId::from_bytes([92; 32]),
        }
    }

    /// Task 164 step 1: a coordinator invites the channel's members over
    /// transport; each admits the invite (coordinator with standing, itself
    /// on the roster), runs the DKG with round-two packages sealed to the
    /// verified device keys, and all end with the same epoch base key. A
    /// member left off the roster holds nothing for that epoch, and an
    /// invite from a coordinator without standing is refused.
    #[tokio::test(start_paused = true)]
    async fn members_run_the_channel_key_ceremony_over_transport() {
        let shared = crate::SharedTransport::new();
        let a = runtime(&shared, 81);
        let b = runtime(&shared, 82);
        let c = runtime(&shared, 83);
        let left_out = runtime(&shared, 84);
        exchange_device_keys(&[&a, &b, &c, &left_out]).await;
        let id =
            |effects: &AuraEffectSystem| aura_guards::GuardContextProvider::authority_id(effects);
        let invite =
            ChannelKeyInvite::new(scope(), 1, id(&a), [id(&a), id(&b), id(&c)]).expect("invite");
        let coordinator = id(&a);
        let standing =
            move |invite: ChannelKeyInvite| async move { Ok(invite.coordinator == coordinator) };
        let (a_key, b_outcomes, c_outcomes) = futures::join!(
            coordinate_channel_key_ceremony(&a, &invite, CEREMONY_MAX_POLLS),
            async {
                loop {
                    let outcomes = process_channel_key_invites(&b, standing, CEREMONY_MAX_POLLS)
                        .await
                        .expect("process");
                    if !outcomes.is_empty() {
                        return outcomes;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
            },
            async {
                loop {
                    let outcomes = process_channel_key_invites(&c, standing, CEREMONY_MAX_POLLS)
                        .await
                        .expect("process");
                    if !outcomes.is_empty() {
                        return outcomes;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
            },
        );
        let a_key = a_key.expect("coordinator derives the epoch key");
        let b_key = b_outcomes[0].1.as_ref().expect("b derives").to_owned();
        let c_key = c_outcomes[0].1.as_ref().expect("c derives").to_owned();
        assert_eq!(a_key, b_key);
        assert_eq!(a_key, c_key);
        let location =
            SecureStorageLocation::amp_channel_base_key(&scope().context, &scope().channel, 1);
        assert!(
            left_out
                .secure_retrieve(&location, &[SecureStorageCapability::Read])
                .await
                .is_err(),
            "a member off the roster holds no key for the epoch"
        );

        // An invite from a coordinator the member does not grant standing.
        let rogue = ChannelKeyInvite::new(scope(), 2, id(&left_out), [id(&left_out), id(&b)])
            .expect("invite");
        let bytes = aura_core::util::serialization::to_vec(&rogue).unwrap();
        left_out
            .send_device_payload(b.device_id().uuid(), CHANNEL_KEY_INVITE_CONTENT_TYPE, bytes)
            .await
            .expect("send");
        let outcomes = process_channel_key_invites(&b, standing, 4)
            .await
            .expect("process");
        assert!(
            outcomes.is_empty(),
            "no ceremony runs for a coordinator without standing"
        );
    }

    /// Task 196 (LAN run 174): a late joiner receives the coordinator's
    /// invite before the coordinator's own join has synced to it. The
    /// joiner holds the invite instead of refusing it, and runs the
    /// ceremony once standing is observed; a coordinator that never gains
    /// standing is refused after the bounded wait.
    #[tokio::test(start_paused = true)]
    async fn late_joiner_waits_for_the_coordinators_standing() {
        let shared = crate::SharedTransport::new();
        let a = runtime(&shared, 85);
        let b = runtime(&shared, 86);
        exchange_device_keys(&[&a, &b]).await;
        let id =
            |effects: &AuraEffectSystem| aura_guards::GuardContextProvider::authority_id(effects);
        let invite = ChannelKeyInvite::new(scope(), 1, id(&a), [id(&a), id(&b)]).expect("invite");
        // B observes the coordinator's standing only after a few rounds.
        let observed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let standing = {
            let observed = observed.clone();
            move |_invite: ChannelKeyInvite| {
                let observed = observed.clone();
                async move { Ok(observed.load(std::sync::atomic::Ordering::SeqCst)) }
            }
        };
        let (a_key, b_outcomes) = futures::join!(
            coordinate_channel_key_ceremony(&a, &invite, CEREMONY_MAX_POLLS),
            async {
                for round in 0u32.. {
                    if round == 5 {
                        observed.store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                    let outcomes = process_channel_key_invites(&b, &standing, CEREMONY_MAX_POLLS)
                        .await
                        .expect("process");
                    if !outcomes.is_empty() {
                        assert!(round >= 5, "no ceremony before standing is observed");
                        return outcomes;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
                unreachable!()
            },
        );
        let a_key = a_key.expect("coordinator derives the epoch key");
        assert_eq!(&a_key, b_outcomes[0].1.as_ref().expect("b derives"));

        // A coordinator that never gains standing: the invite is held across
        // rounds, then refused once held for the coordinator's attempt window.
        let never = ChannelKeyInvite::new(scope(), 2, id(&a), [id(&a), id(&b)]).expect("invite");
        a.send_device_payload(
            b.device_id().uuid(),
            CHANNEL_KEY_INVITE_CONTENT_TYPE,
            aura_core::util::serialization::to_vec(&never).unwrap(),
        )
        .await
        .expect("send");
        let refuse = |_invite: ChannelKeyInvite| async { Ok(false) };
        for _ in 0..3 {
            assert!(process_channel_key_invites(&b, refuse, 4)
                .await
                .expect("process")
                .is_empty());
        }
        let mut held = b
            .take_inbound_envelope(is_invite_envelope)
            .expect("the invite is still held");
        let since = held_since_ms(&held).expect("held invites record when");
        held.metadata.insert(
            HELD_SINCE_METADATA.to_string(),
            since.saturating_sub(STANDING_HOLD_MS).to_string(),
        );
        b.requeue_envelope(held);
        assert!(process_channel_key_invites(&b, refuse, 4)
            .await
            .expect("process")
            .is_empty());
        assert!(
            b.take_inbound_envelope(is_invite_envelope).is_err(),
            "the invite is refused after the bounded wait"
        );
    }
}
