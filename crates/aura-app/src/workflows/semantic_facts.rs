use std::sync::{Arc, LazyLock};

use async_lock::{Mutex, RwLock};
use aura_core::{
    issue_operation_context, AuraError, AuthorizedProgressPublication,
    AuthorizedReadinessPublication, AuthorizedTerminalPublication, LifecyclePublicationCapability,
    OperationContext, OperationContextCapability, OperationProgress, OperationTimeoutBudget,
    OwnedShutdownToken, OwnerEpoch, PublicationSequence, ReadinessPublicationCapability,
    TerminalOutcome, TraceContext,
};

use crate::signal_defs::{
    AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL, AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL_NAME,
};
use crate::ui_contract::{
    AuthoritativeSemanticFact, AuthoritativeSemanticFactKind, OperationId, OperationInstanceId,
    SemanticOperationCausality, SemanticOperationError, SemanticOperationKind,
    SemanticOperationPhase, SemanticOperationStatus,
};
use crate::workflows::signals::{emit_signal, read_signal_or_default};
use crate::AppCore;

pub(in crate::workflows) type SemanticOperationContext =
    OperationContext<OperationId, OperationInstanceId, TraceContext>;

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
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "semantic_lifecycle"
)]
pub(in crate::workflows) fn semantic_lifecycle_publication_capability() -> &'static LifecyclePublicationCapability {
    &SEMANTIC_LIFECYCLE_PUBLICATION_CAPABILITY
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "semantic_readiness"
)]
pub(in crate::workflows) fn semantic_readiness_publication_capability() -> &'static ReadinessPublicationCapability
{
    &SEMANTIC_READINESS_PUBLICATION_CAPABILITY
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "semantic_readiness"
)]
fn authorize_readiness_publication(
    fact: AuthoritativeSemanticFact,
) -> AuthorizedReadinessPublication<AuthoritativeSemanticFact> {
    AuthorizedReadinessPublication::authorize(semantic_readiness_publication_capability(), fact)
}

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

enum SemanticWorkflowPublicationState {
    Legacy,
    Exact(Option<SemanticOperationContext>),
}

pub(in crate::workflows) struct SemanticWorkflowOwner {
    app_core: Arc<RwLock<AppCore>>,
    operation_id: OperationId,
    instance_id: Option<OperationInstanceId>,
    kind: SemanticOperationKind,
    publication_state: Mutex<SemanticWorkflowPublicationState>,
}

impl SemanticWorkflowOwner {
    pub(in crate::workflows) fn new(
        app_core: &Arc<RwLock<AppCore>>,
        operation_id: OperationId,
        instance_id: Option<OperationInstanceId>,
        kind: SemanticOperationKind,
    ) -> Self {
        let publication_state = issue_semantic_operation_context(
            operation_id.clone(),
            instance_id.clone(),
        )
        .map(|context| SemanticWorkflowPublicationState::Exact(Some(context)))
        .unwrap_or(SemanticWorkflowPublicationState::Legacy);
        Self {
            app_core: app_core.clone(),
            operation_id,
            instance_id,
            kind,
            publication_state: Mutex::new(publication_state),
        }
    }

    pub(in crate::workflows) fn app_core(&self) -> &Arc<RwLock<AppCore>> {
        &self.app_core
    }

    pub(in crate::workflows) fn kind(&self) -> SemanticOperationKind {
        self.kind
    }

