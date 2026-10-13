//! Finite enrollment import custody. Observer destruction requests cancellation;
//! the original required-task custodian retains terminal publication.
use super::device_enrollment::{
    prepare_device_enrollment_import, run_enrollment_accept, EnrollmentAcceptState,
    EnrollmentHostFinalization,
};
use super::*;
use aura_core::effects::task::{RequiredTaskExit, RequiredTaskSettlement};
use aura_core::OwnedTaskHandle;
use futures::channel::oneshot;

type ImportOutcome = crate::ui_contract::WorkflowTerminalOutcome<DeviceEnrollmentImportCompleted>;
type ImportControl = OwnedTaskHandle<(), Box<dyn aura_core::effects::task::RequiredTaskControl>>;

enum ImportOwnerSlot {
    Armed(Box<SemanticWorkflowOwner>),
    Closed,
}

/// Original handoff refusal, retained alongside the still-local submission.
#[derive(Debug, thiserror::Error)]
#[error("enrollment import handoff refused: {source}")]
pub struct EnrollmentImportHandoffError {
    #[source]
    source: AuraError,
}
impl EnrollmentImportHandoffError {
    /// Preserve the original concrete refusal independently of its projection.
    pub fn source_error(&self) -> &AuraError {
        &self.source
    }
    /// Typed failure for the original submission that still owns publication.
    pub fn semantic_error(&self) -> crate::ui_contract::SemanticOperationError {
        AcceptInvitationError::AcceptFailed {
            detail: self.source.to_string(),
            source: Some(self.source.clone()),
        }
        .semantic_error(SemanticOperationKind::ImportDeviceEnrollmentCode)
    }
}

/// Observer of one actual required import; Drop requests original cancellation.
pub struct EnrollmentImportObservation {
    result: oneshot::Receiver<ImportOutcome>,
    arm: Option<oneshot::Sender<()>>,
    instance: Option<OperationInstanceId>,
    control: ImportControl,
    app: Arc<RwLock<AppCore>>,
    owner: Arc<std::sync::OnceLock<ImportOwnerSlot>>,
}
impl EnrollmentImportObservation {
    /// Accept the original submission atomically before it relinquishes publication.
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "WorkflowHandoffSubmission", receiver_type = EnrollmentImportObservation, family = "runtime_helper")]
    pub fn accept_handoff<P: crate::frontend_primitives::SubmittedOperationPublisher>(
        &mut self,
        submission: crate::frontend_primitives::WorkflowHandoffSubmission<P>,
    ) -> Result<
        crate::frontend_primitives::WorkflowHandoffRelease,
        (
            crate::frontend_primitives::WorkflowHandoffSubmission<P>,
            EnrollmentImportHandoffError,
        ),
    > {
        submission.try_handoff_to_workflow(|release| {
            self.arm(release)
                .map_err(|source| EnrollmentImportHandoffError { source })
        })
    }

    /// Private acceptance point; no enrollment caller may release its owner first.
    fn arm(
        &mut self,
        release: &crate::frontend_primitives::WorkflowHandoffRelease,
    ) -> Result<(), AuraError> {
        if release.operation_id() != &OperationId::device_enrollment()
            || Some(release.instance_id()) != self.instance.as_ref()
            || release.kind() != SemanticOperationKind::ImportDeviceEnrollmentCode
        {
            return Err(AuraError::invalid(
                "foreign enrollment import workflow handoff",
            ));
        }
        self.owner
            .set(ImportOwnerSlot::Armed(Box::new(
                SemanticWorkflowOwner::new(
                    &self.app,
                    OperationId::device_enrollment(),
                    self.instance.clone(),
                    SemanticOperationKind::ImportDeviceEnrollmentCode,
                ),
            )))
            .map_err(|_| AuraError::invalid("enrollment import original owner already armed"))?;
        // The OnceLock deposit is the acceptance point. Cleanup now owns the
        // original publisher even if execution notification is disconnected.
        if let Some(sender) = self.arm.take() {
            let _ = sender.send(());
        }
        Ok(())
    }
}
impl Drop for EnrollmentImportObservation {
    fn drop(&mut self) {
        self.control.request_cancel();
    }
}

