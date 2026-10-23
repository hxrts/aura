//! Process-local ACK custody attached to the original admitted window lease.

use super::{AdmittedWindowBinding, EnrollmentExecutionChild};
use crate::handlers::invitation::enrollment_manifest_admission::AdmittedEnrollmentManifest;
use crate::runtime::services::issuer_error_custody::SynchronousCustodyCell;
use aura_core::AuraError;
use std::sync::{Arc, Weak};
use tokio::sync::{Notify, OwnedSemaphorePermit, Semaphore};

const MAX_ORIGINAL_ACK_BORROWS: usize = 64;

/// Pure binding derived from sealed admission evidence, never a lease itself.
#[derive(PartialEq, Eq)]
struct OriginalAdmittedTransportBinding(AdmittedWindowBinding);

impl OriginalAdmittedTransportBinding {
    fn from_admitted(admitted: &AdmittedEnrollmentManifest) -> Self {
        Self(EnrollmentExecutionChild::admitted_binding(admitted))
    }
}

/// The existing single-window gate also locates its exact live ACK custodian.
pub(crate) struct OriginalAdmittedTransportGate {
    semaphore: Arc<Semaphore>,
    current: SynchronousCustodyCell<Option<Weak<OriginalAdmittedTransportCustody>>>,
}

impl Default for OriginalAdmittedTransportGate {
    fn default() -> Self {
        Self::new()
    }
}

impl OriginalAdmittedTransportGate {
    pub(crate) fn require_permit(&self, permit: &OwnedSemaphorePermit) -> Result<(), AuraError> {
        if !Arc::ptr_eq(permit.semaphore(), &self.semaphore) {
            return Err(AuraError::permission_denied(
                "foreign admitted window permit",
            ));
        }
        Ok(())
    }

    pub(crate) fn new() -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(1)),
            current: SynchronousCustodyCell::new(None),
        }
    }

    fn acquire_permit(&self) -> Result<OwnedSemaphorePermit, AuraError> {
        self.semaphore
            .clone()
            .try_acquire_owned()
            .map_err(|source| AuraError::Internal {
                message: "admitted enrollment execution already has a window owner".into(),
                source: Some(Arc::new(source)),
            })
    }

    pub(crate) fn acquire(&self) -> Result<OwnedSemaphorePermit, AuraError> {
        self.current.with(|current| {
            if current.as_ref().and_then(Weak::upgrade).is_some() {
                return Err(AuraError::invalid(
                    "original admitted transport custody remains live",
                ));
            }
            self.acquire_permit()
        })
    }

    /// The provider checks the fresh birth before this atomic reservation.
    pub(crate) fn acquire_original(
        &self,
        admitted: &AdmittedEnrollmentManifest,
    ) -> Result<(OwnedSemaphorePermit, Arc<OriginalAdmittedTransportCustody>), AuraError> {
        self.current.with(|current| {
            if current.as_ref().and_then(Weak::upgrade).is_some() {
                return Err(AuraError::invalid(
                    "original admitted transport already attached",
                ));
            }
            let permit = self.acquire_permit()?;
            let custody = Arc::new(OriginalAdmittedTransportCustody {
                binding: OriginalAdmittedTransportBinding::from_admitted(admitted),
                state: SynchronousCustodyCell::new(TransportState::default()),
                changed: Notify::new(),
            });
            *current = Some(Arc::downgrade(&custody));
            Ok((permit, custody))
        })
    }

    pub(crate) fn borrow_ack(
        &self,
        admitted: &AdmittedEnrollmentManifest,
    ) -> Result<Option<OriginalAdmittedAckBorrow>, AuraError> {
        let original = self
            .current
            .with(|current| current.as_ref().and_then(Weak::upgrade));
        // Recovery may drain durable ingress before an original execution is
        // attached. Absence gives no task outcome or replacement owner.
        let Some(original) = original else {
            return Ok(None);
        };
        if original.binding != OriginalAdmittedTransportBinding::from_admitted(admitted) {
            return Err(AuraError::permission_denied("foreign admitted ACK owner"));
        }
        original.borrow_ack().map(Some)
    }
}

#[derive(Default)]
struct TransportState {
    pending: usize,
    closing: bool,
    first: Option<FirstOperationFailure>,
    ack_failure: Option<Arc<AuraError>>,
}

#[derive(Clone, Copy)]
enum FirstOperationFailure {
    Ack,
    Execution,
}

pub(crate) struct OriginalAdmittedAckFailure {
    source: Arc<AuraError>,
    precedes_execution: bool,
}

impl OriginalAdmittedAckFailure {
    pub(crate) fn into_parts(self) -> (Arc<AuraError>, bool) {
        (self.source, self.precedes_execution)
    }
}

