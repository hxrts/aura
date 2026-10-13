use super::types::{
    CeremonyMonitorHandoffRelease, CeremonySubmissionTerminalOutcome, SubmittedOperationPublisher,
    SubmittedOperationRelease, SubmittedTerminalCustody, SubmittedTerminalIntent,
    SubmittedTerminalObservation, WorkflowHandoffRelease,
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
    pub fn submit_unpublished(
        publisher: P,
        operation_id: OperationId,
        kind: SemanticOperationKind,
        allocate_instance: impl FnOnce(&OperationId) -> OperationInstanceId,
    ) -> Self {
        let instance_id = allocate_instance(&operation_id);
        Self::submit_with_instance_unpublished(publisher, operation_id, kind, instance_id)
    }

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
        // Establish the original owner before awaiting publication. Cancelling
        // that await must dispose this exact submission, not lose its identity.
        let submission =
            Self::submit_with_instance_unpublished(publisher, operation_id, kind, instance_id);
        submission
            .publisher
            .publish_dispatched(
                &submission.operation_id,
                &submission.instance_id,
                submission.kind,
            )
            .await;
        submission
    }

    pub fn submit_with_instance_unpublished(
        publisher: P,
        operation_id: OperationId,
        kind: SemanticOperationKind,
        instance_id: OperationInstanceId,
    ) -> Self {
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

    fn terminal(
        mut self,
        causality: Option<SemanticOperationCausality>,
        status: SemanticOperationStatus,
        retained: Option<SubmittedTerminalCustody>,
    ) -> SubmittedTerminalObservation {
        let observation = self.publisher.publish_terminal(SubmittedTerminalIntent {
            operation_id: self.operation_id.clone(),
            instance_id: self.instance_id.clone(),
            causality,
            status,
            retained,
        });
        // The publisher now owns the original intent; observation cancellation
        // cannot take terminal ownership back from that registration.
        self.settled = true;
        observation
    }

    pub fn succeed(
        self,
        causality: Option<SemanticOperationCausality>,
    ) -> SubmittedTerminalObservation {
        let status = SemanticOperationStatus::new(self.kind, SemanticOperationPhase::Succeeded);
        self.terminal(causality, status, None)
    }

    pub fn fail(self, error: SemanticOperationError) -> SubmittedTerminalObservation {
        self.fail_with_custody(error, None)
    }

    pub fn fail_with_custody(
        self,
        error: SemanticOperationError,
        retained: Option<SubmittedTerminalCustody>,
    ) -> SubmittedTerminalObservation {
        let status = SemanticOperationStatus::failed(self.kind, error);
        self.terminal(None, status, retained)
    }

    pub fn cancel(self) -> SubmittedTerminalObservation {
        let status = SemanticOperationStatus::cancelled(self.kind);
        self.terminal(None, status, None)
    }

    #[must_use]
    pub fn handoff(mut self) -> SubmittedOperationRelease {
        self.publisher
            .relinquish(&self.operation_id, &self.instance_id, self.kind);
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
    pub fn submit_unpublished(
        publisher: P,
        operation_id: OperationId,
        kind: SemanticOperationKind,
        allocate_instance: impl FnOnce(&OperationId) -> OperationInstanceId,
    ) -> Self {
        Self(SubmittedOperation::submit_unpublished(
            publisher,
            operation_id,
            kind,
            allocate_instance,
        ))
    }

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

    pub fn submit_with_instance_unpublished(
        publisher: P,
        operation_id: OperationId,
        kind: SemanticOperationKind,
        instance_id: OperationInstanceId,
    ) -> Self {
        Self(SubmittedOperation::submit_with_instance_unpublished(
            publisher,
            operation_id,
            kind,
            instance_id,
        ))
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

    pub fn succeed(
        self,
        causality: Option<SemanticOperationCausality>,
    ) -> SubmittedTerminalObservation {
        self.0.succeed(causality)
    }

    pub fn fail(self, error: SemanticOperationError) -> SubmittedTerminalObservation {
        self.0.fail(error)
    }

    pub fn fail_with_custody(
        self,
        error: SemanticOperationError,
        retained: Option<SubmittedTerminalCustody>,
    ) -> SubmittedTerminalObservation {
        self.0.fail_with_custody(error, retained)
    }

    pub fn cancel(self) -> SubmittedTerminalObservation {
        self.0.cancel()
    }
}

impl<P> WorkflowHandoffSubmission<P>
where
    P: SubmittedOperationPublisher,
{
    pub fn submit_unpublished(
        publisher: P,
        operation_id: OperationId,
        kind: SemanticOperationKind,
        allocate_instance: impl FnOnce(&OperationId) -> OperationInstanceId,
    ) -> Self {
        Self(SubmittedOperation::submit_unpublished(
            publisher,
            operation_id,
            kind,
            allocate_instance,
        ))
    }

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

    pub fn submit_with_instance_unpublished(
        publisher: P,
        operation_id: OperationId,
        kind: SemanticOperationKind,
        instance_id: OperationInstanceId,
    ) -> Self {
        Self(SubmittedOperation::submit_with_instance_unpublished(
            publisher,
            operation_id,
            kind,
            instance_id,
        ))
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

    pub fn fail(self, error: SemanticOperationError) -> SubmittedTerminalObservation {
        self.0.fail(error)
    }

    pub fn fail_with_custody(
        self,
        error: SemanticOperationError,
        retained: Option<SubmittedTerminalCustody>,
    ) -> SubmittedTerminalObservation {
        self.0.fail_with_custody(error, retained)
    }

    #[must_use]
    pub fn handoff_to_workflow(self) -> WorkflowHandoffRelease {
        WorkflowHandoffRelease(self.0.handoff())
    }

    /// Transfer terminal responsibility only after synchronous owner acceptance.
    ///
    /// Refusal returns the original unsettled submission and concrete cause.
    /// The recipient must accept responsibility atomically: after acceptance,
    /// notification failure cannot be reported as refusal of the handoff.
    pub fn try_handoff_to_workflow<E>(
        mut self,
        accept: impl FnOnce(&WorkflowHandoffRelease) -> Result<(), E>,
    ) -> Result<WorkflowHandoffRelease, (Self, E)> {
        let release = WorkflowHandoffRelease(SubmittedOperationRelease::new(
            self.0.operation_id.clone(),
            self.0.instance_id.clone(),
            self.0.kind,
        ));
        match accept(&release) {
            Ok(()) => {
                self.0
                    .publisher
                    .relinquish(&self.0.operation_id, &self.0.instance_id, self.0.kind);
                self.0.settled = true;
                Ok(release)
            }
            Err(source) => Err((self, source)),
        }
    }
}

impl<P> CeremonyMonitorHandoffSubmission<P>
where
    P: SubmittedOperationPublisher,
{
    pub fn submit_unpublished(
        publisher: P,
        operation_id: OperationId,
        kind: SemanticOperationKind,
        allocate_instance: impl FnOnce(&OperationId) -> OperationInstanceId,
    ) -> Self {
        Self(SubmittedOperation::submit_unpublished(
            publisher,
            operation_id,
            kind,
            allocate_instance,
        ))
    }

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

    pub fn submit_with_instance_unpublished(
        publisher: P,
        operation_id: OperationId,
        kind: SemanticOperationKind,
        instance_id: OperationInstanceId,
    ) -> Self {
        Self(SubmittedOperation::submit_with_instance_unpublished(
            publisher,
            operation_id,
            kind,
            instance_id,
        ))
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

    pub fn fail(self, error: SemanticOperationError) -> SubmittedTerminalObservation {
        self.0.fail(error)
    }

    pub fn fail_with_custody(
        self,
        error: SemanticOperationError,
        retained: Option<SubmittedTerminalCustody>,
    ) -> SubmittedTerminalObservation {
        self.0.fail_with_custody(error, retained)
    }

    pub fn monitor_started(
        self,
        causality: Option<SemanticOperationCausality>,
    ) -> impl std::future::Future<
        Output = Result<CeremonySubmissionTerminalOutcome, aura_core::AuraError>,
    > + Send {
        let publication = self.0.succeed(causality);
        async move {
            publication.await?;
            Ok(CeremonySubmissionTerminalOutcome::MonitorStarted)
        }
    }

    pub fn cancel(
        self,
    ) -> impl std::future::Future<
        Output = Result<CeremonySubmissionTerminalOutcome, aura_core::AuraError>,
    > + Send {
        let publication = self.0.cancel();
        async move {
            publication.await?;
            Ok(CeremonySubmissionTerminalOutcome::Cancelled)
        }
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
        drop(self.publisher.publish_terminal(SubmittedTerminalIntent {
            operation_id: self.operation_id.clone(),
            instance_id: self.instance_id.clone(),
            causality: None,
            status: super::types::dropped_owner_error(self.kind),
            retained: None,
        }));
    }
}
