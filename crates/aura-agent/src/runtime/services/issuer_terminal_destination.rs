//! Runtime-owned original issuer result, cancellation and retirement custody.

use super::issuer_error_custody::UndeliveredIssuerTerminal;
use crate::core::{AgentError, AgentResult};
use crate::runtime::AuraEffectSystem;
use std::sync::Arc;

/// One original child writes one original custodian's result. This slot is
/// independent from diagnostic failure retention, never an addressable queue.
#[cfg(test)]
type NegativeCleanupAfterObservationGate = (
    tokio::sync::oneshot::Sender<()>,
    tokio::sync::oneshot::Receiver<()>,
    tokio::sync::oneshot::Sender<aura_core::TimeoutBudgetError>,
);

/// The original producer's fatal error can itself retain an undelivered signed
/// obligation. Keep it with the live custodian, not the post-disposal diagnostic.
struct OriginalIssuerProgressFailure {
    source: aura_core::AuraError,
    earlier_finalizer: Option<aura_core::AuraError>,
}

/// The original caller keeps its exact error category on normal completion.
/// Withdrawal deposits the error synchronously in its prevalidated custodian;
/// this guard neither closes the VM nor acknowledges any resource retirement.
pub(crate) struct OriginalVmOperationFailure<'a> {
    destination: &'a OriginalIssuerTerminalDestination,
    error: Option<AgentError>,
    earlier_finalizer: Option<aura_core::AuraError>,
}
impl OriginalVmOperationFailure<'_> {
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "OriginalVmOperationFailure", receiver_type = OriginalVmOperationFailure<'_>, family = "runtime_helper")]
    pub(crate) fn finish(mut self) -> AgentError {
        self.error
            .take()
            .expect("original VM error has one consumer")
    }
}
impl Drop for OriginalVmOperationFailure<'_> {
    fn drop(&mut self) {
        if let Some(error) = self.error.take() {
            self.destination
                .retain_interrupted_vm_failure(error, self.earlier_finalizer.take());
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("original issuer VM failure: {primary}; independent failure: {secondary}")]
pub(crate) struct OriginalIssuerVmDomainFailure {
    #[source]
    primary: aura_core::AuraError,
    secondary: aura_core::AuraError,
}
#[cfg(all(test, not(target_arch = "wasm32")))]
impl OriginalIssuerVmDomainFailure {
    /// Observe the producer-selected primary without changing Error::source.
    pub(crate) fn primary(&self) -> &aura_core::AuraError {
        &self.primary
    }
    /// The independent actual event remains a separate source-bearing cause.
    pub(crate) fn secondary(&self) -> &aura_core::AuraError {
        &self.secondary
    }
}

fn combine_vm_failures(
    primary: aura_core::AuraError,
    secondary: aura_core::AuraError,
) -> aura_core::AuraError {
    aura_core::AuraError::Internal {
        message: "original issuer VM operation and independent failure".into(),
        source: Some(Arc::new(OriginalIssuerVmDomainFailure {
            primary,
            secondary,
        })),
    }
}

struct RetainedOriginalCancelledSend {
    terminal: std::sync::Weak<crate::handlers::invitation::OriginalIssuerCancellationTerminal>,
    notice: std::sync::Weak<crate::handlers::invitation::SignedCancelledNotice>,
    actual: crate::handlers::invitation::OriginalCancelledNoticeSendCompletion,
}

#[derive(thiserror::Error)]
#[error("original cancellation send completion already has a custodian")]
struct RejectedOriginalCancelledSend {
    #[source]
    source: Option<AgentError>,
    _retained: RetainedOriginalCancelledSend,
}
impl std::fmt::Debug for RejectedOriginalCancelledSend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RejectedOriginalCancelledSend")
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
type OriginalProgressErrorGate = (
    tokio::sync::oneshot::Sender<Option<aura_core::AuraError>>,
    tokio::sync::oneshot::Receiver<()>,
);
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) type OriginalProgressErrorObservationGate = (
    tokio::sync::oneshot::Receiver<Option<aura_core::AuraError>>,
    tokio::sync::oneshot::Sender<()>,
);

