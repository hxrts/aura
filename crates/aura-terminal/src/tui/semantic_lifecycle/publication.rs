//! Native channel publication retained by the existing frontend task root.
use super::{authoritative_operation_status_update, UiTaskOwner, UiUpdateSender};
use async_lock::Mutex;
use aura_app::frontend_primitives::{
    SubmittedTerminalAcknowledgment, SubmittedTerminalAcknowledgmentScope, SubmittedTerminalIntent,
    SubmittedTerminalObservation,
};
use aura_app::ui_contract::{
    OperationId, OperationInstanceId, SemanticOperationKind, SemanticOperationPhase,
    SemanticOperationStatus,
};
use aura_core::effects::task::{
    RejectedRequiredTask, RequiredTaskControl, RequiredTaskCustody, RequiredTaskExit,
    RequiredTaskSettlement,
};
use aura_core::{AuraError, OwnedTaskHandle};
use futures::future::BoxFuture;
use std::sync::{Arc, OnceLock};

type Control = Arc<Mutex<OwnedTaskHandle<(), Box<dyn RequiredTaskControl>>>>;

enum Intent {
    Terminal(SubmittedTerminalIntent),
    HandedOff,
}
struct Slot {
    operation_id: OperationId,
    instance_id: OperationInstanceId,
    kind: SemanticOperationKind,
    intent: OnceLock<Mutex<Intent>>,
    intent_changed: futures::task::AtomicWaker,
    dispatched: OnceLock<Result<(), AuraError>>,
    dispatch_changed: futures::task::AtomicWaker,
    delivered: OnceLock<Result<SubmittedTerminalAcknowledgment, AuraError>>,
    delivery_changed: futures::task::AtomicWaker,
    #[cfg(test)]
    terminal_send_ready: Mutex<Option<futures::channel::oneshot::Sender<()>>>,
}
impl Slot {
    fn complete_dispatch(&self, result: Result<(), AuraError>) {
        let _ = self.dispatched.set(result);
        self.dispatch_changed.wake();
    }
    fn complete_delivery(&self, result: Result<SubmittedTerminalAcknowledgment, AuraError>) {
        let _ = self.delivered.set(result);
        self.delivery_changed.wake();
    }
    async fn dispatch(&self) -> Result<(), AuraError> {
        futures::future::poll_fn(|cx| {
            self.dispatch_changed.register(cx.waker());
            self.dispatched
                .get()
                .cloned()
                .map_or(std::task::Poll::Pending, std::task::Poll::Ready)
        })
        .await
    }
    async fn delivery(&self) -> Result<SubmittedTerminalAcknowledgment, AuraError> {
        futures::future::poll_fn(|cx| {
            self.delivery_changed.register(cx.waker());
            self.delivered
                .get()
                .cloned()
                .map_or(std::task::Poll::Pending, std::task::Poll::Ready)
        })
        .await
    }
    fn set_intent(&self, intent: Intent) {
        assert!(
            self.intent.set(Mutex::new(intent)).is_ok(),
            "linear native submission transfers its intent once"
        );
        self.intent_changed.wake();
    }
}