/// Shared observations cannot publish a domain terminal or acknowledge a VM.
pub(crate) struct OriginalAdmittedTransportCustody {
    binding: OriginalAdmittedTransportBinding,
    state: SynchronousCustodyCell<TransportState>,
    changed: Notify,
}

impl OriginalAdmittedTransportCustody {
    fn borrow_ack(self: &Arc<Self>) -> Result<OriginalAdmittedAckBorrow, AuraError> {
        self.state.with(|state| {
            if state.closing {
                return Err(AuraError::Invalid {
                    message: "original admitted ACK ingress is closed".into(),
                    source: Some(Arc::new(OriginalAdmittedAckAdmissionError::Closed)),
                });
            }
            if state.pending >= MAX_ORIGINAL_ACK_BORROWS {
                let source = AuraError::Invalid {
                    message: "original admitted ACK ingress capacity exceeded".into(),
                    source: Some(Arc::new(
                        OriginalAdmittedAckAdmissionError::CapacityExceeded,
                    )),
                };
                state.first.get_or_insert(FirstOperationFailure::Ack);
                state
                    .ack_failure
                    .get_or_insert_with(|| Arc::new(source.clone()));
                return Err(source);
            }
            state.pending += 1;
            Ok(OriginalAdmittedAckBorrow {
                original: self.clone(),
                finished: false,
            })
        })
    }

    pub(crate) fn observe_execution_failure(&self) {
        self.state.with(|state| {
            state.first.get_or_insert(FirstOperationFailure::Execution);
        });
    }

    /// Close original ingress and observe every actual ACK future's disposition.
    /// Dropping this wait retains the closed gate and outstanding borrows.
    pub(crate) async fn settle(&self) -> Option<OriginalAdmittedAckFailure> {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let settled = self.state.with(|state| {
                state.closing = true;
                if state.pending == 0 {
                    Some(
                        state
                            .ack_failure
                            .as_ref()
                            .map(|source| OriginalAdmittedAckFailure {
                                source: Arc::clone(source),
                                precedes_execution: matches!(
                                    state.first,
                                    Some(FirstOperationFailure::Ack)
                                ),
                            }),
                    )
                } else {
                    None
                }
            });
            if let Some(failure) = settled {
                return failure;
            }
            changed.await;
        }
    }

    fn finish_ack(&self, source: Option<AuraError>) {
        self.state.with(|state| {
            if let Some(source) = source {
                state.first.get_or_insert(FirstOperationFailure::Ack);
                state.ack_failure.get_or_insert_with(|| Arc::new(source));
            }
            assert!(state.pending > 0, "original ACK borrow must finish once");
            state.pending -= 1;
        });
        self.changed.notify_waiters();
    }
}

/// A move-only borrow captured before queueing and awaiting the original ACK.
/// Completion never looks up a later owner from an invitation or manifest id.
pub(crate) struct OriginalAdmittedAckBorrow {
    original: Arc<OriginalAdmittedTransportCustody>,
    finished: bool,
}

impl OriginalAdmittedAckBorrow {
    pub(crate) fn fail(mut self, source: AuraError) -> AuraError {
        self.original.finish_ack(Some(source.clone()));
        self.finished = true;
        source
    }

    pub(crate) fn finish(mut self, result: Result<(), AuraError>) -> Result<(), AuraError> {
        self.original.finish_ack(result.as_ref().err().cloned());
        self.finished = true;
        result
    }
}

#[derive(Debug, thiserror::Error)]
enum OriginalAdmittedAckAdmissionError {
    #[error("original admitted ACK ingress closed before a new borrow")]
    Closed,
    #[error("original admitted ACK borrow capacity exceeded")]
    CapacityExceeded,
}

#[derive(Debug, thiserror::Error)]
#[error("original admitted ACK future was dropped before its disposition")]
struct OriginalAdmittedAckAbandoned;