    pub(in crate::workflows) async fn publish_phase(
        &self,
        phase: SemanticOperationPhase,
    ) -> Result<(), AuraError> {
        let publication = {
            let mut state = self.publication_state.lock().await;
            match &mut *state {
                SemanticWorkflowPublicationState::Exact(context) => {
                    match phase {
                        SemanticOperationPhase::Succeeded => {
                            let Some(context) = context.take() else {
                                return Err(AuraError::invalid(
                                    "semantic workflow owner has already published terminal lifecycle",
                                ));
                            };
                            Some(ExactOperationLifecyclePublication::success_from_context(
                                semantic_lifecycle_publication_capability(),
                                context,
                                self.kind,
                            ))
                        }
                        SemanticOperationPhase::Cancelled => {
                            let Some(context) = context.take() else {
                                return Err(AuraError::invalid(
                                    "semantic workflow owner has already published terminal lifecycle",
                                ));
                            };
                            Some(ExactOperationLifecyclePublication::cancelled_from_context(
                                semantic_lifecycle_publication_capability(),
                                context,
                                self.kind,
                            ))
                        }
                        _ => {
                            let Some(context) = context.as_mut() else {
                                return Err(AuraError::invalid(
                                    "semantic workflow owner has already published terminal lifecycle",
                                ));
                            };
                            Some(ExactOperationLifecyclePublication::phase_from_context(
                                semantic_lifecycle_publication_capability(),
                                context,
                                self.kind,
                                phase,
                            ))
                        }
                    }
                }
                SemanticWorkflowPublicationState::Legacy => None,
            }
        };
        if let Some(publication) = publication {
            publish_exact_operation_lifecycle(&self.app_core, publication).await
        } else {
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
    }

    pub(in crate::workflows) async fn publish_failure(
        &self,
        error: SemanticOperationError,
    ) -> Result<(), AuraError> {
        let publication = {
            let mut state = self.publication_state.lock().await;
            match &mut *state {
                SemanticWorkflowPublicationState::Exact(context) => {
                    let Some(context) = context.take() else {
                        return Err(AuraError::invalid(
                            "semantic workflow owner has already published terminal lifecycle",
                        ));
                    };
                    Some(ExactOperationLifecyclePublication::failure_from_context(
                        semantic_lifecycle_publication_capability(),
                        context,
                        self.kind,
                        error.clone(),
                    ))
                }
                SemanticWorkflowPublicationState::Legacy => None,
            }
        };
        if let Some(publication) = publication {
            publish_exact_operation_lifecycle(&self.app_core, publication).await
        } else {
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

    pub(in crate::workflows) async fn terminal_success_fact(
        &self,
    ) -> Result<AuthoritativeSemanticFact, AuraError> {
        let publication = {
            let mut state = self.publication_state.lock().await;
            match &mut *state {
                SemanticWorkflowPublicationState::Exact(context) => {
                    let Some(context) = context.take() else {
                        return Err(AuraError::invalid(
                            "semantic workflow owner has already published terminal lifecycle",
                        ));
                    };
                    Some(ExactOperationLifecyclePublication::success_from_context(
                        semantic_lifecycle_publication_capability(),
                        context,
                        self.kind,
                    ))
                }
                SemanticWorkflowPublicationState::Legacy => None,
            }
        };

        Ok(match publication {
            Some(publication) => publication.into_fact(),
            None => operation_phase_fact(
                self.operation_id.clone(),
                self.instance_id.clone(),
                self.kind,
                SemanticOperationPhase::Succeeded,
            ),
        })
    }
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "semantic_operation_context"
)]
pub(in crate::workflows) fn issue_semantic_operation_context(
    operation_id: OperationId,
    instance_id: Option<OperationInstanceId>,
) -> Option<SemanticOperationContext> {
    instance_id.map(|instance_id| {
        issue_operation_context(
            &SEMANTIC_OPERATION_CONTEXT_CAPABILITY,
            operation_id,
            instance_id,
            OwnerEpoch::new(0),
            PublicationSequence::new(0),
            OperationTimeoutBudget::deferred_local_policy(),
            OwnedShutdownToken::detached(),
            TraceContext::detached(),
        )
    })
}

impl ExactOperationLifecyclePublication {
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
            phase => ExactLifecyclePublication::Progress(context.publish_progress(capability, phase)),
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

/// Replace the full set of authoritative semantic facts for one fact kind.
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

/// Publish the current phase for a semantic operation with an explicit instance.
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

/// Publish a terminal failure for a semantic operation with an explicit instance.
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
            causality: None,
            status: SemanticOperationStatus::new(
                SemanticOperationKind::AcceptPendingChannelInvitation,
                SemanticOperationPhase::Succeeded,
            ),
        }));
    }

    #[tokio::test]
    async fn exact_operation_lifecycle_round_trips_identity_under_contention() {
        let app_core = Arc::new(RwLock::new(
            AppCore::new(AppConfig::default()).unwrap_or_else(|error| panic!("{error}")),
        ));
        AppCore::init_signals_with_hooks(&app_core)
            .await
            .unwrap_or_else(|error| panic!("{error}"));

        let mut invitation_context = issue_operation_context(
            &SEMANTIC_OPERATION_CONTEXT_CAPABILITY,
            OperationId::invitation_accept(),
            OperationInstanceId("tui-op-invitation_accept-7".to_string()),
            OwnerEpoch::new(2),
            PublicationSequence::new(5),
            OperationTimeoutBudget::deferred_local_policy(),
            OwnedShutdownToken::detached(),
            TraceContext::detached(),
        );
        let mut send_context = issue_operation_context(
            &SEMANTIC_OPERATION_CONTEXT_CAPABILITY,
            OperationId::send_message(),
            OperationInstanceId("tui-op-send_message-9".to_string()),
            OwnerEpoch::new(4),
            PublicationSequence::new(8),
            OperationTimeoutBudget::deferred_local_policy(),
            OwnedShutdownToken::detached(),
            TraceContext::detached(),
        );

        let invitation = publish_exact_operation_lifecycle(
            &app_core,
            ExactOperationLifecyclePublication::phase_from_context(
                semantic_lifecycle_publication_capability(),
                &mut invitation_context,
                SemanticOperationKind::AcceptPendingChannelInvitation,
                SemanticOperationPhase::WorkflowDispatched,
            ),
        );
        let send = publish_exact_operation_lifecycle(
            &app_core,
            ExactOperationLifecyclePublication::phase_from_context(
                semantic_lifecycle_publication_capability(),
                &mut send_context,
                SemanticOperationKind::SendChatMessage,
                SemanticOperationPhase::WorkflowDispatched,
            ),
        );

        let (invitation_result, send_result) = futures::future::join(invitation, send).await;
        invitation_result.unwrap_or_else(|error| panic!("{error}"));
        send_result.unwrap_or_else(|error| panic!("{error}"));

        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(facts.contains(&AuthoritativeSemanticFact::OperationStatus {
            operation_id: OperationId::invitation_accept(),
            instance_id: Some(OperationInstanceId("tui-op-invitation_accept-7".to_string())),
            causality: Some(SemanticOperationCausality {
                owner_epoch: OwnerEpoch::new(2),
                publication_sequence: PublicationSequence::new(5),
            }),
            status: SemanticOperationStatus::new(
                SemanticOperationKind::AcceptPendingChannelInvitation,
                SemanticOperationPhase::WorkflowDispatched,
            ),
        }));
        assert!(facts.contains(&AuthoritativeSemanticFact::OperationStatus {
            operation_id: OperationId::send_message(),
            instance_id: Some(OperationInstanceId("tui-op-send_message-9".to_string())),
            causality: Some(SemanticOperationCausality {
                owner_epoch: OwnerEpoch::new(4),
                publication_sequence: PublicationSequence::new(8),
            }),
            status: SemanticOperationStatus::new(
                SemanticOperationKind::SendChatMessage,
                SemanticOperationPhase::WorkflowDispatched,
            ),
        }));
    }

    #[test]
    fn exact_operation_lifecycle_from_context_carries_causality() {
        let mut context = issue_operation_context(
            &SEMANTIC_OPERATION_CONTEXT_CAPABILITY,
            OperationId::invitation_accept(),
            OperationInstanceId("tui-op-invitation_accept-3".to_string()),
            OwnerEpoch::new(7),
            PublicationSequence::new(11),
            OperationTimeoutBudget::deferred_local_policy(),
            OwnedShutdownToken::detached(),
            TraceContext::detached(),
        );

        let fact = ExactOperationLifecyclePublication::phase_from_context(
            semantic_lifecycle_publication_capability(),
            &mut context,
            SemanticOperationKind::AcceptPendingChannelInvitation,
            SemanticOperationPhase::WorkflowDispatched,
        )
        .into_fact();

        assert_eq!(
            fact,
            AuthoritativeSemanticFact::OperationStatus {
                operation_id: OperationId::invitation_accept(),
                instance_id: Some(OperationInstanceId(
                    "tui-op-invitation_accept-3".to_string()
                )),
                causality: Some(SemanticOperationCausality::new(
                    OwnerEpoch::new(7),
                    PublicationSequence::new(11),
                )),
                status: SemanticOperationStatus::new(
                    SemanticOperationKind::AcceptPendingChannelInvitation,
                    SemanticOperationPhase::WorkflowDispatched,
                ),
            }
        );
    }
}
