use crate::ui_contract::{
    OperationId, OperationInstanceId, SemanticFailureCode, SemanticFailureDomain,
    SemanticOperationCausality, SemanticOperationError, SemanticOperationKind,
    SemanticOperationPhase, SemanticOperationStatus, WorkflowTerminalOutcome,
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
    operation_id: OperationId,
    instance_id: OperationInstanceId,
    kind: SemanticOperationKind,
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

impl std::fmt::Display for SubmittedOperationWorkflowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Workflow(error) => write!(f, "{error}"),
            Self::Protocol(detail) | Self::Panicked(detail) => write!(f, "{detail}"),
        }
    }
}

impl std::error::Error for SubmittedOperationWorkflowError {}

pub struct LocalTerminalSubmission<P: SubmittedOperationPublisher>(SubmittedOperation<P>);

pub struct WorkflowHandoffSubmission<P: SubmittedOperationPublisher>(SubmittedOperation<P>);

pub struct CeremonyMonitorHandoffSubmission<P: SubmittedOperationPublisher>(SubmittedOperation<P>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowHandoffRelease(SubmittedOperationRelease);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CeremonyMonitorHandoffRelease(SubmittedOperationRelease);

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
        SubmittedOperationRelease {
            operation_id: self.operation_id.clone(),
            instance_id: self.instance_id.clone(),
            kind: self.kind,
        }
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

fn panic_detail(panic_context: &'static str, panic: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = panic.downcast_ref::<&str>() {
        format!("{panic_context} panicked: {message}")
    } else if let Some(message) = panic.downcast_ref::<String>() {
        format!("{panic_context} panicked: {message}")
    } else {
        format!("{panic_context} panicked")
    }
}

#[cfg(test)]
mod tests {
    use super::{
        dropped_owner_error, CeremonyMonitorHandoffSubmission, LocalTerminalSubmission,
        SubmittedOperation, SubmittedOperationPublisher, SubmittedOperationRelease,
        SubmittedOperationWorkflowError, WorkflowHandoffSubmission,
    };
    use crate::ui_contract::{
        OperationId, OperationInstanceId, SemanticOperationCausality, SemanticOperationKind,
        SemanticOperationPhase, SemanticOperationStatus, WorkflowTerminalOutcome,
        WorkflowTerminalStatus,
    };
    use async_trait::async_trait;
    use futures::executor::block_on;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum Event {
        Dispatched(OperationId, OperationInstanceId, SemanticOperationKind),
        Terminal(
            OperationId,
            OperationInstanceId,
            Option<SemanticOperationCausality>,
            SemanticOperationStatus,
        ),
        Dropped(OperationId, OperationInstanceId, SemanticOperationKind),
    }

    #[derive(Clone, Default)]
    struct TestPublisher {
        events: Arc<Mutex<Vec<Event>>>,
    }

    #[async_trait]
    impl SubmittedOperationPublisher for TestPublisher {
        async fn publish_dispatched(
            &self,
            operation_id: &OperationId,
            instance_id: &OperationInstanceId,
            kind: SemanticOperationKind,
        ) {
            self.events.lock().await.push(Event::Dispatched(
                operation_id.clone(),
                instance_id.clone(),
                kind,
            ));
        }

        async fn publish_terminal(
            &self,
            operation_id: &OperationId,
            instance_id: &OperationInstanceId,
            causality: Option<SemanticOperationCausality>,
            status: SemanticOperationStatus,
        ) {
            self.events.lock().await.push(Event::Terminal(
                operation_id.clone(),
                instance_id.clone(),
                causality,
                status,
            ));
        }

        fn publish_drop_failure(
            &self,
            operation_id: &OperationId,
            instance_id: &OperationInstanceId,
            kind: SemanticOperationKind,
        ) {
            block_on(async {
                self.events.lock().await.push(Event::Dropped(
                    operation_id.clone(),
                    instance_id.clone(),
                    kind,
                ));
            });
        }
    }

    fn events(publisher: &TestPublisher) -> Vec<Event> {
        block_on(async { publisher.events.lock().await.clone() })
    }

    #[test]
    fn release_run_workflow_returns_outcome_without_rewrapping() {
        let release = SubmittedOperationRelease {
            operation_id: OperationId::invitation_create(),
            instance_id: OperationInstanceId("ui-op-1".to_string()),
            kind: SemanticOperationKind::CreateContactInvitation,
        };

        let outcome = block_on(release.run_workflow("create_invitation", async {
            WorkflowTerminalOutcome {
                result: Ok("ok".to_string()),
                terminal: Some(WorkflowTerminalStatus {
                    causality: None,
                    status: SemanticOperationStatus::new(
                        SemanticOperationKind::CreateContactInvitation,
                        SemanticOperationPhase::Succeeded,
                    ),
                }),
            }
        }))
        .unwrap_or_else(|detail| panic!("unexpected panic detail: {detail}"));

        assert_eq!(
            outcome.result.unwrap_or_else(|error| panic!("{error}")),
            "ok"
        );
        assert_eq!(
            outcome
                .terminal
                .unwrap_or_else(|| panic!("missing terminal"))
                .status
                .phase,
            SemanticOperationPhase::Succeeded
        );
    }

    #[test]
    fn release_run_workflow_reports_panics_with_context() {
        let release = SubmittedOperationRelease {
            operation_id: OperationId::invitation_accept_contact(),
            instance_id: OperationInstanceId("ui-op-2".to_string()),
            kind: SemanticOperationKind::AcceptContactInvitation,
        };

        let detail = block_on(release.run_workflow::<(), _>("accept_invitation", async {
            panic!("boom");
        }))
        .expect_err("expected panic detail");

        assert_eq!(detail, "accept_invitation panicked: boom");
    }

