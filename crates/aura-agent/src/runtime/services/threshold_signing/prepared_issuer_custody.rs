//! Original prepared issuance is borrowed by the canonical required task.
use super::OriginalPreparedIssuerRegistration;
use crate::runtime::AuraEffectSystem;
use crate::runtime_bridge::enrollment_quorum::OriginalPreparedIssuerResources;
use aura_core::effects::task::{RequiredTaskExit, RequiredTaskSettlement};
use aura_core::AuraError;
use std::sync::{Arc, OnceLock};

/// Actual one-shot issuance inputs; borrowed execution futures stay on their
/// selected executor while original resource checkpoints remain in retained.
pub(super) struct OriginalPreparedIssuerOperation {
    pub(super) started: tokio::sync::oneshot::Receiver<()>,
    pub(super) agent: Arc<crate::core::AuraAgent>,
    pub(super) nickname: String,
    pub(super) setup: aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentSetup,
    pub(super) preparation: crate::runtime_bridge::enrollment_quorum::OriginalIssuerPreparation,
    pub(super) completed: tokio::sync::oneshot::Sender<()>,
    pub(super) result: super::enrollment_quorum_registry::OriginalIssuerCompletionSlot,
    pub(super) mode: crate::runtime_bridge::enrollment_quorum::OriginalIssuerLaunchMode,
    pub(super) ready: OriginalPreparedIssuerReadySender,
}
impl OriginalPreparedIssuerOperation {
    async fn execute(self) -> Result<(), AuraError> {
        self.started.await.map_err(|source| AuraError::Internal {
            message: "original prepared issuer admission ended".into(),
            source: Some(Arc::new(source)),
        })?;
        let bridge = crate::runtime_bridge::AgentRuntimeBridge::new(self.agent);
        match bridge
            .execute_original_device_enrollment(self.nickname, self.setup, self.preparation)
            .await
        {
            Ok(result) => {
                self.result.with(|slot| *slot = Some(Ok(result)));
                let _ = self.completed.send(());
                if self.mode == crate::runtime_bridge::enrollment_quorum::OriginalIssuerLaunchMode::SingleSigner {
                    if let Some(ready) = self.ready.lock().take() {
                        let _ = ready.send(Ok(crate::runtime_bridge::enrollment_quorum::OriginalPreparedIssuerReady::Issued));
                    }
                }
                Ok(())
            }
            Err(source) => {
                let native = AuraError::Internal {
                    message: "original owned enrollment issuance failed".into(),
                    source: Some(Arc::new(source)),
                };
                self.result.with(|slot| *slot = Some(Err(native.clone())));
                let _ = self.completed.send(());
                Err(native)
            }
        }
    }
}

#[allow(
    clippy::disallowed_types,
    reason = "Synchronous one-shot ownership deposit; guard never crosses an await"
)]
type OriginalPreparedIssuerReadySender = Arc<
    parking_lot::Mutex<
        Option<
            tokio::sync::oneshot::Sender<
                Result<
                    crate::runtime_bridge::enrollment_quorum::OriginalPreparedIssuerReady,
                    AuraError,
                >,
            >,
        >,
    >,
>;

pub(super) struct OriginalPreparedIssuerTaskCustody {
    pub(super) operation: Option<OriginalPreparedIssuerOperation>,
    pub(super) retained: Arc<tokio::sync::Mutex<OriginalPreparedIssuerResources>>,
    pub(super) registration: Arc<OnceLock<OriginalPreparedIssuerRegistration>>,
    pub(super) effects: Arc<AuraEffectSystem>,
    pub(super) first_failure: Option<AuraError>,
    #[allow(
        clippy::disallowed_types,
        reason = "Synchronous runtime custody deposit; the lock guard never crosses an await"
    )]
    pub(super) ready: OriginalPreparedIssuerReadySender,
}

