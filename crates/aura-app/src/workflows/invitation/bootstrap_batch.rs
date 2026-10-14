//! Finite custody of one bootstrap receive operation. Transport observations
//! select existing workflows; they never approve enrollment or mint authority.
use super::*;
use aura_core::effects::task::{RequiredTaskExit, RequiredTaskSettlement};
use aura_core::invitation::{
    BootstrapBrokerInvitation, InvitationPurpose, MAX_BOOTSTRAP_INVITATIONS_PER_RECIPIENT_COUNT,
};
use aura_core::OwnedTaskHandle;
use futures::channel::oneshot;
use std::collections::VecDeque;

/// Synchronous host acknowledgment of an observed enrollment prefill only.
pub type BootstrapEnrollmentPrefillHost = Arc<dyn Fn(&str) -> Result<(), AuraError> + Send + Sync>;

/// Observed progress, independent of enrollment approval or task disposal.
pub struct BootstrapBatchCompleted {
    /// Canonically verified imports retained by the runtime.
    pub imported: Vec<InvitationHandle>,
    /// Host-acknowledged prefill still requiring explicit enrollment approval.
    pub enrollment_prefill: Option<String>,
}

/// Primary disposition for unprocessed deliveries.
#[derive(Debug)]
pub enum BootstrapBatchFailureReason {
    /// The original receiver was cancelled before all deliveries settled.
    Cancelled,
    /// The first concrete provider, import, validation or host failure.
    Failed(AuraError),
}

/// Retains the actual refused records and progress beside the original cause.
pub struct BootstrapBatchFailure {
    reason: BootstrapBatchFailureReason,
    refused: Vec<BootstrapBrokerInvitation>,
    completed: BootstrapBatchCompleted,
    observation_failure: Option<AuraError>,
}
impl BootstrapBatchFailure {
    /// Original disposition; cancellation does not replace an earlier failure.
    pub fn reason(&self) -> &BootstrapBatchFailureReason {
        &self.reason
    }
    /// Exact unprocessed records, in their original order.
    pub fn refused_deliveries(&self) -> &[BootstrapBrokerInvitation] {
        &self.refused
    }
    /// Actual imported or host-acknowledged progress before refusal.
    pub fn completed(&self) -> &BootstrapBatchCompleted {
        &self.completed
    }
    /// Later selected-provider failure, independent of the original native cause.
    pub fn observation_failure(&self) -> Option<&AuraError> {
        self.observation_failure.as_ref()
    }
}
impl std::fmt::Debug for BootstrapBatchFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BootstrapBatchFailure")
            .field("reason", &self.reason)
            .field("observation_failure", &self.observation_failure)
            .field("refused_count", &self.refused.len())
            .field("imported_count", &self.completed.imported.len())
            .finish_non_exhaustive()
    }
}
impl std::fmt::Display for BootstrapBatchFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "bootstrap receiver refused {} pending deliveries",
            self.refused.len()
        )
    }
}
impl std::error::Error for BootstrapBatchFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.reason {
            BootstrapBatchFailureReason::Failed(source) => Some(source),
            BootstrapBatchFailureReason::Cancelled => None,
        }
    }
}
/// Typed primary result; successful receipt is not required-task disposal proof.
pub type BootstrapBatchOutcome = Result<BootstrapBatchCompleted, BootstrapBatchFailure>;

/// Observer of an admitted app receiver. Drop requests original cancellation.
pub struct BootstrapBatchObservation {
    result: oneshot::Receiver<BootstrapBatchOutcome>,
    control: OwnedTaskHandle<(), Box<dyn aura_core::effects::task::RequiredTaskControl>>,
}
impl Drop for BootstrapBatchObservation {
    fn drop(&mut self) {
        self.control.request_cancel();
    }
}
impl std::future::Future for BootstrapBatchObservation {
    type Output = Result<BootstrapBatchOutcome, AuraError>;
    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        std::pin::Pin::new(&mut self.result).poll(cx).map(|result| {
            result.map_err(|_| AuraError::invalid("original bootstrap result owner disconnected"))
        })
    }
}

