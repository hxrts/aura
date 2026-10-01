//! Replicate committed facts between devices of this authority.
//!
//! Devices of one authority each keep the authority's committed fact store.
//! A newly enrolled device starts empty, and facts committed on one device
//! must reach the others. Anti-entropy compares operation logs only, so this
//! symmetric lockstep exchange carries the facts themselves:
//! digest, then key index, then the facts the peer lacks.

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
    let mut digest_input: Vec<u8> = by_key.keys().flatten().copied().collect();
    digest_input.extend_from_slice(&hash(&journal_bytes));
    let digest = hash(&digest_input);

    send_frame(effects, peer, &SiblingFactsFrame::Digest(digest)).await?;
    let SiblingFactsFrame::Digest(peer_digest) = receive_frame(effects, peer).await? else {
        return Err(protocol_error("digest"));
    };
    if peer_digest == digest {
        return Ok(0);
    }

    send_frame(
        effects,
        peer,
        &SiblingFactsFrame::Index(by_key.keys().cloned().collect()),
    )
    .await?;
    let SiblingFactsFrame::Index(peer_keys) = receive_frame(effects, peer).await? else {
        return Err(protocol_error("index"));
    };
    let peer_keys: BTreeSet<Vec<u8>> = peer_keys.into_iter().collect();
    let missing: Vec<TypedFact> = by_key
        .into_iter()
        .filter(|(key, _)| !peer_keys.contains(key))
        .map(|(_, fact)| fact)
        .collect();

    send_frame(effects, peer, &SiblingFactsFrame::Facts(missing)).await?;
    let SiblingFactsFrame::Facts(received) = receive_frame(effects, peer).await? else {
        return Err(protocol_error("facts"));
    };
    let imported = effects.import_committed_facts(received).await?;

    send_frame(
        effects,
        peer,
        &SiblingFactsFrame::JournalFacts(journal.facts.clone()),
    )
    .await?;
    let SiblingFactsFrame::JournalFacts(peer_facts) = receive_frame(effects, peer).await? else {
        return Err(protocol_error("journal facts"));
    };
    let before = journal.facts.clone();
    journal.merge_facts(peer_facts);
    if journal.facts != before {
        aura_core::effects::JournalEffects::persist_journal(effects, &journal).await?;
    }
    Ok(imported)
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

        let (from_existing, from_joined) = tokio::join!(
            exchange_facts_with_sibling(&existing, joined.device_id()),
            exchange_facts_with_sibling(&joined, existing.device_id()),
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
            exchange_facts_with_sibling(&existing, joined.device_id()),
            exchange_facts_with_sibling(&joined, existing.device_id()),
        );
        assert_eq!((again_a.expect("a"), again_b.expect("b")), (0, 0));
    }
}
