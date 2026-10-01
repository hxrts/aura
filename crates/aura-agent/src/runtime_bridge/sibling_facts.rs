//! Replicate committed facts between devices of this authority.
//!
//! Devices of one authority each keep the authority's committed fact store.
//! A newly enrolled device starts empty, and facts committed on one device
//! must reach the others. Anti-entropy compares operation logs only, so this
//! symmetric lockstep exchange carries the facts themselves:
//! digest, then sealed AMP keys, journal facts, key index and the facts the
//! peer lacks, in that order so a message is openable when its fact lands.

use std::collections::{BTreeMap, BTreeSet};

use aura_core::hash::hash;
use aura_core::{AuraError, DeviceId};
use aura_journal::fact::Fact as TypedFact;
use serde::{Deserialize, Serialize};

use crate::runtime::AuraEffectSystem;

/// Wire content type for sibling fact frames, kept apart from sync frames.
pub(crate) const SIBLING_FACTS_CONTENT_TYPE: &str = "application/aura-sibling-facts";

#[derive(Serialize, Deserialize)]
enum SiblingFactsFrame {
    Digest([u8; 32]),
    Index(Vec<Vec<u8>>),
    Facts(Vec<TypedFact>),
    JournalFacts(aura_core::Fact),
    AmpKeys(Option<aura_sync::protocols::device_sealed::DeviceSealedPayload>),
}

fn fact_key(fact: &TypedFact) -> Result<Vec<u8>, AuraError> {
    aura_core::util::serialization::to_vec(&fact.order)
        .map_err(|error| AuraError::internal(format!("encode fact order: {error}")))
}

async fn send_frame(
    effects: &AuraEffectSystem,
    peer: DeviceId,
    frame: &SiblingFactsFrame,
) -> Result<(), AuraError> {
    let bytes = aura_core::util::serialization::to_vec(frame)
        .map_err(|error| AuraError::internal(format!("encode sibling frame: {error}")))?;
    effects
        .send_device_payload(peer.uuid(), SIBLING_FACTS_CONTENT_TYPE, bytes)
        .await
        .map_err(|error| AuraError::network(format!("send sibling facts: {error}")))
}

async fn receive_frame(
    effects: &AuraEffectSystem,
    peer: DeviceId,
) -> Result<SiblingFactsFrame, AuraError> {
    let bytes = effects
        .receive_device_payload(peer.uuid(), SIBLING_FACTS_CONTENT_TYPE)
        .await
        .map_err(|error| AuraError::network(format!("receive sibling facts: {error}")))?;
    aura_core::util::serialization::from_slice(&bytes)
        .map_err(|error| AuraError::internal(format!("decode sibling frame: {error}")))
}

/// Upper bound on stale frames dropped while waiting for one step.
const MAX_STALE_FRAMES: usize = 8;

/// Receive the next frame of the expected step. Each device starts exchanges on
/// its own schedule, so frames left over from an exchange the other side
/// abandoned are dropped rather than read out of step.
async fn receive_expected(
    effects: &AuraEffectSystem,
    peer: DeviceId,
    is_expected: impl Fn(&SiblingFactsFrame) -> bool,
    step: &str,
) -> Result<SiblingFactsFrame, AuraError> {
    for _ in 0..MAX_STALE_FRAMES {
        let frame = receive_frame(effects, peer).await?;
        if is_expected(&frame) {
            return Ok(frame);
        }
        tracing::debug!(sibling = %peer, step, "dropping stale sibling fact frame");
    }
    Err(protocol_error(step))
}

fn protocol_error(expected: &str) -> AuraError {
    AuraError::internal(format!(
        "sibling fact exchange out of step: expected {expected}"
    ))
}

