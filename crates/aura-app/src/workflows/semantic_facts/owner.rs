use std::sync::{Arc, LazyLock};

use async_lock::{Mutex, RwLock};
use aura_core::{
    issue_operation_context, AuraError, AuthorizedReadinessPublication,
    LifecyclePublicationCapability, OperationContext, OperationContextCapability,
    OperationTimeoutBudget, OwnedShutdownToken, OwnerEpoch, PostconditionProofCapability,
    PublicationSequence, ReadinessPublicationCapability, SemanticSuccessProof, TraceContext,
};

use crate::ui_contract::{
    AuthoritativeSemanticFact, OperationId, OperationInstanceId, SemanticOperationCausality,
    SemanticOperationError, SemanticOperationKind, SemanticOperationPhase, SemanticOperationStatus,
    WorkflowTerminalStatus,
};
use crate::AppCore;

use super::{
    operation_phase_fact, publication::publish_authoritative_semantic_fact,
    publish_authoritative_operation_failure_with_instance,
    publish_authoritative_operation_phase_with_instance, publish_exact_operation_lifecycle,
    ExactOperationLifecyclePublication,
};

pub(in crate::workflows) type SemanticOperationContext =
    OperationContext<OperationId, OperationInstanceId, TraceContext>;

pub(super) struct AuthoritativeSemanticFactsUpdateGate {
    gate: Mutex<()>,
}

impl AuthoritativeSemanticFactsUpdateGate {
    const fn new() -> Self {
        Self {
            gate: Mutex::new(()),
        }
    }

    pub(super) async fn lock(&self) -> async_lock::MutexGuard<'_, ()> {
        self.gate.lock().await
    }
}

pub(super) static AUTHORITATIVE_SEMANTIC_FACTS_UPDATE_GATE: LazyLock<
    AuthoritativeSemanticFactsUpdateGate,
> = LazyLock::new(AuthoritativeSemanticFactsUpdateGate::new);
pub(super) static SEMANTIC_LIFECYCLE_PUBLICATION_CAPABILITY: LazyLock<
    LifecyclePublicationCapability,
> = LazyLock::new(|| LifecyclePublicationCapability::new("aura-app:semantic-lifecycle"));
pub(super) static SEMANTIC_OPERATION_CONTEXT_CAPABILITY: LazyLock<OperationContextCapability> =
    LazyLock::new(|| OperationContextCapability::new("aura-app:semantic-operation-context"));
static SEMANTIC_READINESS_PUBLICATION_CAPABILITY: LazyLock<ReadinessPublicationCapability> =
    LazyLock::new(|| ReadinessPublicationCapability::new("aura-app:semantic-readiness"));
static SEMANTIC_POSTCONDITION_PROOF_CAPABILITY: LazyLock<PostconditionProofCapability> =
    LazyLock::new(|| PostconditionProofCapability::new("aura-app:semantic-postcondition-proof"));

/// Return the sanctioned lifecycle-publication capability for shared semantic
/// operation status publication.
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "semantic_lifecycle",
    family = "capability_accessor"
)]
pub(in crate::workflows) fn semantic_lifecycle_publication_capability(
) -> &'static LifecyclePublicationCapability {
    &SEMANTIC_LIFECYCLE_PUBLICATION_CAPABILITY
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "semantic_readiness",
    family = "capability_accessor"
)]
pub(in crate::workflows) fn semantic_readiness_publication_capability(
) -> &'static ReadinessPublicationCapability {
    &SEMANTIC_READINESS_PUBLICATION_CAPABILITY
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "semantic_postcondition_proof",
    family = "capability_accessor"
)]
pub(in crate::workflows) fn semantic_postcondition_proof_capability(
) -> &'static PostconditionProofCapability {
    &SEMANTIC_POSTCONDITION_PROOF_CAPABILITY
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "semantic_readiness",
    family = "authorizer"
)]
pub(super) fn authorize_readiness_publication(
    fact: AuthoritativeSemanticFact,
) -> AuthorizedReadinessPublication<AuthoritativeSemanticFact> {
    AuthorizedReadinessPublication::authorize(semantic_readiness_publication_capability(), fact)
}

pub(in crate::workflows) struct SemanticWorkflowOwner {
    app_core: Arc<RwLock<AppCore>>,
    operation_id: OperationId,
    instance_id: Option<OperationInstanceId>,
    kind: SemanticOperationKind,
    publication_state: Mutex<Option<SemanticOperationContext>>,
    last_terminal_status: Mutex<Option<WorkflowTerminalStatus>>,
}

impl SemanticWorkflowOwner {
    pub(in crate::workflows) fn new(
        app_core: &Arc<RwLock<AppCore>>,
        operation_id: OperationId,
        instance_id: Option<OperationInstanceId>,
        kind: SemanticOperationKind,
    ) -> Self {
        let publication_state =
            issue_semantic_operation_context(operation_id.clone(), instance_id.clone());
        Self {
            app_core: app_core.clone(),
            operation_id,
            instance_id,
            kind,
            publication_state: Mutex::new(publication_state),
            last_terminal_status: Mutex::new(None),
        }
    }

    pub(in crate::workflows) fn kind(&self) -> SemanticOperationKind {
        self.kind
    }

    async fn record_terminal_status(
        &self,
        causality: Option<SemanticOperationCausality>,
        status: SemanticOperationStatus,
    ) {
        *self.last_terminal_status.lock().await =
            Some(WorkflowTerminalStatus { causality, status });
    }

    pub(in crate::workflows) async fn terminal_status(&self) -> Option<WorkflowTerminalStatus> {
        self.last_terminal_status.lock().await.clone()
    }

