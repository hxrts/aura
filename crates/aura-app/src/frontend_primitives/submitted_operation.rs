mod submission;
mod types;

use futures::executor::block_on;
use std::future::Future;

pub use submission::{
    CeremonyMonitorHandoffSubmission, LocalTerminalSubmission, SubmittedOperation,
    WorkflowHandoffSubmission,
};
pub use types::{
    dropped_owner_error, CeremonyMonitorHandoffRelease, CeremonySubmissionTerminalOutcome,
    SubmittedOperationPublisher, SubmittedOperationRelease, SubmittedOperationWorkflowError,
    WorkflowHandoffRelease,
};

/// Run a frontend-owned sync bridge future at a sanctioned Layer 7 boundary.
///
/// This helper exists so shared/frontend crates do not scatter raw `block_on()`
/// calls across semantic owner wrappers. On native targets it rejects nested
/// Tokio runtime usage before blocking, which catches accidental coupling
/// between synchronous callback paths and already-running async executors.
#[track_caller]
pub fn run_frontend_sync_boundary<F>(boundary: &'static str, future: F) -> F::Output
where
    F: Future,
{
    assert_native_sync_boundary(boundary);
    block_on(future)
}

#[track_caller]
fn assert_native_sync_boundary(_boundary: &'static str) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if tokio::runtime::Handle::try_current().is_ok() {
            panic!(
                "{_boundary} cannot synchronously block inside a Tokio runtime; \
                 use an async handoff or enter through the sanctioned shell/bootstrap boundary"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        dropped_owner_error, run_frontend_sync_boundary, CeremonyMonitorHandoffSubmission,
        LocalTerminalSubmission, SubmittedOperation, SubmittedOperationPublisher,
        SubmittedOperationRelease, SubmittedOperationWorkflowError, WorkflowHandoffSubmission,
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

    #[test]
    fn sync_boundary_runs_future_outside_runtime() {
        let value =
            run_frontend_sync_boundary("sync_boundary_runs_future_outside_runtime", async {
                42usize
            });

        assert_eq!(value, 42);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[should_panic(expected = "cannot synchronously block inside a Tokio runtime")]
    fn sync_boundary_rejects_nested_tokio_runtime() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap_or_else(|error| panic!("failed to build tokio runtime for test: {error}"));

        runtime.block_on(async {
            let _ =
                run_frontend_sync_boundary("sync_boundary_rejects_nested_tokio_runtime", async {
                    7usize
                });
        });
    }
}