struct OriginalPublication {
    slot: Arc<Slot>,
    tx: UiUpdateSender,
}
impl OriginalPublication {
    async fn send(&self, update: super::UiUpdate) -> Result<(), AuraError> {
        let permit = self
            .tx
            .reserve()
            .await
            .map_err(|source| AuraError::Internal {
                message: "original native UI publication channel closed".into(),
                source: Some(Arc::new(source)),
            })?;
        // A real permit accepts the exact message with no subsequent await.
        permit.send(update);
        Ok(())
    }
}
impl RequiredTaskCustody for OriginalPublication {
    fn execute(&mut self) -> BoxFuture<'_, Result<(), AuraError>> {
        Box::pin(async {
            let dispatch = self
                .send(authoritative_operation_status_update(
                    self.slot.operation_id.clone(),
                    Some(self.slot.instance_id.clone()),
                    None,
                    SemanticOperationStatus::new(
                        self.slot.kind,
                        SemanticOperationPhase::WorkflowDispatched,
                    ),
                ))
                .await;
            self.slot.complete_dispatch(dispatch.clone());
            if let Err(source) = dispatch {
                self.slot.complete_delivery(Err(source.clone()));
                return Err(source);
            }
            let intent = futures::future::poll_fn(|cx| {
                self.slot.intent_changed.register(cx.waker());
                self.slot
                    .intent
                    .get()
                    .map_or(std::task::Poll::Pending, std::task::Poll::Ready)
            })
            .await;
            let mut intent = intent.lock().await;
            let Intent::Terminal(terminal) = &mut *intent else {
                return Ok(());
            };
            #[cfg(test)]
            if let Some(ready) = self.slot.terminal_send_ready.lock().await.take() {
                let _ = ready.send(());
            }
            let delivery = self
                .send(authoritative_operation_status_update(
                    terminal.operation_id.clone(),
                    Some(terminal.instance_id.clone()),
                    terminal.causality.clone(),
                    terminal.status.clone(),
                ))
                .await;
            // Preserve actual acceptance before the borrowed execution returns.
            let result = delivery.map(|()| SubmittedTerminalAcknowledgment {
                operation_id: terminal.operation_id.clone(),
                instance_id: terminal.instance_id.clone(),
                scope: SubmittedTerminalAcknowledgmentScope::ChannelAccepted,
            });
            self.slot.complete_delivery(result.clone());
            result.map(|_| ())
        })
    }
    fn cleanup<'a>(
        &'a mut self,
        exit: &'a RequiredTaskExit,
    ) -> BoxFuture<'a, Result<RequiredTaskSettlement, AuraError>> {
        Box::pin(async move {
            if let Some(intent) = self.slot.intent.get() {
                let mut intent = intent.lock().await;
                let accepted = matches!(self.slot.delivered.get(), Some(Ok(_)));
                let handed_off = matches!(&*intent, Intent::HandedOff)
                    && matches!(self.slot.dispatched.get(), Some(Ok(())));
                if accepted || handed_off {
                    if let Intent::Terminal(terminal) = &mut *intent {
                        // Actual rejected enrollment custody stays here until ACK.
                        terminal.retained.take();
                    }
                    return Ok(RequiredTaskSettlement::Resolved);
                }
            }
            let source = match exit {
                RequiredTaskExit::Failed(source) => source.clone(),
                RequiredTaskExit::Cancelled => AuraError::invalid(
                    "original native publication interrupted before channel acknowledgment",
                ),
                RequiredTaskExit::Panicked => {
                    AuraError::invalid("original native publication worker panicked")
                }
                RequiredTaskExit::Completed => AuraError::invalid(
                    "original native publication completed without acknowledgment",
                ),
            };
            self.slot.complete_dispatch(Err(source.clone()));
            self.slot.complete_delivery(Err(source.clone()));
            Err(source)
        })
    }
}

#[derive(thiserror::Error)]
#[error("native frontend publication admission refused: {source}")]
pub(crate) struct PublicationAdmissionRefusal {
    #[source]
    source: AuraError,
    // Keep the actual rejected original obligation until the caller handles refusal.
    _original: Option<Mutex<Box<dyn RequiredTaskCustody>>>,
}
impl std::fmt::Debug for PublicationAdmissionRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PublicationAdmissionRefusal")
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}
impl From<RejectedRequiredTask<dyn RequiredTaskCustody>> for PublicationAdmissionRefusal {
    fn from(rejected: RejectedRequiredTask<dyn RequiredTaskCustody>) -> Self {
        Self {
            source: rejected.source,
            _original: Some(Mutex::new(rejected.obligation)),
        }
    }
}