#[derive(Default)]
pub(crate) struct OriginalIssuerTerminalDestination {
    // The required issuer retains this destination when protocol execution is
    // cancelled. Dropping the borrowed protocol future releases the guard, never
    // the original VM capability needed for acknowledged teardown.
    original_vm: tokio::sync::Mutex<Option<crate::runtime::session_ingress::OwnedVmSession>>,
    terminal_started: std::sync::atomic::AtomicBool,
    vm_disposed: std::sync::atomic::AtomicBool,
    vm_close_retirement:
        std::sync::OnceLock<crate::runtime::session_ingress::AcknowledgedVmResourceRetirement>,
    protocol_completed: std::sync::atomic::AtomicBool,
    cancelled_send_completion: std::sync::OnceLock<RetainedOriginalCancelledSend>,
    original_progress_failure: std::sync::OnceLock<OriginalIssuerProgressFailure>,
    #[allow(clippy::disallowed_types)] // Synchronous error deposit; no guard crosses an await.
    interrupted_vm_failure: parking_lot::Mutex<Option<OriginalIssuerProgressFailure>>,
    original_control: std::sync::OnceLock<
        std::sync::Weak<crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl>,
    >,
    #[allow(clippy::disallowed_types)] // Synchronous move-only deposit; no guard crosses an await.
    staged_terminal:
        parking_lot::Mutex<Option<crate::handlers::invitation::IssuerEnrollmentTerminalReceipt>>,
    #[cfg(any(test, feature = "simulation"))]
    simulated_subtree_disposal:
        std::sync::OnceLock<Arc<crate::task_registry::TaskGroupDisposalAcknowledgment>>,
    #[cfg(all(test, not(target_arch = "wasm32")))]
    #[allow(clippy::disallowed_types)]
    // Original-task test boundary; guard never crosses await.
    progress_error_gate: parking_lot::Mutex<Option<OriginalProgressErrorGate>>,
    #[cfg(test)]
    #[allow(clippy::disallowed_types)] // Synchronous custody slot; no guard crosses an await.
    cancelled_notice_gate: parking_lot::Mutex<
        Option<(
            tokio::sync::oneshot::Sender<()>,
            tokio::sync::oneshot::Receiver<()>,
        )>,
    >,
    #[cfg(test)]
    #[allow(clippy::disallowed_types)] // Synchronous custody slot; no guard crosses an await.
    negative_cleanup_first_observation_gate: parking_lot::Mutex<
        Option<(
            tokio::sync::oneshot::Sender<()>,
            tokio::sync::oneshot::Receiver<()>,
        )>,
    >,
    #[cfg(test)]
    #[allow(clippy::disallowed_types)]
    // Synchronous test observation; guard never crosses an await.
    negative_cleanup_after_observation_gate:
        parking_lot::Mutex<Option<NegativeCleanupAfterObservationGate>>,
    #[cfg(test)]
    #[allow(clippy::disallowed_types)] // Synchronous custody slot; no guard crosses an await.
    receipt_test_hook: parking_lot::Mutex<Option<OriginalTerminalReceiptTestHook>>,
    pub(crate) cancelled:
        std::sync::OnceLock<Arc<crate::handlers::invitation::OriginalIssuerCancellationTerminal>>,
    #[allow(clippy::disallowed_types)] // Synchronous custody slot; no guard crosses an await.
    retirement: parking_lot::Mutex<OriginalIssuerRetirementState>,
    retirement_changed: tokio::sync::Notify,
    signed_cancelled_notice:
        std::sync::OnceLock<Arc<crate::handlers::invitation::SignedCancelledNotice>>,
    pub(crate) finalizer_failure: std::sync::OnceLock<aura_core::AuraError>,
    finalizer_failure_changed: tokio::sync::Notify,
    pub(crate) result: std::sync::OnceLock<
        super::issuer_error_custody::SynchronousCustodyCell<
            Option<AgentResult<crate::handlers::invitation::IssuerEnrollmentTerminalOutcome>>,
        >,
    >,
}
impl OriginalIssuerTerminalDestination {
    /// Deposit before any cancellation-arbitration await. The required task
    /// retains this exact source even if its borrowed protocol future is dropped.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "OriginalIssuerTerminalDestination",
        receiver_type = OriginalIssuerTerminalDestination,
        family = "runtime_helper"
    )]
    pub(crate) fn retain_original_progress_failure(
        &self,
        error: AgentError,
    ) -> Result<aura_core::AuraError, aura_core::AuraError> {
        let source = match error {
            AgentError::Aura(source) => source,
            source => aura_core::AuraError::Internal {
                message: "original issuer protocol progress failed".into(),
                source: Some(Arc::new(source)),
            },
        };
        let retained = source.clone();
        let failure = OriginalIssuerProgressFailure {
            source,
            earlier_finalizer: self.retained_finalizer_failure(),
        };
        self.original_progress_failure
            .set(failure)
            .map_err(|rejected| {
                // The original protocol executes once. An unexpected duplicate
                // retains the rejected source; it must not drop its obligations.
                aura_core::AuraError::Internal {
                    message: "original issuer progress failure already retained".into(),
                    source: Some(Arc::new(rejected.source)),
                }
            })?;
        Ok(retained)
    }

    pub(crate) fn retained_original_progress_failure(
        &self,
    ) -> Option<(aura_core::AuraError, Option<aura_core::AuraError>)> {
        self.original_progress_failure
            .get()
            .map(|failure| (failure.source.clone(), failure.earlier_finalizer.clone()))
    }

    #[aura_macros::capability_boundary(category = "capability_gated", capability = "OwnedVmSession", capability_type = crate::runtime::session_ingress::OwnedVmSession, receiver_type = OriginalIssuerTerminalDestination, family = "runtime_helper")]
    pub(crate) fn hold_original_vm_operation_failure(
        &self,
        effects: &AuraEffectSystem,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        original: &super::enrollment_window::EnrollmentExecutionChild,
        session: &crate::runtime::session_ingress::OwnedVmSession,
        error: AgentError,
    ) -> AgentResult<OriginalVmOperationFailure<'_>> {
        let validation = (|| {
            self.require_original_control(issued)?;
            issued.require_runtime_owner(effects)?;
            original.require_issued_notice_owner(issued)?;
            if !aura_invitation::enrollment_mailbox_session::is_original_enrollment_mailbox_session(
                &issued.manifest().invitation,
                &issued.digest(),
                session.owner().session_id.as_uuid(),
            ) {
                return Err(aura_core::AuraError::permission_denied(
                    "operation error belongs to another original VM",
                ));
            }
            effects
                .assert_owned_choreography_session(session.owner())
                .map_err(|source| aura_core::AuraError::Internal {
                    message: "original VM operation error custody refused".into(),
                    source: Some(Arc::new(source)),
                })
        })();
        if let Err(validation) = validation {
            return Err(AgentError::from(aura_core::AuraError::Internal {
                message: "retain original VM operation failure".into(),
                source: Some(Arc::new(
                    crate::handlers::invitation_service::EnrollmentInitiatorTaskFailure::new(
                        error,
                        Some(validation),
                    ),
                )),
            }));
        }
        Ok(OriginalVmOperationFailure {
            destination: self,
            error: Some(error),
            earlier_finalizer: self.retained_finalizer_failure(),
        })
    }

    fn retain_interrupted_vm_failure(
        &self,
        error: AgentError,
        earlier_finalizer: Option<aura_core::AuraError>,
    ) {
        let source = match error {
            AgentError::Aura(source) => source,
            source => aura_core::AuraError::Internal {
                message: "interrupted original VM operation".into(),
                source: Some(Arc::new(source)),
            },
        };
        let mut retained = self.interrupted_vm_failure.lock();
        if let Some(previous) = retained.as_mut() {
            // Retain subsequent failures without changing the first producer's
            // chronology snapshot or replacing its concrete native allocation.
            previous.source = combine_vm_failures(previous.source.clone(), source);
        } else {
            *retained = Some(OriginalIssuerProgressFailure {
                source,
                earlier_finalizer,
            });
        }
    }

    pub(super) fn with_retained_vm_failures(
        &self,
        primary: aura_core::AuraError,
    ) -> aura_core::AuraError {
        let interrupted = self
            .interrupted_vm_failure
            .lock()
            .as_ref()
            .map(|failure| (failure.source.clone(), failure.earlier_finalizer.clone()));
        let primary = match interrupted {
            Some((operation, earlier)) if self.original_progress_failure.get().is_none() => {
                let original = match earlier {
                    Some(earlier) => combine_vm_failures(earlier, operation),
                    None => operation,
                };
                combine_vm_failures(original, primary)
            }
            Some((operation, _)) => combine_vm_failures(primary, operation),
            None => primary,
        };
        match self.vm_close_retirement.get() {
            Some(retirement) => aura_core::AuraError::Internal {
                message: "original issuer execution and acknowledged VM close validation failed"
                    .into(),
                source: Some(Arc::new(OriginalIssuerCloseDomainFailure {
                    primary,
                    close: aura_core::AuraError::Internal {
                        message: "original issuer VM retirement clock validation failed".into(),
                        source: Some(retirement.native_failure()),
                    },
                })),
            },
            None => primary,
        }
    }
    fn require_original_control(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<(), aura_core::AuraError> {
        if !self
            .original_control
            .get()
            .and_then(std::sync::Weak::upgrade)
            .is_some_and(|actual| std::ptr::eq(actual.as_ref(), issued))
        {
            return Err(aura_core::AuraError::permission_denied(
                "original issuer destination control is missing or foreign",
            ));
        }
        Ok(())
    }
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "IssuerEnrollmentTerminalOutcome", receiver_type = OriginalIssuerTerminalDestination, family = "runtime_helper")]
    pub(super) fn retain_original_protocol_completion(
        &self,
        terminal: &crate::handlers::invitation::IssuerEnrollmentTerminalOutcome,
    ) -> Result<(), aura_core::AuraError> {
        let issued = match terminal {
            crate::handlers::invitation::IssuerEnrollmentTerminalOutcome::Committed(receipt) => {
                receipt.retained()
            }
            crate::handlers::invitation::IssuerEnrollmentTerminalOutcome::Cancelled(receipt) => {
                receipt.issued()
            }
        };
        self.require_original_control(issued)?;
        self.protocol_completed
            .store(true, std::sync::atomic::Ordering::Release);
        Ok(())
    }

    pub(super) fn require_original_protocol_completion(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<(), aura_core::AuraError> {
        if !self
            .protocol_completed
            .load(std::sync::atomic::Ordering::Acquire)
            || !self
                .original_control
                .get()
                .and_then(std::sync::Weak::upgrade)
                .is_some_and(|actual| std::ptr::eq(actual.as_ref(), issued))
        {
            return Err(aura_core::AuraError::permission_denied(
                "original issuer protocol completion missing or foreign",
            ));
        }
        Ok(())
    }

    pub(super) fn original_protocol_completed(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<bool, aura_core::AuraError> {
        self.require_original_control(issued)?;
        Ok(self
            .protocol_completed
            .load(std::sync::atomic::Ordering::Acquire))
    }
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "IssuerEnrollmentTerminalReceipt", receiver_type = OriginalIssuerTerminalDestination, family = "runtime_helper")]
    pub(crate) fn stage_original_terminal(
        &self,
        receipt: crate::handlers::invitation::IssuerEnrollmentTerminalReceipt,
        effects: &AuraEffectSystem,
    ) -> Result<(), aura_core::AuraError> {
        let mut staged = self.staged_terminal.lock();
        let valid = receipt.require_runtime_owner(effects).and_then(|()| {
            if self
                .original_control
                .get()
                .and_then(std::sync::Weak::upgrade)
                .is_some_and(|actual| std::ptr::eq(actual.as_ref(), receipt.retained()))
                && staged.is_none()
            {
                Ok(())
            } else {
                Err(aura_core::AuraError::permission_denied(
                    "original signed terminal destination differs or is occupied",
                ))
            }
        });
        if let Err(source) = valid {
            return Err(aura_core::AuraError::Internal {
                message: "original issuer terminal staging failed".into(),
                source: Some(Arc::new(UndeliveredIssuerTerminal {
                    source: Some(source),
                    retained: crate::runtime::services::issuer_error_custody::SynchronousCustodyCell::new(Some(Ok(
                        crate::handlers::invitation::IssuerEnrollmentTerminalOutcome::Committed(
                            receipt,
                        ),
                    ))),
                })),
            });
        }
        *staged = Some(receipt);
        Ok(())
    }

    pub(crate) fn take_original_terminal(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<crate::handlers::invitation::IssuerEnrollmentTerminalReceipt, aura_core::AuraError>
    {
        self.require_original_control(issued)?;
        self.staged_terminal
            .lock()
            .take()
            .ok_or_else(|| aura_core::AuraError::invalid("original signed terminal missing"))
    }
    pub(super) fn bind_original_control(
        &self,
        effects: &AuraEffectSystem,
        issued: &Arc<crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl>,
        original: &super::enrollment_window::EnrollmentExecutionChild,
    ) -> Result<(), aura_core::AuraError> {
        issued.require_runtime_owner(effects)?;
        original.require_issued_notice_owner(issued)?;
        let actual = self.original_control.get_or_init(|| Arc::downgrade(issued));
        if !actual
            .upgrade()
            .is_some_and(|actual| Arc::ptr_eq(&actual, issued))
        {
            return Err(aura_core::AuraError::permission_denied(
                "original VM destination belongs to another issuer",
            ));
        }
        Ok(())
    }
    pub(crate) fn begin_original_terminal(&self) {
        self.terminal_started
            .store(true, std::sync::atomic::Ordering::Release);
    }

    #[aura_macros::capability_boundary(category = "capability_gated", capability = "EnrollmentExecutionChild", receiver_type = OriginalIssuerTerminalDestination, family = "runtime_helper")]
    pub(super) async fn dispose_original_vm(
        &self,
        effects: &AuraEffectSystem,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        original: &super::enrollment_window::EnrollmentExecutionChild,
    ) -> Result<(), aura_core::AuraError> {
        let mut slot = self.borrow_original_vm(effects, issued, original).await?;
        if let Some(session) = slot.as_mut() {
            if let Err(source) = session.close_retained().await {
                match session.acknowledged_close_resource_failure() {
                    Some(retirement) => {
                        self.vm_close_retirement.get_or_init(|| retirement);
                    }
                    None => {
                        return Err(aura_core::AuraError::Internal {
                            message: "close retained original issuer VM".into(),
                            source: Some(Arc::new(source)),
                        })
                    }
                }
            }
            slot.take();
        }
        self.vm_disposed
            .store(true, std::sync::atomic::Ordering::Release);
        Ok(())
    }

    pub(super) fn require_unsigned_vm_disposed(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<(), aura_core::AuraError> {
        if !self
            .original_control
            .get()
            .and_then(std::sync::Weak::upgrade)
            .is_some_and(|actual| std::ptr::eq(actual.as_ref(), issued))
        {
            return Err(aura_core::AuraError::permission_denied(
                "unsigned VM disposal belongs to another original issuer",
            ));
        }
        if self
            .terminal_started
            .load(std::sync::atomic::Ordering::Acquire)
            || !self.vm_disposed.load(std::sync::atomic::Ordering::Acquire)
            || self.cancelled.get().is_some()
        {
            return Err(aura_core::AuraError::permission_denied(
                "original issuer still has terminal or VM obligations",
            ));
        }
        Ok(())
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "EnrollmentExecutionChild",
        receiver_type = OriginalIssuerTerminalDestination,
        family = "runtime_helper"
    )]
    pub(crate) async fn borrow_original_vm(
        &self,
        effects: &AuraEffectSystem,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        original: &super::enrollment_window::EnrollmentExecutionChild,
    ) -> Result<
        tokio::sync::MutexGuard<'_, Option<crate::runtime::session_ingress::OwnedVmSession>>,
        aura_core::AuraError,
    > {
        issued.require_runtime_owner(effects)?;
        original.require_issued_notice_owner(issued)?;
        self.require_original_control(issued)?;
        Ok(self.original_vm.lock().await)
    }
}
#[derive(Debug, thiserror::Error)]
#[error("original issuer execution failed: {primary}; close validation failed: {close}")]
pub(crate) struct OriginalIssuerCloseDomainFailure {
    #[source]
    pub(crate) primary: aura_core::AuraError,
    pub(crate) close: aura_core::AuraError,
}
#[cfg(test)]
pub(crate) type OriginalNegativeCleanupObservationGate = (
    tokio::sync::oneshot::Receiver<()>,
    tokio::sync::oneshot::Sender<()>,
    tokio::sync::oneshot::Receiver<aura_core::TimeoutBudgetError>,
);
#[cfg(test)]
pub(crate) type OriginalTerminalReceiptTestHook = Box<
    dyn FnOnce(
            crate::handlers::invitation::IssuerEnrollmentTerminalReceipt,
        ) -> crate::handlers::invitation::IssuerEnrollmentTerminalReceipt
        + Send,
