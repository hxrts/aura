//! Verified device-key exchange between accounts (work/8.md Task 164 step 3).
//!
//! A channel key ceremony seals each member's DKG round-two package to that
//! member's device key, so every member must hold the other members' device
//! keys as verified. A device announces its ceremony key only over the
//! authenticated transport its account established with the peer (the
//! contact-exchanged authority keys): the receiver admits an announcement
//! only when the transport-authenticated source authority is the announced
//! authority and that authority is a peer it knows (a contact, or a member of
//! the channel the key is wanted for). Nothing relayed by a third party is
//! admitted, and a peer-supplied device id never selects another account's
//! key.

use super::channel_key_ceremony::{
    own_device_key, record_verified_device_key, verified_device_key,
};
use super::context_dkg::ChannelKeyScope;
use super::AuraEffectSystem;
use aura_core::effects::transport::TransportEnvelope;
use aura_core::effects::PhysicalTimeEffects;
use aura_core::types::identifiers::{AuthorityId, DeviceId};
use aura_core::AuraError;

/// Content type of device-key exchange messages.
pub(crate) const DEVICE_KEY_CONTENT_TYPE: &str = "application/aura-device-key";
const MAX_MESSAGE_BYTES: usize = 4_096;
const POLL_MS: u64 = 50;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
enum DeviceKeyMessage {
    /// Ask the receiver for its device key, for a ceremony of `scope`.
    Request { scope: Option<ChannelKeyScope> },
    /// The sender's device key.
    Announce {
        authority: AuthorityId,
        device: DeviceId,
        public_key: Vec<u8>,
    },
}

fn decode(envelope: &TransportEnvelope) -> Option<DeviceKeyMessage> {
    if envelope.metadata.get("content-type").map(String::as_str) != Some(DEVICE_KEY_CONTENT_TYPE)
        || envelope.payload.len() > MAX_MESSAGE_BYTES
    {
        return None;
    }
    aura_core::util::serialization::from_slice(&envelope.payload).ok()
}

async fn send(
    effects: &AuraEffectSystem,
    to: AuthorityId,
    message: &DeviceKeyMessage,
) -> Result<(), AuraError> {
    let bytes = aura_core::util::serialization::to_vec(message)
        .map_err(|error| AuraError::serialization(error.to_string()))?;
    // An unresolved uuid addresses the peer authority itself.
    effects
        .send_device_payload(to.uuid(), DEVICE_KEY_CONTENT_TYPE, bytes)
        .await
        .map_err(|error| AuraError::network(error.to_string()))
}

fn request_key(authority: AuthorityId) -> String {
    format!("device_key_request/{authority}")
}

/// Remember that this device asked `authority` for its key for `scope`.
async fn record_request(
    effects: &AuraEffectSystem,
    authority: AuthorityId,
    scope: Option<ChannelKeyScope>,
) -> Result<(), AuraError> {
    use aura_core::effects::StorageCoreEffects;
    let bytes = aura_core::util::serialization::to_vec(&scope)
        .map_err(|error| AuraError::serialization(error.to_string()))?;
    effects
        .store(&request_key(authority), bytes)
        .await
        .map_err(|error| AuraError::storage(error.to_string()))
}

/// The scope this device asked `authority` about, if it asked.
async fn requested_scope(
    effects: &AuraEffectSystem,
    authority: AuthorityId,
) -> Result<Option<ChannelKeyScope>, AuraError> {
    use aura_core::effects::StorageCoreEffects;
    let Some(bytes) = effects
        .retrieve(&request_key(authority))
        .await
        .map_err(|error| AuraError::storage(error.to_string()))?
    else {
        return Ok(None);
    };
    aura_core::util::serialization::from_slice::<Option<ChannelKeyScope>>(&bytes)
        .map_err(|error| AuraError::serialization(error.to_string()))
}

/// The runtime's peer policy: a peer with standing in the ceremony's
/// channel, or a contact.
pub(crate) async fn runtime_known_peer(
    effects: &AuraEffectSystem,
    peer: AuthorityId,
    scope: Option<ChannelKeyScope>,
) -> Result<bool, AuraError> {
    use aura_core::effects::reactive::ReactiveEffects;
    if let Some(scope) = scope {
        if aura_amp::channel_membership_observations(effects, scope.context, scope.channel)
            .await?
            .has_standing(peer)
        {
            return Ok(true);
        }
    }
    // Without a signal graph (bare runtimes) there are no contacts.
    Ok(effects
        .reactive_handler()
        .read(&*aura_app::signal_defs::CONTACTS_SIGNAL)
        .await
        .is_ok_and(|contacts| contacts.has_contact(&peer)))
}