#[derive(thiserror::Error)]
#[error("original native terminal publication unresolved: {source}")]
struct PublicationFailure {
    #[source]
    source: AuraError,
    _control: Control,
}
impl std::fmt::Debug for PublicationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PublicationFailure")
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
pub(super) struct NativePublication {
    slot: Arc<Slot>,
    control: Control,
}
impl NativePublication {
    pub(super) fn admit(
        tasks: &UiTaskOwner,
        tx: UiUpdateSender,
        operation_id: OperationId,
        instance_id: OperationInstanceId,
        kind: SemanticOperationKind,
    ) -> Result<Self, PublicationAdmissionRefusal> {
        if tokio::runtime::Handle::try_current().is_err() {
            return Err(PublicationAdmissionRefusal {
                source: AuraError::invalid(
                    "native frontend publication requires its active shell runtime",
                ),
                _original: None,
            });
        }
        let slot = Arc::new(Slot {
            operation_id,
            instance_id,
            kind,
            intent: OnceLock::new(),
            intent_changed: futures::task::AtomicWaker::new(),
            dispatched: OnceLock::new(),
            dispatch_changed: futures::task::AtomicWaker::new(),
            delivered: OnceLock::new(),
            delivery_changed: futures::task::AtomicWaker::new(),
            #[cfg(test)]
            terminal_send_ready: Mutex::new(None),
        });
        let control = tasks
            .owned_spawner()
            .spawn_required_custody(
                "original-native-submission-publication",
                Box::new(OriginalPublication {
                    slot: slot.clone(),
                    tx,
                }),
            )
            .map_err(PublicationAdmissionRefusal::from)?;
        Ok(Self {
            slot,
            control: Arc::new(Mutex::new(control)),
        })
    }
    pub(super) async fn dispatched(&self) -> Result<(), AuraError> {
        self.slot.dispatch().await
    }
    pub(super) fn terminal(&self, intent: SubmittedTerminalIntent) -> SubmittedTerminalObservation {
        assert_eq!(intent.operation_id, self.slot.operation_id);
        assert_eq!(intent.instance_id, self.slot.instance_id);
        self.slot.set_intent(Intent::Terminal(intent));
        let slot = self.slot.clone();
        let control = self.control.clone();
        Box::pin(async move {
            slot.delivery().await.map_err(|source| AuraError::Internal {
                message: "original terminal publication delivery failed".into(),
                source: Some(Arc::new(PublicationFailure {
                    source,
                    _control: control,
                })),
            })
        })
    }
    pub(super) fn relinquish(&self) {
        self.slot.set_intent(Intent::HandedOff);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_app::ui_contract::{
        SemanticFailureCode, SemanticFailureDomain, SemanticOperationError,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::mpsc;

    struct RetainedCustody(Arc<AtomicUsize>);
    impl Drop for RetainedCustody {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::AcqRel);
        }
    }
    fn terminal(
        publication: &NativePublication,
        dropped: &Arc<AtomicUsize>,
    ) -> SubmittedTerminalIntent {
        SubmittedTerminalIntent {
            operation_id: publication.slot.operation_id.clone(),
            instance_id: publication.slot.instance_id.clone(),
            causality: None,
            status: SemanticOperationStatus::failed(
                publication.slot.kind,
                SemanticOperationError::new(
                    SemanticFailureDomain::Command,
                    SemanticFailureCode::InternalError,
                )
                .with_detail("original retained native refusal"),
            ),
            retained: Some(Box::new(RetainedCustody(dropped.clone()))),
        }
    }
    fn admitted(tasks: &UiTaskOwner, tx: UiUpdateSender, instance: &str) -> NativePublication {
        NativePublication::admit(
            tasks,
            tx,
            OperationId::device_enrollment(),
            OperationInstanceId(instance.into()),
            SemanticOperationKind::ImportDeviceEnrollmentCode,
        )
        .unwrap()
    }
    async fn send_readiness(
        publication: &NativePublication,
    ) -> futures::channel::oneshot::Receiver<()> {
        let (ready, observed) = futures::channel::oneshot::channel();
        *publication.slot.terminal_send_ready.lock().await = Some(ready);
        observed
    }
    fn native_source(error: &AuraError) -> &Arc<dyn std::error::Error + Send + Sync> {
        let AuraError::Internal {
            source: Some(source),
            ..
        } = error
        else {
            panic!("original native channel source must survive");
        };
        assert!(source
            .downcast_ref::<mpsc::error::SendError<()>>()
            .is_some());
        source
    }
    async fn assert_original_intent(
        publication: &NativePublication,
        status: &SemanticOperationStatus,
    ) {
        let intent = publication.slot.intent.get().unwrap().lock().await;
        let Intent::Terminal(intent) = &*intent else {
            panic!("original terminal intent missing");
        };
        assert_eq!(intent.operation_id, publication.slot.operation_id);
        assert_eq!(intent.instance_id, publication.slot.instance_id);
        assert_eq!(&intent.status, status);
        assert!(intent.retained.is_some());
    }