>;
#[derive(thiserror::Error)]
#[error("original cancellation notice was not transferred")]
struct RejectedOriginalCancelledNotice {
    retained: Arc<crate::handlers::invitation::SignedCancelledNotice>,
}
#[cfg(test)]
impl OriginalIssuerTerminalDestination {
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn install_progress_error_gate_for_test(
        &self,
    ) -> Result<OriginalProgressErrorObservationGate, aura_core::AuraError> {
        let mut slot = self.progress_error_gate.lock();
        if slot.is_some() || self.result.get().is_some() {
            return Err(aura_core::AuraError::invalid(
                "original progress error gate already used",
            ));
        }
        let (entered, observed) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel();
        *slot = Some((entered, released));
        Ok((observed, release))
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) async fn await_progress_error_gate_for_test(&self, error: &AgentError) {
        let gate = self.progress_error_gate.lock().take();
        if let Some((entered, released)) = gate {
            entered
                .send(match error {
                    AgentError::Aura(source) => Some(source.clone()),
                    _ => None,
                })
                .expect("actual original protocol error observed");
            released
                .await
                .expect("controller releases original error arbitration");
        }
    }
    pub(crate) fn install_cancelled_notice_gate_for_test(
        &self,
    ) -> Result<
        (
            tokio::sync::oneshot::Receiver<()>,
            tokio::sync::oneshot::Sender<()>,
        ),
        aura_core::AuraError,
    > {
        let mut slot = self.cancelled_notice_gate.lock();
        if slot.is_some() || self.cancelled.get().is_some() {
            return Err(aura_core::AuraError::invalid(
                "original cancelled notice gate already used",
            ));
        }
        let (entered, observed) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel();
        *slot = Some((entered, released));
        Ok((observed, release))
    }
    pub(crate) fn install_negative_cleanup_first_observation_gate_for_test(
        &self,
    ) -> Result<
        (
            tokio::sync::oneshot::Receiver<()>,
            tokio::sync::oneshot::Sender<()>,
        ),
        aura_core::AuraError,
    > {
        let mut slot = self.negative_cleanup_first_observation_gate.lock();
        if slot.is_some() || self.cancelled.get().is_some() {
            return Err(aura_core::AuraError::invalid(
                "original cancelled notice gate already used",
            ));
        }
        let (entered, observed) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel();
        *slot = Some((entered, released));
        Ok((observed, release))
    }
    pub(crate) async fn await_cancelled_notice_gate_for_test(&self) {
        let gate = self.cancelled_notice_gate.lock().take();
        if let Some((entered, released)) = gate {
            assert!(
                self.cancelled.get().is_some(),
                "actual original cancellation retained"
            );
            assert!(
                self.signed_cancelled_notice.get().is_none(),
                "no notice signed before crash boundary"
            );
            assert!(
                !self.retains_original_retirement_acknowledgment(),
                "no retirement acknowledgment before crash boundary"
            );
            entered
                .send(())
                .expect("original crash boundary observer retained");
            let _ = released.await;
        }
    }
    pub(crate) async fn await_negative_cleanup_first_observation_gate_for_test(
        &self,
        checkpoint_retained: bool,
        disposal_retained: bool,
    ) {
        let gate = self.negative_cleanup_first_observation_gate.lock().take();
        if let Some((entered, released)) = gate {
            assert!(
                !checkpoint_retained,
                "no durable negative checkpoint acknowledgment before first cleanup observation"
            );
            assert!(
                !disposal_retained,
                "no original subtree disposal acknowledgment before first cleanup observation"
            );
            assert!(
                self.cancelled.get().is_some(),
                "actual original cancellation retained"
            );
            assert!(
                self.retains_original_retirement_acknowledgment(),
                "actual generation retirement precedes required negative cleanup"
            );
            entered
                .send(())
                .expect("actual first negative observation retained");
            released
                .await
                .expect("same original negative observation gate released");
        }
    }
    pub(crate) fn install_negative_cleanup_after_observation_gate_for_test(
        &self,
    ) -> Result<OriginalNegativeCleanupObservationGate, aura_core::AuraError> {
        let mut slot = self.negative_cleanup_after_observation_gate.lock();
        if slot.is_some() || self.cancelled.get().is_some() {
            return Err(aura_core::AuraError::invalid(
                "original negative observation gate already used",
            ));
        }
        let (entered, observed) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel();
        let (refused, refusal) = tokio::sync::oneshot::channel();
        *slot = Some((entered, released, refused));
        Ok((observed, release, refusal))
    }
    pub(crate) async fn await_negative_cleanup_after_observation_gate_for_test(
        &self,
        retained: &mut Option<tokio::sync::oneshot::Sender<aura_core::TimeoutBudgetError>>,
    ) {
        let gate = self.negative_cleanup_after_observation_gate.lock().take();
        if let Some((entered, released, refusal)) = gate {
            assert!(
                retained.is_none(),
                "original refusal observer retained only once"
            );
            *retained = Some(refusal);
            entered
                .send(())
                .expect("actual negative provider observation retained");
            released
                .await
                .expect("same original negative operation released");
        }
    }
    pub(crate) fn install_receipt_test_hook(
        &self,
        hook: OriginalTerminalReceiptTestHook,
    ) -> Result<(), aura_core::AuraError> {
        let mut slot = self.receipt_test_hook.lock();
        if slot.is_some() || self.result.get().is_some() {
            return Err(aura_core::AuraError::invalid(
                "original receipt hook is already installed or published",
            ));
        }
        *slot = Some(hook);
        Ok(())
    }
    pub(crate) fn original_finalizer_failure_for_test(&self) -> Option<aura_core::AuraError> {
        self.finalizer_failure.get().cloned()
    }
}
impl std::fmt::Debug for RejectedOriginalCancelledNotice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RejectedOriginalCancelledNotice")
    }
}

