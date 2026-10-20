//! Original cancellation publication state retained by its required task owner.
use super::*;

struct OriginalCancellationPublication {
    record: crate::handlers::invitation::SenderInvitationRecordCapability,
    issued: Arc<crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl>,
    prepared: Option<crate::handlers::invitation::AuthorizedInvitationCancellationCapability>,
    cancelled: Option<
        Arc<crate::runtime::services::ceremony_tracker::VerifiedEnrollmentCancellationCapability>,
    >,
    result: Option<InvitationResult>,
    first_failure: Option<aura_core::AuraError>,
}
impl OriginalCancellationPublication {
    async fn publish(
        &mut self,
        handler: &InvitationHandler,
        effects: &Arc<AuraEffectSystem>,
        runner: &CeremonyRunner,
    ) -> Result<(), aura_core::AuraError> {
        if let Some(source) = &self.first_failure {
            return Err(source.clone());
        }
        let result = self.publish_original(handler, effects, runner).await;
        if let Err(source) = &result {
            self.first_failure.get_or_insert_with(|| source.clone());
        }
        result
    }
    async fn publish_original(
        &mut self,
        handler: &InvitationHandler,
        effects: &Arc<AuraEffectSystem>,
        runner: &CeremonyRunner,
    ) -> Result<(), aura_core::AuraError> {
        if self.prepared.is_none() {
            let prepared = handler
                .prepare_enrollment_cancellation(&self.issued, &self.record)
                .await
                .map_err(cancellation_native_source)?;
            self.prepared = Some(prepared);
        }
        if self.cancelled.is_none() {
            let cancelled = runner.cancel_verified_enrollment(&self.issued).await?;
            self.cancelled = Some(Arc::new(cancelled));
        }
        if self.result.is_none() {
            let prepared = self.prepared.as_mut().ok_or_else(|| {
                aura_core::AuraError::invalid("original cancellation preparation missing")
            })?;
            let cancelled = self.cancelled.as_ref().ok_or_else(|| {
                aura_core::AuraError::invalid("original cancellation decision missing")
            })?;
            let result = handler
                .publish_verified_enrollment_cancellation(
                    effects.clone(),
                    prepared,
                    cancelled.as_ref(),
                )
                .await
                .map_err(cancellation_native_source)?;
            // The actual publication result belongs to this holder before Ready.
            self.result = Some(result);
        }
        Ok(())
    }
}
fn cancellation_native_source(source: AgentError) -> aura_core::AuraError {
    match source {
        AgentError::Aura(source) => source,
        source => aura_core::AuraError::Internal {
            message: "original cancellation operation failed".into(),
            source: Some(Arc::new(source)),
        },
    }
}

#[derive(Debug, thiserror::Error)]
#[error(
    "original cancellation failed: {primary}; independent original issuer cleanup: {secondary}"
)]
pub(crate) struct OriginalCancellationCleanupFailure {
    #[source]
    primary: aura_core::AuraError,
    secondary: aura_core::AuraError,
}
#[cfg(all(test, not(target_arch = "wasm32")))]
impl OriginalCancellationCleanupFailure {
    pub(crate) fn primary(&self) -> &aura_core::AuraError {
        &self.primary
    }
    pub(crate) fn secondary(&self) -> &aura_core::AuraError {
        &self.secondary
    }
}

fn retain_cancellation_cleanup_cause(
    primary: aura_core::AuraError,
    secondary: aura_core::AuraError,
) -> aura_core::AuraError {
    aura_core::AuraError::Internal {
        message: "original cancellation and original issuer cleanup failures retained".into(),
        source: Some(Arc::new(OriginalCancellationCleanupFailure {
            primary,
            secondary,
        })),
    }
}