/// Send this device's key to `peer`.
pub(crate) async fn announce_device_key(
    effects: &AuraEffectSystem,
    peer: AuthorityId,
) -> Result<(), AuraError> {
    let own = own_device_key(effects).await?;
    send(
        effects,
        peer,
        &DeviceKeyMessage::Announce {
            authority: own.authority,
            device: own.device,
            public_key: own.public_key,
        },
    )
    .await
}

/// Answer key requests and admit announcements. `is_known_peer` decides
/// whether an authenticated source authority may exchange keys with this
/// device (for a ceremony of the given scope, if any).
pub(crate) async fn process_device_key_messages<F, Fut>(
    effects: &AuraEffectSystem,
    is_known_peer: F,
) -> Result<usize, AuraError>
where
    F: Fn(AuthorityId, Option<ChannelKeyScope>) -> Fut,
    Fut: std::future::Future<Output = Result<bool, AuraError>>,
{
    let mut processed = 0;
    while let Ok(envelope) = effects.take_inbound_envelope(|envelope| decode(envelope).is_some()) {
        processed += 1;
        let source = envelope.source;
        match decode(&envelope) {
            Some(DeviceKeyMessage::Request { scope }) => {
                if is_known_peer(source, scope).await? {
                    announce_device_key(effects, source).await?;
                } else {
                    tracing::warn!(%source, "device key request from an unknown peer refused");
                }
            }
            Some(DeviceKeyMessage::Announce {
                authority,
                device,
                public_key,
            }) => {
                // Only the authenticated authority itself may announce its
                // key, and only a peer known for the scope we asked about
                // (or a known peer, unasked).
                let scope = requested_scope(effects, source).await?;
                if authority != source || !is_known_peer(source, scope).await? {
                    tracing::warn!(%source, %authority, "device key announcement refused");
                    continue;
                }
                record_verified_device_key(
                    effects,
                    &super::channel_key_ceremony::VerifiedDeviceKey {
                        authority,
                        device,
                        public_key,
                    },
                )
                .await?;
            }
            None => {}
        }
    }
    Ok(processed)
}

