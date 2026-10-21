//! Original issuer obligations retained when task admission or terminal delivery fails.

mod sync_cell;
pub(crate) use sync_cell::SynchronousCustodyCell;

#[derive(thiserror::Error)]
#[error("original issuer registration publication failed")]
pub(crate) struct UnpublishedIssuerTaskCustody {
    #[source]
    pub(crate) source: aura_core::AuraError,
    pub(crate) retained:
        SynchronousCustodyCell<Option<crate::task_registry::OriginalRequiredTaskHandle>>,
}

impl std::fmt::Debug for UnpublishedIssuerTaskCustody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("UnpublishedIssuerTaskCustody")
    }
}

#[derive(thiserror::Error)]
#[error("original issuer custody admission failed")]
pub(crate) struct RejectedIssuerTaskCustody<T: ?Sized + Send> {
    #[source]
    pub(crate) source: aura_core::AuraError,
    pub(crate) retained: SynchronousCustodyCell<Box<T>>,
}

impl<T: ?Sized + Send> std::fmt::Debug for RejectedIssuerTaskCustody<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RejectedIssuerTaskCustody")
    }
}

#[derive(thiserror::Error)]
#[error("original issuer terminal destination rejected publication")]
pub(crate) struct UndeliveredIssuerTerminal {
    #[source]
    pub(crate) source: Option<aura_core::AuraError>,
    pub(crate) retained: SynchronousCustodyCell<
        Option<
            crate::core::AgentResult<crate::handlers::invitation::IssuerEnrollmentTerminalOutcome>,
        >,
    >,
}

impl std::fmt::Debug for UndeliveredIssuerTerminal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("UndeliveredIssuerTerminal")
    }
}