impl std::future::Future for EnrollmentImportObservation {
    type Output = ImportOutcome;
    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        std::pin::Pin::new(&mut self.result).poll(cx).map(|result| {
            result.unwrap_or_else(|_| ImportOutcome {
                result: Err(AuraError::invalid(
                    "original import result owner disconnected",
                )),
                terminal: None,
            })
        })
    }
}

/// App-issued admission retains its actual attached runtime and unique origin.
#[derive(Clone)]
pub struct EnrollmentImportAdmission {
    origin: Arc<()>,
    runtime: Arc<dyn crate::runtime_bridge::RuntimeBridge>,
}
impl EnrollmentImportAdmission {
    // Only AppCore's canonical runtime constructors/attachment call this.
    pub(crate) fn from_attached_runtime(
        runtime: Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    ) -> Self {
        Self {
            origin: Arc::new(()),
            runtime,
        }
    }
    fn require_core(&self, app: &AppCore) -> Result<(), AuraError> {
        match app.enrollment_import_admission() {
            Some(actual)
                if Arc::ptr_eq(&actual.origin, &self.origin)
                    && Arc::ptr_eq(&actual.runtime, &self.runtime) =>
            {
                Ok(())
            }
            _ => Err(AuraError::invalid(
                "foreign enrollment import app/runtime admission",
            )),
        }
    }
}

/// Rejected required admission retains the actual unadmitted obligation until
/// the still-local submission owner synchronously publishes its explicit failure.
pub struct EnrollmentImportAdmissionRefusal {
    source: AuraError,
    #[cfg(not(target_arch = "wasm32"))]
    _custody: Option<Box<dyn aura_core::effects::task::RequiredTaskCustody>>,
    #[cfg(target_arch = "wasm32")]
    _custody: Option<Box<dyn aura_core::effects::task::LocalRequiredTaskCustody>>,
}
impl EnrollmentImportAdmissionRefusal {
    /// Refusal before any task or app workflow is admitted.
    pub fn unavailable() -> Self {
        Self {
            source: AuraError::invalid("attached enrollment import admission unavailable"),
            _custody: None,
        }
    }
    /// Browser identity selection must have an actual host commit owner before
    /// the runtime import can be admitted or reported successful.
    #[cfg(target_arch = "wasm32")]
    pub fn browser_host_unavailable() -> Self {
        Self {
            source: AuraError::invalid(
                "browser enrollment import requires host persistence acknowledgment",
            ),
            _custody: None,
        }
    }
    /// Observe the original concrete admission failure.
    pub fn source_error(&self) -> &AuraError {
        &self.source
    }
    /// Project failure for the still-local submission owner.
    pub fn semantic_error(&self) -> crate::ui_contract::SemanticOperationError {
        AcceptInvitationError::AcceptFailed {
            detail: self.source.to_string(),
            source: Some(self.source.clone()),
        }
        .semantic_error(SemanticOperationKind::ImportDeviceEnrollmentCode)
    }
}