struct OriginalCancellationTaskCustody {
    service: Option<InvitationServiceApi>,
    registration:
        Arc<crate::runtime::services::ceremony_tracker::OriginalCancellationTaskRegistration>,
    admission: Option<tokio::sync::oneshot::Receiver<()>>,
    publication: OriginalCancellationPublication,
    preparation: Option<
        crate::runtime::services::enrollment_window::EnrollmentCancellationPreparationCapability,
    >,
    checkpoint: Option<aura_core::time::timeout::CheckpointExecution<(), aura_core::AuraError, ()>>,
    original_retirement:
        Option<Arc<crate::runtime::services::ceremony_tracker::RunningIssuerObservation>>,
    retirement_observed: bool,
    original_issuer_settlement:
        Option<crate::runtime::services::ceremony_tracker::OriginalIssuerCancellationSettlement>,
    notice: Option<crate::handlers::invitation::RetainedCancelledNotice>,
    acknowledged_notice: Option<crate::handlers::invitation::AcknowledgedCancelledNotice>,
    completion:
        Option<tokio::sync::oneshot::Sender<Result<InvitationResult, aura_core::AuraError>>>,
    first_failure: Option<aura_core::AuraError>,
}
impl OriginalCancellationTaskCustody {
    async fn cleanup_observed_original(
        &mut self,
        exit: &aura_core::effects::task::RequiredTaskExit,
    ) -> Result<aura_core::effects::task::RequiredTaskSettlement, aura_core::AuraError> {
        let actual = self.cleanup_original(exit).await;
        #[cfg(test)]
        self.registration
            .publish_cleanup_observation_for_test(actual.as_ref().map(|_| ()).map_err(Clone::clone))
            .await;
        actual
    }