    #[test]
    fn submitted_operation_workflow_error_preserves_message_shapes() {
        let protocol = SubmittedOperationWorkflowError::Protocol("protocol failed".to_string());
        let panic = SubmittedOperationWorkflowError::Panicked("workflow panicked".to_string());

        match protocol {
            SubmittedOperationWorkflowError::Protocol(detail) => {
                assert_eq!(detail, "protocol failed");
            }
            other => panic!("unexpected variant: {other:?}"),
        }

        match panic {
            SubmittedOperationWorkflowError::Panicked(detail) => {
                assert_eq!(detail, "workflow panicked");
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn submit_publishes_dispatched_and_success() {
        let publisher = TestPublisher::default();
        let operation = block_on(SubmittedOperation::submit(
            publisher.clone(),
            OperationId::create_home(),
            SemanticOperationKind::CreateHome,
            |_| OperationInstanceId("test-op-1".to_string()),
        ));

        block_on(operation.succeed(None));

        let observed = events(&publisher);
        assert_eq!(
            observed[0],
            Event::Dispatched(
                OperationId::create_home(),
                OperationInstanceId("test-op-1".to_string()),
                SemanticOperationKind::CreateHome,
            )
        );
        match &observed[1] {
            Event::Terminal(operation_id, instance_id, causality, status) => {
                assert_eq!(operation_id, &OperationId::create_home());
                assert_eq!(instance_id, &OperationInstanceId("test-op-1".to_string()));
                assert_eq!(causality, &None);
                assert_eq!(status.phase, SemanticOperationPhase::Succeeded);
            }
            other => panic!("unexpected terminal event: {other:?}"),
        }
    }

    #[test]
    fn handoff_marks_operation_settled_and_preserves_identity() {
        let publisher = TestPublisher::default();
        let operation = block_on(SubmittedOperation::submit(
            publisher.clone(),
            OperationId::send_message(),
            SemanticOperationKind::SendChatMessage,
            |_| OperationInstanceId("test-op-2".to_string()),
        ));

        let release: SubmittedOperationRelease = operation.handoff();
        assert_eq!(release.operation_id(), &OperationId::send_message());
        assert_eq!(
            release.instance_id(),
            &OperationInstanceId("test-op-2".to_string())
        );
        assert_eq!(release.kind(), SemanticOperationKind::SendChatMessage);

        let observed = events(&publisher);
        assert_eq!(observed.len(), 1);
    }

    #[test]
    fn explicit_owner_models_expose_distinct_handoff_shapes() {
        let publisher = TestPublisher::default();
        let workflow = block_on(WorkflowHandoffSubmission::submit(
            publisher.clone(),
            OperationId::invitation_accept_contact(),
            SemanticOperationKind::AcceptContactInvitation,
            |_| OperationInstanceId("test-op-4".to_string()),
        ));
        let ceremony = block_on(CeremonyMonitorHandoffSubmission::submit(
            publisher,
            OperationId::device_enrollment(),
            SemanticOperationKind::StartDeviceEnrollment,
            |_| OperationInstanceId("test-op-5".to_string()),
        ));

        let workflow_release = workflow.handoff_to_workflow();
        let ceremony_release = ceremony.handoff_to_monitor();

        assert_eq!(
            workflow_release.operation_id(),
            &OperationId::invitation_accept_contact()
        );
        assert_eq!(
            workflow_release.kind(),
            SemanticOperationKind::AcceptContactInvitation
        );
        assert_eq!(
            ceremony_release.operation_id(),
            &OperationId::device_enrollment()
        );
        assert_eq!(
            ceremony_release.kind(),
            SemanticOperationKind::StartDeviceEnrollment
        );
    }

    #[test]
    fn local_terminal_submission_delegates_terminal_publication() {
        let publisher = TestPublisher::default();
        let operation = block_on(LocalTerminalSubmission::submit(
            publisher.clone(),
            OperationId::create_channel(),
            SemanticOperationKind::CreateChannel,
            |_| OperationInstanceId("test-op-6".to_string()),
        ));
        block_on(operation.succeed(None));

        let observed = events(&publisher);
        assert_eq!(observed.len(), 2);
        match &observed[1] {
            Event::Terminal(_, _, _, status) => {
                assert_eq!(status.phase, SemanticOperationPhase::Succeeded);
                assert_eq!(status.kind, SemanticOperationKind::CreateChannel);
            }
            other => panic!("unexpected terminal event: {other:?}"),
        }
    }

    #[test]
    fn dropped_operation_publishes_drop_failure() {
        let publisher = TestPublisher::default();
        let operation = block_on(SubmittedOperation::submit(
            publisher.clone(),
            OperationId::invitation_decline(),
            SemanticOperationKind::DeclineInvitation,
            |_| OperationInstanceId("test-op-3".to_string()),
        ));
        drop(operation);

        let observed = events(&publisher);
        assert_eq!(observed.len(), 2);
        assert_eq!(
            observed[1],
            Event::Dropped(
                OperationId::invitation_decline(),
                OperationInstanceId("test-op-3".to_string()),
                SemanticOperationKind::DeclineInvitation,
            )
        );
    }

    #[test]
    fn dropped_owner_error_uses_internal_error_failure() {
        let status = dropped_owner_error(SemanticOperationKind::CreateHome);
        assert_eq!(status.kind, SemanticOperationKind::CreateHome);
        assert_eq!(status.phase, SemanticOperationPhase::Failed);
    }
}
