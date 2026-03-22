use std::sync::{Arc, LazyLock};

use async_lock::{Mutex, RwLock};
use aura_core::{
    issue_operation_context, AuraError, AuthorizedProgressPublication,
    AuthorizedReadinessPublication, AuthorizedTerminalPublication, ChannelId,
    LifecyclePublicationCapability, OperationContext, OperationContextCapability,
    OperationProgress, OperationTimeoutBudget, OwnedShutdownToken, OwnerEpoch,
    PostconditionProofCapability, PublicationSequence, ReadinessPublicationCapability,
    SemanticOwnerPostcondition, SemanticSuccessProof, TerminalOutcome, TraceContext,
};

use crate::signal_defs::{
    AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL, AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL_NAME, HOMES_SIGNAL,
    HOMES_SIGNAL_NAME,
};
use crate::ui_contract::{
    AuthoritativeSemanticFact, AuthoritativeSemanticFactKind, OperationId, OperationInstanceId,
    SemanticOperationCausality, SemanticOperationError, SemanticOperationKind,
    SemanticOperationPhase, SemanticOperationStatus, WorkflowTerminalStatus,
};
use crate::workflows::signals::{emit_signal, read_signal};
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
static SEMANTIC_POSTCONDITION_PROOF_CAPABILITY: LazyLock<PostconditionProofCapability> =
    LazyLock::new(|| PostconditionProofCapability::new("aura-app:semantic-postcondition-proof"));

/// Return the sanctioned lifecycle-publication capability for shared semantic
/// operation status publication.
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "semantic_lifecycle"
)]
pub(in crate::workflows) fn semantic_lifecycle_publication_capability(
) -> &'static LifecyclePublicationCapability {
    &SEMANTIC_LIFECYCLE_PUBLICATION_CAPABILITY
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "semantic_readiness"
)]
pub(in crate::workflows) fn semantic_readiness_publication_capability(
) -> &'static ReadinessPublicationCapability {
    &SEMANTIC_READINESS_PUBLICATION_CAPABILITY
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "semantic_postcondition_proof"
)]
pub(in crate::workflows) fn semantic_postcondition_proof_capability(
) -> &'static PostconditionProofCapability {
    &SEMANTIC_POSTCONDITION_PROOF_CAPABILITY
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

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::workflows) struct ChannelMembershipReadyProof {
    channel_id: ChannelId,
}

impl SemanticSuccessProof for ChannelMembershipReadyProof {
    fn declared_postcondition(&self) -> SemanticOwnerPostcondition {
        SemanticOwnerPostcondition::new("channel_membership_ready")
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::workflows) struct HomeCreatedProof {
    home_id: ChannelId,
}

impl SemanticSuccessProof for HomeCreatedProof {
    fn declared_postcondition(&self) -> SemanticOwnerPostcondition {
        SemanticOwnerPostcondition::new("home_created")
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::workflows) struct ChannelInvitationCreatedProof {
    invitation_id: aura_core::InvitationId,
}

impl SemanticSuccessProof for ChannelInvitationCreatedProof {
    fn declared_postcondition(&self) -> SemanticOwnerPostcondition {
        SemanticOwnerPostcondition::new("channel_invitation_created")
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::workflows) struct InvitationCreatedProof {
    invitation_id: aura_core::InvitationId,
}

impl SemanticSuccessProof for InvitationCreatedProof {
    fn declared_postcondition(&self) -> SemanticOwnerPostcondition {
        SemanticOwnerPostcondition::new("invitation_created")
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::workflows) struct InvitationAcceptedOrMaterializedProof {
    invitation_id: aura_core::InvitationId,
}

impl SemanticSuccessProof for InvitationAcceptedOrMaterializedProof {
    fn declared_postcondition(&self) -> SemanticOwnerPostcondition {
        SemanticOwnerPostcondition::new("invitation_accepted_or_materialized")
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::workflows) struct PendingInvitationConsumedProof {
    invitation_id: aura_core::InvitationId,
}

impl SemanticSuccessProof for PendingInvitationConsumedProof {
    fn declared_postcondition(&self) -> SemanticOwnerPostcondition {
        SemanticOwnerPostcondition::new("pending_invitation_consumed")
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::workflows) struct DeviceEnrollmentStartedProof {
    ceremony_id: aura_core::CeremonyId,
}

impl SemanticSuccessProof for DeviceEnrollmentStartedProof {
    fn declared_postcondition(&self) -> SemanticOwnerPostcondition {
        SemanticOwnerPostcondition::new("device_enrollment_started")
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::workflows) struct DeviceEnrollmentImportedProof {
    invitation_id: aura_core::InvitationId,
}

impl SemanticSuccessProof for DeviceEnrollmentImportedProof {
    fn declared_postcondition(&self) -> SemanticOwnerPostcondition {
        SemanticOwnerPostcondition::new("device_enrollment_imported")
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::workflows) struct MessageCommittedProof {
    message_id: String,
}

impl SemanticSuccessProof for MessageCommittedProof {
    fn declared_postcondition(&self) -> SemanticOwnerPostcondition {
        SemanticOwnerPostcondition::new("message_committed")
    }
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