/// Exchange committed facts with another device of this authority.
///
/// Both devices run this concurrently (each syncs with its siblings), and both
/// take the same branch because they compare the same pair of digests.
/// Returns how many facts were newly imported here.
pub(crate) async fn exchange_facts_with_sibling(
    effects: &AuraEffectSystem,
    peer: DeviceId,
    key_agreement_secret: Option<[u8; 32]>,
) -> Result<usize, AuraError> {
    let authority = aura_guards::GuardContextProvider::authority_id(effects);
    let local = effects.load_committed_facts(authority).await?;
    let mut by_key = BTreeMap::new();
    for fact in local {
        by_key.insert(fact_key(&fact)?, fact);
    }
    // AMP channel state lives in the authority journal's facts, not the typed store.
    let mut journal = aura_core::effects::JournalEffects::get_journal(effects).await?;
    let journal_bytes = aura_core::util::serialization::to_vec(&journal.facts)
        .map_err(|error| AuraError::internal(format!("encode journal facts: {error}")))?;
    // Held bootstrap keys are part of the digest, so a sibling missing a key
    // still runs the full exchange after the channel facts have converged.
    let held_keys = held_bootstrap_keys(effects).await?;
    let held_ids = aura_core::util::serialization::to_vec(
        &held_keys
            .iter()
            .map(|key| (key.context, key.channel, key.bootstrap_id))
            .collect::<Vec<_>>(),
    )
    .map_err(|error| AuraError::internal(format!("encode held key ids: {error}")))?;
    let mut digest_input: Vec<u8> = by_key.keys().flatten().copied().collect();
    digest_input.extend_from_slice(&hash(&journal_bytes));
    digest_input.extend_from_slice(&hash(&held_ids));
    let digest = hash(&digest_input);

    send_frame(effects, peer, &SiblingFactsFrame::Digest(digest)).await?;
    let SiblingFactsFrame::Digest(peer_digest) = receive_expected(
        effects,
        peer,
        |frame| matches!(frame, SiblingFactsFrame::Digest(_)),
        "digest",
    )
    .await?
    else {
        return Err(protocol_error("digest"));
    };
    if peer_digest == digest {
        return Ok(0);
    }

    // AMP bootstrap keys never enter the journal (docs/112_amp.md §1.2.1); they
    // travel sealed to the sibling device's leaf key. They go first so the
    // chat view can open messages as soon as their facts arrive.
    let sealed = seal_bootstrap_keys(effects, authority, peer, &held_keys).await?;
    send_frame(effects, peer, &SiblingFactsFrame::AmpKeys(sealed)).await?;
    let SiblingFactsFrame::AmpKeys(peer_sealed) = receive_expected(
        effects,
        peer,
        |frame| matches!(frame, SiblingFactsFrame::AmpKeys(_)),
        "amp_keys",
    )
    .await?
    else {
        return Err(protocol_error("amp keys"));
    };
    if let (Some(peer_sealed), Some(secret)) = (peer_sealed, key_agreement_secret) {
        store_bootstrap_keys(effects, authority, &peer_sealed, &secret).await?;
    }

    send_frame(
        effects,
        peer,
        &SiblingFactsFrame::JournalFacts(journal.facts.clone()),
    )
    .await?;
    let SiblingFactsFrame::JournalFacts(peer_facts) = receive_expected(
        effects,
        peer,
        |frame| matches!(frame, SiblingFactsFrame::JournalFacts(_)),
        "journal_facts",
    )
    .await?
    else {
        return Err(protocol_error("journal facts"));
    };
    let before = journal.facts.clone();
    journal.merge_facts(peer_facts);
    if journal.facts != before {
        aura_core::effects::JournalEffects::persist_journal(effects, &journal).await?;
    }

    send_frame(
        effects,
        peer,
        &SiblingFactsFrame::Index(by_key.keys().cloned().collect()),
    )
    .await?;
    let SiblingFactsFrame::Index(peer_keys) = receive_expected(
        effects,
        peer,
        |frame| matches!(frame, SiblingFactsFrame::Index(_)),
        "index",
    )
    .await?
    else {
        return Err(protocol_error("index"));
    };
    let peer_keys: BTreeSet<Vec<u8>> = peer_keys.into_iter().collect();
    let missing: Vec<TypedFact> = by_key
        .into_iter()
        .filter(|(key, _)| !peer_keys.contains(key))
        .map(|(_, fact)| fact)
        .collect();

    send_frame(effects, peer, &SiblingFactsFrame::Facts(missing)).await?;
    let SiblingFactsFrame::Facts(received) = receive_expected(
        effects,
        peer,
        |frame| matches!(frame, SiblingFactsFrame::Facts(_)),
        "facts",
    )
    .await?
    else {
        return Err(protocol_error("facts"));
    };
    let imported = effects.import_committed_facts(received).await?;
    Ok(imported)
}

/// One AMP channel bootstrap key, as held in this device's secure storage.
#[derive(Serialize, Deserialize)]
struct SiblingBootstrapKey {
    context: aura_core::types::identifiers::ContextId,
    channel: aura_core::types::identifiers::ChannelId,
    bootstrap_id: aura_core::Hash32,
    key: Vec<u8>,
}

const SIBLING_AMP_KEYS_PURPOSE: &str = "aura.sibling.amp-bootstrap-keys";

fn bootstrap_key_location(key: &SiblingBootstrapKey) -> aura_core::effects::SecureStorageLocation {
    aura_core::effects::SecureStorageLocation::amp_bootstrap_key(
        &key.context,
        &key.channel,
        &key.bootstrap_id,
    )
}