    #[tokio::test]
    async fn rejected_enrollment_retains_prior_owner_until_delivery_ack() {
        use aura_app::frontend_primitives::WorkflowHandoffSubmission;
        use aura_app::runtime_bridge::OfflineRuntimeBridge;
        use aura_app::ui::workflows::invitation::{
            import_device_enrollment_with_terminal_status, EnrollmentImportHostPolicy,
        };
        use aura_app::{AppConfig, AppCore};
        let runtime = Arc::new(OfflineRuntimeBridge::new(
            aura_core::AuthorityId::from_entropy([87; 32]),
        ));
        let app = Arc::new(async_lock::RwLock::new(
            AppCore::with_runtime(AppConfig::default(), runtime).unwrap(),
        ));
        let admission = app
            .read()
            .await
            .enrollment_import_admission()
            .unwrap()
            .clone();
        let tasks = Arc::new(UiTaskOwner::new());
        let (tx, mut rx) = mpsc::channel(1);
        let operation_id = OperationId::device_enrollment();
        let kind = SemanticOperationKind::ImportDeviceEnrollmentCode;
        let (publisher, instance_id) =
            super::super::admit_native_publisher(&tasks, tx, &operation_id, kind).unwrap();
        let publication = publisher.publication.clone();
        let operation = super::super::WorkflowHandoffOperationOwner(
            super::super::SubmittedWorkflowOperationOwner(
                WorkflowHandoffSubmission::submit_with_instance_unpublished(
                    publisher,
                    operation_id.clone(),
                    kind,
                    instance_id.clone(),
                ),
            ),
        );
        publication.dispatched().await.unwrap();
        let ready = send_readiness(&publication).await;
        let dropped = Arc::new(AtomicUsize::new(0));
        let witness = Arc::new(RetainedCustody(dropped.clone()));
        let actual_host_custody = Arc::downgrade(&witness);
        let host_calls = Arc::new(AtomicUsize::new(0));
        let calls = host_calls.clone();
        let host = EnrollmentImportHostPolicy::Finalize(Arc::new(move |_| {
            calls.fetch_add(1, Ordering::AcqRel);
            let retained = witness.clone();
            Box::pin(async move {
                drop(retained);
                panic!("unadmitted original enrollment host must never execute");
            })
        }));
        // Real offline admission returns the actual EnrollmentImportCoordinator
        // before code decoding/workflow execution, not an unavailable stand-in.
        let refused = import_device_enrollment_with_terminal_status(
            &app,
            &admission,
            "unpolled-code".into(),
            None,
            Some(instance_id.clone()),
            host,
        )
        .err()
        .expect("offline supervision must reject actual required custody");
        let AuraError::Internal {
            source: Some(source),
            ..
        } = refused.source_error()
        else {
            panic!("actual retained admission source missing");
        };
        assert!(matches!(
            source.downcast_ref::<aura_core::effects::task::TaskSpawnError>(),
            Some(
                aura_core::effects::task::TaskSpawnError::UnsupportedFallible {
                    name: "app.enrollment_import"
                }
            )
        ));
        let expected_status = SemanticOperationStatus::failed(kind, refused.semantic_error());
        let observation =
            operation.fail_before_handoff_with_custody(refused.semantic_error(), refused);
        ready.await.unwrap();
        drop(observation);
        assert!(actual_host_custody.upgrade().is_some());
        assert_eq!(dropped.load(Ordering::Acquire), 0);
        assert_eq!(host_calls.load(Ordering::Acquire), 0);
        rx.recv().await.unwrap();
        let acknowledgment = publication.slot.delivery().await.unwrap();
        assert_eq!(acknowledgment.operation_id, operation_id);
        assert_eq!(acknowledgment.instance_id, instance_id);
        assert_eq!(
            acknowledgment.scope,
            SubmittedTerminalAcknowledgmentScope::ChannelAccepted
        );
        tasks.shutdown();
        tasks.wait_required_disposed().await.unwrap();
        assert!(actual_host_custody.upgrade().is_none());
        assert_eq!(dropped.load(Ordering::Acquire), 1);
        assert_eq!(host_calls.load(Ordering::Acquire), 0);
        assert!(
            matches!(rx.recv().await.unwrap(), super::super::UiUpdate::AuthoritativeOperationStatus {
            instance_id: Some(instance), status, ..
        } if instance == instance_id && status == expected_status)
        );
    }