/// Rejected admission keeps the actual unadmitted box; no broker read has started.
pub struct BootstrapBatchAdmissionRefusal {
    source: AuraError,
    #[cfg(not(target_arch = "wasm32"))]
    _custody: Box<dyn aura_core::effects::task::RequiredTaskCustody>,
    #[cfg(target_arch = "wasm32")]
    _custody: Box<dyn aura_core::effects::task::LocalRequiredTaskCustody>,
}
impl std::fmt::Debug for BootstrapBatchAdmissionRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BootstrapBatchAdmissionRefusal")
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}
impl std::fmt::Display for BootstrapBatchAdmissionRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "bootstrap receiver admission refused: {}", self.source)
    }
}
impl std::error::Error for BootstrapBatchAdmissionRefusal {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}
impl BootstrapBatchAdmissionRefusal {
    /// Original admission cause; the unadmitted custody has performed no read.
    pub fn source_error(&self) -> &AuraError {
        &self.source
    }
}

#[aura_macros::actor_owned(owner = "bootstrap-invitation-batch", domain = "bootstrap-invitation", gate = "attached-runtime-required-admission", command = BootstrapEnrollmentPrefillHost, capacity = 16, category = "actor_owned")]
struct BootstrapBatchCoordinator {
    runtime: Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    host: BootstrapEnrollmentPrefillHost,
    pending: VecDeque<BootstrapBrokerInvitation>,
    completed: BootstrapBatchCompleted,
    first_failure: Option<AuraError>,
    observation_failure: Option<AuraError>,
    result: Option<oneshot::Sender<BootstrapBatchOutcome>>,
    fetched: bool,
}
impl BootstrapBatchCoordinator {
    async fn execute_original(&mut self) -> Result<(), AuraError> {
        let result = self.run_original().await;
        match &self.first_failure {
            Some(source) => Err(source.clone()),
            None => result,
        }
    }

    async fn run_original(&mut self) -> Result<(), AuraError> {
        let runtime = self.runtime.clone();
        let original = super::import::prepare_bootstrap_invitation_batch(&runtime).await?;
        let expected_authority = runtime.authority_id();
        // Deposit the actual batch before the executor's final clock read. A
        // native import/fetch failure is retained before that read as well.
        let received = execute_with_runtime_timeout_budget(&runtime, &original, || async {
            let batch = match runtime.take_bootstrap_invitation_batch().await {
                Ok(batch) => batch,
                Err(source) => {
                    let source = AuraError::from(source);
                    self.first_failure.get_or_insert_with(|| source.clone());
                    return Err(source);
                }
            };
            self.pending = batch.deliveries.into();
            self.fetched = true;
            let valid = batch.recipient_authority_id == expected_authority
                && self.pending.len() <= MAX_BOOTSTRAP_INVITATIONS_PER_RECIPIENT_COUNT
                && self.pending.iter().all(|delivery| {
                    delivery.recipient_authority_id == expected_authority.to_string()
                        && delivery.recipient_device_id == batch.recipient_device_id
                });
            if !valid {
                let source = AuraError::permission_denied("foreign or oversized bootstrap batch");
                self.first_failure.get_or_insert_with(|| source.clone());
                return Err(source);
            }
            Ok(())
        })
        .await;
        received.map_err(|error| match error {
            TimeoutRunError::Operation(source) => source,
            TimeoutRunError::Timeout(source) => {
                let source: AuraError = source.into();
                if self.first_failure.is_some() {
                    self.observation_failure = Some(source.clone());
                }
                source
            }
        })?;
        while let Some(delivery) = self.pending.front() {
            super::import::require_bootstrap_invitation_purpose(
                &delivery.invitation_code,
                delivery.purpose,
            )?;
            if delivery.purpose == InvitationPurpose::DeviceEnrollment {
                if let Some(current) = &self.completed.enrollment_prefill {
                    if current != &delivery.invitation_code {
                        return Err(AuraError::invalid(
                            "an original enrollment prefill is already retained",
                        ));
                    }
                } else {
                    let host = &self.host;
                    let prefill = &mut self.completed.enrollment_prefill;
                    let first_failure = &mut self.first_failure;
                    aura_core::time::timeout::acknowledge_with_timeout_budget(
                        &runtime.physical_time_provider(),
                        &original,
                        || async { Ok::<_, aura_core::TimeoutBudgetError>(()) },
                        |()| {
                            if let Err(source) = host(&delivery.invitation_code) {
                                first_failure.get_or_insert_with(|| source.clone());
                                return Err(source);
                            }
                            *prefill = Some(delivery.invitation_code.clone());
                            Ok(())
                        },
                    )
                    .await
                    .map_err(AuraError::from)??;
                }
            } else {
                let imported = super::import::import_bootstrap_invitation_retained(
                    &runtime,
                    &delivery.invitation_code,
                    delivery.purpose,
                    &original,
                    &mut self.first_failure,
                    &mut self.observation_failure,
                )
                .await?;
                self.completed.imported.push(imported);
            }
            // A matching workflow/host accepted this original record. No await
            // separates its acknowledgment and removal from retained custody.
            self.pending.pop_front();
        }
        Ok(())
    }

