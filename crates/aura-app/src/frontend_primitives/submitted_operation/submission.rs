use super::types::{
    CeremonyMonitorHandoffRelease, CeremonySubmissionTerminalOutcome, SubmittedOperationPublisher,
    SubmittedOperationRelease, WorkflowHandoffRelease,
};
use crate::ui_contract::{
    OperationId, OperationInstanceId, SemanticOperationCausality, SemanticOperationError,
    SemanticOperationKind, SemanticOperationPhase, SemanticOperationStatus,
};

pub struct LocalTerminalSubmission<P: SubmittedOperationPublisher>(SubmittedOperation<P>);

pub struct WorkflowHandoffSubmission<P: SubmittedOperationPublisher>(SubmittedOperation<P>);

pub struct CeremonyMonitorHandoffSubmission<P: SubmittedOperationPublisher>(SubmittedOperation<P>);

pub struct SubmittedOperation<P: SubmittedOperationPublisher> {
    publisher: P,
    operation_id: OperationId,
    instance_id: OperationInstanceId,
    kind: SemanticOperationKind,
    settled: bool,
}

impl<P> SubmittedOperation<P>
where
    P: SubmittedOperationPublisher,
{
    pub async fn submit(
        publisher: P,
        operation_id: OperationId,
        kind: SemanticOperationKind,
        allocate_instance: impl FnOnce(&OperationId) -> OperationInstanceId,
    ) -> Self {
        let instance_id = allocate_instance(&operation_id);
        Self::submit_with_instance(publisher, operation_id, kind, instance_id).await
    }

    pub async fn submit_with_instance(
        publisher: P,
        operation_id: OperationId,
        kind: SemanticOperationKind,
        instance_id: OperationInstanceId,
    ) -> Self {
        publisher
            .publish_dispatched(&operation_id, &instance_id, kind)
            .await;
        Self {
            publisher,
            operation_id,
            instance_id,
            kind,
            settled: false,
        }
    }

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

    pub async fn succeed(mut self, causality: Option<SemanticOperationCausality>) {
        self.settled = true;
        self.publisher
            .publish_terminal(
                &self.operation_id,
                &self.instance_id,
                causality,
                SemanticOperationStatus::new(self.kind, SemanticOperationPhase::Succeeded),
            )
            .await;
    }

    pub async fn fail(mut self, error: SemanticOperationError) {
        self.settled = true;
        self.publisher
            .publish_terminal(
                &self.operation_id,
                &self.instance_id,
                None,
                SemanticOperationStatus::failed(self.kind, error),
            )
            .await;
    }

    pub async fn cancel(mut self) {
        self.settled = true;
        self.publisher
            .publish_terminal(
                &self.operation_id,
                &self.instance_id,
                None,
                SemanticOperationStatus::cancelled(self.kind),
            )
            .await;
    }

    #[must_use]
    pub fn handoff(mut self) -> SubmittedOperationRelease {
        self.settled = true;
        SubmittedOperationRelease::new(
            self.operation_id.clone(),
            self.instance_id.clone(),
            self.kind,
        )
    }
}

impl<P> LocalTerminalSubmission<P>
where
    P: SubmittedOperationPublisher,
{
    pub async fn submit(
        publisher: P,
        operation_id: OperationId,
        kind: SemanticOperationKind,
        allocate_instance: impl FnOnce(&OperationId) -> OperationInstanceId,
    ) -> Self {
        Self(SubmittedOperation::submit(publisher, operation_id, kind, allocate_instance).await)
    }

    pub async fn submit_with_instance(
        publisher: P,
        operation_id: OperationId,
        kind: SemanticOperationKind,
        instance_id: OperationInstanceId,
    ) -> Self {
        Self(
            SubmittedOperation::submit_with_instance(publisher, operation_id, kind, instance_id)
                .await,
        )
    }

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

    pub async fn succeed(self, causality: Option<SemanticOperationCausality>) {
        self.0.succeed(causality).await;
    }

    pub async fn fail(self, error: SemanticOperationError) {
        self.0.fail(error).await;
    }

    pub async fn cancel(self) {
        self.0.cancel().await;
    }
}

impl<P> WorkflowHandoffSubmission<P>
where
    P: SubmittedOperationPublisher,
{
    pub async fn submit(
        publisher: P,
        operation_id: OperationId,
        kind: SemanticOperationKind,
        allocate_instance: impl FnOnce(&OperationId) -> OperationInstanceId,
    ) -> Self {
        Self(SubmittedOperation::submit(publisher, operation_id, kind, allocate_instance).await)
    }

    pub async fn submit_with_instance(
        publisher: P,
        operation_id: OperationId,
        kind: SemanticOperationKind,
        instance_id: OperationInstanceId,
    ) -> Self {
        Self(
            SubmittedOperation::submit_with_instance(publisher, operation_id, kind, instance_id)
                .await,
        )
    }

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

    pub async fn fail(self, error: SemanticOperationError) {
        self.0.fail(error).await;
    }

    #[must_use]
    pub fn handoff_to_workflow(self) -> WorkflowHandoffRelease {
        WorkflowHandoffRelease(self.0.handoff())
    }
}

impl<P> CeremonyMonitorHandoffSubmission<P>
where
    P: SubmittedOperationPublisher,
{
    pub async fn submit(
        publisher: P,
        operation_id: OperationId,
        kind: SemanticOperationKind,
        allocate_instance: impl FnOnce(&OperationId) -> OperationInstanceId,
    ) -> Self {
        Self(SubmittedOperation::submit(publisher, operation_id, kind, allocate_instance).await)
    }

    pub async fn submit_with_instance(
        publisher: P,
        operation_id: OperationId,
        kind: SemanticOperationKind,
        instance_id: OperationInstanceId,
    ) -> Self {
        Self(
            SubmittedOperation::submit_with_instance(publisher, operation_id, kind, instance_id)
                .await,
        )
    }

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

    pub async fn fail(self, error: SemanticOperationError) {
        self.0.fail(error).await;
    }

    pub async fn monitor_started(
        self,
        causality: Option<SemanticOperationCausality>,
    ) -> CeremonySubmissionTerminalOutcome {
        self.0.succeed(causality).await;
        CeremonySubmissionTerminalOutcome::MonitorStarted
    }

    pub async fn cancel(self) -> CeremonySubmissionTerminalOutcome {
        self.0.cancel().await;
        CeremonySubmissionTerminalOutcome::Cancelled
    }

    #[must_use]
    pub fn handoff_to_monitor(self) -> CeremonyMonitorHandoffRelease {
        CeremonyMonitorHandoffRelease(self.0.handoff())
    }
}

impl<P> Drop for SubmittedOperation<P>
where
    P: SubmittedOperationPublisher,
{
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        self.publisher
            .publish_drop_failure(&self.operation_id, &self.instance_id, self.kind);
    }
}