    pub(in crate::workflows) async fn publish_phase(
        &self,
        phase: SemanticOperationPhase,
    ) -> Result<(), AuraError> {
        let publication = {
            let mut state = self.publication_state.lock().await;
            match phase {
                SemanticOperationPhase::Succeeded => state.take().map(|context| {
                    ExactOperationLifecyclePublication::success_from_context(
                        semantic_lifecycle_publication_capability(),
                        context,
                        self.kind,
                    )
                }),
                SemanticOperationPhase::Cancelled => state.take().map(|context| {
                    ExactOperationLifecyclePublication::cancelled_from_context(
                        semantic_lifecycle_publication_capability(),
                        context,
                        self.kind,
                    )
                }),
                _ => state.as_mut().map(|context| {
                    ExactOperationLifecyclePublication::phase_from_context(
                        semantic_lifecycle_publication_capability(),
                        context,
                        self.kind,
                        phase,
                    )
                }),
            }
        };
        let terminal_status = matches!(
            phase,
            SemanticOperationPhase::Succeeded | SemanticOperationPhase::Cancelled
        )
        .then(|| WorkflowTerminalStatus {
            causality: publication
                .as_ref()
                .and_then(ExactOperationLifecyclePublication::causality),
            status: if phase == SemanticOperationPhase::Cancelled {
                SemanticOperationStatus::cancelled(self.kind)
            } else {
                SemanticOperationStatus::new(self.kind, phase)
            },
        });
        if let Some(publication) = publication {
            publish_exact_operation_lifecycle(&self.app_core, publication).await?;
        } else {
            publish_authoritative_operation_phase_with_instance(
                &self.app_core,
                semantic_lifecycle_publication_capability(),
                self.operation_id.clone(),
                self.instance_id.clone(),
                self.kind,
                phase,
            )
            .await?;
        }

        if let Some(status) = terminal_status {
            self.record_terminal_status(status.causality, status.status)
                .await;
        }
        Ok(())
    }

    pub(in crate::workflows) async fn publish_success_with<Proof>(
        &self,
        proof: Proof,
    ) -> Result<(), AuraError>
    where
        Proof: SemanticSuccessProof,
    {
        let _postcondition = proof.declared_postcondition();
        let publication = {
            let mut state = self.publication_state.lock().await;
            state.take().map(|context| {
                ExactOperationLifecyclePublication::success_from_context(
                    semantic_lifecycle_publication_capability(),
                    context,
                    self.kind,
                )
            })
        };
        let terminal_status = WorkflowTerminalStatus {
            causality: publication
                .as_ref()
                .and_then(ExactOperationLifecyclePublication::causality),
            status: SemanticOperationStatus::new(self.kind, SemanticOperationPhase::Succeeded),
        };
        if let Some(publication) = publication {
            publish_exact_operation_lifecycle(&self.app_core, publication).await?;
        } else {
            let success_fact = operation_phase_fact(
                self.operation_id.clone(),
                self.instance_id.clone(),
                self.kind,
                SemanticOperationPhase::Succeeded,
            );
            publish_authoritative_semantic_fact(
                &self.app_core,
                authorize_readiness_publication(success_fact),
            )
            .await?;
        }
        self.record_terminal_status(terminal_status.causality, terminal_status.status)
            .await;
        Ok(())
    }

    pub(in crate::workflows) async fn publish_failure(
        &self,
        error: SemanticOperationError,
    ) -> Result<(), AuraError> {
        let publication = {
            let mut state = self.publication_state.lock().await;
            state.take().map(|context| {
                ExactOperationLifecyclePublication::failure_from_context(
                    semantic_lifecycle_publication_capability(),
                    context,
                    self.kind,
                    error.clone(),
                )
            })
        };
        let terminal_status = WorkflowTerminalStatus {
            causality: publication
                .as_ref()
                .and_then(ExactOperationLifecyclePublication::causality),
            status: SemanticOperationStatus::failed(self.kind, error.clone()),
        };
        if let Some(publication) = publication {
            publish_exact_operation_lifecycle(&self.app_core, publication).await?;
        } else {
            publish_authoritative_operation_failure_with_instance(
                &self.app_core,
                semantic_lifecycle_publication_capability(),
                self.operation_id.clone(),
                self.instance_id.clone(),
                self.kind,
                error,
            )
            .await?;
        }
        self.record_terminal_status(terminal_status.causality, terminal_status.status)
            .await;
        Ok(())
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(in crate::workflows) async fn terminal_success_fact(
        &self,
    ) -> Result<AuthoritativeSemanticFact, AuraError> {
        let publication = {
            let mut state = self.publication_state.lock().await;
            state.take().map(|context| {
                ExactOperationLifecyclePublication::success_from_context(
                    semantic_lifecycle_publication_capability(),
                    context,
                    self.kind,
                )
            })
        };
        let (causality, fact) = match publication {
            Some(publication) => {
                let causality = publication.causality();
                (causality, publication.into_fact())
            }
            None => (
                None,
                operation_phase_fact(
                    self.operation_id.clone(),
                    self.instance_id.clone(),
                    self.kind,
                    SemanticOperationPhase::Succeeded,
                ),
            ),
        };
        self.record_terminal_status(
            causality,
            SemanticOperationStatus::new(self.kind, SemanticOperationPhase::Succeeded),
        )
        .await;
        Ok(fact)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(in crate::workflows) async fn terminal_success_fact_with<Proof>(
        &self,
        proof: Proof,
    ) -> Result<AuthoritativeSemanticFact, AuraError>
    where
        Proof: SemanticSuccessProof,
    {
        let _postcondition = proof.declared_postcondition();
        self.terminal_success_fact().await
    }
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "semantic_operation_context",
    family = "runtime_helper"
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