    async fn execute_original(&mut self) -> Result<(), aura_core::AuraError> {
        let result = self.execute_stages().await;
        if let Err(source) = &result {
            self.first_failure.get_or_insert_with(|| source.clone());
            self.registration.retain_completion(Err(source.clone()));
            if let Some(sender) = self.completion.take() {
                let _ = sender.send(Err(source.clone()));
            }
        }
        result
    }
    async fn execute_stages(&mut self) -> Result<(), aura_core::AuraError> {
        let service = self.service.as_ref().cloned().ok_or_else(|| {
            aura_core::AuraError::invalid("original cancellation already settled")
        })?;
        if let Some(admission) = self.admission.take() {
            self.registration
                .observe_publication(service.effects.as_ref(), admission)
                .await?;
        }
        if self.preparation.is_none() {
            self.original_retirement = service
                .ceremony_runner
                .original_issuer_retirement_observation(&self.publication.issued)?;
            let preparation = service
                .ceremony_runner
                .original_cancellation_preparation(
                    &self.publication.issued,
                    service.effects.as_ref(),
                )
                .await?;
            self.preparation = Some(preparation);
        }
        if matches!(self.preparation.as_ref(), Some(
            crate::runtime::services::enrollment_window::EnrollmentCancellationPreparationCapability::Decided(_))) {
            if let Some(crate::runtime::services::enrollment_window::EnrollmentCancellationPreparationCapability::Decided(actual)) = self.preparation.take() {
                self.publication.cancelled = Some(Arc::new(actual));
            }
            self.publication.publish(&service.handler, &service.effects,
                &service.ceremony_runner).await?;
        } else if let Some(crate::runtime::services::enrollment_window::EnrollmentCancellationPreparationCapability::Active(window)) = self.preparation.as_mut() {
            window.admit_original(service.effects.as_ref()).await;
            require_original_cancellation_observation(window.admitted_observation())?;
            let publication = &mut self.publication;
            let service = &service;
            let actual = window.execute(service.effects.as_ref(), || async {
                publication.publish(&service.handler, &service.effects, &service.ceremony_runner)
                    .await
            }).await.map_err(aura_core::AuraError::from)?;
            self.checkpoint = Some(actual);
            require_original_cancellation_observation(self.checkpoint.as_ref())?;
        }
        if let Some(original) = &self.original_retirement {
            original
                .observe_original_retirement(service.effects.as_ref(), &self.publication.issued)
                .await
                .map_err(cancellation_native_source)?;
            self.retirement_observed = true;
        } else {
            if self.notice.is_none() {
                let registered = service
                    .ceremony_runner
                    .prepare_cancelled_notice(&self.publication.issued, service.effects.as_ref())
                    .await?;
                self.notice = Some(crate::handlers::invitation::RetainedCancelledNotice::new(
                    registered,
                ));
            }
            let notice = self
                .notice
                .as_mut()
                .ok_or_else(|| aura_core::AuraError::invalid("original notice custody missing"))?;
            #[cfg(test)]
            notice.observe_registration_for_test(Arc::clone(&self.registration));
            notice
                .sign_original(
                    &service.effects,
                    &self.publication.issued,
                    &service.ceremony_runner,
                )
                .await
                .map_err(cancellation_native_source)?;
            notice
                .retire_original(
                    &service.effects,
                    &self.publication.issued,
                    &service.ceremony_runner,
                )
                .await
                .map_err(cancellation_native_source)?;
            notice
                .publish_settlement(&service.ceremony_runner, &self.publication.issued)
                .await;
        }
        let result = self.publication.result.as_ref().ok_or_else(|| {
            aura_core::AuraError::invalid("original cancellation publication missing")
        })?;
        self.registration.retain_completion(Ok(result.clone()));
        if let Some(sender) = self.completion.take() {
            let _ = sender.send(Ok(result.clone()));
        }
        // Subsidiary delivery runs only after required terminal publication.
        if let Some(notice) = self.notice.as_mut() {
            if let Err(source) = notice
                .deliver_original(&service.effects, &self.publication.issued)
                .await
            {
                service
                    .tasks
                    .group("invitation_service.cancelled_notice_delivery")
                    .record_subsidiary_failure(
                        "settled-negative-notice",
                        cancellation_native_source(source),
                    );
            }
        }
        Ok(())
    }
    async fn cleanup_original(
        &mut self,
        _exit: &aura_core::effects::task::RequiredTaskExit,
    ) -> Result<aura_core::effects::task::RequiredTaskSettlement, aura_core::AuraError> {
        use aura_core::effects::task::RequiredTaskSettlement;
        let service = self.service.as_ref().cloned();
        if self.acknowledged_notice.is_none() && !self.retirement_observed {
            let service = service.as_ref().ok_or_else(|| {
                aura_core::AuraError::invalid("original cancellation context missing")
            })?;
            if let Some(notice) = self.notice.as_mut() {
                notice
                    .retire_original(
                        &service.effects,
                        &self.publication.issued,
                        &service.ceremony_runner,
                    )
                    .await
                    .map_err(cancellation_native_source)?;
                notice
                    .publish_settlement(&service.ceremony_runner, &self.publication.issued)
                    .await;
                notice
                    .acknowledge_negative(&service.effects, &self.publication.issued)
                    .await
                    .map_err(cancellation_native_source)?;
                // No awaited gap while the original owner changes terminal state.
                let original = self.notice.take().ok_or_else(|| {
                    aura_core::AuraError::invalid("original notice owner missing")
                })?;
                match original
                    .consume_acknowledged(&self.publication.issued, service.effects.as_ref())
                {
                    Ok(actual) => self.acknowledged_notice = Some(actual),
                    Err((actual, source)) => {
                        self.notice = Some(*actual);
                        return Err(cancellation_native_source(source));
                    }
                }
            } else if !self.retirement_observed {
                if let Some(original) = &self.original_retirement {
                    let actual = original
                        .observe_original_cancellation_settlement(
                            service.effects.as_ref(),
                            &self.publication.issued,
                        )
                        .await?;
                    // Retain the actual same-control receipt before marking the
                    // physical phase complete; its failed outcome is independent.
                    self.original_issuer_settlement = Some(actual);
                    self.retirement_observed = true;
                } else {
                    return Err(self.first_failure.clone().unwrap_or_else(|| {
                        aura_core::AuraError::permission_denied(
                            "original cancellation cleanup unresolved",
                        )
                    }));
                }
            }
        }
        let failure = self
            .first_failure
            .as_ref()
            .or(self.publication.first_failure.as_ref())
            .or_else(|| {
                self.acknowledged_notice
                    .as_ref()
                    .and_then(|actual| actual.domain_failure())
            });
        let mut failure = failure.cloned();
        if let Some(actual) = &self.original_issuer_settlement {
            for source in actual
                .cleanup_failure()
                .into_iter()
                .chain(actual.independent_failures().iter().cloned())
            {
                failure = Some(match failure {
                    Some(primary) => retain_cancellation_cleanup_cause(primary, source),
                    None => source,
                });
            }
        }
        self.service = None;
        Ok(match failure {
            Some(source) => RequiredTaskSettlement::ResolvedWithDomainFailure(source),
            None => RequiredTaskSettlement::Resolved,
        })
    }
}
fn require_original_cancellation_observation(
    observation: Option<
        &aura_core::time::timeout::CheckpointExecution<(), aura_core::AuraError, ()>,
    >,
) -> Result<(), aura_core::AuraError> {
    use aura_core::time::timeout::CheckpointExecution;
    match observation {
        Some(CheckpointExecution::Finished {
            operation: Err(source),
            ..
        })
        | Some(CheckpointExecution::Negative {
            operation: Some(Err(source)),
            ..
        })
        | Some(CheckpointExecution::Unresolved {
            operation: Some(Err(source)),
            ..
        }) => Err(source.clone()),
        Some(CheckpointExecution::Finished {
            operation: Ok(()), ..
        }) => Ok(()),
        Some(CheckpointExecution::Negative { source, .. }) => Err(source.clone().into()),
        Some(CheckpointExecution::Unresolved {
            validation: Err(source),
            ..
        }) => Err(source.clone().into()),
        _ => Err(aura_core::AuraError::permission_denied(
            "original cancellation observation unresolved",
        )),
    }
}
#[cfg(not(target_arch = "wasm32"))]
impl aura_core::effects::task::RequiredTaskCustody for OriginalCancellationTaskCustody {
    fn execute(&mut self) -> futures::future::BoxFuture<'_, Result<(), aura_core::AuraError>> {
        Box::pin(self.execute_original())
    }
    fn cleanup<'a>(
        &'a mut self,
        exit: &'a aura_core::effects::task::RequiredTaskExit,
    ) -> futures::future::BoxFuture<
        'a,
        Result<aura_core::effects::task::RequiredTaskSettlement, aura_core::AuraError>,
    > {
        Box::pin(self.cleanup_observed_original(exit))
    }
}
#[cfg(target_arch = "wasm32")]
impl aura_core::effects::task::LocalRequiredTaskCustody for OriginalCancellationTaskCustody {
    fn execute(&mut self) -> futures::future::LocalBoxFuture<'_, Result<(), aura_core::AuraError>> {
        Box::pin(self.execute_original())
    }
    fn cleanup<'a>(
        &'a mut self,
        exit: &'a aura_core::effects::task::RequiredTaskExit,
    ) -> futures::future::LocalBoxFuture<
        'a,
        Result<aura_core::effects::task::RequiredTaskSettlement, aura_core::AuraError>,
    > {
        Box::pin(self.cleanup_observed_original(exit))
    }
}

