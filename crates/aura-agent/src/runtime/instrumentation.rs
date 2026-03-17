//! Shared runtime instrumentation taxonomy.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeSessionEvent {
    OwnerAssigned,
    OwnerRejected,
    OwnerTransferred,
    OwnerTransferRejected,
    IngressReceived,
    IngressDropped,
}

impl RuntimeSessionEvent {
    pub const fn as_event_name(self) -> &'static str {
        match self {
            Self::OwnerAssigned => "runtime.session.owner.assigned",
            Self::OwnerRejected => "runtime.session.owner.rejected",
            Self::OwnerTransferred => "runtime.session.owner.transferred",
            Self::OwnerTransferRejected => "runtime.session.owner.transfer_rejected",
            Self::IngressReceived => "runtime.session.ingress.received",
            Self::IngressDropped => "runtime.session.ingress.dropped",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
pub enum RuntimeVmEvent {
    ConcurrencyProfileSelected,
    ConcurrencyProfileFallback,
    ConcurrencyProfileAdmissionFailed,
}

impl RuntimeVmEvent {
    pub const fn as_event_name(self) -> &'static str {
        match self {
            Self::ConcurrencyProfileSelected => "runtime.vm.concurrency_profile.selected",
            Self::ConcurrencyProfileFallback => "runtime.vm.concurrency_profile.fallback",
            Self::ConcurrencyProfileAdmissionFailed => {
                "runtime.vm.concurrency_profile.admission_failed"
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeShutdownEvent {
    AlreadyInProgress,
    Stage,
    ReactivePipelineSignalFailed,
    TaskTreeEscalated,
    ServicesFailed,
    LifecycleFailed,
}

impl RuntimeShutdownEvent {
    pub const fn as_event_name(self) -> &'static str {
        match self {
            Self::AlreadyInProgress => "runtime.shutdown.already_in_progress",
            Self::Stage => "runtime.shutdown.stage",
            Self::ReactivePipelineSignalFailed => {
                "runtime.shutdown.reactive_pipeline_signal_failed"
            }
            Self::TaskTreeEscalated => "runtime.shutdown.task_tree_escalated",
            Self::ServicesFailed => "runtime.shutdown.services_failed",
            Self::LifecycleFailed => "runtime.shutdown.lifecycle_failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeReconfigurationEvent {
    SourceFootprintBackfilled,
    DelegationPersisted,
}

impl RuntimeReconfigurationEvent {
    pub const fn as_event_name(self) -> &'static str {
        match self {
            Self::SourceFootprintBackfilled => "runtime.reconfiguration.source_backfilled",
            Self::DelegationPersisted => "runtime.reconfiguration.delegation.persisted",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_instrumentation_event_names_are_stable() {
        assert_eq!(
            RuntimeSessionEvent::OwnerTransferred.as_event_name(),
            "runtime.session.owner.transferred"
        );
        assert_eq!(
            RuntimeVmEvent::ConcurrencyProfileFallback.as_event_name(),
            "runtime.vm.concurrency_profile.fallback"
        );
        assert_eq!(
            RuntimeShutdownEvent::TaskTreeEscalated.as_event_name(),
            "runtime.shutdown.task_tree_escalated"
        );
        assert_eq!(
            RuntimeReconfigurationEvent::DelegationPersisted.as_event_name(),
            "runtime.reconfiguration.delegation.persisted"
        );
    }
}