#[aura_macros::actor_owned(owner = "enrollment-import", domain = "enrollment-import", gate = "original-app-admission", command = EnrollmentImportHostPolicy, capacity = 1, category = "actor_owned")]
struct EnrollmentImportCoordinator {
    app: Arc<RwLock<AppCore>>,
    admission: EnrollmentImportAdmission,
    owner: Arc<std::sync::OnceLock<ImportOwnerSlot>>,
    arm: oneshot::Receiver<()>,
    state: EnrollmentAcceptState<DeviceEnrollmentImportCompleted>,
    code: Option<String>,
    transfer: Option<crate::ui_contract::EnrollmentManifestTransferInput>,
    host: Option<EnrollmentImportHostPolicy>,
    result: Option<oneshot::Sender<ImportOutcome>>,
}
impl EnrollmentImportCoordinator {
    async fn execute_original(&mut self) -> Result<(), AuraError> {
        let armed = (&mut self.arm).await;
        armed.map_err(|_| AuraError::invalid("enrollment import never handed off"))?;
        let runtime = self.admission.runtime.clone();
        let budget = workflow_timeout_budget(&runtime, DEVICE_ENROLLMENT_ACCEPT_TIMEOUT).await?;
        self.state.runtime = Some(runtime.clone());
        self.state.budget = Some(budget.clone());
        let validation = execute_with_runtime_timeout_budget(&runtime, &budget, || async {
            let core = self.app.read().await;
            let result = self.admission.require_core(&core);
            if let Err(error) = &result {
                self.state
                    .first_failure
                    .get_or_insert_with(|| error.clone());
            }
            result
        })
        .await;
        match validation {
            Ok(()) => {}
            Err(TimeoutRunError::Operation(error)) => return Err(error),
            Err(TimeoutRunError::Timeout(error)) => return Err(error.into()),
        }
        let code = self
            .code
            .take()
            .ok_or_else(|| AuraError::invalid("import already admitted"))?;
        let transfer = self.transfer.take();
        let host = self
            .host
            .take()
            .ok_or_else(|| AuraError::invalid("host policy already admitted"))?;
        let owner = match self.owner.get() {
            Some(ImportOwnerSlot::Armed(owner)) => owner.as_ref(),
            _ => return Err(AuraError::invalid("original semantic owner is not armed")),
        };
        run_enrollment_accept(
            &mut self.state,
            &self.app,
            owner,
            prepare_device_enrollment_import(&runtime, &budget, code, transfer),
            match host {
                EnrollmentImportHostPolicy::RuntimeOnly => None,
                EnrollmentImportHostPolicy::Finalize(callback) => {
                    Some(EnrollmentHostFinalization::Async(callback))
                }
                EnrollmentImportHostPolicy::Prepare(callback) => {
                    Some(EnrollmentHostFinalization::Prepared(callback))
                }
            },
        )
        .await
    }
    async fn cleanup_original(
        &mut self,
        exit: &RequiredTaskExit,
    ) -> Result<RequiredTaskSettlement, AuraError> {
        // Close live unarmed admission synchronously. Shutdown does not depend
        // on the observer dropping its sender, and late handoff retains its
        // original prior publisher when this deposit wins.
        let _ = self.owner.set(ImportOwnerSlot::Closed);
        let owner = match self.owner.get() {
            Some(ImportOwnerSlot::Armed(owner)) => owner.as_ref(),
            _ => return Ok(RequiredTaskSettlement::Resolved),
        };
        Self::settle_original(owner, &mut self.state, &mut self.result, exit).await
    }
    #[aura_macros::semantic_owner(
        owner="settle_original",wrapper="import_device_enrollment_with_terminal_status",
        terminal="publish_success_with",postcondition="device_enrollment_imported",
        proof=crate::workflows::semantic_facts::DeviceEnrollmentImportedProof,
        authoritative_inputs="original_completion,host_acknowledgment,first_failure",depends_on="runtime_verified_enrollment_confirmation,host_storage_acknowledgment",
        child_ops="",category="move_owned"
    )]
    async fn settle_original(
        owner: &SemanticWorkflowOwner<
            OperationContext<OperationId, OperationInstanceId, TraceContext>,
        >,
        state: &mut EnrollmentAcceptState<DeviceEnrollmentImportCompleted>,
        result_sender: &mut Option<oneshot::Sender<ImportOutcome>>,
        exit: &RequiredTaskExit,
    ) -> Result<RequiredTaskSettlement, AuraError> {
        if let RequiredTaskExit::Failed(error) = exit {
            state.first_failure.get_or_insert_with(|| error.clone());
        }
        let result = if let Some(error) = state.first_failure.clone() {
            super::device_enrollment::fail_device_enrollment_accept(owner, error).await
        } else if state.host_acknowledged {
            let invitation = state
                .invitation
                .as_ref()
                .ok_or_else(|| AuraError::invalid("acknowledged import missing invitation"))?;
            owner
                .publish_success_with(issue_device_enrollment_imported_proof(
                    invitation.invitation_id.clone(),
                ))
                .await?;
            state
                .completion
                .take()
                .ok_or_else(|| AuraError::invalid("acknowledged import missing completion"))
        } else {
            owner
                .publish_phase(SemanticOperationPhase::Cancelled)
                .await?;
            Err(AuraError::invalid(
                "original enrollment import cancelled before host acknowledgment",
            ))
        };
        let domain_failure = result.as_ref().err().cloned();
        if let Some(sender) = result_sender.take() {
            // A disconnected observer does not undo the retained terminal result.
            let _disconnected = sender
                .send(ImportOutcome {
                    result,
                    terminal: owner.terminal_status().await,
                })
                .err();
        }
        Ok(match domain_failure {
            Some(error) if state.first_failure.is_some() => {
                RequiredTaskSettlement::ResolvedWithDomainFailure(error)
            }
            _ => RequiredTaskSettlement::Resolved,
        })
    }
}
#[cfg(not(target_arch = "wasm32"))]
impl aura_core::effects::task::RequiredTaskCustody for EnrollmentImportCoordinator {
    fn execute(&mut self) -> futures::future::BoxFuture<'_, Result<(), AuraError>> {
        Box::pin(self.execute_original())
    }
    fn cleanup<'a>(
        &'a mut self,
        exit: &'a RequiredTaskExit,
    ) -> futures::future::BoxFuture<'a, Result<RequiredTaskSettlement, AuraError>> {
        Box::pin(self.cleanup_original(exit))
    }
}
#[cfg(target_arch = "wasm32")]
impl aura_core::effects::task::LocalRequiredTaskCustody for EnrollmentImportCoordinator {
    fn execute(&mut self) -> futures::future::LocalBoxFuture<'_, Result<(), AuraError>> {
        Box::pin(self.execute_original())
    }
    fn cleanup<'a>(
        &'a mut self,
        exit: &'a RequiredTaskExit,
    ) -> futures::future::LocalBoxFuture<'a, Result<RequiredTaskSettlement, AuraError>> {
        Box::pin(self.cleanup_original(exit))
    }
}