    fn settle_original(&mut self, exit: &RequiredTaskExit) -> RequiredTaskSettlement {
        if let RequiredTaskExit::Failed(source) = exit {
            self.first_failure.get_or_insert_with(|| source.clone());
        }
        if matches!(exit, RequiredTaskExit::Panicked) {
            self.first_failure
                .get_or_insert_with(|| AuraError::invalid("bootstrap receiver worker panicked"));
        }
        let completed = std::mem::replace(
            &mut self.completed,
            BootstrapBatchCompleted {
                imported: Vec::new(),
                enrollment_prefill: None,
            },
        );
        let primary = self.first_failure.clone();
        let outcome = if let Some(source) = primary.clone() {
            Err(BootstrapBatchFailure {
                reason: BootstrapBatchFailureReason::Failed(source),
                refused: self.pending.drain(..).collect(),
                completed,
                observation_failure: self.observation_failure.take(),
            })
        } else if self.fetched && self.pending.is_empty() {
            Ok(completed)
        } else {
            Err(BootstrapBatchFailure {
                reason: BootstrapBatchFailureReason::Cancelled,
                refused: self.pending.drain(..).collect(),
                completed,
                observation_failure: self.observation_failure.take(),
            })
        };
        if let Some(sender) = self.result.take() {
            if let Err(Err(refusal)) = sender.send(outcome) {
                // The original required root retains this actual refused batch
                // as a typed domain result when its observer is disconnected.
                return RequiredTaskSettlement::ResolvedWithDomainFailure(AuraError::Internal {
                    message: "retained bootstrap receiver disposition".into(),
                    source: Some(Arc::new(refusal)),
                });
            }
        }
        match primary {
            Some(source) => RequiredTaskSettlement::ResolvedWithDomainFailure(source),
            None => RequiredTaskSettlement::Resolved,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl aura_core::effects::task::RequiredTaskCustody for BootstrapBatchCoordinator {
    fn execute(&mut self) -> futures::future::BoxFuture<'_, Result<(), AuraError>> {
        Box::pin(self.execute_original())
    }
    fn cleanup<'a>(
        &'a mut self,
        exit: &'a RequiredTaskExit,
    ) -> futures::future::BoxFuture<'a, Result<RequiredTaskSettlement, AuraError>> {
        Box::pin(async move { Ok(self.settle_original(exit)) })
    }
}
#[cfg(target_arch = "wasm32")]
impl aura_core::effects::task::LocalRequiredTaskCustody for BootstrapBatchCoordinator {
    fn execute(&mut self) -> futures::future::LocalBoxFuture<'_, Result<(), AuraError>> {
        Box::pin(self.execute_original())
    }
    fn cleanup<'a>(
        &'a mut self,
        exit: &'a RequiredTaskExit,
    ) -> futures::future::LocalBoxFuture<'a, Result<RequiredTaskSettlement, AuraError>> {
        Box::pin(async move { Ok(self.settle_original(exit)) })
    }
}

