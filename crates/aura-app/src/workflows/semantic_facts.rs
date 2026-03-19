use std::sync::{Arc, LazyLock};

use async_lock::{Mutex, RwLock};
use aura_core::{
    issue_operation_context, AuraError, AuthorizedProgressPublication,
    AuthorizedReadinessPublication, AuthorizedTerminalPublication, LifecyclePublicationCapability,
    OperationContextCapability, OperationProgress, OperationTimeoutBudget, OwnedShutdownToken,
    OwnerEpoch, PublicationSequence, ReadinessPublicationCapability, TerminalOutcome, TraceContext,
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
static SEMANTIC_OPERATION_CONTEXT_CAPABILITY: LazyLock<OperationContextCapability> =
    LazyLock::new(|| OperationContextCapability::new("aura-app:semantic-operation-context"));
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

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ExactOperationLifecyclePublication {
    operation_id: OperationId,
    instance_id: OperationInstanceId,
    kind: SemanticOperationKind,
    publication: ExactLifecyclePublication,
}

#[derive(Debug, PartialEq, Eq)]
enum ExactLifecyclePublication {
    Progress(
        AuthorizedProgressPublication<
            OperationId,
            OperationInstanceId,
            TraceContext,
            SemanticOperationPhase,
        >,
    ),
    Terminal(
        AuthorizedTerminalPublication<
            OperationId,
            OperationInstanceId,
            TraceContext,
            (),
            SemanticOperationError,
        >,
    ),
}

#[derive(Clone)]
pub(crate) struct SemanticWorkflowOwner {
    app_core: Arc<RwLock<AppCore>>,
    operation_id: OperationId,
    instance_id: Option<OperationInstanceId>,
    kind: SemanticOperationKind,
}

impl SemanticWorkflowOwner {
    pub(crate) fn new(
        app_core: &Arc<RwLock<AppCore>>,
        operation_id: OperationId,
        instance_id: Option<OperationInstanceId>,
        kind: SemanticOperationKind,
    ) -> Self {
        Self {
            app_core: app_core.clone(),
            operation_id,
            instance_id,
            kind,
        }
    }

    pub(crate) fn app_core(&self) -> &Arc<RwLock<AppCore>> {
        &self.app_core
    }

    pub(crate) fn kind(&self) -> SemanticOperationKind {
        self.kind
    }

    pub(crate) fn phase_fact(
        &self,
        phase: SemanticOperationPhase,
    ) -> AuthoritativeSemanticFact {
        operation_phase_fact(
            self.operation_id.clone(),
            self.instance_id.clone(),
            self.kind,
            phase,
        )
    }

    pub(crate) async fn publish_phase(
        &self,
        phase: SemanticOperationPhase,
    ) -> Result<(), AuraError> {
        publish_authoritative_operation_phase_with_instance(
            &self.app_core,
            semantic_lifecycle_publication_capability(),
            self.operation_id.clone(),
            self.instance_id.clone(),
            self.kind,
            phase,
        )
        .await
    }

    pub(crate) async fn publish_failure(
        &self,
        error: SemanticOperationError,
    ) -> Result<(), AuraError> {
        publish_authoritative_operation_failure_with_instance(
            &self.app_core,
            semantic_lifecycle_publication_capability(),
            self.operation_id.clone(),
            self.instance_id.clone(),
            self.kind,
            error,
        )
        .await
    }
}

impl ExactOperationLifecyclePublication {
    pub(crate) fn phase(
        capability: &LifecyclePublicationCapability,
        operation_id: OperationId,
        instance_id: OperationInstanceId,
        kind: SemanticOperationKind,
        phase: SemanticOperationPhase,
    ) -> Self {
        assert_ne!(
            phase,
            SemanticOperationPhase::Failed,
            "failed terminal publication requires explicit failure payload"
        );
        let mut context = issue_operation_context(
            &SEMANTIC_OPERATION_CONTEXT_CAPABILITY,
            operation_id.clone(),
            instance_id.clone(),
            OwnerEpoch::new(0),
            PublicationSequence::new(0),
            OperationTimeoutBudget::deferred_local_policy(),
            OwnedShutdownToken::detached(),
            TraceContext::detached(),
        );
        let publication = match phase {
            SemanticOperationPhase::Submitted => ExactLifecyclePublication::Progress(
                context.publish_update(capability, OperationProgress::submitted()),
            ),
            SemanticOperationPhase::Cancelled => ExactLifecyclePublication::Terminal(
                context.begin_terminal::<(), SemanticOperationError>(capability).cancel(),
            ),
            SemanticOperationPhase::Succeeded => ExactLifecyclePublication::Terminal(
                context
                    .begin_terminal::<(), SemanticOperationError>(capability)
                    .succeed(()),
            ),
            phase => ExactLifecyclePublication::Progress(context.publish_progress(capability, phase)),
        };
        Self {
            operation_id,
            instance_id,
            kind,
            publication,
        }
    }

    pub(crate) fn failure(
        capability: &LifecyclePublicationCapability,
        operation_id: OperationId,
        instance_id: OperationInstanceId,
        kind: SemanticOperationKind,
        error: SemanticOperationError,
    ) -> Self {
        let context = issue_operation_context(
            &SEMANTIC_OPERATION_CONTEXT_CAPABILITY,
            operation_id.clone(),
            instance_id.clone(),
            OwnerEpoch::new(0),
            PublicationSequence::new(0),
            OperationTimeoutBudget::deferred_local_policy(),
            OwnedShutdownToken::detached(),
            TraceContext::detached(),
        );
        Self {
            operation_id,
            instance_id,
            kind,
            publication: ExactLifecyclePublication::Terminal(
                context
                    .begin_terminal::<(), SemanticOperationError>(capability)
                    .fail(error),
            ),
        }
    }

    pub(crate) fn into_fact(self) -> AuthoritativeSemanticFact {
        let status = match self.publication {
            ExactLifecyclePublication::Progress(publication) => {
                let (_capability, _operation_id, _instance_id, _owner_epoch, _publication_sequence, _trace_context, progress) =
                    publication.into_parts();
                match progress {
                    OperationProgress::Submitted => {
                        SemanticOperationStatus::new(self.kind, SemanticOperationPhase::Submitted)
                    }
                    OperationProgress::Progress { phase } => {
                        SemanticOperationStatus::new(self.kind, phase)
                    }
                }
            }
            ExactLifecyclePublication::Terminal(publication) => {
                let (_capability, _operation_id, _instance_id, _owner_epoch, _publication_sequence, _trace_context, outcome) =
                    publication.into_parts();
                match outcome {
                    TerminalOutcome::Succeeded { .. } => {
                        SemanticOperationStatus::new(self.kind, SemanticOperationPhase::Succeeded)
                    }
                    TerminalOutcome::Failed { error } => {
                        SemanticOperationStatus::failed(self.kind, error)
                    }
                    TerminalOutcome::Cancelled => {
                        SemanticOperationStatus::new(self.kind, SemanticOperationPhase::Cancelled)
                    }
                }
            }
        };

        AuthoritativeSemanticFact::OperationStatus {
            operation_id: self.operation_id,
            instance_id: Some(self.instance_id),
            status,
        }
    }
}

pub(crate) fn operation_phase_fact(
    operation_id: OperationId,
    instance_id: Option<OperationInstanceId>,
    kind: SemanticOperationKind,
    phase: SemanticOperationPhase,
) -> AuthoritativeSemanticFact {
    match instance_id {
        Some(instance_id) => ExactOperationLifecyclePublication::phase(
            semantic_lifecycle_publication_capability(),
            operation_id,
            instance_id,
            kind,
            phase,
        )
        .into_fact(),
        None => AuthoritativeSemanticFact::OperationStatus {
            operation_id,
            instance_id: None,
            status: SemanticOperationStatus::new(kind, phase),
        },
    }
}

pub(crate) fn operation_failure_fact(
    operation_id: OperationId,
    instance_id: Option<OperationInstanceId>,
    kind: SemanticOperationKind,
    error: SemanticOperationError,
) -> AuthoritativeSemanticFact {
    match instance_id {
        Some(instance_id) => ExactOperationLifecyclePublication::failure(
            semantic_lifecycle_publication_capability(),
            operation_id,
            instance_id,
            kind,
            error,
        )
        .into_fact(),
        None => AuthoritativeSemanticFact::OperationStatus {
            operation_id,
            instance_id: None,
            status: SemanticOperationStatus::failed(kind, error),
        },
    }
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

pub(crate) async fn publish_exact_operation_lifecycle(
    app_core: &Arc<RwLock<AppCore>>,
    publication: ExactOperationLifecyclePublication,
) -> Result<(), AuraError> {
    let fact = publication.into_fact();
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
    capability: &LifecyclePublicationCapability,
    operation_id: OperationId,
    instance_id: Option<OperationInstanceId>,
    kind: SemanticOperationKind,
    phase: SemanticOperationPhase,
) -> Result<(), AuraError> {
    if let Some(instance_id) = instance_id {
        publish_exact_operation_lifecycle(
            app_core,
            ExactOperationLifecyclePublication::phase(
                capability,
                operation_id,
                instance_id,
                kind,
                phase,
            ),
        )
        .await
    } else {
        publish_authoritative_semantic_fact(
            app_core,
            authorize_readiness_publication(operation_phase_fact(
                operation_id,
                None,
                kind,
                phase,
            )),
        )
        .await
    }
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
    capability: &LifecyclePublicationCapability,
    operation_id: OperationId,
    instance_id: Option<OperationInstanceId>,
    kind: SemanticOperationKind,
    error: SemanticOperationError,
) -> Result<(), AuraError> {
    if let Some(instance_id) = instance_id {
        publish_exact_operation_lifecycle(
            app_core,
            ExactOperationLifecyclePublication::failure(
                capability,
                operation_id,
                instance_id,
                kind,
                error,
            ),
        )
        .await
    } else {
        publish_authoritative_semantic_fact(
            app_core,
            authorize_readiness_publication(operation_failure_fact(
                operation_id,
                None,
                kind,
                error,
            )),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal_defs::AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL;
    use crate::ui_contract::{SemanticOperationKind, SemanticOperationPhase};
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

    #[tokio::test]
    async fn exact_operation_lifecycle_publication_retains_instance_identity() {
        let app_core = Arc::new(RwLock::new(
            AppCore::new(AppConfig::default()).unwrap_or_else(|error| panic!("{error}")),
        ));
        AppCore::init_signals_with_hooks(&app_core)
            .await
            .unwrap_or_else(|error| panic!("{error}"));

        publish_exact_operation_lifecycle(
            &app_core,
            ExactOperationLifecyclePublication::phase(
                semantic_lifecycle_publication_capability(),
                OperationId::invitation_accept(),
                OperationInstanceId("tui-op-invitation_accept-3".to_string()),
                SemanticOperationKind::AcceptPendingChannelInvitation,
                SemanticOperationPhase::Succeeded,
            ),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));

        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(facts.contains(&AuthoritativeSemanticFact::OperationStatus {
            operation_id: OperationId::invitation_accept(),
            instance_id: Some(OperationInstanceId("tui-op-invitation_accept-3".to_string())),
            status: SemanticOperationStatus::new(
                SemanticOperationKind::AcceptPendingChannelInvitation,
                SemanticOperationPhase::Succeeded,
            ),
        }));
    }
}