struct CancelledNoticeTaskCustody {
    service: Option<InvitationServiceApi>,
    registration:
        Arc<crate::runtime::services::ceremony_tracker::OriginalCancellationTaskRegistration>,
    admission: Option<tokio::sync::oneshot::Receiver<()>>,
    issued: crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    notice: Option<crate::handlers::invitation::RetainedCancelledNotice>,
    acknowledged: Option<crate::handlers::invitation::AcknowledgedCancelledNotice>,
    completion: Option<tokio::sync::oneshot::Sender<Result<(), aura_core::AuraError>>>,
    first_failure: Option<aura_core::AuraError>,
}
impl CancelledNoticeTaskCustody {
    async fn execute_original(&mut self) -> Result<(), aura_core::AuraError> {
        let result = self.execute_stages().await;
        if let Err(source) = &result {
            self.first_failure.get_or_insert_with(|| source.clone());
            if let Some(sender) = self.completion.take() {
                let _ = sender.send(Err(source.clone()));
            }
        }
        result
    }
    async fn execute_stages(&mut self) -> Result<(), aura_core::AuraError> {
        let service = self.service.as_ref().cloned().ok_or_else(|| {
            aura_core::AuraError::invalid("original cancellation already settled")
        })?;
        if let Some(admission) = self.admission.take() {
            self.registration
                .observe_publication(service.effects.as_ref(), admission)
                .await?;
        }
        let notice = self
            .notice
            .as_mut()
            .ok_or_else(|| aura_core::AuraError::invalid("original notice missing"))?;
        #[cfg(test)]
        notice.observe_registration_for_test(Arc::clone(&self.registration));
        notice
            .sign_original(&service.effects, &self.issued, &service.ceremony_runner)
            .await
            .map_err(cancellation_native_source)?;
        notice
            .retire_original(&service.effects, &self.issued, &service.ceremony_runner)
            .await
            .map_err(cancellation_native_source)?;
        notice
            .publish_settlement(&service.ceremony_runner, &self.issued)
            .await;
        if let Some(sender) = self.completion.take() {
            let _ = sender.send(Ok(()));
        }
        if let Err(source) = notice
            .deliver_original(&service.effects, &self.issued)
            .await
        {
            service
                .tasks
                .group("invitation_service.cancelled_notice_delivery")
                .record_subsidiary_failure(
                    "settled-negative-notice",
                    cancellation_native_source(source),
                );
        }
        Ok(())
    }
    async fn cleanup_original(
        &mut self,
    ) -> Result<aura_core::effects::task::RequiredTaskSettlement, aura_core::AuraError> {
        let service = self.service.as_ref().cloned();
        if self.acknowledged.is_none() {
            let service = service.as_ref().ok_or_else(|| {
                aura_core::AuraError::invalid("original cancellation context missing")
            })?;
            let notice = self
                .notice
                .as_mut()
                .ok_or_else(|| aura_core::AuraError::invalid("original notice missing"))?;
            notice
                .retire_original(&service.effects, &self.issued, &service.ceremony_runner)
                .await
                .map_err(cancellation_native_source)?;
            notice
                .publish_settlement(&service.ceremony_runner, &self.issued)
                .await;
            notice
                .acknowledge_negative(&service.effects, &self.issued)
                .await
                .map_err(cancellation_native_source)?;
            let actual = self
                .notice
                .take()
                .ok_or_else(|| aura_core::AuraError::invalid("original notice missing"))?;
            match actual.consume_acknowledged(&self.issued, service.effects.as_ref()) {
                Ok(actual) => self.acknowledged = Some(actual),
                Err((actual, source)) => {
                    self.notice = Some(*actual);
                    return Err(cancellation_native_source(source));
                }
            }
        }
        let settlement = match self.first_failure.as_ref().or_else(|| {
            self.acknowledged
                .as_ref()
                .and_then(|actual| actual.domain_failure())
        }) {
            Some(source) => {
                aura_core::effects::task::RequiredTaskSettlement::ResolvedWithDomainFailure(
                    source.clone(),
                )
            }
            None => aura_core::effects::task::RequiredTaskSettlement::Resolved,
        };
        self.service = None;
        Ok(settlement)
    }
}
#[cfg(not(target_arch = "wasm32"))]
impl aura_core::effects::task::RequiredTaskCustody for CancelledNoticeTaskCustody {
    fn execute(&mut self) -> futures::future::BoxFuture<'_, Result<(), aura_core::AuraError>> {
        Box::pin(self.execute_original())
    }
    fn cleanup<'a>(
        &'a mut self,
        _: &'a aura_core::effects::task::RequiredTaskExit,
    ) -> futures::future::BoxFuture<
        'a,
        Result<aura_core::effects::task::RequiredTaskSettlement, aura_core::AuraError>,
    > {
        Box::pin(self.cleanup_original())
    }
}
#[cfg(target_arch = "wasm32")]
impl aura_core::effects::task::LocalRequiredTaskCustody for CancelledNoticeTaskCustody {
    fn execute(&mut self) -> futures::future::LocalBoxFuture<'_, Result<(), aura_core::AuraError>> {
        Box::pin(self.execute_original())
    }
    fn cleanup<'a>(
        &'a mut self,
        _: &'a aura_core::effects::task::RequiredTaskExit,
    ) -> futures::future::LocalBoxFuture<
        'a,
        Result<aura_core::effects::task::RequiredTaskSettlement, aura_core::AuraError>,
    > {
        Box::pin(self.cleanup_original())
    }
}
impl InvitationServiceApi {
    pub(super) async fn cancel_with_original_custody(
        &self,
        record: crate::handlers::invitation::SenderInvitationRecordCapability,
        issued: Arc<crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl>,
    ) -> AgentResult<InvitationResult> {
        if let Some(original) = self.ceremony_runner.existing_original_cancellation_task(
            &issued, crate::runtime::services::ceremony_tracker::OriginalCancellationTaskRole::Publication,
        ).map_err(AgentError::from)? {
            return original.observe_completion(self.effects.as_ref()).await.map_err(AgentError::from);
        }
        let registration = self.ceremony_runner.prepare_original_cancellation_task(&issued,
            crate::runtime::services::ceremony_tracker::OriginalCancellationTaskRole::Publication).map_err(AgentError::from)?;
        let (publication_sender, publication_receiver) = tokio::sync::oneshot::channel();
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let custody = OriginalCancellationTaskCustody {
            service: Some(self.clone()),
            registration: Arc::clone(&registration),
            admission: Some(publication_receiver),
            publication: OriginalCancellationPublication {
                record,
                issued,
                prepared: None,
                cancelled: None,
                result: None,
                first_failure: None,
            },
            preparation: None,
            checkpoint: None,
            original_retirement: None,
            retirement_observed: false,
            original_issuer_settlement: None,
            notice: None,
            acknowledged_notice: None,
            completion: Some(sender),
            first_failure: None,
        };
        let tasks = self.tasks.group("invitation_service.original_cancellation");
        cfg_if::cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                let admitted = tasks.spawn_original_local_required_custody_with_resident_cleanup(
                    "original-enrollment-cancellation".into(), Box::new(custody), None,
                ).map_err(|rejected| retain_rejected_local_issuer(rejected.into_local()))?;
            } else {
                let admitted = tasks.spawn_original_required_custody_with_resident_cleanup(
                    "original-enrollment-cancellation".into(), Box::new(custody), None,
                ).map_err(|rejected| retain_rejected_native_issuer(rejected.into_native()))?;
            }
        }
        let mut pending = Some(admitted);
        if let Err(source) = registration.publish(&mut pending) {
            return Err(AgentError::from(aura_core::AuraError::Internal {
                message: "publish original cancellation task control".into(),
                source: Some(Arc::new(UnpublishedIssuerTaskCustody {
                    source,
                    #[allow(clippy::disallowed_types)] // Error retains rejected custody; no lock is awaited or acquired.
                    retained: crate::runtime::services::issuer_error_custody::SynchronousCustodyCell::new(pending),
                })),
            }));
        }
        publication_sender.send(()).map_err(|_| {
            AgentError::from(aura_core::AuraError::invalid(
                "original cancellation admission observer lost",
            ))
        })?;
        receiver
            .await
            .map_err(|source| {
                AgentError::from(aura_core::AuraError::Internal {
                    message: "original cancellation completion observer lost".into(),
                    source: Some(Arc::new(source)),
                })
            })?
            .map_err(AgentError::from)
    }
    pub(super) fn admit_cancelled_notice_custody(
        &self,
        capability: CancelledEnrollmentNoticeRecoveryCapability,
        completion: Option<tokio::sync::oneshot::Sender<Result<(), aura_core::AuraError>>>,
    ) -> AgentResult<()> {
        if !Arc::ptr_eq(&capability.effects, &self.effects) {
            return Err(
                crate::handlers::invitation::enrollment_trust::EnrollmentVerifierError::RuntimeOwner.into(),
            );
        }
        let registration = self
            .ceremony_runner
            .prepare_original_cancellation_task(
                &capability.issued,
                crate::runtime::services::ceremony_tracker::OriginalCancellationTaskRole::Notice,
            )
            .map_err(AgentError::from)?;
        self.admit_cancelled_notice_with_original_registration(capability, completion, registration)
    }
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "OriginalCancellationTaskRegistration",
        family = "runtime_helper"
    )]
    pub(super) fn admit_cancelled_notice_with_original_registration(
        &self,
        capability: CancelledEnrollmentNoticeRecoveryCapability,
        completion: Option<tokio::sync::oneshot::Sender<Result<(), aura_core::AuraError>>>,
        registration: Arc<
            crate::runtime::services::ceremony_tracker::OriginalCancellationTaskRegistration,
        >,
    ) -> AgentResult<()> {
        registration
            .require_notice_input(&capability.issued, self.effects.as_ref())
            .map_err(AgentError::from)?;
        let (publication_sender, publication_receiver) = tokio::sync::oneshot::channel();
        let custody = CancelledNoticeTaskCustody {
            service: Some(self.clone()),
            registration: Arc::clone(&registration),
            admission: Some(publication_receiver),
            issued: capability.issued,
            notice: Some(crate::handlers::invitation::RetainedCancelledNotice::new(
                capability.registered,
            )),
            acknowledged: None,
            completion,
            first_failure: None,
        };
        let tasks = self
            .tasks
            .group("invitation_service.original_cancelled_notice");
        cfg_if::cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                let admitted = tasks.spawn_original_local_required_custody_with_resident_cleanup(
                    "original-cancelled-notice".into(), Box::new(custody), None,
                ).map_err(|rejected| retain_rejected_local_issuer(rejected.into_local()))?;
            } else {
                let admitted = tasks.spawn_original_required_custody_with_resident_cleanup(
                    "original-cancelled-notice".into(), Box::new(custody), None,
                ).map_err(|rejected| retain_rejected_native_issuer(rejected.into_native()))?;
            }
        }
        let mut pending = Some(admitted);
        if let Err(source) = registration.publish(&mut pending) {
            return Err(AgentError::from(aura_core::AuraError::Internal {
                message: "publish original cancellation task control".into(),
                source: Some(Arc::new(UnpublishedIssuerTaskCustody {
                    source,
                    #[allow(clippy::disallowed_types)] // Error retains rejected custody; no lock is awaited or acquired.
                    retained: crate::runtime::services::issuer_error_custody::SynchronousCustodyCell::new(pending),
                })),
            }));
        }
        publication_sender.send(()).map_err(|_| {
            AgentError::from(aura_core::AuraError::invalid(
                "original cancellation admission observer lost",
            ))
        })?;
        Ok(())
    }
}
#[cfg(test)]
mod cancellation_observation_tests {
    use super::require_original_cancellation_observation;
    use aura_core::time::timeout::{CheckpointAttempt, CheckpointExecution};
    use aura_core::{AuraError, TimeoutBudgetError};
    use std::sync::Arc;