#[derive(Debug, thiserror::Error)]
#[error("the original prepared enrollment execution was cancelled")]
struct OriginalPreparedIssuerCancelled;
#[derive(Debug, thiserror::Error)]
#[error("the original prepared enrollment execution panicked")]
struct OriginalPreparedIssuerPanicked;

cfg_if::cfg_if! {
    if #[cfg(target_arch = "wasm32")] {
        type RequiredObligation = dyn aura_core::effects::task::LocalRequiredTaskCustody;
    } else {
        type RequiredObligation = dyn aura_core::effects::task::RequiredTaskCustody;
    }
}
#[derive(thiserror::Error)]
#[error("original prepared issuer admission was rejected")]
struct RejectedPreparedIssuer {
    #[source]
    source: AuraError,
    #[allow(
        clippy::disallowed_types,
        reason = "Synchronous runtime custody deposit; the lock guard never crosses an await"
    )]
    _original: parking_lot::Mutex<Box<RequiredObligation>>,
}
impl std::fmt::Debug for RejectedPreparedIssuer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RejectedPreparedIssuer")
    }
}
#[allow(
    clippy::disallowed_types,
    reason = "Synchronous runtime custody deposit; the lock guard never crosses an await"
)]
pub(super) fn retain_rejected(
    rejected: aura_core::effects::task::RejectedRequiredTask<RequiredObligation>,
) -> AuraError {
    AuraError::Internal {
        message: "admit original prepared issuer custody".into(),
        source: Some(Arc::new(RejectedPreparedIssuer {
            source: rejected.source,
            _original: parking_lot::Mutex::new(rejected.obligation),
        })),
    }
}

impl OriginalPreparedIssuerTaskCustody {
    async fn execute_original(&mut self) -> Result<(), AuraError> {
        let result = match self.operation.take() {
            Some(original) => original.execute().await,
            None => {
                return Err(AuraError::invalid(
                    "original prepared execution already ended",
                ))
            }
        };
        if let Err(source) = &result {
            self.first_failure.get_or_insert_with(|| source.clone());
            if let Some(ready) = self.ready.lock().take() {
                let _ = ready.send(Err(source.clone()));
            }
        }
        self.operation = None;
        result?;
        let mut retained = self.retained.lock().await;
        match retained.registered_custody.as_mut() {
            Some(original) => original.execute_original().await,
            None => Err(AuraError::invalid(
                "prepared issuance ended without registered custody",
            )),
        }
    }

    #[cfg(test)]
    async fn cleanup_observed_original_for_test(
        &mut self,
        exit: &RequiredTaskExit,
    ) -> Result<RequiredTaskSettlement, AuraError> {
        let settlement = self.cleanup_original(exit).await?;
        if let Some(registration) = self.registration.get() {
            registration.observe_cleanup_before_retirement_for_test(&self.retained)?;
        }
        Ok(settlement)
    }

