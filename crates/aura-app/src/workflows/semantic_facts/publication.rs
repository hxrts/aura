#![allow(missing_docs)]

use super::owner::AUTHORITATIVE_SEMANTIC_FACTS_UPDATE_GATE;
use crate::signal_defs::{
    AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL, AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL_NAME,
};
use crate::ui_contract::{
    next_projection_revision, AuthoritativeSemanticFact, AuthoritativeSemanticFactKind,
    AuthoritativeSemanticFactsSnapshot,
};
use crate::workflows::signals::emit_signal;
use crate::AppCore;
use async_lock::RwLock;
use aura_core::{AuraError, AuthorizedReadinessPublication};
use std::sync::Arc;

/// Mutate the authoritative semantic-fact set and publish the replacement atomically.
pub async fn update_authoritative_semantic_facts<F>(
    app_core: &Arc<RwLock<AppCore>>,
    update: F,
) -> Result<(), AuraError>
where
    F: FnOnce(&mut Vec<AuthoritativeSemanticFact>),
{
    let _guard = AUTHORITATIVE_SEMANTIC_FACTS_UPDATE_GATE.lock().await;
    let (previous_facts, updated_facts, changed) = {
        let mut core = app_core.write().await;
        let previous_facts = core.authoritative_semantic_facts();
        let mut updated_facts = previous_facts.clone();
        update(&mut updated_facts);
        let changed = updated_facts != previous_facts;
        if changed {
            core.set_authoritative_semantic_facts(updated_facts.clone());
        }
        (previous_facts, updated_facts, changed)
    };
    if !changed {
        return Ok(());
    }
    if let Err(error) = emit_signal(
        app_core,
        &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL,
        AuthoritativeSemanticFactsSnapshot {
            revision: next_projection_revision(None),
            facts: updated_facts,
        },
        AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL_NAME,
    )
    .await
    {
        app_core
            .write()
            .await
            .set_authoritative_semantic_facts(previous_facts);
        return Err(error);
    }
    Ok(())
}

pub(in crate::workflows) async fn publish_authoritative_semantic_fact(
    app_core: &Arc<RwLock<AppCore>>,
    publication: AuthorizedReadinessPublication<AuthoritativeSemanticFact>,
) -> Result<(), AuraError> {
    let (_capability, fact) = publication.into_parts();
    update_authoritative_semantic_facts(app_core, |facts| {
        facts.retain(|existing| existing.key() != fact.key());
        facts.push(fact);
    })
    .await
}

pub(in crate::workflows) async fn replace_authoritative_semantic_facts_of_kind(
    app_core: &Arc<RwLock<AppCore>>,
    publication: AuthorizedReadinessPublication<(
        AuthoritativeSemanticFactKind,
        Vec<AuthoritativeSemanticFact>,
    )>,
) -> Result<(), AuraError> {
    let (_capability, (kind, replacements)) = publication.into_parts();
    update_authoritative_semantic_facts(app_core, |facts| {
        facts.retain(|existing| existing.kind() != kind);
        facts.extend(replacements);
    })
    .await
}