#[derive(Default)]
struct OriginalIssuerRetirementState {
    first_failure: Option<aura_core::AuraError>,
    acknowledged: Option<
        crate::runtime::services::ceremony_tracker::OriginalEnrollmentRetirementAcknowledgment,
    >,
    refused:
        Option<crate::runtime::services::ceremony_tracker::RejectedOriginalEnrollmentRetirement>,
}

#[derive(thiserror::Error)]
#[error("original cancellation terminal refused duplicate custody")]
struct RejectedOriginalCancellationTerminal {
    retained: Arc<crate::handlers::invitation::OriginalIssuerCancellationTerminal>,
}
impl std::fmt::Debug for RejectedOriginalCancellationTerminal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RejectedOriginalCancellationTerminal")
    }
}
/// Observation of a genuine newly accepted original retirement ACK. The
/// destination owns the actual receipt and keeps any prior failed domain cause.
/// This value grants no terminal, VM-disposal or overall-cleanup authority.
pub(crate) struct AcceptedOriginalRetirementPublication {
    retained_failure: Option<aura_core::AuraError>,
}
impl AcceptedOriginalRetirementPublication {
    pub(super) fn into_retained_failure(self) -> Option<aura_core::AuraError> {
        self.retained_failure
    }
}

#[derive(thiserror::Error)]
pub(crate) enum RejectedOriginalRetirementPublication {
    #[error("original retirement retains its primary failure")]
    StoredFailure(#[source] aura_core::AuraError),
    #[error("original retirement already retains an acknowledgment")]
    UnacceptedAcknowledgment {
        receipt: Box<
            crate::runtime::services::ceremony_tracker::OriginalEnrollmentRetirementAcknowledgment,
        >,
    },
    #[error("original retirement already retains refused writes")]
    UnacceptedRefusal {
        #[source]
        source: aura_core::AuraError,
        rejected:
            Box<crate::runtime::services::ceremony_tracker::RejectedOriginalEnrollmentRetirement>,
    },
}
impl std::fmt::Debug for RejectedOriginalRetirementPublication {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RejectedOriginalRetirementPublication")
    }
}
impl OriginalIssuerTerminalDestination {
    #[cfg(any(test, feature = "simulation"))]
    pub(crate) fn retain_simulated_original_subtree_disposal(
        &self,
        disposal: Arc<crate::task_registry::TaskGroupDisposalAcknowledgment>,
        original: &crate::task_registry::TaskGroup,
    ) -> Result<(), aura_core::AuraError> {
        disposal.require_original_group(original)?;
        if let Some(actual) = self.simulated_subtree_disposal.get() {
            if !Arc::ptr_eq(actual, &disposal) {
                return Err(aura_core::AuraError::permission_denied(
                    "foreign original subtree disposal",
                ));
            }
            return Ok(());
        }
        self.simulated_subtree_disposal.set(disposal).map_err(|_| {
            aura_core::AuraError::invalid("original subtree disposal already retained")
        })
    }
    #[cfg(any(test, feature = "simulation"))]
    pub(crate) fn simulated_original_subtree_disposal(
        &self,
    ) -> Option<Arc<crate::task_registry::TaskGroupDisposalAcknowledgment>> {
        self.simulated_subtree_disposal.get().map(Arc::clone)
    }
    pub(crate) fn retain_finalizer_failure(&self, source: aura_core::AuraError) {
        let _ = self.finalizer_failure.set(source);
        self.finalizer_failure_changed.notify_waiters();
    }