async fn held_bootstrap_keys(
    effects: &AuraEffectSystem,
) -> Result<Vec<SiblingBootstrapKey>, AuraError> {
    use aura_core::effects::{SecureStorageCapability, SecureStorageEffects};
    let mut held = Vec::new();
    for (context, channel, bootstrap_id) in aura_amp::list_channel_bootstraps(effects).await? {
        let location = aura_core::effects::SecureStorageLocation::amp_bootstrap_key(
            &context,
            &channel,
            &bootstrap_id,
        );
        if let Ok(key) = effects
            .secure_retrieve(&location, &[SecureStorageCapability::Read])
            .await
        {
            held.push(SiblingBootstrapKey {
                context,
                channel,
                bootstrap_id,
                key,
            });
        }
    }
    Ok(held)
}

async fn device_leaf_public_key(
    effects: &AuraEffectSystem,
    device: DeviceId,
) -> Result<Option<Vec<u8>>, AuraError> {
    let tree = aura_protocol::effects::TreeEffects::get_current_state(effects).await?;
    Ok(tree
        .leaves
        .values()
        .find(|leaf| leaf.device_id == device)
        .map(|leaf| Vec::from(&leaf.public_key)))
}

async fn seal_bootstrap_keys(
    effects: &AuraEffectSystem,
    authority: aura_core::AuthorityId,
    peer: DeviceId,
    held: &[SiblingBootstrapKey],
) -> Result<Option<aura_sync::protocols::device_sealed::DeviceSealedPayload>, AuraError> {
    // A sibling not yet in the tree has no verified key to seal to.
    let peer_public_key = device_leaf_public_key(effects, peer).await?;
    tracing::debug!(
        sibling = %peer,
        held = held.len(),
        sibling_in_tree = peer_public_key.is_some(),
        "sealing bootstrap keys for sibling"
    );
    let Some(peer_public_key) = peer_public_key else {
        return Ok(None);
    };
    if held.is_empty() {
        return Ok(None);
    }
    let bundle = aura_core::util::serialization::to_vec(&held)
        .map_err(|error| AuraError::internal(format!("encode bootstrap keys: {error}")))?;
    aura_sync::protocols::device_sealed::seal_for_device(
        effects,
        SIBLING_AMP_KEYS_PURPOSE,
        authority,
        peer,
        &peer_public_key,
        &bundle,
    )
    .await
    .map(Some)
}

