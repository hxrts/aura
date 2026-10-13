//! Original issuer obligations retained when task admission or terminal delivery fails.

#[derive(thiserror::Error)]
#[error("original issuer registration publication failed")]
pub(crate) struct UnpublishedIssuerTaskCustody {
    #[source]
    pub(crate) source: aura_core::AuraError,
    #[allow(clippy::disallowed_types)] // Synchronous custody slot; no guard crosses an await.
    pub(crate) retained:
        parking_lot::Mutex<Option<crate::task_registry::OriginalRequiredTaskHandle>>,
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
    #[allow(clippy::disallowed_types)] // Synchronous custody slot; no guard crosses an await.
    pub(crate) retained: parking_lot::Mutex<Box<T>>,
}

impl<T: ?Sized + Send> std::fmt::Debug for RejectedIssuerTaskCustody<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RejectedIssuerTaskCustody")
    }
}

#[derive(thiserror::Error)]
#[error("original issuer terminal destination rejected a duplicate producer")]
pub(crate) struct UndeliveredIssuerTerminal {
    #[allow(clippy::disallowed_types)] // Synchronous custody slot; no guard crosses an await.
    pub(crate) retained: parking_lot::Mutex<
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