    pub(crate) async fn await_finalizer_failure(&self) -> aura_core::AuraError {
        loop {
            let changed = self.finalizer_failure_changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if let Some(source) = self.finalizer_failure.get() {
                return source.clone();
            }
            changed.await;
        }
    }

    pub(crate) fn retained_finalizer_failure(&self) -> Option<aura_core::AuraError> {
        self.finalizer_failure.get().cloned()
    }
    pub(super) fn with_retained_finalizer_failure(
        &self,
        primary: aura_core::AuraError,
    ) -> aura_core::AuraError {
        match self.retained_finalizer_failure() {
            Some(finalizer) => aura_core::AuraError::Internal {
                message: "original issuer failure with retained finalizer cause".into(),
                source: Some(Arc::new(
                    crate::handlers::invitation_service::EnrollmentInitiatorTaskFailure::new(
                        AgentError::from(primary),
                        Some(finalizer),
                    ),
                )),
            },
            None => primary,
        }
    }

    pub(crate) fn retain_original_cancelled_retirement(
        &self,
        original: &crate::runtime::services::enrollment_window::EnrollmentExecutionChild,
        terminal: &Arc<crate::handlers::invitation::OriginalIssuerCancellationTerminal>,
        writes: Result<
            crate::runtime::effects::AcknowledgedEnrollmentRetirement,
            aura_core::AuraError,
        >,
    ) -> Result<AcceptedOriginalRetirementPublication, aura_core::AuraError> {
        self.require_original_cancelled_terminal(terminal)?;
        self.retain_original_retirement(original, terminal.issued(), writes)
            .map_err(|source| aura_core::AuraError::Internal {
                message: "retain original cancelled retirement".into(),
                source: Some(Arc::new(source)),
            })
    }

