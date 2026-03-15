use std::sync::{Arc, LazyLock};

use async_lock::RwLock;
use aura_core::AuraError;

use crate::signal_defs::{
    AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL, AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL_NAME,
};
use crate::ui_contract::{
    AuthoritativeSemanticFact, AuthoritativeSemanticFactKind, OperationId, OperationInstanceId,
    SemanticOperationError, SemanticOperationKind, SemanticOperationPhase,
    SemanticOperationStatus,
};
use crate::workflows::signals::{emit_signal, read_signal_or_default};
use crate::AppCore;

static AUTHORITATIVE_SEMANTIC_FACTS_UPDATE_GATE: LazyLock<tokio::sync::Mutex<()>> =
    LazyLock::new(|| tokio::sync::Mutex::new(()));

/// Mutate the authoritative semantic-fact set and publish the replacement atomically.
pub async fn update_authoritative_semantic_facts<F>(
    app_core: &Arc<RwLock<AppCore>>,
    update: F,
) -> Result<(), AuraError>
where
    F: FnOnce(&mut Vec<AuthoritativeSemanticFact>),
{
    let _guard = AUTHORITATIVE_SEMANTIC_FACTS_UPDATE_GATE.lock().await;
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
    publish_authoritative_operation_phase_with_instance(app_core, operation_id, None, kind, phase)
        .await
}

/// Publish the current phase for a semantic operation with an explicit instance.
pub async fn publish_authoritative_operation_phase_with_instance(
    app_core: &Arc<RwLock<AppCore>>,
    operation_id: OperationId,
    instance_id: Option<OperationInstanceId>,
    kind: SemanticOperationKind,
    phase: SemanticOperationPhase,
) -> Result<(), AuraError> {
    publish_authoritative_semantic_fact(
        app_core,
        AuthoritativeSemanticFact::OperationStatus {
            operation_id,
            instance_id,
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
    publish_authoritative_operation_failure_with_instance(app_core, operation_id, None, kind, error)
        .await
}

/// Publish a terminal failure for a semantic operation with an explicit instance.
pub async fn publish_authoritative_operation_failure_with_instance(
    app_core: &Arc<RwLock<AppCore>>,
    operation_id: OperationId,
    instance_id: Option<OperationInstanceId>,
    kind: SemanticOperationKind,
    error: SemanticOperationError,
) -> Result<(), AuraError> {
    publish_authoritative_semantic_fact(
        app_core,
        AuthoritativeSemanticFact::OperationStatus {
            operation_id,
            instance_id,
            status: SemanticOperationStatus::failed(kind, error),
        },
    )
    .await
}

/// Publish explicit cancellation for a semantic operation.
pub async fn publish_authoritative_operation_cancellation(
    app_core: &Arc<RwLock<AppCore>>,
    operation_id: OperationId,
    kind: SemanticOperationKind,
) -> Result<(), AuraError> {
    publish_authoritative_semantic_fact(
        app_core,
        AuthoritativeSemanticFact::OperationStatus {
            operation_id,
            instance_id: None,
            status: SemanticOperationStatus::cancelled(kind),
        },
    )
    .await
}
