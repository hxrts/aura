use std::sync::{Arc, LazyLock};

use async_lock::{Mutex, RwLock};
use aura_core::{
    AuraError, AuthorizedReadinessPublication, LifecyclePublicationCapability,
    ReadinessPublicationCapability,
};

use crate::signal_defs::{
    AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL, AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL_NAME,
};
use crate::ui_contract::{
    AuthoritativeSemanticFact, AuthoritativeSemanticFactKind, OperationId, OperationInstanceId,
    SemanticOperationError, SemanticOperationKind, SemanticOperationPhase, SemanticOperationStatus,
};
use crate::workflows::signals::{emit_signal, read_signal_or_default};
use crate::AppCore;

struct AuthoritativeSemanticFactsUpdateGate {
    gate: Mutex<()>,
}

impl AuthoritativeSemanticFactsUpdateGate {
    const fn new() -> Self {
        Self {
            gate: Mutex::new(()),
        }
    }

    async fn lock(&self) -> async_lock::MutexGuard<'_, ()> {
        self.gate.lock().await
    }
}

static AUTHORITATIVE_SEMANTIC_FACTS_UPDATE_GATE: LazyLock<AuthoritativeSemanticFactsUpdateGate> =
    LazyLock::new(AuthoritativeSemanticFactsUpdateGate::new);
static SEMANTIC_LIFECYCLE_PUBLICATION_CAPABILITY: LazyLock<LifecyclePublicationCapability> =
    LazyLock::new(|| LifecyclePublicationCapability::new("aura-app:semantic-lifecycle"));
static SEMANTIC_READINESS_PUBLICATION_CAPABILITY: LazyLock<ReadinessPublicationCapability> =
    LazyLock::new(|| ReadinessPublicationCapability::new("aura-app:semantic-readiness"));

/// Return the sanctioned lifecycle-publication capability for shared semantic
/// operation status publication.
pub(crate) fn semantic_lifecycle_publication_capability() -> &'static LifecyclePublicationCapability {
    &SEMANTIC_LIFECYCLE_PUBLICATION_CAPABILITY
}

pub(crate) fn semantic_readiness_publication_capability() -> &'static ReadinessPublicationCapability
{
    &SEMANTIC_READINESS_PUBLICATION_CAPABILITY
}

fn authorize_readiness_publication(
    fact: AuthoritativeSemanticFact,
) -> AuthorizedReadinessPublication<AuthoritativeSemanticFact> {
    AuthorizedReadinessPublication::authorize(semantic_readiness_publication_capability(), fact)
}

/// Mutate the authoritative semantic-fact set and publish the replacement atomically.
///
/// The update gate serializes read-modify-emit sequences.  If `emit_signal`
/// fails the mutations are lost and the signal retains its prior value — the
/// error is propagated so callers can react.  A single retry is attempted
/// before giving up because the most common transient cause is signal
/// initialization timing.
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
    let result = emit_signal(
        app_core,
        &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL,
        facts.clone(),
        AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL_NAME,
    )
    .await;
    match result {
        Ok(()) => Ok(()),
        Err(_first_err) => {
            // Single retry — re-emit the already-mutated facts.
            emit_signal(
                app_core,
                &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL,
                facts,
                AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL_NAME,
            )
            .await
        }
    }
}

/// Publish one authoritative semantic fact, replacing any prior fact with the same key.
pub(crate) async fn publish_authoritative_semantic_fact(
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

/// Replace the full set of authoritative semantic facts for one fact kind.
pub(crate) async fn replace_authoritative_semantic_facts_of_kind(
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

/// Publish the current phase for a semantic operation.
pub(crate) async fn publish_authoritative_operation_phase(
    app_core: &Arc<RwLock<AppCore>>,
    capability: &LifecyclePublicationCapability,
    operation_id: OperationId,
    kind: SemanticOperationKind,
    phase: SemanticOperationPhase,
) -> Result<(), AuraError> {
    publish_authoritative_operation_phase_with_instance(
        app_core,
        capability,
        operation_id,
        None,
        kind,
        phase,
    )
    .await
}

/// Publish the current phase for a semantic operation with an explicit instance.
pub(crate) async fn publish_authoritative_operation_phase_with_instance(
    app_core: &Arc<RwLock<AppCore>>,
    _capability: &LifecyclePublicationCapability,
    operation_id: OperationId,
    instance_id: Option<OperationInstanceId>,
    kind: SemanticOperationKind,
    phase: SemanticOperationPhase,
) -> Result<(), AuraError> {
    publish_authoritative_semantic_fact(
        app_core,
        authorize_readiness_publication(AuthoritativeSemanticFact::OperationStatus {
            operation_id,
            instance_id,
            status: SemanticOperationStatus::new(kind, phase),
        }),
    )
    .await
}

/// Publish a terminal failure for a semantic operation.
pub(crate) async fn publish_authoritative_operation_failure(
    app_core: &Arc<RwLock<AppCore>>,
    capability: &LifecyclePublicationCapability,
    operation_id: OperationId,
    kind: SemanticOperationKind,
    error: SemanticOperationError,
) -> Result<(), AuraError> {
    publish_authoritative_operation_failure_with_instance(
        app_core,
        capability,
        operation_id,
        None,
        kind,
        error,
    )
    .await
}

/// Publish a terminal failure for a semantic operation with an explicit instance.
pub(crate) async fn publish_authoritative_operation_failure_with_instance(
    app_core: &Arc<RwLock<AppCore>>,
    _capability: &LifecyclePublicationCapability,
    operation_id: OperationId,
    instance_id: Option<OperationInstanceId>,
    kind: SemanticOperationKind,
    error: SemanticOperationError,
) -> Result<(), AuraError> {
    publish_authoritative_semantic_fact(
        app_core,
        authorize_readiness_publication(AuthoritativeSemanticFact::OperationStatus {
            operation_id,
            instance_id,
            status: SemanticOperationStatus::failed(kind, error),
        }),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal_defs::AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL;
    use crate::{AppConfig, AppCore};

    #[tokio::test]
    async fn concurrent_authoritative_fact_updates_do_not_lose_entries() {
        let app_core = Arc::new(RwLock::new(
            AppCore::new(AppConfig::default()).unwrap_or_else(|error| panic!("{error}")),
        ));
        AppCore::init_signals_with_hooks(&app_core)
            .await
            .unwrap_or_else(|error| panic!("{error}"));

        let first = publish_authoritative_semantic_fact(
            &app_core,
            authorize_readiness_publication(AuthoritativeSemanticFact::PendingHomeInvitationReady),
        );
        let second = publish_authoritative_semantic_fact(
            &app_core,
            authorize_readiness_publication(AuthoritativeSemanticFact::ContactLinkReady {
                authority_id: "owner-a".into(),
                contact_count: 1,
            }),
        );

        let (first_result, second_result) = futures::future::join(first, second).await;
        first_result.unwrap_or_else(|error| panic!("{error}"));
        second_result.unwrap_or_else(|error| panic!("{error}"));

        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;

        assert!(facts.contains(&AuthoritativeSemanticFact::PendingHomeInvitationReady));
        assert!(
            facts.contains(&AuthoritativeSemanticFact::ContactLinkReady {
                authority_id: "owner-a".into(),
                contact_count: 1,
            })
        );
        assert_eq!(facts.len(), 2);
    }
}