    #[tokio::test]
    async fn native_publication_closed_retains_original_custody() {
        let tasks = UiTaskOwner::new();
        let (tx, rx) = mpsc::channel(1);
        let publication = admitted(&tasks, tx, "original-closed");
        publication.dispatched().await.unwrap();
        drop(rx);
        let dropped = Arc::new(AtomicUsize::new(0));
        let intent = terminal(&publication, &dropped);
        let original_status = intent.status.clone();
        let failure = publication.terminal(intent).await.unwrap_err();
        let AuraError::Internal {
            source: Some(source),
            ..
        } = &failure
        else {
            panic!("retained failure holder missing");
        };
        let failure_holder = source.downcast_ref::<PublicationFailure>().unwrap();
        let original_source = native_source(&failure_holder.source).clone();
        assert_original_intent(&publication, &original_status).await;
        tasks.shutdown();
        let disposal = tasks.wait_required_disposed().await.unwrap_err();
        assert!(Arc::ptr_eq(&original_source, native_source(&disposal)));
        let reconciliation = failure_holder
            ._control
            .lock()
            .await
            .reconcile()
            .await
            .unwrap_err();
        assert!(Arc::ptr_eq(
            &original_source,
            native_source(&reconciliation)
        ));
        assert_eq!(dropped.load(Ordering::Acquire), 0);
        assert!(matches!(publication.slot.delivered.get(), Some(Err(_))));
    }