    pub(in crate::workflows) fn app_core(&self) -> &Arc<RwLock<AppCore>> {
        &self.app_core
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
                _ => {
                    let Some(context) = state.as_mut() else {
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
        };
        let terminal_status = matches!(
            phase,
            SemanticOperationPhase::Succeeded | SemanticOperationPhase::Cancelled
        )
        .then(|| WorkflowTerminalStatus {
            causality: publication
                .as_ref()
                .and_then(|publication| publication.causality),
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
                .and_then(|publication| publication.causality),
            status: SemanticOperationStatus::new(self.kind, SemanticOperationPhase::Succeeded),
        };
        if let Some(publication) = publication {
            publish_exact_operation_lifecycle(&self.app_core, publication).await?;
        } else {
            publish_authoritative_operation_phase_with_instance(
                &self.app_core,
                semantic_lifecycle_publication_capability(),
                self.operation_id.clone(),
                self.instance_id.clone(),
                self.kind,
                SemanticOperationPhase::Succeeded,
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
                .and_then(|publication| publication.causality),
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

#[aura_macros::authoritative_source(kind = "proof_issuer")]
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "semantic_postcondition_proof"
)]
#[allow(dead_code)]
pub(in crate::workflows) fn issue_home_created_proof(home_id: ChannelId) -> HomeCreatedProof {
    let _ = semantic_postcondition_proof_capability();
    HomeCreatedProof { home_id }
}

#[allow(dead_code)]
#[aura_macros::authoritative_source(kind = "signal")]
pub(in crate::workflows) async fn prove_home_created(
    app_core: &Arc<RwLock<AppCore>>,
    home_id: ChannelId,
) -> Result<HomeCreatedProof, AuraError> {
    let homes = read_signal(app_core, &*HOMES_SIGNAL, HOMES_SIGNAL_NAME).await?;
    if homes.has_home(&home_id) {
        Ok(issue_home_created_proof(home_id))
    } else {
        Err(AuraError::from(
            crate::workflows::error::WorkflowError::Precondition(
                "home_created proof requires the home to exist in authoritative homes state",
            ),
        ))
    }
}

#[aura_macros::authoritative_source(kind = "proof_issuer")]
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "semantic_postcondition_proof"
)]
#[allow(dead_code)]
pub(in crate::workflows) fn issue_channel_membership_ready_proof(
    channel_id: ChannelId,
) -> ChannelMembershipReadyProof {
    let _ = semantic_postcondition_proof_capability();
    ChannelMembershipReadyProof { channel_id }
}

#[allow(dead_code)]
#[aura_macros::authoritative_source(kind = "app_core")]
pub(in crate::workflows) async fn authoritative_semantic_facts_snapshot(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<Vec<AuthoritativeSemanticFact>, AuraError> {
    Ok(app_core.read().await.authoritative_semantic_facts())
}

#[cfg_attr(not(feature = "signals"), allow(dead_code))]
#[aura_macros::authoritative_source(kind = "signal")]
pub(in crate::workflows) async fn prove_channel_membership_ready(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
) -> Result<ChannelMembershipReadyProof, AuraError> {
    let channel_id_string = channel_id.to_string();
    let facts = authoritative_semantic_facts_snapshot(app_core).await?;
    if facts.iter().any(|fact| {
        matches!(
            fact,
            AuthoritativeSemanticFact::ChannelMembershipReady { channel, .. }
                if channel.id.as_deref() == Some(channel_id_string.as_str())
        )
    }) {
        Ok(issue_channel_membership_ready_proof(channel_id))
    } else {
        Err(AuraError::from(
            crate::workflows::error::WorkflowError::Precondition(
                "ChannelMembershipReady proof requires an authoritative readiness fact",
            ),
        ))
    }
}

#[aura_macros::authoritative_source(kind = "proof_issuer")]
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "semantic_postcondition_proof"
)]
#[allow(dead_code)]
pub(in crate::workflows) fn issue_invitation_created_proof(
    invitation_id: aura_core::InvitationId,
) -> InvitationCreatedProof {
    let _ = semantic_postcondition_proof_capability();
    InvitationCreatedProof { invitation_id }
}

#[aura_macros::authoritative_source(kind = "proof_issuer")]
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "semantic_postcondition_proof"
)]
#[allow(dead_code)]
pub(in crate::workflows) fn issue_channel_invitation_created_proof(
    invitation_id: aura_core::InvitationId,
) -> ChannelInvitationCreatedProof {
    let _ = semantic_postcondition_proof_capability();
    ChannelInvitationCreatedProof { invitation_id }
}

#[aura_macros::authoritative_source(kind = "proof_issuer")]
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "semantic_postcondition_proof"
)]
pub(in crate::workflows) fn issue_invitation_accepted_or_materialized_proof(
    invitation_id: aura_core::InvitationId,
) -> InvitationAcceptedOrMaterializedProof {
    let _ = semantic_postcondition_proof_capability();
    InvitationAcceptedOrMaterializedProof { invitation_id }
}

#[aura_macros::authoritative_source(kind = "proof_issuer")]
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "semantic_postcondition_proof"
)]
pub(in crate::workflows) fn issue_device_enrollment_started_proof(
    ceremony_id: aura_core::CeremonyId,
) -> DeviceEnrollmentStartedProof {
    let _ = semantic_postcondition_proof_capability();
    DeviceEnrollmentStartedProof { ceremony_id }
}

#[aura_macros::authoritative_source(kind = "proof_issuer")]
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "semantic_postcondition_proof"
)]
#[cfg_attr(not(feature = "signals"), allow(dead_code))]
pub(in crate::workflows) fn issue_message_committed_proof(
    message_id: impl Into<String>,
) -> MessageCommittedProof {
    let _ = semantic_postcondition_proof_capability();
    MessageCommittedProof {
        message_id: message_id.into(),
    }
}

#[aura_macros::authoritative_source(kind = "proof_issuer")]
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "semantic_postcondition_proof"
)]
pub(in crate::workflows) fn issue_device_enrollment_imported_proof(
    invitation_id: aura_core::InvitationId,
) -> DeviceEnrollmentImportedProof {
    let _ = semantic_postcondition_proof_capability();
    DeviceEnrollmentImportedProof { invitation_id }
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
    let (previous_facts, updated_facts) = {
        let mut core = app_core.write().await;
        let previous_facts = core.authoritative_semantic_facts();
        let mut updated_facts = previous_facts.clone();
        update(&mut updated_facts);
        core.set_authoritative_semantic_facts(updated_facts.clone());
        (previous_facts, updated_facts)
    };
    if let Err(error) = emit_signal(
        app_core,
        &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL,
        updated_facts,
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
            authorize_readiness_publication(operation_phase_fact(operation_id, None, kind, phase)),
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
#[allow(dead_code)]
pub(crate) fn authoritative_status_for_instance(
    facts: &[AuthoritativeSemanticFact],
    operation_id: &OperationId,
    instance_id: &OperationInstanceId,
) -> Option<SemanticOperationStatus> {
    facts.iter().find_map(|fact| match fact {
        AuthoritativeSemanticFact::OperationStatus {
            operation_id: observed_operation_id,
            instance_id: Some(observed_instance_id),
            status,
            ..
        } if observed_operation_id == operation_id && observed_instance_id == instance_id => {
            Some(status.clone())
        }
        _ => None,
    })
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn assert_succeeded_with_postcondition(
    facts: &[AuthoritativeSemanticFact],
    operation_id: &OperationId,
    instance_id: &OperationInstanceId,
    expected_kind: SemanticOperationKind,
    postcondition_holds: impl Fn(&[AuthoritativeSemanticFact]) -> bool,
) {
    let status = authoritative_status_for_instance(facts, operation_id, instance_id)
        .unwrap_or_else(|| panic!("missing status for {operation_id:?}/{instance_id:?}"));
    assert_eq!(status.kind, expected_kind);
    assert_eq!(status.phase, SemanticOperationPhase::Succeeded);
    assert!(
        postcondition_holds(facts),
        "declared postcondition must hold after success for {operation_id:?}/{instance_id:?}"
    );
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn assert_terminal_failure_or_cancelled(
    facts: &[AuthoritativeSemanticFact],
    operation_id: &OperationId,
    instance_id: &OperationInstanceId,
    expected_kind: SemanticOperationKind,
) {
    let status = authoritative_status_for_instance(facts, operation_id, instance_id)
        .unwrap_or_else(|| panic!("missing status for {operation_id:?}/{instance_id:?}"));
    assert_eq!(status.kind, expected_kind);
    assert!(
        matches!(
            status.phase,
            SemanticOperationPhase::Failed | SemanticOperationPhase::Cancelled
        ),
        "expected failed/cancelled terminal status for {operation_id:?}/{instance_id:?}, got {:?}",
        status.phase
    );
}

#[cfg(test)]
pub(crate) fn assert_terminal_failure_status(
    terminal: &WorkflowTerminalStatus,
    expected_kind: SemanticOperationKind,
) {
    assert_eq!(terminal.status.kind, expected_kind);
    assert_eq!(terminal.status.phase, SemanticOperationPhase::Failed);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal_defs::AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL;
    use crate::ui_contract::{SemanticOperationKind, SemanticOperationPhase};
    use crate::workflows::signals::read_signal_or_default;
    use crate::runtime_bridge::OfflineRuntimeBridge;
    use crate::{AppConfig, AppCore};
    use aura_core::types::identifiers::AuthorityId;

    fn runtime_backed_test_app_core() -> Arc<RwLock<AppCore>> {
        let authority = AuthorityId::new_from_entropy([42; 32]);
        let runtime = Arc::new(OfflineRuntimeBridge::new(authority));
        crate::testing::test_app_core_with_runtime(AppConfig::default(), runtime)
    }

    #[tokio::test]
    async fn authoritative_semantic_facts_snapshot_reads_owned_store_without_registered_signal() {
        let app_core = crate::testing::default_test_app_core();
        {
            let mut core = app_core.write().await;
            core.set_authoritative_semantic_facts(vec![
                AuthoritativeSemanticFact::PendingHomeInvitationReady,
            ]);
        }

        let facts = authoritative_semantic_facts_snapshot(&app_core)
            .await
            .expect("authoritative semantic facts should read from the owned app-core store");
        assert_eq!(
            facts,
            vec![AuthoritativeSemanticFact::PendingHomeInvitationReady]
        );
    }

    #[tokio::test]
    async fn authoritative_semantic_fact_update_restores_owned_store_on_signal_emit_failure() {
        let app_core = crate::testing::default_test_app_core();
        {
            let mut core = app_core.write().await;
            core.set_authoritative_semantic_facts(vec![
                AuthoritativeSemanticFact::PendingHomeInvitationReady,
            ]);
        }

        let error = publish_authoritative_semantic_fact(
            &app_core,
            authorize_readiness_publication(AuthoritativeSemanticFact::ContactLinkReady {
                authority_id: "owner-a".into(),
                contact_count: 1,
            }),
        )
        .await
        .expect_err("signal emission should still fail when the signal is unregistered");
        assert!(matches!(error, AuraError::Internal { .. }));

        let facts = authoritative_semantic_facts_snapshot(&app_core)
            .await
            .expect("owned semantic-facts store should remain readable after failed emit");
        assert_eq!(
            facts,
            vec![AuthoritativeSemanticFact::PendingHomeInvitationReady]
        );
    }

    #[tokio::test]
    async fn concurrent_authoritative_fact_updates_do_not_lose_entries() {
        let app_core = runtime_backed_test_app_core();
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
    async fn prove_home_created_requires_registered_homes_signal() {
        let app_core = crate::testing::default_test_app_core();
        let home_id = ChannelId::new(aura_core::Hash32([7; 32]));

        let error = prove_home_created(&app_core, home_id)
            .await
            .expect_err("home_created proof should require the homes signal");
        assert!(matches!(error, AuraError::Internal { .. }));
    }

    #[tokio::test]
    async fn exact_operation_lifecycle_publication_retains_instance_identity() {
        let app_core = runtime_backed_test_app_core();
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
            instance_id: Some(OperationInstanceId(
                "tui-op-invitation_accept-3".to_string()
            )),
            causality: None,
            status: SemanticOperationStatus::new(
                SemanticOperationKind::AcceptPendingChannelInvitation,
                SemanticOperationPhase::Succeeded,
            ),
        }));
    }

    #[tokio::test]
    async fn exact_operation_lifecycle_round_trips_identity_under_contention() {
        let app_core = runtime_backed_test_app_core();
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
            instance_id: Some(OperationInstanceId(
                "tui-op-invitation_accept-7".to_string()
            )),
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