impl Drop for OriginalAdmittedAckBorrow {
    fn drop(&mut self) {
        if !self.finished {
            self.original.finish_ack(Some(AuraError::Internal {
                message: "original admitted ACK owner was interrupted".into(),
                source: Some(Arc::new(OriginalAdmittedAckAbandoned)),
            }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    fn original() -> Arc<OriginalAdmittedTransportCustody> {
        Arc::new(OriginalAdmittedTransportCustody {
            binding: OriginalAdmittedTransportBinding(AdmittedWindowBinding {
                ceremony: aura_core::CeremonyId::new("ack-custody"),
                invitation: aura_core::InvitationId::new("ack-custody-invitation"),
                manifest_digest: [71; 32],
                device: aura_core::DeviceId::new_from_entropy([72; 32]),
                admitted_at_ms: 100,
                expires_at_ms: 900_100,
            }),
            state: SynchronousCustodyCell::new(TransportState::default()),
            changed: Notify::new(),
        })
    }

    #[tokio::test]
    async fn original_ack_settlement_waits_for_actual_disposition_and_native_source() {
        let original = original();
        let borrowed = original.borrow_ack().expect("actual original borrow");
        let mut settled = Box::pin(original.settle());
        assert!(futures::poll!(settled.as_mut()).is_pending());
        let native = Arc::new(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "selected reset",
        ));
        let returned = borrowed
            .finish(Err(AuraError::Network {
                message: "selected ACK read failed".into(),
                source: Some(native.clone()),
            }))
            .expect_err("actual ACK failure returns to maintenance");
        original.observe_execution_failure();
        let (retained, first) = settled.await.expect("same original ACK cause").into_parts();
        assert!(
            first,
            "later execution failure cannot replace the first ACK cause"
        );
        assert!(std::ptr::eq(
            retained.source().unwrap(),
            returned.source().unwrap()
        ));
        let actual = retained
            .source()
            .and_then(|source| source.downcast_ref::<std::io::Error>())
            .expect("actual native cause is directly exposed");
        assert!(std::ptr::eq(actual, native.as_ref()));
        assert_eq!(original.state.with(|state| state.pending), 0);
    }

    #[tokio::test]
    async fn dropped_ack_wait_retains_closed_gate_until_actual_borrow_disposal() {
        let original = original();
        let borrowed = original.borrow_ack().expect("original pending ACK");
        let mut settled = Box::pin(original.settle());
        assert!(futures::poll!(settled.as_mut()).is_pending());
        drop(settled);
        assert!(
            original.borrow_ack().is_err(),
            "interrupted cleanup cannot reopen ingress"
        );
        assert_eq!(original.state.with(|state| state.pending), 1);
        drop(borrowed);
        let (retained, first) = original
            .settle()
            .await
            .expect("actual dropped ACK disposition")
            .into_parts();
        assert!(first);
        assert!(retained
            .source()
            .is_some_and(|source| source.is::<OriginalAdmittedAckAbandoned>()));
        assert_eq!(original.state.with(|state| state.pending), 0);
    }

    #[tokio::test]
    async fn original_execution_failure_precedes_later_ack_failure() {
        let original = original();
        let borrowed = original.borrow_ack().expect("original pending ACK");
        original.observe_execution_failure();
        borrowed
            .finish(Err(AuraError::invalid("later original ACK refusal")))
            .expect_err("later ACK failure retained independently");
        let (_, first) = original
            .settle()
            .await
            .expect("independent ACK cause retained")
            .into_parts();
        assert!(
            !first,
            "later ACK cause cannot replace earlier execution cause"
        );
    }

    #[test]
    fn original_ack_borrow_blocks_replacement_after_window_permit_drop() {
        let gate = OriginalAdmittedTransportGate::new();
        let permit = gate.acquire().expect("real original semaphore permit");
        let original = original();
        gate.current
            .with(|current| *current = Some(Arc::downgrade(&original)));
        let borrowed = original.borrow_ack().expect("original pending ACK");
        drop(original);
        drop(permit);
        assert!(
            gate.acquire().is_err(),
            "live original ACK cannot migrate into a replacement window"
        );
        borrowed
            .finish(Ok(()))
            .expect("actual original disposition");
        drop(
            gate.acquire()
                .expect("new owner only after actual borrow disposal"),
        );
    }

    #[test]
    fn original_transport_gate_rejects_foreign_semaphore_permit() {
        let original = OriginalAdmittedTransportGate::new();
        let foreign = OriginalAdmittedTransportGate::new();
        let permit = original.acquire().expect("actual original permit");
        original
            .require_permit(&permit)
            .expect("exact original permit");
        assert!(foreign.require_permit(&permit).is_err());
    }

    #[tokio::test]
    async fn original_ack_borrows_refuse_capacity_without_early_settlement() {
        let original = original();
        let borrows: Vec<_> = (0..MAX_ORIGINAL_ACK_BORROWS)
            .map(|_| original.borrow_ack().expect("bounded original borrow"))
            .collect();
        assert!(original.borrow_ack().is_err());
        assert_eq!(
            original.state.with(|state| state.pending),
            MAX_ORIGINAL_ACK_BORROWS
        );
        let mut settled = Box::pin(original.settle());
        assert!(futures::poll!(settled.as_mut()).is_pending());
        drop(borrows);
        let (failure, first) = settled
            .await
            .expect("actual capacity refusal retained")
            .into_parts();
        assert!(first);
        assert!(matches!(
            failure
                .source()
                .and_then(|source| source.downcast_ref::<OriginalAdmittedAckAdmissionError>()),
            Some(OriginalAdmittedAckAdmissionError::CapacityExceeded)
        ));
        assert_eq!(original.state.with(|state| state.pending), 0);
    }
}