/// Admit the original receiver before any asynchronous broker read starts.
pub fn receive_bootstrap_invitation_batch(
    runtime: Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    existing_prefill: Option<String>,
    host: BootstrapEnrollmentPrefillHost,
) -> Result<BootstrapBatchObservation, BootstrapBatchAdmissionRefusal> {
    let (sender, result) = oneshot::channel();
    let custody = Box::new(BootstrapBatchCoordinator {
        runtime: runtime.clone(),
        host,
        pending: VecDeque::new(),
        completed: BootstrapBatchCompleted {
            imported: Vec::new(),
            enrollment_prefill: existing_prefill,
        },
        first_failure: None,
        observation_failure: None,
        result: Some(sender),
        fetched: false,
    });
    #[cfg(not(target_arch = "wasm32"))]
    let admitted = runtime
        .task_spawner()
        .spawn_required_custody("app.bootstrap_invitation_batch", custody);
    #[cfg(target_arch = "wasm32")]
    let admitted = runtime
        .task_spawner()
        .spawn_local_required_custody("app.bootstrap_invitation_batch", custody);
    admitted
        .map(|control| BootstrapBatchObservation { result, control })
        .map_err(|refused| BootstrapBatchAdmissionRefusal {
            source: refused.source,
            _custody: refused.obligation,
        })
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use futures::FutureExt;
    use std::error::Error;

    async fn signed_delivery(
        receiver: AuthorityId,
        device: aura_core::DeviceId,
        guardian: bool,
    ) -> (BootstrapBrokerInvitation, InvitationInfo) {
        use aura_signature::invitation::{ShareableInvitation, ShareableInvitationSenderProof};
        let private = aura_core::crypto::ed25519::Ed25519SigningKey::from_bytes([192; 32]);
        let public = private.verifying_key().expect("fixture public key");
        let sender = AuthorityId::new_from_entropy(aura_core::hash::hash(public.as_bytes()));
        let invitation_type = if guardian {
            aura_core::invitation::InvitationType::Guardian {
                subject_authority: sender,
            }
        } else {
            aura_core::invitation::InvitationType::Contact { nickname: None }
        };
        let payload = ShareableInvitation {
            version: ShareableInvitation::CURRENT_VERSION,
            invitation_id: aura_core::InvitationId::new(if guardian {
                "batch guardian"
            } else {
                "batch contact"
            }),
            sender_id: sender,
            context_id: None,
            invitation_type,
            expires_at: Some(10_000),
            message: None,
        };
        let signature = aura_signature::sign_ed25519_transcript(
            &aura_effects::crypto::RealCryptoHandler::new(),
            &payload.signing_transcript(),
            private.as_bytes(),
        )
        .await
        .expect("canonical fixture signature");
        let code = payload
            .to_signed_code(ShareableInvitationSenderProof {
                scheme: ShareableInvitation::SENDER_PROOF_SCHEME.into(),
                public_key: public.as_bytes().to_vec(),
                signature,
                sender_device_id: None,
                key_epoch: None,
            })
            .expect("canonical fixture code");
        let purpose = payload.invitation_type.purpose();
        let info = InvitationInfo {
            invitation_id: payload.invitation_id,
            sender_id: sender,
            receiver_id: receiver,
            invitation_type: if guardian {
                InvitationBridgeType::Guardian {
                    subject_authority: sender,
                }
            } else {
                InvitationBridgeType::Contact { nickname: None }
            },
            status: crate::runtime_bridge::InvitationBridgeStatus::Pending,
            created_at_ms: 100,
            expires_at_ms: payload.expires_at,
            message: None,
            receiver_nickname: None,
        };
        (
            BootstrapBrokerInvitation {
                recipient_authority_id: receiver.to_string(),
                recipient_device_id: device,
                purpose,
                invitation_code: code,
            },
            info,
        )
    }

    #[tokio::test]
    async fn bootstrap_batch_dispatches_contact_and_guardian_in_original_order() {
        let authority = AuthorityId::new_from_entropy([193; 32]);
        let device = aura_core::DeviceId::new_from_entropy([194; 32]);
        let mut deliveries = Vec::new();
        let mut answers = Vec::new();
        let mut expected = Vec::new();
        for guardian in [false, true] {
            let (delivery, info) = signed_delivery(authority, device, guardian).await;
            deliveries.push(delivery);
            expected.push(info.invitation_id.clone());
            answers.push(async move { Ok(info) }.boxed());
        }
        let supervisor = aura_testkit::task_custody::SupervisedTaskTestkit::new();
        let mut runtime = crate::runtime_bridge::OfflineRuntimeBridge::new(authority);
        runtime.use_time_provider(Arc::new(aura_testkit::time::ManualPhysicalClock::new(100)));
        runtime.use_test_task_spawner(supervisor.owned_spawner());
        runtime.queue_import_answers(answers);
        runtime.queue_bootstrap_answers(vec![async move {
            Ok(aura_core::invitation::BootstrapInvitationBatch {
                recipient_authority_id: authority,
                recipient_device_id: device,
                deliveries,
            })
        }
        .boxed()]);
        let outcome = receive_bootstrap_invitation_batch(
            Arc::new(runtime),
            None,
            Arc::new(|_| panic!("contact and guardian cannot prefill enrollment")),
        )
        .expect("original admission")
        .await
        .expect("original observation");
        let completed = outcome.expect("matching canonical imports");
        assert_eq!(
            completed
                .imported
                .iter()
                .map(|handle| handle.info().invitation_id.clone())
                .collect::<Vec<_>>(),
            expected
        );
        assert!(completed.enrollment_prefill.is_none());
        supervisor.shutdown();
        let reports = supervisor.drain().await.expect("original disposal");
        assert_eq!(reports.len(), 1);
        assert!(reports[0].acknowledged);
        assert!(reports[0].domain_failure.is_none());
        assert!(matches!(reports[0].execution, RequiredTaskExit::Completed));
    }

    #[tokio::test]
    async fn bootstrap_batch_rejects_declared_and_verified_purpose_mismatch() {
        for declared_mismatch in [true, false] {
            let authority = AuthorityId::new_from_entropy([197; 32]);
            let device = aura_core::DeviceId::new_from_entropy([198; 32]);
            let (mut delivery, mut info) = signed_delivery(authority, device, false).await;
            if declared_mismatch {
                delivery.purpose = InvitationPurpose::Guardian;
            } else {
                info.invitation_type = InvitationBridgeType::Guardian {
                    subject_authority: info.sender_id,
                };
            }
            let expected = vec![delivery];
            let deliveries = expected.clone();
            let supervisor = aura_testkit::task_custody::SupervisedTaskTestkit::new();
            let mut runtime = crate::runtime_bridge::OfflineRuntimeBridge::new(authority);
            runtime.use_time_provider(Arc::new(aura_testkit::time::ManualPhysicalClock::new(100)));
            runtime.use_test_task_spawner(supervisor.owned_spawner());
            runtime.queue_import_answers(vec![async move {
                assert!(
                    !declared_mismatch,
                    "declared mismatch cannot enter runtime import"
                );
                Ok(info)
            }
            .boxed()]);
            runtime.queue_bootstrap_answers(vec![async move {
                Ok(aura_core::invitation::BootstrapInvitationBatch {
                    recipient_authority_id: authority,
                    recipient_device_id: device,
                    deliveries,
                })
            }
            .boxed()]);
            let outcome = receive_bootstrap_invitation_batch(
                Arc::new(runtime),
                None,
                Arc::new(|_| panic!("purpose mismatch cannot prefill enrollment")),
            )
            .expect("original admission")
            .await
            .expect("original observation");
            let failure = match outcome {
                Err(failure) => failure,
                Ok(_) => panic!("purpose mismatch must refuse"),
            };
            assert_eq!(failure.refused_deliveries(), expected);
            assert!(failure.completed().imported.is_empty());
            assert!(failure.completed().enrollment_prefill.is_none());
            if declared_mismatch {
                assert!(matches!(
                    failure.reason(),
                    BootstrapBatchFailureReason::Failed(AuraError::Invalid { .. })
                ));
            } else {
                assert!(matches!(
                    failure.reason(),
                    BootstrapBatchFailureReason::Failed(AuraError::PermissionDenied { .. })
                ));
            }
            supervisor.shutdown();
            let reports = supervisor.drain().await.expect("original disposal");
            assert_eq!(reports.len(), 1);
            assert!(reports[0].acknowledged);
            assert!(matches!(reports[0].execution, RequiredTaskExit::Failed(_)));
            assert!(reports[0].cleanup_failure.is_none());
        }
    }

    #[tokio::test]
    async fn bootstrap_batch_cancelled_import_retains_disconnected_original_records() {
        let authority = AuthorityId::new_from_entropy([195; 32]);
        let device = aura_core::DeviceId::new_from_entropy([196; 32]);
        let (delivery, _) = signed_delivery(authority, device, false).await;
        let expected = vec![delivery];
        let deliveries = expected.clone();
        let supervisor = aura_testkit::task_custody::SupervisedTaskTestkit::new();
        let mut runtime = crate::runtime_bridge::OfflineRuntimeBridge::new(authority);
        runtime.use_time_provider(Arc::new(aura_testkit::time::ManualPhysicalClock::new(100)));
        runtime.use_test_task_spawner(supervisor.owned_spawner());
        let (entered, ready) = oneshot::channel();
        let released = Arc::new(std::sync::atomic::AtomicBool::new(false));
        struct ImportBorrow(Arc<std::sync::atomic::AtomicBool>);
        impl Drop for ImportBorrow {
            fn drop(&mut self) {
                self.0.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let original_borrow = released.clone();
        runtime.queue_import_answers(vec![async move {
            let _borrow = ImportBorrow(original_borrow);
            entered.send(()).expect("original import observation");
            futures::future::pending().await
        }
        .boxed()]);
        runtime.queue_bootstrap_answers(vec![async move {
            Ok(aura_core::invitation::BootstrapInvitationBatch {
                recipient_authority_id: authority,
                recipient_device_id: device,
                deliveries,
            })
        }
        .boxed()]);
        let observer = receive_bootstrap_invitation_batch(
            Arc::new(runtime),
            None,
            Arc::new(|_| panic!("contact cannot prefill enrollment")),
        )
        .expect("original admission");
        ready.await.expect("actual import entered");
        drop(observer);
        supervisor.shutdown();
        let reports = supervisor.drain().await.expect("original disposal");
        assert_eq!(reports.len(), 1);
        assert!(released.load(std::sync::atomic::Ordering::SeqCst));
        assert!(reports[0].acknowledged);
        assert!(matches!(reports[0].execution, RequiredTaskExit::Cancelled));
        let retained = reports[0]
            .domain_failure
            .as_ref()
            .expect("disconnected result retained")
            .source()
            .expect("actual retained batch")
            .downcast_ref::<BootstrapBatchFailure>()
            .expect("typed original refusal");
        assert!(matches!(
            retained.reason(),
            BootstrapBatchFailureReason::Cancelled
        ));
        assert_eq!(retained.refused_deliveries(), expected);
        assert!(retained.completed().imported.is_empty());
        assert!(reports[0].cleanup_failure.is_none());
    }

    #[tokio::test]
    async fn bootstrap_batch_received_records_survive_clock_and_recipient_refusal() {
        use aura_core::invitation::BootstrapInvitationBatch;
        for foreign_recipient in [false, true] {
            let authority = AuthorityId::new_from_entropy([189; 32]);
            let device = aura_core::DeviceId::new_from_entropy([190; 32]);
            let selected = if foreign_recipient {
                AuthorityId::new_from_entropy([191; 32])
            } else {
                authority
            };
            // Deliberately opaque transport observations: neither failure may
            // reach canonical decoding, import, or enrollment prefill.
            let deliveries: Vec<_> = [
                InvitationPurpose::Contact,
                InvitationPurpose::Guardian,
                InvitationPurpose::DeviceEnrollment,
            ]
            .into_iter()
            .enumerate()
            .map(|(index, purpose)| BootstrapBrokerInvitation {
                recipient_authority_id: selected.to_string(),
                recipient_device_id: device,
                purpose,
                invitation_code: format!("unprocessed-original-record-{index}"),
            })
            .collect();
            let expected = deliveries.clone();
            let clock = aura_testkit::time::ManualPhysicalClock::new(100);
            let supervisor = aura_testkit::task_custody::SupervisedTaskTestkit::new();
            let mut runtime = crate::runtime_bridge::OfflineRuntimeBridge::new(authority);
            runtime.use_time_provider(Arc::new(clock.clone()));
            runtime.use_test_task_spawner(supervisor.owned_spawner());
            runtime.queue_bootstrap_answers(vec![async move {
                if !foreign_recipient {
                    clock
                        .fail_next_observation(aura_core::effects::TimeError::ServiceUnavailable)
                        .await;
                }
                Ok(BootstrapInvitationBatch {
                    recipient_authority_id: selected,
                    recipient_device_id: device,
                    deliveries,
                })
            }
            .boxed()]);
            let result = receive_bootstrap_invitation_batch(
                Arc::new(runtime),
                None,
                Arc::new(|_| panic!("refused batch cannot prefill")),
            )
            .expect("original admission")
            .await
            .expect("original observation");
            let failure = match result {
                Err(failure) => failure,
                Ok(_) => panic!("batch must refuse"),
            };
            assert_eq!(failure.refused_deliveries(), expected);
            assert!(failure.completed().imported.is_empty());
            assert!(failure.completed().enrollment_prefill.is_none());
            assert!(matches!(
                failure.reason(),
                BootstrapBatchFailureReason::Failed(_)
            ));
            supervisor.shutdown();
            let reports = supervisor.drain().await.expect("actual required disposal");
            assert_eq!(reports.len(), 1);
            assert!(reports[0].acknowledged);
            assert!(matches!(reports[0].execution, RequiredTaskExit::Failed(_)));
            assert!(reports[0].domain_failure.is_some());
            assert!(reports[0].cleanup_failure.is_none());
        }
    }

    #[tokio::test]
    async fn bootstrap_batch_native_failure_precedes_postread_clock_failure() {
        let native = Arc::new(std::io::Error::from(std::io::ErrorKind::ConnectionReset));
        let clock_native = Arc::new(std::io::Error::from(std::io::ErrorKind::BrokenPipe));
        let clock_cause = clock_native.clone();
        let source = crate::runtime_bridge::RuntimeBridgeError::with_source(
            crate::IntentError::internal_error("original broker read failed"),
            AuraError::Network {
                message: "native broker refusal".into(),
                source: Some(native.clone()),
            },
        );
        let clock = aura_testkit::time::ManualPhysicalClock::new(100);
        let supervisor = aura_testkit::task_custody::SupervisedTaskTestkit::new();
        let mut runtime = crate::runtime_bridge::OfflineRuntimeBridge::new(
            AuthorityId::new_from_entropy([188; 32]),
        );
        runtime.use_time_provider(Arc::new(clock.clone()));
        runtime.use_test_task_spawner(supervisor.owned_spawner());
        runtime.queue_bootstrap_answers(vec![async move {
            clock
                .fail_next_observation(aura_core::effects::TimeError::ProviderFailure {
                    operation: aura_core::effects::time::TimeProviderOperation::ReadPhysicalClock,
                    source: Some(clock_cause),
                })
                .await;
            Err(source)
        }
        .boxed()]);
        let observed = receive_bootstrap_invitation_batch(
            Arc::new(runtime),
            None,
            Arc::new(|_| panic!("failed fetch cannot prefill")),
        )
        .expect("original receiver admission")
        .await
        .expect("original result observation");
        let failure = match observed {
            Err(failure) => failure,
            Ok(_) => panic!("native fetch must refuse"),
        };
        fn contains_native(error: &(dyn Error + 'static), native: &std::io::Error) -> bool {
            let mut current = Some(error);
            while let Some(error) = current {
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|retained| std::ptr::eq(retained, native))
                {
                    return true;
                }
                current = error.source();
            }
            false
        }
        assert!(contains_native(&failure, &native));
        assert!(contains_native(
            failure
                .observation_failure()
                .expect("actual later clock failure"),
            &clock_native,
        ));
        supervisor.shutdown();
        let reports = supervisor.drain().await.expect("actual required cleanup");
        assert_eq!(reports.len(), 1);
        let report = &reports[0];
        assert!(report.acknowledged);
        assert!(report.cleanup_failure.is_none());
        let RequiredTaskExit::Failed(source) = &report.execution else {
            panic!("original execution must fail")
        };
        assert!(
            contains_native(source, &native),
            "execution must retain the original native source"
        );
    }
}
