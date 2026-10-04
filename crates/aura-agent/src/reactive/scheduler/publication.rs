//! Exact scheduler-issued publication and processed observation custody.
use super::FactSource;
use aura_core::{
    effects::PhysicalTimeEffects,
    time::timeout::{execute_with_timeout_budget, TimeoutRunError},
    TimeoutBudget,
};
use std::sync::{Arc, Weak};
use tokio::sync::{mpsc, watch, Mutex};

/// Structural publication and observation failures.
#[derive(Debug, thiserror::Error)]
pub enum FactProcessingError {
    /// The original scheduler is no longer accepting publications.
    #[error("reactive scheduler publication sink is closed")]
    SinkClosed {
        #[source]
        source: mpsc::error::SendError<FactSource>,
    },
    /// A scheduler was never attached to the required owner.
    #[error("required reactive scheduler ingress is absent")]
    IngressAbsent,
    /// Standalone scheduler ingress carries no canonical runtime attachment.
    #[error("reactive ingress has no retained runtime owner")]
    RuntimeOwnerAbsent,
    /// Original ordered publication ids are exhausted.
    #[error("reactive scheduler publication sequence is exhausted")]
    SequenceExhausted,
    /// A target belongs to another original scheduler.
    #[error("reactive processing target belongs to another scheduler owner")]
    ForeignOwner,
    /// Actual scheduler failure before the selected publication was processed.
    #[error("reactive scheduler failed before processing the selected publication")]
    SchedulerFailed {
        #[source]
        source: aura_core::AuraError,
    },
    /// Scheduler stopped before the selected accepted publication was processed.
    #[error("reactive scheduler stopped before processing the selected publication")]
    SchedulerStopped {
        #[source]
        source: watch::error::RecvError,
    },
}

#[derive(Clone, Debug, Default)]
pub(super) struct ProcessingProgress {
    pub(super) processed: u64,
    pub(super) failure: Option<aura_core::AuraError>,
}

#[derive(Debug)]
struct IngressOwner {
    runtime: Option<Weak<crate::runtime::AuraEffectSystem>>,
    issued: Mutex<u64>,
    processed: watch::Receiver<ProcessingProgress>,
}

/// Scheduler-created handle; callers cannot substitute counters or watches.
#[derive(Clone, Debug)]
pub struct FactIngress {
    sender: mpsc::Sender<FactSource>,
    owner: Arc<IngressOwner>,
}

/// A queue envelope minted only while the original ingress owns ordered enqueue.
/// Its sequence is private and is not serialization or caller supplied authority.
#[derive(Clone, Debug)]
pub struct IssuedFactPublication {
    facts: Vec<aura_journal::fact::Fact>,
    sequence: u64,
    owner: Arc<IngressOwner>,
}
impl IssuedFactPublication {
    pub(super) fn into_parts(self) -> (Vec<aura_journal::fact::Fact>, u64) {
        (self.facts, self.sequence)
    }
}

/// Move-only exact target retained from successful enqueue into the original scheduler.
/// Its processing acknowledgment does not replace canonical entity or app
/// semantic readiness evidence. It proves that configured views completed the
/// accepted publication; diagnostic broadcasts cannot mint that evidence.
///
/// Callers cannot substitute a chosen sequence or watch:
/// ```compile_fail
/// use aura_agent::reactive::FactProcessingTargetCapability;
/// fn fabricate() -> FactProcessingTargetCapability {
///     FactProcessingTargetCapability { sequence: 1, owner: panic!(), processed: panic!() }
/// }
/// ```
/// A retained target has one observation owner:
/// ```compile_fail
/// use aura_agent::reactive::FactProcessingTargetCapability;
/// fn duplicate(target: &FactProcessingTargetCapability) -> FactProcessingTargetCapability {
///     Clone::clone(target)
/// }
/// ```
/// Diagnostic bytes do not restore scheduler provenance:
/// ```compile_fail
/// use aura_agent::reactive::FactProcessingTargetCapability;
/// fn restore(bytes: &[u8]) -> FactProcessingTargetCapability {
///     serde_json::from_slice(bytes).unwrap()
/// }
/// ```

#[must_use = "choose bounded processing observation or explicit observed-only completion"]
pub struct FactProcessingTargetCapability {
    owner: Arc<IngressOwner>,
    sequence: u64,
    processed: watch::Receiver<ProcessingProgress>,
}