pub(super) fn admit_import(
    app: &Arc<RwLock<AppCore>>,
    admission: &EnrollmentImportAdmission,
    code: String,
    transfer: Option<crate::ui_contract::EnrollmentManifestTransferInput>,
    instance: Option<OperationInstanceId>,
    host: EnrollmentImportHostPolicy,
) -> Result<EnrollmentImportObservation, EnrollmentImportAdmissionRefusal> {
    let (arm_sender, arm_receiver) = oneshot::channel();
    let (sender, receiver) = oneshot::channel();
    let owner = Arc::new(std::sync::OnceLock::new());
    let custody = Box::new(EnrollmentImportCoordinator {
        app: app.clone(),
        admission: admission.clone(),
        owner: owner.clone(),
        arm: arm_receiver,
        state: EnrollmentAcceptState::new(true),
        code: Some(code),
        transfer,
        host: Some(host),
        result: Some(sender),
    });
    #[cfg(not(target_arch = "wasm32"))]
    let admitted = admission
        .runtime
        .task_spawner()
        .spawn_required_custody("app.enrollment_import", custody);
    #[cfg(target_arch = "wasm32")]
    let admitted = admission
        .runtime
        .task_spawner()
        .spawn_local_required_custody("app.enrollment_import", custody);
    match admitted {
        Ok(control) => Ok(EnrollmentImportObservation {
            control,
            result: receiver,
            arm: Some(arm_sender),
            instance,
            app: app.clone(),
            owner,
        }),
        Err(rejected) => Err(EnrollmentImportAdmissionRefusal {
            source: rejected.source,
            _custody: Some(rejected.obligation),
        }),
    }
}

#[cfg(all(test, feature = "signals"))]
pub(super) mod test_support {
    use super::*;
    use crate::frontend_primitives::{SubmittedOperationPublisher, WorkflowHandoffSubmission};
    use crate::ui_contract::SemanticOperationStatus;

    // Unpublished prior custody must be relinquished through the real typed
    // handoff. These successful test submissions never use its failure path.
    type PriorEvents = Arc<async_lock::Mutex<Vec<(OperationInstanceId, SemanticOperationStatus)>>>;

