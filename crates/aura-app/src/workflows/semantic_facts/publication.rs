#![allow(missing_docs)]

use super::{
    authorize_readiness_publication, semantic_lifecycle_publication_capability,
    SemanticOperationContext, SEMANTIC_OPERATION_CONTEXT_CAPABILITY,
};
use crate::signal_defs::{
    AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL, AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL_NAME,
};
use crate::ui_contract::{
    next_projection_revision, AuthoritativeSemanticFact, AuthoritativeSemanticFactKind,
    AuthoritativeSemanticFactsSnapshot, OperationId, OperationInstanceId,
    SemanticOperationCausality, SemanticOperationError, SemanticOperationKind,
    SemanticOperationPhase, SemanticOperationStatus,
};
use crate::workflows::signals::emit_signal;
use crate::AppCore;
use async_lock::RwLock;
use aura_core::{
    issue_operation_context, AuraError, AuthorizedProgressPublication,
    AuthorizedReadinessPublication, AuthorizedTerminalPublication, LifecyclePublicationCapability,
    OperationProgress, OperationTimeoutBudget, OwnedShutdownToken, OwnerEpoch, PublicationSequence,
    TerminalOutcome, TraceContext,
};
use std::sync::Arc;

#[derive(Debug, PartialEq, Eq)]
pub(in crate::workflows) struct ExactOperationLifecyclePublication {
    operation_id: OperationId,
    instance_id: OperationInstanceId,
    kind: SemanticOperationKind,
    causality: Option<SemanticOperationCausality>,
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

impl ExactOperationLifecyclePublication {
    pub(in crate::workflows) fn causality(&self) -> Option<SemanticOperationCausality> {
        self.causality
    }

    pub(in crate::workflows) fn phase(
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
                context
                    .begin_terminal::<(), SemanticOperationError>(capability)
                    .cancel(),
            ),
            SemanticOperationPhase::Succeeded => ExactLifecyclePublication::Terminal(
                context
                    .begin_terminal::<(), SemanticOperationError>(capability)
                    .succeed(()),
            ),
            phase => {
                ExactLifecyclePublication::Progress(context.publish_progress(capability, phase))
            }
        };
        Self {
            operation_id,
            instance_id,
            kind,
            causality: None,
            publication,
        }
    }

    pub(in crate::workflows) fn phase_from_context(
        capability: &LifecyclePublicationCapability,
        context: &mut SemanticOperationContext,
        kind: SemanticOperationKind,
        phase: SemanticOperationPhase,
    ) -> Self {
        assert_ne!(
            phase,
            SemanticOperationPhase::Failed,
            "failed terminal publication requires explicit failure payload"
        );
        let operation_id = context.operation_id().clone();
        let instance_id = context.instance_id().clone();
        let causality = Some(SemanticOperationCausality::new(
            context.owner_epoch(),
            context.publication_sequence(),
        ));
        let publication = match phase {
            SemanticOperationPhase::Submitted => ExactLifecyclePublication::Progress(
                context.publish_update(capability, OperationProgress::submitted()),
            ),
            phase => {
                ExactLifecyclePublication::Progress(context.publish_progress(capability, phase))
            }
        };
        Self {
            operation_id,
            instance_id,
            kind,
            causality,
            publication,
        }
    }

    pub(in crate::workflows) fn failure(
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
            causality: None,
            publication: ExactLifecyclePublication::Terminal(
                context
                    .begin_terminal::<(), SemanticOperationError>(capability)
                    .fail(error),
            ),
        }
    }

    pub(in crate::workflows) fn success_from_context(
        capability: &LifecyclePublicationCapability,
        context: SemanticOperationContext,
        kind: SemanticOperationKind,
    ) -> Self {
        let operation_id = context.operation_id().clone();
        let instance_id = context.instance_id().clone();
        let causality = Some(SemanticOperationCausality::new(
            context.owner_epoch(),
            context.publication_sequence(),
        ));
        Self {
            operation_id,
            instance_id,
            kind,
            causality,
            publication: ExactLifecyclePublication::Terminal(
                context
                    .begin_terminal::<(), SemanticOperationError>(capability)
                    .succeed(()),
            ),
        }
    }

    pub(in crate::workflows) fn cancelled_from_context(
        capability: &LifecyclePublicationCapability,
        context: SemanticOperationContext,
        kind: SemanticOperationKind,
    ) -> Self {
        let operation_id = context.operation_id().clone();
        let instance_id = context.instance_id().clone();
        let causality = Some(SemanticOperationCausality::new(
            context.owner_epoch(),
            context.publication_sequence(),
        ));
        Self {
            operation_id,
            instance_id,
            kind,
            causality,
            publication: ExactLifecyclePublication::Terminal(
                context
                    .begin_terminal::<(), SemanticOperationError>(capability)
                    .cancel(),
            ),
        }
    }

    pub(in crate::workflows) fn failure_from_context(
        capability: &LifecyclePublicationCapability,
        context: SemanticOperationContext,
        kind: SemanticOperationKind,
        error: SemanticOperationError,
    ) -> Self {
        let operation_id = context.operation_id().clone();
        let instance_id = context.instance_id().clone();
        let causality = Some(SemanticOperationCausality::new(
            context.owner_epoch(),
            context.publication_sequence(),
        ));
        Self {
            operation_id,
            instance_id,
            kind,
            causality,
            publication: ExactLifecyclePublication::Terminal(
                context
                    .begin_terminal::<(), SemanticOperationError>(capability)
                    .fail(error),
            ),
        }
    }

    pub(in crate::workflows) fn into_fact(self) -> AuthoritativeSemanticFact {
        let status = match self.publication {
            ExactLifecyclePublication::Progress(publication) => {
                let (
                    _capability,
                    _operation_id,
                    _instance_id,
                    _owner_epoch,
                    _publication_sequence,
                    _trace_context,
                    progress,
                ) = publication.into_parts();
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
                let (
                    _capability,
                    _operation_id,
                    _instance_id,
                    _owner_epoch,
                    _publication_sequence,
                    _trace_context,
                    outcome,
                ) = publication.into_parts();
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
            causality: self.causality,
            status,
        }
    }
}

pub(in crate::workflows) fn operation_phase_fact(
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
            causality: None,
            status: SemanticOperationStatus::new(kind, phase),
        },
    }
}

pub(in crate::workflows) fn operation_failure_fact(
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
            causality: None,
            status: SemanticOperationStatus::failed(kind, error),
        },
    }
}

/// Mutate the authoritative semantic-fact set and publish the replacement atomically.
pub async fn update_authoritative_semantic_facts<F>(
    app_core: &Arc<RwLock<AppCore>>,
    update: F,
) -> Result<(), AuraError>
where
    F: FnOnce(&mut Vec<AuthoritativeSemanticFact>),
{
    let _guard = super::AUTHORITATIVE_SEMANTIC_FACTS_UPDATE_GATE.lock().await;
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

pub(in crate::workflows) async fn publish_exact_operation_lifecycle(
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

pub(in crate::workflows) async fn publish_authoritative_operation_phase_with_instance(
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
            authorize_readiness_publication(operation_phase_fact(operation_id, None, kind, phase)),
        )
        .await
    }
}

pub(in crate::workflows) async fn publish_authoritative_operation_failure_with_instance(
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