    pub(crate) fn require_original_cancelled_terminal(
        &self,
        terminal: &Arc<crate::handlers::invitation::OriginalIssuerCancellationTerminal>,
    ) -> Result<(), aura_core::AuraError> {
        if !self
            .cancelled
            .get()
            .is_some_and(|actual| Arc::ptr_eq(actual, terminal))
        {
            return Err(aura_core::AuraError::permission_denied(
                "cancelled retirement belongs to another original terminal",
            ));
        }
        Ok(())
    }

    #[aura_macros::capability_boundary(category = "capability_gated", capability = "OriginalCancelledNoticeSendCompletion", capability_type = crate::handlers::invitation::OriginalCancelledNoticeSendCompletion, receiver_type = OriginalIssuerTerminalDestination, family = "runtime_helper")]
    pub(crate) fn retain_original_cancelled_send(
        &self,
        effects: &Arc<AuraEffectSystem>,
        terminal: &Arc<crate::handlers::invitation::OriginalIssuerCancellationTerminal>,
        notice: &Arc<crate::handlers::invitation::SignedCancelledNotice>,
        completed: crate::handlers::invitation::OriginalCancelledNoticeSendCompletion,
    ) -> AgentResult<()> {
        let validation = (|| {
            self.require_original_cancelled_terminal(terminal)?;
            self.require_original_control(terminal.issued())?;
            if !self
                .signed_cancelled_notice
                .get()
                .is_some_and(|actual| Arc::ptr_eq(actual, notice))
            {
                return Err(AgentError::invalid(
                    "completed send has another original signed notice",
                ));
            }
            completed.require_live_original(effects, terminal, notice)
        })();
        let retained = RetainedOriginalCancelledSend {
            terminal: Arc::downgrade(terminal),
            notice: Arc::downgrade(notice),
            actual: completed,
        };
        if let Err(source) = validation {
            return Err(AgentError::from(aura_core::AuraError::PermissionDenied {
                message: "original cancellation send completion binding was refused".into(),
                source: Some(Arc::new(RejectedOriginalCancelledSend {
                    source: Some(source),
                    _retained: retained,
                })),
            }));
        }
        self.cancelled_send_completion
            .set(retained)
            .map_err(|retained| {
                AgentError::from(aura_core::AuraError::PermissionDenied {
                    message: "original cancellation send completion is already retained".into(),
                    source: Some(Arc::new(RejectedOriginalCancelledSend {
                        source: None,
                        _retained: retained,
                    })),
                })
            })
    }

    pub(super) fn original_cancelled_send_completion(
        &self,
        terminal: &Arc<crate::handlers::invitation::OriginalIssuerCancellationTerminal>,
    ) -> Result<
        Option<&crate::handlers::invitation::OriginalCancelledNoticeSendCompletion>,
        aura_core::AuraError,
    > {
        self.require_original_cancelled_terminal(terminal)?;
        self.require_original_control(terminal.issued())?;
        let Some(completed) = self.cancelled_send_completion.get() else {
            return Ok(None);
        };
        let notice = self.signed_cancelled_notice.get().ok_or_else(|| {
            aura_core::AuraError::invalid("original completed send has no signed notice")
        })?;
        if !completed
            .terminal
            .upgrade()
            .is_some_and(|actual| Arc::ptr_eq(&actual, terminal))
            || !completed
                .notice
                .upgrade()
                .is_some_and(|actual| Arc::ptr_eq(&actual, notice))
        {
            return Err(aura_core::AuraError::permission_denied(
                "send completion has another original custodian",
            ));
        }
        completed
            .actual
            .require_original_binding(terminal, notice)
            .map_err(|source| match source {
                AgentError::Aura(source) => source,
                source => aura_core::AuraError::Internal {
                    message: "original cancellation send binding failed".into(),
                    source: Some(Arc::new(source)),
                },
            })?;
        Ok(Some(&completed.actual))
    }

