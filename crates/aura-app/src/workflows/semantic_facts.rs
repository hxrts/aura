use std::sync::Arc;

use async_lock::RwLock;
use aura_core::AuraError;

use crate::signal_defs::{
    AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL, AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL_NAME,
};
use crate::ui_contract::{
    AuthoritativeSemanticFact, AuthoritativeSemanticFactKind, OperationId,
    SemanticOperationError, SemanticOperationKind, SemanticOperationPhase,
    SemanticOperationStatus,
};
use crate::workflows::signals::{emit_signal, read_signal_or_default};
use crate::AppCore;

/// Mutate the authoritative semantic-fact set and publish the replacement atomically.
pub async fn update_authoritative_semantic_facts<F>(
    app_core: &Arc<RwLock<AppCore>>,
    update: F,
) -> Result<(), AuraError>
where
    F: FnOnce(&mut Vec<AuthoritativeSemanticFact>),
{
    let mut facts = read_signal_or_default(app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
    update(&mut facts);
    emit_signal(
        app_core,
        &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL,
        facts,
        AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL_NAME,
    )
    .await
}

/// Publish one authoritative semantic fact, replacing any prior fact with the same key.
pub async fn publish_authoritative_semantic_fact(
    app_core: &Arc<RwLock<AppCore>>,
    fact: AuthoritativeSemanticFact,
) -> Result<(), AuraError> {
    update_authoritative_semantic_facts(app_core, |facts| {
        facts.retain(|existing| existing.key() != fact.key());
        facts.push(fact);
    })
    .await
}

/// Replace the full set of authoritative semantic facts for one fact kind.
pub async fn replace_authoritative_semantic_facts_of_kind(
    app_core: &Arc<RwLock<AppCore>>,
    kind: AuthoritativeSemanticFactKind,
    replacements: Vec<AuthoritativeSemanticFact>,
) -> Result<(), AuraError> {
    update_authoritative_semantic_facts(app_core, |facts| {
        facts.retain(|existing| existing.kind() != kind);
        facts.extend(replacements);
    })
    .await
}

/// Publish the current phase for a semantic operation.
pub async fn publish_authoritative_operation_phase(
    app_core: &Arc<RwLock<AppCore>>,
    operation_id: OperationId,
    kind: SemanticOperationKind,
    phase: SemanticOperationPhase,
) -> Result<(), AuraError> {
    publish_authoritative_semantic_fact(
        app_core,
        AuthoritativeSemanticFact::OperationStatus {
            operation_id,
            instance_id: None,
            status: SemanticOperationStatus::new(kind, phase),
        },
    )
    .await
}

/// Publish a terminal failure for a semantic operation.
pub async fn publish_authoritative_operation_failure(
    app_core: &Arc<RwLock<AppCore>>,
    operation_id: OperationId,
    kind: SemanticOperationKind,
    error: SemanticOperationError,
) -> Result<(), AuraError> {
    publish_authoritative_semantic_fact(
        app_core,
        AuthoritativeSemanticFact::OperationStatus {
            operation_id,
            instance_id: None,
            status: SemanticOperationStatus::failed(kind, error),
        },
    )
    .await
}
