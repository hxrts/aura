use crate::ui_contract::{
    OperationId, OperationInstanceId, SemanticFailureCode, SemanticFailureDomain,
    SemanticOperationCausality, SemanticOperationError, SemanticOperationKind,
    SemanticOperationStatus, WorkflowTerminalOutcome,
};
use async_trait::async_trait;
use futures::FutureExt;
use std::future::Future;
use std::panic::AssertUnwindSafe;

#[async_trait]
pub trait SubmittedOperationPublisher: Clone + Send + Sync + 'static {
    async fn publish_dispatched(
        &self,
        operation_id: &OperationId,
        instance_id: &OperationInstanceId,
        kind: SemanticOperationKind,
    );

    async fn publish_terminal(
        &self,
        operation_id: &OperationId,
        instance_id: &OperationInstanceId,
        causality: Option<SemanticOperationCausality>,
        status: SemanticOperationStatus,
    );

    fn publish_drop_failure(
        &self,
        operation_id: &OperationId,
        instance_id: &OperationInstanceId,
        kind: SemanticOperationKind,
    );
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmittedOperationRelease {
    pub(crate) operation_id: OperationId,
    pub(crate) instance_id: OperationInstanceId,
    pub(crate) kind: SemanticOperationKind,
}

#[derive(Debug)]
pub enum SubmittedOperationWorkflowError {
    Workflow(aura_core::AuraError),
    Protocol(String),
    Panicked(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CeremonySubmissionTerminalOutcome {
    MonitorStarted,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowHandoffRelease(pub(crate) SubmittedOperationRelease);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CeremonyMonitorHandoffRelease(pub(crate) SubmittedOperationRelease);

impl std::fmt::Display for SubmittedOperationWorkflowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Workflow(error) => write!(f, "{error}"),
            Self::Protocol(detail) | Self::Panicked(detail) => write!(f, "{detail}"),
        }
    }
}

impl std::error::Error for SubmittedOperationWorkflowError {}

impl SubmittedOperationRelease {
    #[must_use]
    pub fn operation_id(&self) -> &OperationId {
        &self.operation_id
    }

    #[must_use]
    pub fn instance_id(&self) -> &OperationInstanceId {
        &self.instance_id
    }

    #[must_use]
    pub const fn kind(&self) -> SemanticOperationKind {
        self.kind
    }

    pub(crate) fn new(
        operation_id: OperationId,
        instance_id: OperationInstanceId,
        kind: SemanticOperationKind,
    ) -> Self {
        Self {
            operation_id,
            instance_id,
            kind,
        }
    }

    pub async fn run_workflow<T, Fut>(
        &self,
        panic_context: &'static str,
        workflow: Fut,
    ) -> Result<WorkflowTerminalOutcome<T>, String>
    where
        Fut: Future<Output = WorkflowTerminalOutcome<T>>,
    {
        match AssertUnwindSafe(workflow).catch_unwind().await {
            Ok(outcome) => Ok(outcome),
            Err(panic) => Err(panic_detail(panic_context, panic.as_ref())),
        }
    }
}

impl WorkflowHandoffRelease {
    #[must_use]
    pub fn operation_id(&self) -> &OperationId {
        self.0.operation_id()
    }

    #[must_use]
    pub fn instance_id(&self) -> &OperationInstanceId {
        self.0.instance_id()
    }

    #[must_use]
    pub const fn kind(&self) -> SemanticOperationKind {
        self.0.kind()
    }

    pub async fn run_workflow<T, Fut>(
        &self,
        panic_context: &'static str,
        workflow: Fut,
    ) -> Result<WorkflowTerminalOutcome<T>, String>
    where
        Fut: Future<Output = WorkflowTerminalOutcome<T>>,
    {
        self.0.run_workflow(panic_context, workflow).await
    }
}

impl CeremonyMonitorHandoffRelease {
    #[must_use]
    pub fn operation_id(&self) -> &OperationId {
        self.0.operation_id()
    }

    #[must_use]
    pub fn instance_id(&self) -> &OperationInstanceId {
        self.0.instance_id()
    }

    #[must_use]
    pub const fn kind(&self) -> SemanticOperationKind {
        self.0.kind()
    }
}

#[must_use]
pub fn dropped_owner_error(kind: SemanticOperationKind) -> SemanticOperationStatus {
    SemanticOperationStatus::failed(
        kind,
        SemanticOperationError::new(
            SemanticFailureDomain::Command,
            SemanticFailureCode::InternalError,
        )
        .with_detail(
            "semantic operation owner dropped before terminal publication or explicit handoff",
        ),
    )
}

pub(crate) fn panic_detail(
    panic_context: &'static str,
    panic: &(dyn std::any::Any + Send),
) -> String {
    if let Some(message) = panic.downcast_ref::<&str>() {
        format!("{panic_context} panicked: {message}")
    } else if let Some(message) = panic.downcast_ref::<String>() {
        format!("{panic_context} panicked: {message}")
    } else {
        format!("{panic_context} panicked")
    }
}
