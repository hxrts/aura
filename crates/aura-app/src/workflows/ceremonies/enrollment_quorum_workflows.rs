//! Explicit local prepare/consent/resume for an owned enrollment quorum.
//! Preparation and consent admission do not publish enrollment success.

use super::{
    DeviceEnrollmentCeremonyStart, UserApprovedEnrollmentSigningIntent,
    UserTransferredEnrollmentSetup,
};
use crate::ui_contract::{
    OperationId, OperationInstanceId, SemanticOperationKind, SemanticOperationPhase,
    WorkflowTerminalOutcome,
};
use crate::workflows::runtime::require_runtime;
use crate::workflows::semantic_facts::{
    issue_device_enrollment_started_proof, SemanticWorkflowOwner,
};
use crate::AppCore;
use async_lock::RwLock;
use aura_core::AuraError;
use std::sync::Arc;

/// Retain one prepared native issuer allocation for explicit user review.
/// Native runtime owns the original bounded clock; this call never retries or
/// replaces it with a new phase timeout.
pub async fn prepare_device_enrollment_quorum(
    app_core: &Arc<RwLock<AppCore>>,
    nickname_suggestion: String,
    setup: UserTransferredEnrollmentSetup,
) -> Result<crate::runtime_bridge::PreparedDeviceEnrollmentSigning, AuraError> {
    let runtime = require_runtime(app_core).await?;
    crate::workflows::runtime::prepare_original_enrollment_issuer(
        &runtime,
        nickname_suggestion,
        setup,
    )
    .await
    .map_err(|source| AuraError::Internal {
        message: "prepare original enrollment signing quorum".into(),
        source: Some(Arc::new(source)),
    })
}

/// Admit this original device's explicit local consent to its native bounded
/// participant owner. Receiving an intent or round packet cannot call this API.
pub async fn approve_device_enrollment_quorum(
    app_core: &Arc<RwLock<AppCore>>,
    approval: UserApprovedEnrollmentSigningIntent,
) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;
    crate::workflows::runtime::approve_original_enrollment_participant(&runtime, approval).await
}

/// Resume the same original prepared issuer. Genuine runtime completion is the
/// only source of code-issuance success; durable enrollment completion remains
/// a separate operation with its own proof.
pub async fn resume_device_enrollment_quorum_with_terminal_status(
    app_core: &Arc<RwLock<AppCore>>,
    approval: UserApprovedEnrollmentSigningIntent,
    instance_id: Option<OperationInstanceId>,
) -> WorkflowTerminalOutcome<DeviceEnrollmentCeremonyStart> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::device_enrollment(),
        instance_id,
        SemanticOperationKind::StartDeviceEnrollment,
    );
    let result = resume_device_enrollment_quorum_owned(app_core, approval, &owner, None).await;
    WorkflowTerminalOutcome {
        result,
        terminal: owner.terminal_status().await,
    }
}

#[aura_macros::semantic_owner(
    owner = "resume_device_enrollment_quorum_owned",
    wrapper = "resume_device_enrollment_quorum_with_terminal_status",
    terminal = "publish_success_with",
    postcondition = "device_enrollment_started",
    proof = crate::workflows::semantic_facts::DeviceEnrollmentStartedProof,
    authoritative_inputs = "runtime,approval",
    depends_on = "runtime_device_enrollment_started",
    child_ops = "",
    category = "move_owned"
)]
async fn resume_device_enrollment_quorum_owned(
    app_core: &Arc<RwLock<AppCore>>,
    approval: UserApprovedEnrollmentSigningIntent,
    owner: &SemanticWorkflowOwner,
    _operation_context: Option<
        &mut aura_core::OperationContext<OperationId, OperationInstanceId, aura_core::TraceContext>,
    >,
) -> Result<DeviceEnrollmentCeremonyStart, AuraError> {
    owner
        .publish_phase(SemanticOperationPhase::WorkflowDispatched)
        .await?;
    let runtime = match require_runtime(app_core).await {
        Ok(runtime) => runtime,
        Err(primary) => {
            let failure = super::enrollment_runtime_failure(&primary);
            return Err(publish_failure_preserving_cause(owner, failure, primary).await);
        }
    };
    if let Err(primary) = approval.require_runtime_owner(runtime.as_ref()) {
        let failure = super::enrollment_runtime_failure(&primary);
        return Err(publish_failure_preserving_cause(owner, failure, primary).await);
    }
    let start = match crate::workflows::runtime::resume_original_enrollment_issuer(
        &runtime, approval,
    )
    .await
    {
        Ok(start) => start,
        Err(source) => {
            let failure = super::enrollment_issuance_failure(&source);
            let primary = AuraError::Internal {
                message: "resume original approved enrollment quorum".into(),
                source: Some(Arc::new(source)),
            };
            return Err(publish_failure_preserving_cause(owner, failure, primary).await);
        }
    };
    owner
        .publish_success_with(issue_device_enrollment_started_proof(
            start.ceremony_id.clone(),
        ))
        .await?;
    Ok(super::device_enrollment_handle_from_start(start))
}

#[derive(Debug, thiserror::Error)]
#[error("original quorum workflow failed: {primary}; required failure publication failed: {publication}")]
struct QuorumFailurePublication {
    #[source]
    primary: AuraError,
    publication: AuraError,
}

async fn publish_failure_preserving_cause(
    owner: &SemanticWorkflowOwner,
    failure: crate::ui_contract::SemanticOperationError,
    primary: AuraError,
) -> AuraError {
    match owner.publish_failure(failure).await {
        Ok(()) => primary,
        Err(publication) => AuraError::Internal {
            message: "original enrollment quorum failure and required publication failure".into(),
            source: Some(Arc::new(QuorumFailurePublication {
                primary,
                publication,
            })),
        },
    }
}