    #[tokio::test]
    async fn native_publication_observer_drop_preserves_pending_send() {
        for poll_observation in [false, true] {
            let tasks = UiTaskOwner::new();
            let (tx, mut rx) = mpsc::channel(1);
            let publication = admitted(
                &tasks,
                tx,
                if poll_observation {
                    "original-polled"
                } else {
                    "original-unpolled"
                },
            );
            publication.dispatched().await.unwrap();
            let ready = send_readiness(&publication).await;
            let dropped = Arc::new(AtomicUsize::new(0));
            let intent = terminal(&publication, &dropped);
            let original_status = intent.status.clone();
            let mut observation = publication.terminal(intent);
            ready.await.unwrap();
            assert!(publication.slot.delivered.get().is_none());
            if poll_observation {
                assert!(futures::poll!(&mut observation).is_pending());
            }
            drop(observation);
            assert_eq!(dropped.load(Ordering::Acquire), 0);
            // The actual full queue holds dispatch. Releasing that message is
            // the only event that allows the original terminal send to finish.
            assert!(
                matches!(rx.recv().await.unwrap(), super::super::UiUpdate::AuthoritativeOperationStatus { status, .. }
                if status.phase == SemanticOperationPhase::WorkflowDispatched)
            );
            let acknowledgment = publication.slot.delivery().await.unwrap();
            assert_eq!(acknowledgment.instance_id, publication.slot.instance_id);
            assert_eq!(
                acknowledgment.scope,
                SubmittedTerminalAcknowledgmentScope::ChannelAccepted
            );
            assert!(
                matches!(rx.recv().await.unwrap(), super::super::UiUpdate::AuthoritativeOperationStatus {
                operation_id, instance_id: Some(instance), status, ..
            } if operation_id == publication.slot.operation_id && instance == publication.slot.instance_id && status == original_status)
            );
            tasks.shutdown();
            tasks.wait_required_disposed().await.unwrap();
            assert_eq!(dropped.load(Ordering::Acquire), 1);
            assert!(rx.try_recv().is_err());
        }
    }

    #[tokio::test]
    async fn native_publication_acknowledges_exact_instance_once() {
        let tasks = UiTaskOwner::new();
        let (tx, mut rx) = mpsc::channel(1);
        let publication = admitted(&tasks, tx, "original-accepted-once");
        publication.dispatched().await.unwrap();
        rx.recv().await.unwrap();
        let dropped = Arc::new(AtomicUsize::new(0));
        let intent = terminal(&publication, &dropped);
        let original_status = intent.status.clone();
        let acknowledgment = publication.terminal(intent).await.unwrap();
        assert_eq!(acknowledgment.operation_id, publication.slot.operation_id);
        assert_eq!(acknowledgment.instance_id, publication.slot.instance_id);
        assert_eq!(
            acknowledgment.scope,
            SubmittedTerminalAcknowledgmentScope::ChannelAccepted
        );
        // Cancellation after the actual acceptance cannot replace its receipt.
        publication.control.lock().await.request_cancel();
        tasks.shutdown();
        tasks.wait_required_disposed().await.unwrap();
        publication.control.lock().await.reconcile().await.unwrap();
        assert_eq!(publication.slot.delivery().await.unwrap(), acknowledgment);
        assert_eq!(dropped.load(Ordering::Acquire), 1);
        assert!(
            matches!(rx.recv().await.unwrap(), super::super::UiUpdate::AuthoritativeOperationStatus {
            instance_id: Some(instance), status, ..
        } if instance == acknowledgment.instance_id && status == original_status)
        );
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn native_publication_shutdown_reports_unresolved_cause() {
        let tasks = UiTaskOwner::new();
        let (tx, _rx) = mpsc::channel(1);
        let publication = admitted(&tasks, tx, "original-unresolved-shutdown");
        publication.dispatched().await.unwrap();
        let ready = send_readiness(&publication).await;
        let dropped = Arc::new(AtomicUsize::new(0));
        let intent = terminal(&publication, &dropped);
        let original_status = intent.status.clone();
        let observation = publication.terminal(intent);
        ready.await.unwrap();
        tasks.shutdown();
        assert!(tasks.wait_required_disposed().await.is_err());
        let failure = observation.await.unwrap_err();
        let AuraError::Internal {
            source: Some(source),
            ..
        } = failure
        else {
            panic!("original failure control must survive shutdown");
        };
        let original = source.downcast_ref::<PublicationFailure>().unwrap();
        assert!(matches!(original.source, AuraError::Invalid { .. }));
        assert!(original._control.lock().await.reconcile().await.is_err());
        assert_original_intent(&publication, &original_status).await;
        assert_eq!(dropped.load(Ordering::Acquire), 0);
        assert!(matches!(publication.slot.delivered.get(), Some(Err(_))));
    }
}
