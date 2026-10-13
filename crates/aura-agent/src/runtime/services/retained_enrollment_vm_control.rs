//! Runtime-owned mutable custody behind the sealed enrollment VM control.

use crate::runtime::AuraEffectSystem;
use aura_core::time::timeout::NegativeCleanupExecution;
use aura_core::{AuraError, TimeoutBudgetError};
#[allow(clippy::disallowed_types)]
// Synchronous before-Ready custody; no guard crosses an await.
use std::sync::Mutex;
use std::sync::{Arc, OnceLock};

use super::threshold_signing::OriginalIssuerTranscriptSigningCustody;

pub(crate) type OriginalSigner = Arc<tokio::sync::Mutex<OriginalIssuerTranscriptSigningCustody>>;

/// Mutable state is retained by the runtime; the handler exposes only checked
/// operations on its original, non-deserializable control capability.
pub(crate) struct RetainedEnrollmentVmState {
    // Synchronous before-Ready outcome deposit; this guard never crosses await.
    #[allow(clippy::disallowed_types)]
    conditional_acquisition: Mutex<OriginalConditionalAcquisitionCustody>,
    conditional_signing: OnceLock<OriginalSigner>,
    runtime_owner: Arc<AuraEffectSystem>,
}

#[derive(Default)]
struct OriginalConditionalAcquisitionCustody {
    actual_lock_acquired: bool,
    latest: Option<NegativeCleanupExecution<(), (), AuraError>>,
    first_operation_failure: Option<AuraError>,
    first_validation_failure: Option<TimeoutBudgetError>,
}

impl RetainedEnrollmentVmState {
    pub(crate) fn new(runtime_owner: Arc<AuraEffectSystem>) -> Self {
        Self {
            #[allow(clippy::disallowed_types)]
            conditional_acquisition: Mutex::new(OriginalConditionalAcquisitionCustody::default()),
            conditional_signing: OnceLock::new(),
            runtime_owner,
        }
    }

    pub(crate) fn retain_original_conditional_lock_ready(&self) -> Result<(), AuraError> {
        self.conditional_acquisition
            .lock()
            .map_err(|_| {
                AuraError::invalid("original conditional acquisition observation is poisoned")
            })?
            .actual_lock_acquired = true;
        Ok(())
    }

    pub(crate) fn retain_original_conditional_acquisition<P, T>(
        &self,
        observed: &NegativeCleanupExecution<&P, T, AuraError>,
    ) -> Result<(), AuraError> {
        let mut retained = self.conditional_acquisition.lock().map_err(|_| {
            AuraError::invalid("original conditional acquisition observation is poisoned")
        })?;
        if matches!(&observed.operation, Some(Ok(_))) && !retained.actual_lock_acquired {
            return Err(AuraError::permission_denied(
                "conditional acquisition has no original callback observation",
            ));
        }
        let operation = observed
            .operation
            .as_ref()
            .map(|actual| actual.as_ref().map(|_| ()).map_err(Clone::clone));
        if let Some(Err(source)) = &operation {
            retained
                .first_operation_failure
                .get_or_insert_with(|| source.clone());
        }
        if let Err(source) = &observed.validation {
            retained
                .first_validation_failure
                .get_or_insert_with(|| source.clone());
        }
        retained.latest = Some(NegativeCleanupExecution {
            primary: (),
            operation,
            validation: observed.validation.clone(),
        });
        // The cursor records outcomes only; it retains no mutex guard or signer grant.
        Ok(())
    }

    pub(crate) fn require_original_conditional_acquisition(&self) -> Result<(), AuraError> {
        let retained = self.conditional_acquisition.lock().map_err(|_| {
            AuraError::invalid("original conditional acquisition observation is poisoned")
        })?;
        if let Some(source) = &retained.first_operation_failure {
            return Err(source.clone());
        }
        if let Some(source) = &retained.first_validation_failure {
            return Err(AuraError::crypto_with_source(
                "original conditional acquisition validation failed",
                Arc::new(source.clone()),
            ));
        }
        Ok(())
    }

    pub(crate) fn has_original_conditional_signing(&self) -> bool {
        self.conditional_signing.get().is_some()
    }

    pub(crate) fn original_conditional_signing_state(&self) -> Result<OriginalSigner, AuraError> {
        self.conditional_signing.get().cloned().ok_or_else(|| {
            AuraError::permission_denied("original control has no retained conditional signer")
        })
    }

    pub(crate) fn bind_original_conditional_signing(
        &self,
        original: OriginalSigner,
    ) -> Result<(), AuraError> {
        if let Some(retained) = self.conditional_signing.get() {
            return if Arc::ptr_eq(retained, &original) {
                Ok(())
            } else {
                Err(AuraError::permission_denied(
                    "original conditional signing state already bound",
                ))
            };
        }
        self.conditional_signing
            .set(original)
            .map_err(|_| AuraError::permission_denied("original conditional signing binding raced"))
    }

    pub(crate) fn owns_runtime(&self, effects: &AuraEffectSystem) -> bool {
        std::ptr::eq(self.runtime_owner.as_ref(), effects)
    }

    pub(crate) fn runtime_owner(&self) -> &Arc<AuraEffectSystem> {
        &self.runtime_owner
    }
}