async fn store_bootstrap_keys(
    effects: &AuraEffectSystem,
    authority: aura_core::AuthorityId,
    sealed: &aura_sync::protocols::device_sealed::DeviceSealedPayload,
    secret: &[u8; 32],
) -> Result<(), AuraError> {
    use aura_core::effects::{SecureStorageCapability, SecureStorageEffects};
    let own_device = effects.device_id();
    let Some(own_public_key) = device_leaf_public_key(effects, own_device).await? else {
        return Ok(());
    };
    let bundle = aura_sync::protocols::device_sealed::open_for_device(
        effects,
        SIBLING_AMP_KEYS_PURPOSE,
        authority,
        own_device,
        &own_public_key,
        secret,
        sealed,
    )
    .await?;
    let keys: Vec<SiblingBootstrapKey> = aura_core::util::serialization::from_slice(&bundle)
        .map_err(|error| AuraError::internal(format!("decode bootstrap keys: {error}")))?;
    let mut newly_keyed = std::collections::BTreeSet::new();
    tracing::debug!(received = keys.len(), "opened bootstrap keys from sibling");
    for key in keys {
        let location = bootstrap_key_location(&key);
        if effects.secure_exists(&location).await.unwrap_or(false) {
            continue;
        }
        effects
            .secure_store(
                &location,
                &key.key,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await
            .map_err(|error| AuraError::storage(format!("store bootstrap key: {error}")))?;
        newly_keyed.insert(key.context);
    }
    // Messages in these contexts may have been rendered sealed before the key
    // arrived; re-publishing lets the chat view open them.
    if !newly_keyed.is_empty() {
        effects
            .republish_committed_facts_for_contexts(&newly_keyed)
            .await?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use crate::core::AgentConfig;
    use aura_core::types::identifiers::{AuthorityId, ContextId};
    use aura_journal::fact::RelationalFact;

    fn device(
        shared: &crate::SharedTransport,
        authority: AuthorityId,
        seed: u8,
    ) -> AuraEffectSystem {
        let config = AgentConfig {
            device_id: DeviceId::new_from_entropy([seed; 32]),
            ..AgentConfig::default()
        };
        AuraEffectSystem::simulation_with_shared_transport_for_authority(
            &config,
            0x51B1_0000 + u64::from(seed),
            authority,
            shared.clone(),
        )
        .expect("simulation effect system")
    }

    fn generic_fact(n: u8) -> RelationalFact {
        RelationalFact::Generic {
            context_id: ContextId::new_from_entropy([n; 32]),
            envelope: aura_core::types::facts::FactEnvelope {
                type_id: aura_core::types::facts::FactTypeId::from("test/sibling"),
                schema_version: 1,
                encoding: aura_core::types::facts::FactEncoding::DagCbor,
                payload: vec![n],
            },
        }
    }

    // Regression (work/8.md task 7, L3): a newly enrolled device must receive
    // Bootstrap keys travel only sealed to the sibling's leaf key: the right
    // device opens them; another device, a tampered ciphertext or a different
    // purpose does not.
    #[tokio::test]
    async fn bootstrap_keys_are_sealed_to_the_sibling_device() {
        use aura_core::effects::{CryptoCoreEffects, CryptoExtendedEffects};
        use aura_sync::protocols::device_sealed::{open_for_device, seal_for_device};
        let shared = crate::SharedTransport::new();
        let authority = AuthorityId::new_from_entropy([0x5C; 32]);
        let effects = device(&shared, authority, 0x65);
        let recipient = DeviceId::new_from_entropy([0x66; 32]);
        let (private_key, public_key) = effects.ed25519_generate_keypair().await.expect("keypair");
        let secret = effects
            .convert_ed25519_to_x25519_private(&private_key)
            .await
            .expect("x25519 secret");

        let sealed = seal_for_device(
            &effects,
            SIBLING_AMP_KEYS_PURPOSE,
            authority,
            recipient,
            &public_key,
            b"bootstrap keys",
        )
        .await
        .expect("seal");
        let opened = open_for_device(
            &effects,
            SIBLING_AMP_KEYS_PURPOSE,
            authority,
            recipient,
            &public_key,
            &secret,
            &sealed,
        )
        .await
        .expect("recipient opens");
        assert_eq!(opened, b"bootstrap keys".to_vec());

        let (other_private, other_public) =
            effects.ed25519_generate_keypair().await.expect("keypair");
        let other_secret = effects
            .convert_ed25519_to_x25519_private(&other_private)
            .await
            .expect("x25519 secret");
        assert!(open_for_device(
            &effects,
            SIBLING_AMP_KEYS_PURPOSE,
            authority,
            recipient,
            &other_public,
            &other_secret,
            &sealed,
        )
        .await
        .is_err());

        let mut tampered = sealed.clone();
        tampered.ciphertext[0] ^= 0x01;
        assert!(open_for_device(
            &effects,
            SIBLING_AMP_KEYS_PURPOSE,
            authority,
            recipient,
            &public_key,
            &secret,
            &tampered,
        )
        .await
        .is_err());

        assert!(open_for_device(
            &effects,
            "another-purpose",
            authority,
            recipient,
            &public_key,
            &secret,
            &sealed,
        )
        .await
        .is_err());
    }

    // the authority's existing facts from its sibling.
    #[tokio::test]
    async fn new_sibling_device_receives_existing_facts() {
        let shared = crate::SharedTransport::new();
        let authority = AuthorityId::new_from_entropy([0x5B; 32]);
        let existing = device(&shared, authority, 0x61);
        let joined = device(&shared, authority, 0x63);
        existing
            .commit_relational_facts((1..=3).map(generic_fact).collect())
            .await
            .expect("commit facts");
        let mut journal = aura_core::effects::JournalEffects::get_journal(&existing)
            .await
            .expect("journal");
        journal
            .facts
            .insert(
                "amp_channel_state",
                aura_core::FactValue::String("epoch-1".to_string()),
            )
            .expect("insert journal fact");
        aura_core::effects::JournalEffects::persist_journal(&existing, &journal)
            .await
            .expect("persist journal");

        // A frame left over from an exchange the other side abandoned.
        send_frame(
            &existing,
            joined.device_id(),
            &SiblingFactsFrame::Index(vec![vec![0xAA]]),
        )
        .await
        .expect("stale frame");

        let (from_existing, from_joined) = tokio::join!(
            exchange_facts_with_sibling(&existing, joined.device_id(), None),
            exchange_facts_with_sibling(&joined, existing.device_id(), None),
        );
        let originals = existing
            .load_committed_facts(authority)
            .await
            .expect("load original facts");
        assert_eq!(from_existing.expect("existing side"), 0);
        assert_eq!(from_joined.expect("joined side"), originals.len());
        let replicated = joined
            .load_committed_facts(authority)
            .await
            .expect("load replicated facts");
        assert_eq!(replicated, originals);
        let joined_journal = aura_core::effects::JournalEffects::get_journal(&joined)
            .await
            .expect("joined journal");
        assert!(joined_journal.facts.contains_key("amp_channel_state"));

        // A second round finds the stores equal and moves nothing.
        let (again_a, again_b) = tokio::join!(
            exchange_facts_with_sibling(&existing, joined.device_id(), None),
            exchange_facts_with_sibling(&joined, existing.device_id(), None),
        );
        assert_eq!((again_a.expect("a"), again_b.expect("b")), (0, 0));
    }
}