/// Make sure a verified device key is held for every one of `authorities`,
/// requesting the missing ones and waiting up to `max_polls` polls.
pub(crate) async fn ensure_device_keys<F, Fut>(
    effects: &AuraEffectSystem,
    authorities: &[AuthorityId],
    scope: Option<ChannelKeyScope>,
    is_known_peer: F,
    max_polls: u32,
) -> Result<(), AuraError>
where
    F: Fn(AuthorityId, Option<ChannelKeyScope>) -> Fut + Clone,
    Fut: std::future::Future<Output = Result<bool, AuraError>>,
{
    let mut missing = Vec::new();
    for authority in authorities {
        if verified_device_key(effects, *authority).await?.is_none() {
            missing.push(*authority);
        }
    }
    for authority in &missing {
        record_request(effects, *authority, scope).await?;
        send(effects, *authority, &DeviceKeyMessage::Request { scope }).await?;
    }
    for _ in 0..max_polls {
        process_device_key_messages(effects, is_known_peer.clone()).await?;
        let mut still_missing = false;
        for authority in &missing {
            if verified_device_key(effects, *authority).await?.is_none() {
                still_missing = true;
                break;
            }
        }
        if !still_missing {
            return Ok(());
        }
        effects
            .sleep_ms(POLL_MS)
            .await
            .map_err(|error| AuraError::internal(error.to_string()))?;
    }
    Err(AuraError::internal(
        "peer device keys did not arrive before the deadline",
    ))
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::core::AgentConfig;

    fn runtime(shared: &crate::SharedTransport, seed: u8) -> AuraEffectSystem {
        let config = AgentConfig {
            device_id: DeviceId::new_from_entropy([seed.wrapping_add(100); 32]),
            ..AgentConfig::default()
        };
        AuraEffectSystem::simulation_for_named_test_with_shared_transport_for_authority(
            &config,
            &format!("device-key-exchange-{seed}"),
            AuthorityId::new_from_entropy([seed; 32]),
            shared.clone(),
        )
        .expect("effects")
    }

    fn id(effects: &AuraEffectSystem) -> AuthorityId {
        aura_guards::GuardContextProvider::authority_id(effects)
    }

    /// Task 164 step 3: a member fetches a known peer's device key over the
    /// authenticated transport and holds exactly the key the peer's device
    /// uses; an announcement claiming another account, or one from an
    /// unknown peer, is not admitted.
    #[tokio::test(start_paused = true)]
    async fn known_peers_exchange_device_keys_and_forgeries_are_refused() {
        let shared = crate::SharedTransport::new();
        let a = runtime(&shared, 111);
        let b = runtime(&shared, 112);
        let stranger = runtime(&shared, 113);
        let (a_id, b_id, stranger_id) = (id(&a), id(&b), id(&stranger));
        let known = move |peer: AuthorityId, _scope: Option<ChannelKeyScope>| async move {
            Ok(peer == a_id || peer == b_id)
        };
        let wanted = [b_id];
        let (fetched, ()) =
            futures::join!(ensure_device_keys(&a, &wanted, None, known, 200), async {
                for _ in 0..200 {
                    process_device_key_messages(&b, known).await.unwrap();
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            });
        fetched.expect("a holds b's key");
        let held = verified_device_key(&a, b_id).await.unwrap().unwrap();
        assert_eq!(
            held,
            own_device_key(&b).await.unwrap(),
            "exactly b's own key"
        );

        // The stranger announces a key in b's name, and one in its own name.
        let forged = DeviceKeyMessage::Announce {
            authority: b_id,
            device: stranger.device_id(),
            public_key: vec![7; 32],
        };
        send(&stranger, a_id, &forged).await.unwrap();
        announce_device_key(&stranger, a_id).await.unwrap();
        for _ in 0..20 {
            process_device_key_messages(&a, known).await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(
            verified_device_key(&a, b_id).await.unwrap().unwrap(),
            held,
            "a relayed key in another account's name is not admitted"
        );
        assert!(
            verified_device_key(&a, stranger_id)
                .await
                .unwrap()
                .is_none(),
            "an unknown peer's key is not admitted"
        );
    }

    /// Task 164 step 3 with step 1: channel members who hold no device keys
    /// of each other run the channel key ceremony; each fetches the others'
    /// keys from members with standing in the channel, and all derive the
    /// same epoch key.
    #[tokio::test(start_paused = true)]
    async fn ceremony_fetches_member_device_keys_by_channel_standing() {
        use crate::runtime::channel_key_ceremony::{
            coordinate_channel_key_ceremony, process_channel_key_invites, ChannelKeyInvite,
            CEREMONY_MAX_POLLS,
        };
        use aura_core::types::identifiers::{ChannelId, ContextId};
        use aura_journal::DomainFact;
        use aura_protocol::amp::AmpJournalEffects;
        let shared = crate::SharedTransport::new();
        let members: &'static [AuraEffectSystem; 3] = Box::leak(Box::new([
            runtime(&shared, 121),
            runtime(&shared, 122),
            runtime(&shared, 123),
        ]));
        let scope = ChannelKeyScope {
            context: ContextId::new_from_entropy([124; 32]),
            channel: ChannelId::from_bytes([125; 32]),
        };
        // Every member observes every member's own join.
        for observer in members {
            for member in members {
                let join = aura_amp::ChannelMembershipFact::new(
                    scope.context,
                    scope.channel,
                    id(member),
                    aura_amp::ChannelParticipantEvent::Joined,
                    aura_core::time::TimeStamp::OrderClock(aura_core::time::OrderTime(
                        [member.device_id().uuid().as_bytes()[0]; 32],
                    )),
                );
                observer
                    .insert_relational_fact(join.to_generic())
                    .await
                    .unwrap();
            }
        }
        let invite =
            ChannelKeyInvite::new(scope, 1, id(&members[0]), members.iter().map(id)).unwrap();
        let participate = |effects: &'static AuraEffectSystem| async move {
            loop {
                process_device_key_messages(effects, |peer, scope| {
                    runtime_known_peer(effects, peer, scope)
                })
                .await
                .unwrap();
                let outcomes = process_channel_key_invites(
                    effects,
                    |invite: ChannelKeyInvite| async move {
                        Ok(aura_amp::channel_membership_observations(
                            effects,
                            invite.scope.context,
                            invite.scope.channel,
                        )
                        .await?
                        .has_standing(invite.coordinator))
                    },
                    CEREMONY_MAX_POLLS,
                )
                .await
                .unwrap();
                if let Some((_, outcome)) = outcomes.into_iter().next() {
                    return outcome.expect("member derives the epoch key");
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        };
        let (coordinator_key, b_key, c_key) = futures::join!(
            async {
                // The coordinator also answers key requests while it waits.
                coordinate_channel_key_ceremony(&members[0], &invite, CEREMONY_MAX_POLLS).await
            },
            participate(&members[1]),
            participate(&members[2]),
        );
        let coordinator_key = coordinator_key.expect("coordinator derives the epoch key");
        assert_eq!(coordinator_key, b_key);
        assert_eq!(coordinator_key, c_key);
    }
}