    #[test]
    fn original_cancellation_observation_preserves_native_operation_error_across_checkpoint_outcomes(
    ) {
        let original = Arc::new(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "original cancellation publication producer failed",
        ));
        let cause = AuraError::Internal {
            message: "actual cancellation publication".into(),
            source: Some(original.clone()),
        };
        let checkpoint_failure = TimeoutBudgetError::checkpoint_failure(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "independent post-publication checkpoint failed",
        ));
        let outcomes = [
            CheckpointExecution::Finished {
                operation: Err(cause.clone()),
                checkpoint: (),
            },
            CheckpointExecution::Negative {
                source: checkpoint_failure.clone(),
                validation: Err(checkpoint_failure.clone()),
                operation: Some(Err(cause.clone())),
                checkpoint: CheckpointAttempt::Failed {
                    source: checkpoint_failure.clone(),
                },
            },
            CheckpointExecution::Unresolved {
                operation: Some(Err(cause)),
                checkpoint: CheckpointAttempt::Interrupted,
                validation: Err(checkpoint_failure),
            },
        ];
        for actual in &outcomes {
            let returned = require_original_cancellation_observation(Some(actual))
                .expect_err("native publication failure cannot be successful");
            let AuraError::Internal {
                source: Some(source),
                ..
            } = returned
            else {
                panic!("original native cause was replaced");
            };
            let native = source
                .downcast_ref::<std::io::Error>()
                .expect("original concrete native failure");
            assert!(std::ptr::eq(native, original.as_ref()));
            assert_eq!(native.kind(), std::io::ErrorKind::BrokenPipe);
        }
        // Borrowed observation must also leave independent checkpoint failure
        // and interrupted custody in the original complete outcome.
        assert!(matches!(
            outcomes[1],
            CheckpointExecution::Negative {
                checkpoint: CheckpointAttempt::Failed { .. },
                ..
            }
        ));
        assert!(matches!(
            outcomes[2],
            CheckpointExecution::Unresolved {
                checkpoint: CheckpointAttempt::Interrupted,
                ..
            }
        ));
    }
}