    #[aura_macros::capability_boundary(category = "capability_gated", capability = "OriginalCancelledNoticeSendCompletion", capability_type = crate::handlers::invitation::OriginalCancelledNoticeSendCompletion, receiver_type = OriginalIssuerTerminalDestination, family = "runtime_helper")]
    pub(super) async fn complete_original_cancelled_send(
        &self,
        effects: &AuraEffectSystem,
        original: &super::enrollment_window::EnrollmentExecutionChild,
        terminal: &Arc<crate::handlers::invitation::OriginalIssuerCancellationTerminal>,
        completed: &crate::handlers::invitation::OriginalCancelledNoticeSendCompletion,
    ) -> Result<crate::handlers::invitation::IssuerEnrollmentTerminalOutcome, aura_core::AuraError>
    {
        if !self
            .original_cancelled_send_completion(terminal)?
            .is_some_and(|actual| std::ptr::eq(actual, completed))
        {
            return Err(aura_core::AuraError::permission_denied(
                "missing original send completion custody",
            ));
        }
        // Native close failure remains in vm_close_retirement. Only its
        // actual resource acknowledgment permits this same owner to finish.
        self.dispose_original_vm(effects, terminal.issued(), original)
            .await?;
        self.protocol_completed
            .store(true, std::sync::atomic::Ordering::Release);
        Ok(
            crate::handlers::invitation::IssuerEnrollmentTerminalOutcome::Cancelled(Arc::clone(
                terminal,
            )),
        )
    }

    pub(crate) fn with_original_cancelled_retirement<T>(
        &self,
        original: &Arc<
            crate::runtime::services::ceremony_tracker::RegisteredEnrollmentWindowCapability,
        >,
        terminal: &Arc<crate::handlers::invitation::OriginalIssuerCancellationTerminal>,
        consume: impl FnOnce(
            &crate::runtime::services::ceremony_tracker::OriginalEnrollmentRetirementAcknowledgment,
            &crate::handlers::invitation::SignedCancelledNotice,
            Option<&aura_core::AuraError>,
        ) -> Result<T, aura_core::AuraError>,
    ) -> Result<T, aura_core::AuraError> {
        let retained = self.cancelled.get().ok_or_else(|| {
            aura_core::AuraError::invalid("original cancelled decision is not retained")
        })?;
        if !Arc::ptr_eq(retained, terminal) {
            return Err(aura_core::AuraError::invalid(
                "cancelled decision belongs to another original owner",
            ));
        }
        original.require_original_cancellation(terminal)?;
        let notice = self.signed_cancelled_notice.get().ok_or_else(|| {
            aura_core::AuraError::invalid("original signed cancellation is not retained")
        })?;
        let state = self.retirement.lock();
        let receipt = state.acknowledged.as_ref().ok_or_else(|| {
            state.first_failure.clone().unwrap_or_else(|| {
                aura_core::AuraError::invalid("original retirement acknowledgment is unresolved")
            })
        })?;
        receipt.require_original(original, terminal.issued())?;
        // A late genuine receipt settles resources independently of the first
        // failure. The caller must retain that failure as the domain outcome.
        consume(receipt, notice, state.first_failure.as_ref())
    }
    pub(crate) fn retain_cancelled_terminal(
        &self,
        terminal: crate::handlers::invitation::OriginalIssuerCancellationTerminal,
    ) -> AgentResult<Arc<crate::handlers::invitation::OriginalIssuerCancellationTerminal>> {
        let terminal = Arc::new(terminal);
        self.cancelled.set(terminal.clone()).map_err(|retained| {
            aura_core::AuraError::PermissionDenied {
                message: "original cancellation terminal was already retained".into(),
                source: Some(Arc::new(RejectedOriginalCancellationTerminal { retained })),
            }
        })?;
        Ok(terminal)
    }
    #[cfg(test)]
    pub(crate) fn retains_original_retirement_acknowledgment(&self) -> bool {
        self.retirement.lock().acknowledged.is_some()
    }
    pub(crate) fn retain_original_retirement_failure(&self, source: aura_core::AuraError) {
        let mut state = self.retirement.lock();
        state.first_failure.get_or_insert(source);
        drop(state);
        self.retirement_changed.notify_waiters();
    }
    pub(crate) fn retain_original_cancelled_notice(
        &self,
        notice: crate::handlers::invitation::SignedCancelledNotice,
    ) -> AgentResult<Arc<crate::handlers::invitation::SignedCancelledNotice>> {
        let notice = Arc::new(notice);
        match self.signed_cancelled_notice.set(Arc::clone(&notice)) {
            Ok(()) => Ok(notice),
            Err(returned) => Err(AgentError::from(aura_core::AuraError::Internal {
                message: "original signed cancellation notice already retained".into(),
                source: Some(Arc::new(RejectedOriginalCancelledNotice {
                    retained: returned,
                })),
            })),
        }
    }

    pub(crate) fn original_cancelled_notice(
        &self,
        terminal: &Arc<crate::handlers::invitation::OriginalIssuerCancellationTerminal>,
    ) -> Result<Option<Arc<crate::handlers::invitation::SignedCancelledNotice>>, aura_core::AuraError>
    {
        self.require_original_cancelled_terminal(terminal)?;
        self.require_original_control(terminal.issued())?;
        Ok(self.signed_cancelled_notice.get().cloned())
    }