impl FactIngress {
    pub(super) fn original(
        sender: mpsc::Sender<FactSource>,
        processed: watch::Receiver<ProcessingProgress>,
        runtime: Option<Weak<crate::runtime::AuraEffectSystem>>,
    ) -> Self {
        Self {
            sender,
            owner: Arc::new(IngressOwner {
                runtime,
                issued: Mutex::new(0),
                processed,
            }),
        }
    }

    pub(crate) fn require_runtime_owner(
        &self,
        effects: &crate::runtime::AuraEffectSystem,
    ) -> Result<(), FactProcessingError> {
        let runtime = self
            .owner
            .runtime
            .as_ref()
            .and_then(Weak::upgrade)
            .ok_or(FactProcessingError::RuntimeOwnerAbsent)?;
        if !std::ptr::eq(runtime.as_ref(), effects) {
            return Err(FactProcessingError::ForeignOwner);
        }
        Ok(())
    }

    /// Legacy observed publication, without a required-processing target.
    pub async fn send(&self, source: FactSource) -> Result<(), FactProcessingError> {
        self.sender
            .send(source)
            .await
            .map_err(|source| FactProcessingError::SinkClosed { source })
    }

    /// Enqueue actual facts in original sequence order and issue their exact target.
    /// Holding the guard through enqueue prevents cancellation gaps from reordering
    /// accepted sequences. A failed/cancelled enqueue grants no target.
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "fact_processing_target", capability_type = FactProcessingTargetCapability, family = "proof_issuer")]
    #[aura_macros::authoritative_source(kind = "proof_issuer")]
    pub(crate) async fn publish_required(
        &self,
        facts: Vec<aura_journal::fact::Fact>,
    ) -> Result<FactProcessingTargetCapability, FactProcessingError> {
        let mut issued = self.owner.issued.lock().await;
        let sequence = issued
            .checked_add(1)
            .ok_or(FactProcessingError::SequenceExhausted)?;
        self.sender
            .send(FactSource::Published(IssuedFactPublication {
                facts,
                sequence,
                owner: self.owner.clone(),
            }))
            .await
            .map_err(|source| FactProcessingError::SinkClosed { source })?;
        *issued = sequence;
        Ok(FactProcessingTargetCapability {
            owner: self.owner.clone(),
            sequence,
            processed: self.owner.processed.clone(),
        })
    }

    pub(super) fn require_publication_owner(
        &self,
        publication: &IssuedFactPublication,
    ) -> Result<(), FactProcessingError> {
        if !Arc::ptr_eq(&self.owner, &publication.owner) {
            return Err(FactProcessingError::ForeignOwner);
        }
        Ok(())
    }

    pub(crate) fn require_target_owner(
        &self,
        target: &FactProcessingTargetCapability,
    ) -> Result<(), FactProcessingError> {
        if !Arc::ptr_eq(&self.owner, &target.owner) {
            return Err(FactProcessingError::ForeignOwner);
        }
        Ok(())
    }
}

impl FactProcessingTargetCapability {
    /// Explicitly complete publication without claiming reactive processing readiness.
    pub fn acknowledge_observed_only(self) {}

    /// Wait for this accepted target using the caller's original physical window.
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "fact_processing_target", capability_type = FactProcessingTargetCapability, receiver_type = FactProcessingTargetCapability, family = "runtime_helper")]
    pub(crate) async fn await_processed<T: PhysicalTimeEffects + Sync>(
        mut self,
        original_ingress: &FactIngress,
        time: &T,
        budget: &TimeoutBudget,
    ) -> Result<(), TimeoutRunError<FactProcessingError>> {
        original_ingress
            .require_target_owner(&self)
            .map_err(TimeoutRunError::Operation)?;
        execute_with_timeout_budget(time, budget, || async {
            loop {
                let progress = self.processed.borrow_and_update().clone();
                if progress.processed >= self.sequence {
                    return Ok(());
                }
                if let Some(source) = progress.failure {
                    return Err(FactProcessingError::SchedulerFailed { source });
                }
                self.processed
                    .changed()
                    .await
                    .map_err(|source| FactProcessingError::SchedulerStopped { source })?;
            }
        })
        .await
    }
}