    #[derive(Clone)]
    pub(in crate::workflows::invitation) struct PriorSubmission(PriorEvents);
    #[async_trait::async_trait]
    impl SubmittedOperationPublisher for PriorSubmission {
        async fn publish_dispatched(
            &self,
            _: &OperationId,
            _: &OperationInstanceId,
            _: SemanticOperationKind,
        ) {
            panic!("component submission is explicitly unpublished");
        }
        fn publish_terminal(
            &self,
            intent: crate::frontend_primitives::SubmittedTerminalIntent,
        ) -> crate::frontend_primitives::SubmittedTerminalObservation {
            let acknowledgment = crate::frontend_primitives::SubmittedTerminalAcknowledgment {
                operation_id: intent.operation_id,
                instance_id: intent.instance_id.clone(),
                scope: crate::frontend_primitives::SubmittedTerminalAcknowledgmentScope::ComponentObserved,
            };
            self.0
                .try_lock()
                .expect("fixture publication has no competing borrower")
                .push((intent.instance_id, intent.status));
            drop(intent.retained);
            Box::pin(async move { Ok(acknowledgment) })
        }
        fn relinquish(&self, _: &OperationId, _: &OperationInstanceId, _: SemanticOperationKind) {}
    }

    pub(in crate::workflows::invitation) fn prior_submission(
        instance: OperationInstanceId,
    ) -> (WorkflowHandoffSubmission<PriorSubmission>, PriorEvents) {
        let events = Arc::new(async_lock::Mutex::new(Vec::new()));
        let submission = WorkflowHandoffSubmission::submit_with_instance_unpublished(
            PriorSubmission(events.clone()),
            OperationId::device_enrollment(),
            SemanticOperationKind::ImportDeviceEnrollmentCode,
            instance,
        );
        (submission, events)
    }

    pub(in crate::workflows::invitation) async fn admit(
        app: &Arc<RwLock<AppCore>>,
        code: String,
        transfer: Option<crate::ui_contract::EnrollmentManifestTransferInput>,
        instance: Option<OperationInstanceId>,
        host: EnrollmentImportHostPolicy,
    ) -> EnrollmentImportObservation {
        let instance = instance.expect("component import has exact original instance");
        let admission = app
            .read()
            .await
            .enrollment_import_admission()
            .cloned()
            .expect("actual attached app admission");
        let (submission, prior_events) = prior_submission(instance.clone());
        let mut observation =
            super::super::device_enrollment::import_device_enrollment_with_terminal_status(
                app,
                &admission,
                code,
                transfer,
                Some(instance),
                host,
            )
            .unwrap_or_else(|refusal| {
                panic!(
                    "actual required admission refused: {:?}",
                    refusal.source_error()
                )
            });
        observation
            .accept_handoff(submission)
            .unwrap_or_else(|(_, error)| panic!("same typed original handoff: {error}"));
        assert!(prior_events.lock().await.is_empty());
        observation
    }

    pub(in crate::workflows::invitation) fn arm_for_test(
        observation: &mut EnrollmentImportObservation,
        release: &crate::frontend_primitives::WorkflowHandoffRelease,
    ) -> Result<(), AuraError> {
        observation.arm(release)
    }

    pub(in crate::workflows::invitation) async fn drain(
        supervisor: &aura_testkit::task_custody::SupervisedTaskTestkit,
    ) -> aura_testkit::task_custody::TestTaskDisposalReport {
        let mut reports = supervisor
            .drain()
            .await
            .expect("actual supervised worker join");
        assert_eq!(reports.len(), 1, "one original registered import custodian");
        let report = reports.pop().expect("one original required task report");
        assert_eq!(report.name, "app.enrollment_import");
        assert!(
            report.worker_failure.is_none(),
            "native worker failed: {report:?}"
        );
        assert!(
            report.acknowledged,
            "actual cleanup acknowledgment missing: {report:?}"
        );
        assert!(
            report.cleanup_failure.is_none(),
            "independent cleanup failed: {report:?}"
        );
        report
    }

    pub(in crate::workflows::invitation) async fn run(
        app: &Arc<RwLock<AppCore>>,
        code: String,
        transfer: Option<crate::ui_contract::EnrollmentManifestTransferInput>,
        instance: Option<OperationInstanceId>,
        host: EnrollmentImportHostPolicy,
    ) -> ImportOutcome {
        admit(app, code, transfer, instance, host).await.await
    }
}