    pub(crate) fn original_cancelled_retirement_acknowledged(
        &self,
        original: &Arc<
            crate::runtime::services::ceremony_tracker::RegisteredEnrollmentWindowCapability,
        >,
        terminal: &Arc<crate::handlers::invitation::OriginalIssuerCancellationTerminal>,
    ) -> Result<bool, aura_core::AuraError> {
        self.require_original_cancelled_terminal(terminal)?;
        original.require_original_cancellation(terminal)?;
        let state = self.retirement.lock();
        if let Some(receipt) = state.acknowledged.as_ref() {
            receipt.require_original(original, terminal.issued())?;
            // A genuine late acknowledgment settles resources while the
            // retained first failure still determines the domain outcome.
            return Ok(true);
        }
        if let Some(refused) = state.refused.as_ref() {
            return Err(refused.source.clone());
        }
        Ok(false)
    }
    pub(crate) fn retain_original_retirement(
        &self,
        original: &crate::runtime::services::enrollment_window::EnrollmentExecutionChild,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        writes: Result<
            crate::runtime::effects::AcknowledgedEnrollmentRetirement,
            aura_core::AuraError,
        >,
    ) -> Result<AcceptedOriginalRetirementPublication, RejectedOriginalRetirementPublication> {
        match writes {
            Err(source) => {
                self.retain_original_retirement_failure(source.clone());
                Err(RejectedOriginalRetirementPublication::StoredFailure(source))
            }
            Ok(writes) => self.retain_original_retirement_receipt(
                original.acknowledge_original_retirement(writes, issued),
            ),
        }
    }
    pub(crate) fn retain_original_retirement_receipt(
        &self,
        incoming: Result<
            crate::runtime::services::ceremony_tracker::OriginalEnrollmentRetirementAcknowledgment,
            crate::runtime::services::ceremony_tracker::RejectedOriginalEnrollmentRetirement,
        >,
    ) -> Result<AcceptedOriginalRetirementPublication, RejectedOriginalRetirementPublication> {
        let mut state = self.retirement.lock();
        let accepted_acknowledgment = match incoming {
            Ok(receipt) if state.acknowledged.is_none() => {
                state.acknowledged = Some(receipt);
                true
            }
            Ok(receipt) => {
                return Err(
                    RejectedOriginalRetirementPublication::UnacceptedAcknowledgment {
                        receipt: Box::new(receipt),
                    },
                )
            }
            Err(rejected) if state.refused.is_none() => {
                state
                    .first_failure
                    .get_or_insert_with(|| rejected.source.clone());
                state.refused = Some(rejected);
                false
            }
            Err(rejected) => {
                return Err(RejectedOriginalRetirementPublication::UnacceptedRefusal {
                    source: rejected.source.clone(),
                    rejected: Box::new(rejected),
                })
            }
        };
        let outcome = if accepted_acknowledgment {
            // Physical progress does not replace the first domain failure.
            // Only the actual incoming ACK accepted above can issue this value.
            Ok(AcceptedOriginalRetirementPublication {
                retained_failure: state.first_failure.clone(),
            })
        } else {
            Err(RejectedOriginalRetirementPublication::StoredFailure(
                state.first_failure.clone().ok_or_else(|| {
                    RejectedOriginalRetirementPublication::StoredFailure(
                        aura_core::AuraError::invalid(
                            "refused original receipt lost its actual failure",
                        ),
                    )
                })?,
            ))
        };
        drop(state);
        self.retirement_changed.notify_waiters();
        outcome
    }
    pub(crate) fn require_original_retirement(
        &self,
        original: &Arc<
            crate::runtime::services::ceremony_tracker::RegisteredEnrollmentWindowCapability,
        >,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<bool, aura_core::AuraError> {
        let state = self.retirement.lock();
        if let Some(source) = state.first_failure.as_ref() {
            return Err(source.clone());
        }
        if let Some(receipt) = state.acknowledged.as_ref() {
            receipt.require_original(original, issued)?;
            return Ok(true);
        }
        Ok(false)
    }
    /// Physical cleanup observation accepts only the actual original receipt.
    /// Its retained domain failure remains available through the domain accessor.
    pub(crate) fn require_original_retirement_cleanup_acknowledgment(
        &self,
        original: &Arc<
            crate::runtime::services::ceremony_tracker::RegisteredEnrollmentWindowCapability,
        >,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<(), aura_core::AuraError> {
        let state = self.retirement.lock();
        if let Some(receipt) = state.acknowledged.as_ref() {
            return receipt.require_original(original, issued);
        }
        Err(state.first_failure.clone().unwrap_or_else(|| {
            aura_core::AuraError::invalid(
                "original retirement cleanup has no actual acknowledgment",
            )
        }))
    }
    pub(crate) async fn await_original_retirement(
        &self,
        original: &Arc<
            crate::runtime::services::ceremony_tracker::RegisteredEnrollmentWindowCapability,
        >,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<(), aura_core::AuraError> {
        loop {
            let changed = self.retirement_changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.require_original_retirement(original, issued)? {
                return Ok(());
            }
            changed.await;
        }
    }
    pub(crate) fn publish(
        &self,
        result: AgentResult<crate::handlers::invitation::IssuerEnrollmentTerminalOutcome>,
    ) -> Result<(), aura_core::AuraError> {
        #[cfg(test)]
        let result = match result {
            Ok(crate::handlers::invitation::IssuerEnrollmentTerminalOutcome::Committed(
                receipt,
            )) => {
                let hook = self.receipt_test_hook.lock().take();
                let receipt = match hook {
                    Some(hook) => hook(receipt),
                    None => receipt,
                };
                Ok(
                    crate::handlers::invitation::IssuerEnrollmentTerminalOutcome::Committed(
                        receipt,
                    ),
                )
            }
            other => other,
        };
        // Atomic producer deposit into the shared synchronous custody slot.
        let result = super::issuer_error_custody::SynchronousCustodyCell::new(Some(result));
        self.result
            .set(result)
            .map_err(|returned| aura_core::AuraError::PermissionDenied {
                message: "original issuer terminal destination was already published".into(),
                // Keep the original slot immutable. An impossible second producer
                // result is also retained in the concrete refusal, never substituted.
                source: Some(Arc::new(UndeliveredIssuerTerminal {
                    source: None,
                    retained: returned,
                })),
            })
    }
}
