//! Runtime-owned original issuer result, cancellation and retirement custody.

use super::issuer_error_custody::UndeliveredIssuerTerminal;
use crate::core::{AgentError, AgentResult};
use std::sync::Arc;

/// One original child writes one original custodian's result. This slot is
/// independent from diagnostic failure retention, never an addressable queue.
#[cfg(test)]
type NegativeCleanupAfterObservationGate = (
    tokio::sync::oneshot::Sender<()>,
    tokio::sync::oneshot::Receiver<()>,
    tokio::sync::oneshot::Sender<aura_core::TimeoutBudgetError>,
);

#[derive(Default)]
pub(crate) struct OriginalIssuerTerminalDestination {
    #[cfg(feature = "simulation")]
    simulated_subtree_disposal:
        std::sync::OnceLock<Arc<crate::task_registry::TaskGroupDisposalAcknowledgment>>,
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
    #[allow(clippy::disallowed_types)] // Synchronous custody slot; no guard crosses an await.
    pub(crate) result: std::sync::OnceLock<
        parking_lot::Mutex<
            Option<AgentResult<crate::handlers::invitation::IssuerEnrollmentTerminalOutcome>>,
        >,
    >,
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
    #[cfg(feature = "simulation")]
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
    #[cfg(feature = "simulation")]
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

    pub(crate) fn retain_original_cancelled_retirement(
        &self,
        original: &crate::runtime::services::enrollment_window::EnrollmentExecutionChild,
        terminal: &Arc<crate::handlers::invitation::OriginalIssuerCancellationTerminal>,
        writes: Result<
            crate::runtime::effects::AcknowledgedEnrollmentRetirement,
            aura_core::AuraError,
        >,
    ) -> Result<(), aura_core::AuraError> {
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
    pub(crate) fn retain_original_retirement(
        &self,
        original: &crate::runtime::services::enrollment_window::EnrollmentExecutionChild,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        writes: Result<
            crate::runtime::effects::AcknowledgedEnrollmentRetirement,
            aura_core::AuraError,
        >,
    ) -> Result<(), RejectedOriginalRetirementPublication> {
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
    ) -> Result<(), RejectedOriginalRetirementPublication> {
        let mut state = self.retirement.lock();
        match incoming {
            Ok(receipt) if state.acknowledged.is_none() => state.acknowledged = Some(receipt),
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
            }
            Err(rejected) => {
                return Err(RejectedOriginalRetirementPublication::UnacceptedRefusal {
                    source: rejected.source.clone(),
                    rejected: Box::new(rejected),
                })
            }
        }
        let outcome = match state.first_failure.as_ref() {
            Some(source) => Err(RejectedOriginalRetirementPublication::StoredFailure(
                source.clone(),
            )),
            None => Ok(()),
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
        #[allow(clippy::disallowed_types)]
        // Atomic producer deposit into a synchronous custody slot.
        let result = parking_lot::Mutex::new(Some(result));
        self.result
            .set(result)
            .map_err(|returned| aura_core::AuraError::PermissionDenied {
                message: "original issuer terminal destination was already published".into(),
                // Keep the original slot immutable. An impossible second producer
                // result is also retained in the concrete refusal, never substituted.
                source: Some(Arc::new(UndeliveredIssuerTerminal { retained: returned })),
            })
    }
}