    async fn cleanup_original(
        &mut self,
        exit: &RequiredTaskExit,
    ) -> Result<RequiredTaskSettlement, AuraError> {
        // Dispose only the borrowed execution frame; original resources were
        // deposited in this task's retained cell before subsequent awaits.
        self.operation = None;
        let initial_failure = self.first_failure.clone().or_else(|| match exit {
            RequiredTaskExit::Failed(source) => Some(source.clone()),
            RequiredTaskExit::Cancelled => Some(AuraError::Internal {
                message: "original prepared issuer was cancelled".into(),
                source: Some(Arc::new(OriginalPreparedIssuerCancelled)),
            }),
            RequiredTaskExit::Panicked => Some(AuraError::Internal {
                message: "original prepared issuer panicked".into(),
                source: Some(Arc::new(OriginalPreparedIssuerPanicked)),
            }),
            RequiredTaskExit::Completed => None,
        });
        if let (Some(ready), Some(source)) = (self.ready.lock().take(), initial_failure.as_ref()) {
            let _ = ready.send(Err(source.clone()));
        }
        let mut retained = self
            .retained
            .try_lock()
            .map_err(|source| AuraError::Internal {
                message: "original prepared resource cell is borrowed".into(),
                source: Some(Arc::new(source)),
            })?;
        if let Some(original) = retained.registered_custody.as_mut() {
            let settlement = original.cleanup_original(exit).await?;
            if let Some(registration) = self.registration.get() {
                registration.acknowledge_original_removal()?;
            }
            return Ok(settlement);
        }
        if retained.first_failure.is_none() {
            retained.first_failure = initial_failure;
        }
        if retained.issuer_reservation.is_some() {
            if let Some(signing) = retained.issuer_signing.as_mut() {
                signing
                    .try_lock()
                    .map_err(|source| AuraError::Internal {
                        message: "original conditional signing state remains borrowed".into(),
                        source: Some(Arc::new(source)),
                    })?
                    .retire_original_for_cleanup()?;
            }
            retained.issuer_inventory = None;
            if retained.reservation.is_some() {
                return Err(AuraError::invalid(
                    "original issuer reservation output occupied",
                ));
            }
            let actual = retained
                .issuer_reservation
                .take()
                .ok_or_else(|| AuraError::invalid("original issuer reservation missing"))?;
            match Arc::try_unwrap(actual) {
                Ok(actual) => retained.reservation = Some(actual),
                Err(actual) => {
                    retained.issuer_reservation = Some(actual);
                    return Err(AuraError::invalid(
                        "original issuer reservation remains borrowed",
                    ));
                }
            }
        }
        if retained.allocation.is_some() && retained.reservation.is_none() {
            let OriginalPreparedIssuerResources {
                launch,
                allocation,
                first_failure,
                allocation_checkpoint,
                ..
            } = &mut *retained;
            let selected = launch
                .as_ref()
                .ok_or_else(|| AuraError::invalid("original allocation launch missing"))?;
            let producer = allocation
                .as_mut()
                .ok_or_else(|| AuraError::invalid("original allocation producer missing"))?;
            let primary = first_failure
                .as_ref()
                .ok_or_else(|| AuraError::invalid("original allocation failure missing"))?;
            if producer.observe_original_cleanup_acknowledged(self.effects.as_ref())? {
                if let Some(registration) = self.registration.get() {
                    registration.acknowledge_original_removal()?;
                }
                return Ok(RequiredTaskSettlement::ResolvedWithDomainFailure(
                    primary.clone(),
                ));
            }
            let observed = selected
                .execute_original_allocation_cleanup(&self.effects, primary, || {
                    producer.retire_original(self.effects.as_ref())
                })
                .await?;
            if let Some(Err(source)) = &observed.operation {
                producer
                    .cleanup_failure
                    .get_or_insert_with(|| source.clone());
            }
            if let Err(source) = &observed.validation {
                producer
                    .cleanup_validation_failure
                    .get_or_insert_with(|| source.clone());
            }
            producer.cleanup_observation =
                Some(aura_core::time::timeout::NegativeCleanupExecution {
                    primary: observed.primary.clone(),
                    operation: observed.operation,
                    validation: observed.validation,
                });
            let work = producer
                .cleanup_observation
                .as_ref()
                .ok_or_else(|| AuraError::invalid("original cleanup observation missing"))?;
            if let Some(Err(source)) = work.operation.as_ref() {
                return Err(source.clone());
            }
            work.validation.clone().map_err(AuraError::from)?;
            if !matches!(work.operation, Some(Ok(()))) {
                return Err(AuraError::invalid(
                    "original allocation retirement incomplete",
                ));
            }
            let acknowledgment = selected
                .acknowledge_original_allocation_cleanup(
                    &self.effects,
                    primary,
                    || async {
                        producer
                            .acknowledge_original(self.effects.as_ref())
                            .map_err(|source| {
                                aura_core::time::timeout::TimeoutBudgetError::CheckpointFailure {
                                    detail: "acknowledge original allocation retirement".into(),
                                    source: Some(source),
                                }
                            })
                    },
                    allocation_checkpoint,
                )
                .await?;
            producer.release_original(self.effects.as_ref(), acknowledgment)?;
            if let Some(registration) = self.registration.get() {
                registration.acknowledge_original_removal()?;
            }
            return Ok(RequiredTaskSettlement::ResolvedWithDomainFailure(
                primary.clone(),
            ));
        }
        if retained.registered.is_some() {
            let OriginalPreparedIssuerResources {
                registered,
                reservation,
                ..
            } = &mut *retained;
            crate::runtime::effects::RegisteredEnrollmentGenerationCapability::take_original_registration(
                registered, self.effects.as_ref(), reservation,
            )?;
        }
        if retained.root.is_none() {
            if let Some(reservation) = retained.reservation.as_ref() {
                let tracker = reservation.decision().tracker();
                retained.root = Some(
                    tracker
                        .held_issuer_window(&self.effects, reservation)
                        .await?,
                );
            }
        }
        if retained.negative.is_none() && retained.reservation.is_some() && retained.root.is_some()
        {
            let primary = retained.first_failure.clone().ok_or_else(|| {
                AuraError::invalid("original held cleanup lacks its execution cause")
            })?;
            let root = retained
                .root
                .take()
                .ok_or_else(|| AuraError::invalid("original prepared root missing"))?;
            let reservation = retained
                .reservation
                .take()
                .ok_or_else(|| AuraError::invalid("original prepared reservation missing"))?;
            retained.negative = Some(crate::runtime::services::enrollment_window::OriginalHeldIssuerNegativeCleanupOwner::from_original(root, reservation, primary));
        }
        let settlement = if let Some(original) = retained.negative.as_mut() {
            original.settle(&self.effects).await?;
            original.settlement()?
        } else if retained.allocation.is_some()
            || retained.registered.is_some()
            || retained.root.is_some()
            || retained.reservation.is_some()
        {
            // The next canonical handoff must consume the actual registered
            // capability. Keeping it unresolved cannot manufacture a held owner.
            return Err(AuraError::invalid(
                "original registered issuance handoff is unresolved",
            ));
        } else {
            match retained.first_failure.as_ref() {
                Some(source) => RequiredTaskSettlement::ResolvedWithDomainFailure(source.clone()),
                None => RequiredTaskSettlement::Resolved,
            }
        };
        if let Some(registration) = self.registration.get() {
            registration.acknowledge_original_removal()?;
        }
        Ok(settlement)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl aura_core::effects::task::RequiredTaskCustody for OriginalPreparedIssuerTaskCustody {
    fn execute(&mut self) -> futures::future::BoxFuture<'_, Result<(), AuraError>> {
        Box::pin(self.execute_original())
    }
    fn cleanup<'a>(
        &'a mut self,
        exit: &'a RequiredTaskExit,
    ) -> futures::future::BoxFuture<'a, Result<RequiredTaskSettlement, AuraError>> {
        #[cfg(test)]
        {
            Box::pin(self.cleanup_observed_original_for_test(exit))
        }
        #[cfg(not(test))]
        {
            Box::pin(self.cleanup_original(exit))
        }
    }
}
#[cfg(target_arch = "wasm32")]
impl aura_core::effects::task::LocalRequiredTaskCustody for OriginalPreparedIssuerTaskCustody {
    fn execute(&mut self) -> futures::future::LocalBoxFuture<'_, Result<(), AuraError>> {
        Box::pin(self.execute_original())
    }
    fn cleanup<'a>(
        &'a mut self,
        exit: &'a RequiredTaskExit,
    ) -> futures::future::LocalBoxFuture<'a, Result<RequiredTaskSettlement, AuraError>> {
        #[cfg(test)]
        {
            Box::pin(self.cleanup_observed_original_for_test(exit))
        }
        #[cfg(not(test))]
        {
            Box::pin(self.cleanup_original(exit))
        }
    }
}
