//! Ceremony workflows (portable)
//!
//! Provides portable helpers for starting/polling/canceling Category C ceremonies.

#![allow(missing_docs)] // Ceremony workflow types are self-documenting

mod enrollment_quorum_workflows;
mod enrollment_signing_intent;
pub use enrollment_quorum_workflows::{
    approve_device_enrollment_quorum, prepare_device_enrollment_quorum,
    resume_device_enrollment_quorum_with_terminal_status,
};
pub use enrollment_signing_intent::{
    approve_user_selected_enrollment_signing_intent,
    select_user_transferred_enrollment_signing_intent, UserApprovedEnrollmentSigningIntent,
    UserTransferredEnrollmentSigningIntent,
};

use std::sync::Arc;

use async_lock::RwLock;
pub use aura_invitation::enrollment_manifest::EnrollmentManifestError;

use super::error::{ceremony_op, WorkflowError};
use crate::core::IntentError;
use crate::runtime_bridge::{
    CeremonyFailureReason, CeremonyTerminalOutcome, KeyRotationCeremonyStatus,
};
use crate::ui_contract::{
    OperationId, OperationInstanceId, SemanticFailureCode, SemanticFailureDomain,
    SemanticOperationError, SemanticOperationKind, SemanticOperationPhase,
};
use crate::workflows::runtime::{timeout_runtime_call, workflow_retry_policy};
use crate::workflows::semantic_facts::{
    issue_device_enrollment_completed_proof, issue_device_enrollment_started_proof,
    SemanticWorkflowOwner,
};
use crate::AppCore;
use aura_core::types::identifiers::{AuthorityId, CeremonyId};
use aura_core::types::FrostThreshold;
use aura_core::{AttemptBudget, AuraError, OperationContext, TraceContext};
use std::future::Future;
use std::time::Duration;

const DEVICE_ENROLLMENT_START_TIMEOUT: Duration = Duration::from_millis(30_000);
const DEVICE_ENROLLMENT_TERMINAL_QUERY_TIMEOUT: Duration = Duration::from_secs(10);
const DEVICE_REMOVAL_START_TIMEOUT: Duration = Duration::from_millis(20_000);

/// App-owned selection of a setup code explicitly transferred by the user.
/// This is scoped setup evidence, not durable authority/device trust.
///
/// Constructing a pin from possession evidence is forbidden:
/// ```compile_fail
/// use aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentSetup;
/// use aura_invitation::enrollment_setup::VerifiedEnrollmentSetupPossession;
/// fn forge(possession: VerifiedEnrollmentSetupPossession) -> UserTransferredEnrollmentSetup {
///     UserTransferredEnrollmentSetup { possession }
/// }
/// ```
/// Persisted bytes cannot restore a trusted pin without verification and selection:
/// ```compile_fail
/// use aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentSetup;
/// fn restore(bytes: &[u8]) -> UserTransferredEnrollmentSetup {
///     serde_json::from_slice(bytes).unwrap()
/// }
/// ```
/// A raw authority ID cannot replace the selected setup at issuance:
/// ```compile_fail
/// use aura_app::runtime_bridge::RuntimeBridge;
/// use aura_core::AuthorityId;
/// async fn downgrade(runtime: &dyn RuntimeBridge, authority: AuthorityId) {
///     runtime.initiate_device_enrollment_ceremony("Device".into(), authority).await;
/// }
/// ```
#[derive(Debug, Clone)]
pub struct UserTransferredEnrollmentSetup {
    possession: aura_invitation::enrollment_setup::VerifiedEnrollmentSetupPossession,
}

impl UserTransferredEnrollmentSetup {
    /// Forward the original public possession proof for explicit sibling selection.
    /// This code never grants signature permission or carries private material.
    pub fn transfer_code(
        &self,
    ) -> Result<String, aura_invitation::enrollment_setup::EnrollmentSetupError> {
        self.possession.transfer_code()
    }
    /// The exact device-owned signing statement selected by the user.
    pub fn statement(&self) -> &aura_invitation::enrollment_setup::DeviceEnrollmentSetupStatement {
        self.possession.statement()
    }

    /// Digest of the canonical setup statement, not the encoded proof envelope.
    pub fn digest(&self) -> [u8; 32] {
        self.possession.digest()
    }
}

/// Export the actual local device's setup code through a bounded runtime call.
/// Signing identity must be ready; callers transfer the returned code unchanged.
pub async fn export_device_enrollment_setup_code(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<String, aura_invitation::enrollment_setup::EnrollmentSetupExportError> {
    use aura_invitation::enrollment_setup::EnrollmentSetupExportError;
    let runtime = app_core
        .read()
        .await
        .runtime()
        .cloned()
        .ok_or(EnrollmentSetupExportError::Unavailable)?;
    let export_runtime = runtime.clone();
    timeout_runtime_call(
        &runtime,
        "export_device_enrollment_setup_code",
        "export_device_enrollment_setup_request",
        DEVICE_ENROLLMENT_TERMINAL_QUERY_TIMEOUT,
        move || async move {
            export_runtime
                .export_device_enrollment_setup_request()
                .await
        },
    )
    .await
    .map_err(EnrollmentSetupExportError::Boundary)?
}

/// Explicitly select a user-transferred setup code after bounded verification.
/// Call only from the user-transfer submission path, never discovery or inbox
/// observation. Issuance must separately recheck validity and replay status.
pub async fn pin_user_transferred_device_enrollment_setup(
    app_core: &Arc<RwLock<AppCore>>,
    code: String,
) -> Result<
    UserTransferredEnrollmentSetup,
    aura_invitation::enrollment_setup::EnrollmentSetupVerificationError,
> {
    use aura_invitation::enrollment_setup::EnrollmentSetupVerificationError;
    let runtime = app_core
        .read()
        .await
        .runtime()
        .cloned()
        .ok_or(EnrollmentSetupVerificationError::Unavailable)?;
    let verification_runtime = runtime.clone();
    let possession = timeout_runtime_call(
        &runtime,
        "pin_user_transferred_device_enrollment_setup",
        "verify_device_enrollment_setup_possession",
        DEVICE_ENROLLMENT_TERMINAL_QUERY_TIMEOUT,
        move || async move {
            verification_runtime
                .verify_device_enrollment_setup_possession(code)
                .await
        },
    )
    .await
    .map_err(EnrollmentSetupVerificationError::Boundary)??;
    Ok(UserTransferredEnrollmentSetup { possession })
}

/// Selection minted only by the explicit two-input user transfer workflow.
/// Inbox observation and enrollment-code decoding cannot construct this pin.
#[derive(Debug, Clone)]
pub struct UserTransferredEnrollmentManifest {
    verified: aura_invitation::enrollment_manifest::VerifiedEnrollmentManifestSignature,
    signed_code: String,
}
impl UserTransferredEnrollmentManifest {
    pub fn manifest(&self) -> &aura_invitation::enrollment_manifest::EnrollmentTrustManifest {
        self.verified.manifest()
    }
    pub fn digest(&self) -> [u8; 32] {
        self.verified.digest()
    }
    pub fn signed_code(&self) -> &str {
        &self.signed_code
    }
    pub fn verify_baseline(
        &self,
        baseline: &[Vec<u8>],
    ) -> Result<
        aura_invitation::enrollment_manifest::VerifiedEnrollmentBaseline,
        aura_invitation::enrollment_manifest::EnrollmentManifestError,
    > {
        self.verified.clone().verify_baseline(baseline)
    }
}

/// Transfer the signed manifest and initiator verifier obtained independently
/// from the account owner's confirmation screen. Never derive the second
/// input from the manifest or imported invitation's embedded sender proof.
pub async fn pin_user_transferred_enrollment_manifest(
    app_core: &Arc<RwLock<AppCore>>,
    manifest_code: String,
    initiator_verifier_code: String,
) -> Result<
    UserTransferredEnrollmentManifest,
    aura_invitation::enrollment_manifest::EnrollmentManifestError,
> {
    use aura_invitation::enrollment_manifest::EnrollmentManifestError;
    if initiator_verifier_code.trim().is_empty() {
        return Err(EnrollmentManifestError::MissingPin);
    }
    let runtime = app_core
        .read()
        .await
        .runtime()
        .cloned()
        .ok_or(EnrollmentManifestError::Unavailable)?;
    let verification_runtime = runtime.clone();
    let signed_code = manifest_code.clone();
    let verified = timeout_runtime_call(
        &runtime,
        "pin_user_transferred_enrollment_manifest",
        "verify_enrollment_manifest_transfer",
        DEVICE_ENROLLMENT_TERMINAL_QUERY_TIMEOUT,
        move || async move {
            verification_runtime
                .verify_enrollment_manifest_transfer(manifest_code, initiator_verifier_code)
                .await
        },
    )
    .await
    .map_err(EnrollmentManifestError::Boundary)??;
    Ok(UserTransferredEnrollmentManifest {
        verified,
        signed_code,
    })
}

fn ceremony_start_timeout(kind: crate::runtime_bridge::CeremonyKind) -> Duration {
    match kind {
        crate::runtime_bridge::CeremonyKind::DeviceEnrollment => DEVICE_ENROLLMENT_START_TIMEOUT,
        crate::runtime_bridge::CeremonyKind::DeviceRemoval => DEVICE_REMOVAL_START_TIMEOUT,
        crate::runtime_bridge::CeremonyKind::GuardianRotation
        | crate::runtime_bridge::CeremonyKind::DeviceRotation => Duration::from_secs(30),
        crate::runtime_bridge::CeremonyKind::Recovery
        | crate::runtime_bridge::CeremonyKind::OtaActivation => Duration::from_secs(45),
        crate::runtime_bridge::CeremonyKind::Invitation
        | crate::runtime_bridge::CeremonyKind::RendezvousSecureChannel => Duration::from_secs(15),
    }
}

fn ceremony_monitor_timeout(kind: crate::runtime_bridge::CeremonyKind) -> Duration {
    match kind {
        // Approvals by guardians or other devices, and enrollment imports, wait on
        // a person; this matches the runtime's 10 minute window for each.
        crate::runtime_bridge::CeremonyKind::GuardianRotation
        | crate::runtime_bridge::CeremonyKind::DeviceRotation
        | crate::runtime_bridge::CeremonyKind::Recovery
        | crate::runtime_bridge::CeremonyKind::DeviceEnrollment => Duration::from_secs(600),
        crate::runtime_bridge::CeremonyKind::DeviceRemoval => Duration::from_secs(45),
        crate::runtime_bridge::CeremonyKind::OtaActivation => Duration::from_secs(90),
        crate::runtime_bridge::CeremonyKind::Invitation
        | crate::runtime_bridge::CeremonyKind::RendezvousSecureChannel => Duration::from_secs(20),
    }
}

fn ceremony_monitor_attempts(kind: crate::runtime_bridge::CeremonyKind, interval: Duration) -> u32 {
    let interval_ms = interval.as_millis().max(1);
    let window_ms = ceremony_monitor_timeout(kind).as_millis();
    let attempts = window_ms.div_ceil(interval_ms).saturating_add(2);
    u32::try_from(attempts).unwrap_or(u32::MAX)
}

fn ceremony_start_retry_policy(
    kind: crate::runtime_bridge::CeremonyKind,
) -> Result<aura_core::RetryBudgetPolicy, AuraError> {
    let (attempts, initial_delay, max_delay) = match kind {
        crate::runtime_bridge::CeremonyKind::DeviceEnrollment => {
            (4, Duration::from_millis(250), Duration::from_secs(1))
        }
        crate::runtime_bridge::CeremonyKind::DeviceRemoval => {
            (3, Duration::from_millis(150), Duration::from_millis(750))
        }
        _ => (3, Duration::from_millis(200), Duration::from_millis(750)),
    };
    workflow_retry_policy(attempts, initial_delay, max_delay).map_err(AuraError::from)
}

fn retryable_ceremony_intent_error(error: &IntentError) -> bool {
    matches!(
        error,
        IntentError::NetworkError { .. } | IntentError::ServiceError { .. }
    )
}

async fn start_ceremony_with_retry<T, F, Fut>(
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    kind: crate::runtime_bridge::CeremonyKind,
    operation: &'static str,
    stage: &'static str,
    mut call: F,
) -> Result<Result<T, IntentError>, AuraError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, IntentError>>,
{
    let policy = ceremony_start_retry_policy(kind)?;
    let mut attempts = AttemptBudget::new(policy.max_attempts());

    loop {
        let attempt = attempts.record_attempt().map_err(AuraError::from)?;
        match timeout_runtime_call(
            runtime,
            operation,
            stage,
            ceremony_start_timeout(kind),
            &mut call,
        )
        .await
        {
            Ok(Ok(value)) => return Ok(Ok(value)),
            Ok(Err(error)) if retryable_ceremony_intent_error(&error) && attempts.can_attempt() => {
                let delay_ms = u64::try_from(policy.delay_for_attempt(attempt).as_millis())
                    .unwrap_or(u64::MAX);
                runtime
                    .sleep_ms(delay_ms)
                    .await
                    .map_err(|error| super::error::runtime_call("ceremony retry delay", error))?;
            }
            Ok(Err(error)) => return Ok(Err(error)),
            Err(error) if error.is_retryable() && attempts.can_attempt() => {
                let delay_ms = u64::try_from(policy.delay_for_attempt(attempt).as_millis())
                    .unwrap_or(u64::MAX);
                runtime
                    .sleep_ms(delay_ms)
                    .await
                    .map_err(|error| super::error::runtime_call("ceremony retry delay", error))?;
            }
            Err(error) => return Err(error),
        }
    }
}

async fn start_device_enrollment_from_runtime(
    runtime: Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    nickname_suggestion: String,
    setup: UserTransferredEnrollmentSetup,
) -> Result<
    Result<
        crate::runtime_bridge::DeviceEnrollmentStart,
        aura_invitation::enrollment_setup::EnrollmentIssuanceError,
    >,
    AuraError,
> {
    let policy =
        ceremony_start_retry_policy(crate::runtime_bridge::CeremonyKind::DeviceEnrollment)?;
    let mut attempts = AttemptBudget::new(policy.max_attempts());
    loop {
        let attempt = attempts.record_attempt().map_err(AuraError::from)?;
        let next_runtime = runtime.clone();
        let next_name = nickname_suggestion.clone();
        let next_setup = setup.clone();
        let outcome = timeout_runtime_call(
            &runtime,
            "start_device_enrollment_ceremony",
            "initiate_device_enrollment_ceremony",
            DEVICE_ENROLLMENT_START_TIMEOUT,
            move || async move {
                next_runtime
                    .initiate_device_enrollment_ceremony(next_name, next_setup)
                    .await
            },
        )
        .await?;
        match outcome {
            Err(error) if error.is_retryable() && attempts.can_attempt() => {
                let delay_ms = u64::try_from(policy.delay_for_attempt(attempt).as_millis())
                    .unwrap_or(u64::MAX);
                runtime
                    .sleep_ms(delay_ms)
                    .await
                    .map_err(|error| super::error::runtime_call("ceremony retry delay", error))?;
            }
            other => return Ok(other),
        }
    }
}

async fn start_device_removal_from_runtime(
    runtime: Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    device_id: String,
) -> Result<Result<CeremonyId, IntentError>, AuraError> {
    let retry_runtime = runtime.clone();
    start_ceremony_with_retry(
        &runtime,
        crate::runtime_bridge::CeremonyKind::DeviceRemoval,
        "start_device_removal_ceremony",
        "initiate_device_removal_ceremony",
        move || {
            let runtime = retry_runtime.clone();
            let device_id = device_id.clone();
            async move { runtime.initiate_device_removal_ceremony(device_id).await }
        },
    )
    .await
}

async fn start_guardian_ceremony_from_runtime(
    runtime: Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    threshold_k: FrostThreshold,
    total_n: u16,
    guardian_ids: Vec<AuthorityId>,
) -> Result<Result<CeremonyId, IntentError>, AuraError> {
    let retry_runtime = runtime.clone();
    start_ceremony_with_retry(
        &runtime,
        crate::runtime_bridge::CeremonyKind::GuardianRotation,
        "start_guardian_ceremony",
        "initiate_guardian_ceremony",
        move || {
            let runtime = retry_runtime.clone();
            let guardian_ids = guardian_ids.clone();
            async move {
                runtime
                    .initiate_guardian_ceremony(threshold_k, total_n, &guardian_ids)
                    .await
            }
        },
    )
    .await
}

async fn start_device_threshold_ceremony_from_runtime(
    runtime: Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    threshold_k: FrostThreshold,
    total_n: u16,
    device_ids: Vec<String>,
) -> Result<Result<CeremonyId, IntentError>, AuraError> {
    let retry_runtime = runtime.clone();
    start_ceremony_with_retry(
        &runtime,
        crate::runtime_bridge::CeremonyKind::DeviceRotation,
        "start_device_threshold_ceremony",
        "initiate_device_threshold_ceremony",
        move || {
            let runtime = retry_runtime.clone();
            let device_ids = device_ids.clone();
            async move {
                runtime
                    .initiate_device_threshold_ceremony(threshold_k, total_n, &device_ids)
                    .await
            }
        },
    )
    .await
}

/// Move-owned ceremony handle.
///
/// This is the canonical owner token for parity-critical key rotation and
/// membership-change ceremonies. Cancellation consumes the handle so callers
/// cannot issue multiple cancels on the same owned ceremony instance.
#[aura_macros::strong_reference(domain = "ceremony")]
#[derive(Debug)]
pub struct CeremonyHandle {
    ceremony_id: CeremonyId,
    kind: crate::runtime_bridge::CeremonyKind,
}

#[derive(Debug, Clone)]
pub struct CeremonyStatusHandle {
    ceremony_id: CeremonyId,
    kind: crate::runtime_bridge::CeremonyKind,
}

impl CeremonyHandle {
    fn new(ceremony_id: CeremonyId, kind: crate::runtime_bridge::CeremonyKind) -> Self {
        Self { ceremony_id, kind }
    }

    pub fn ceremony_id(&self) -> &CeremonyId {
        &self.ceremony_id
    }

    pub fn kind(&self) -> crate::runtime_bridge::CeremonyKind {
        self.kind
    }

    pub fn status_handle(&self) -> CeremonyStatusHandle {
        CeremonyStatusHandle::new(self.ceremony_id.clone(), self.kind)
    }
}

impl CeremonyStatusHandle {
    fn new(ceremony_id: CeremonyId, kind: crate::runtime_bridge::CeremonyKind) -> Self {
        Self { ceremony_id, kind }
    }

    pub fn ceremony_id(&self) -> &CeremonyId {
        &self.ceremony_id
    }

    pub fn kind(&self) -> crate::runtime_bridge::CeremonyKind {
        self.kind
    }
}

#[derive(Debug)]
pub struct DeviceEnrollmentCeremonyStart {
    pub ceremony_id: CeremonyId,
    pub enrollment_code: String,
    pub manifest_transfer: Option<crate::ui_contract::EnrollmentManifestTransferInput>,
    pub pending_epoch: aura_core::types::Epoch,
    pub device_id: aura_core::types::identifiers::DeviceId,
    pub handle: CeremonyHandle,
    pub status_handle: CeremonyStatusHandle,
}

/// Start a guardian key-rotation ceremony.
pub async fn start_guardian_ceremony(
    app_core: &Arc<RwLock<AppCore>>,
    threshold_k: FrostThreshold,
    total_n: u16,
    guardian_ids: Vec<AuthorityId>,
) -> Result<CeremonyHandle, AuraError> {
    let runtime = {
        let core = app_core.read().await;
        core.runtime()
            .cloned()
            .ok_or_else(|| AuraError::from(WorkflowError::RuntimeUnavailable))?
    };
    start_guardian_ceremony_from_runtime(runtime, threshold_k, total_n, guardian_ids)
        .await?
        .map(|ceremony_id| {
            CeremonyHandle::new(
                ceremony_id,
                crate::runtime_bridge::CeremonyKind::GuardianRotation,
            )
        })
        .map_err(|e| ceremony_op("start guardian ceremony", e).into())
}

/// Start a device threshold (multifactor) ceremony.
pub async fn start_device_threshold_ceremony(
    app_core: &Arc<RwLock<AppCore>>,
    threshold_k: FrostThreshold,
    total_n: u16,
    device_ids: Vec<String>,
) -> Result<CeremonyHandle, AuraError> {
    let runtime = {
        let core = app_core.read().await;
        core.runtime()
            .cloned()
            .ok_or_else(|| AuraError::from(WorkflowError::RuntimeUnavailable))?
    };
    start_device_threshold_ceremony_from_runtime(runtime, threshold_k, total_n, device_ids)
        .await?
        .map(|ceremony_id| {
            CeremonyHandle::new(
                ceremony_id,
                crate::runtime_bridge::CeremonyKind::DeviceRotation,
            )
        })
        .map_err(|e| ceremony_op("start device threshold ceremony", e).into())
}

/// Start a device enrollment ("add device") ceremony.
///
/// For the two-step exchange flow:
/// 1. The new device creates its own authority first
/// 2. The new device exports its signed setup code and the user transfers it
/// 3. The app verifies possession and pins the explicit transfer
/// 4. An addressed enrollment invitation is created
///
/// # Arguments
/// * `nickname_suggestion` - Suggested name for the device
/// * `setup` - App-owned pin for the exact transferred device signing statement
pub async fn start_device_enrollment_ceremony(
    app_core: &Arc<RwLock<AppCore>>,
    nickname_suggestion: String,
    setup: UserTransferredEnrollmentSetup,
) -> Result<DeviceEnrollmentCeremonyStart, AuraError> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::device_enrollment(),
        None,
        SemanticOperationKind::StartDeviceEnrollment,
    );
    start_device_enrollment_ceremony_owned(app_core, nickname_suggestion, setup, &owner, None).await
}

#[aura_macros::semantic_owner(
    owner = "start_device_enrollment_ceremony_owned",
    wrapper = "start_device_enrollment_ceremony",
    terminal = "publish_success_with",
    postcondition = "device_enrollment_started",
    proof = crate::workflows::semantic_facts::DeviceEnrollmentStartedProof,
    authoritative_inputs = "runtime,authoritative_source",
    depends_on = "runtime_device_enrollment_started",
    child_ops = "",
    category = "move_owned"
)]
async fn start_device_enrollment_ceremony_owned(
    app_core: &Arc<RwLock<AppCore>>,
    nickname_suggestion: String,
    setup: UserTransferredEnrollmentSetup,
    owner: &SemanticWorkflowOwner,
    _operation_context: Option<
        &mut OperationContext<OperationId, OperationInstanceId, TraceContext>,
    >,
) -> Result<DeviceEnrollmentCeremonyStart, AuraError> {
    owner
        .publish_phase(SemanticOperationPhase::WorkflowDispatched)
        .await?;
    let start =
        prepare_device_enrollment_start(app_core, nickname_suggestion, setup, owner).await?;
    owner
        .publish_success_with(issue_device_enrollment_started_proof(
            start.ceremony_id.clone(),
        ))
        .await?;
    Ok(device_enrollment_handle_from_start(start))
}

fn enrollment_boundary_code(error: &(dyn std::error::Error + 'static)) -> SemanticFailureCode {
    let mut current = Some(error);
    while let Some(cause) = current {
        if let Some(budget) = cause.downcast_ref::<aura_core::TimeoutBudgetError>() {
            return crate::workflows::runtime_error_classification::timeout_budget_failure_code(
                budget,
            );
        }
        if let Some(workflow) = cause.downcast_ref::<WorkflowError>() {
            if matches!(workflow, WorkflowError::TimedOut { .. }) {
                return SemanticFailureCode::OperationTimedOut;
            }
            if matches!(workflow, WorkflowError::RuntimeUnavailable) {
                return SemanticFailureCode::Unavailable;
            }
        }
        current = cause.source();
    }
    SemanticFailureCode::CeremonyRuntimeFailed
}

fn enrollment_setup_failure(
    error: &aura_invitation::enrollment_setup::EnrollmentSetupVerificationError,
) -> SemanticOperationError {
    use aura_invitation::enrollment_setup::{
        EnrollmentSetupError, EnrollmentSetupVerificationError,
    };
    let code = match error {
        EnrollmentSetupVerificationError::Unavailable => SemanticFailureCode::Unavailable,
        EnrollmentSetupVerificationError::Time(_) => SemanticFailureCode::CeremonyRuntimeFailed,
        EnrollmentSetupVerificationError::Boundary(error) => enrollment_boundary_code(error),
        EnrollmentSetupVerificationError::Setup(error) => match error {
            EnrollmentSetupError::InvalidFormat
            | EnrollmentSetupError::SizeLimit
            | EnrollmentSetupError::UnsupportedVersion(_)
            | EnrollmentSetupError::InvalidValidity
            | EnrollmentSetupError::OutsideValidity
            | EnrollmentSetupError::InvalidSigningPolicy
            | EnrollmentSetupError::InputEncoding(_)
            | EnrollmentSetupError::Codec(_)
            | EnrollmentSetupError::Transcript(_) => SemanticFailureCode::InvalidArgument,
            EnrollmentSetupError::ProofBinding | EnrollmentSetupError::InvalidSignature => {
                SemanticFailureCode::PermissionDenied
            }
            EnrollmentSetupError::Crypto(_) => SemanticFailureCode::CeremonyRuntimeFailed,
        },
    };
    SemanticOperationError::new(SemanticFailureDomain::Ceremony, code)
        .with_detail(error.to_string())
}

fn enrollment_issuance_failure(
    error: &aura_invitation::enrollment_setup::EnrollmentIssuanceError,
) -> SemanticOperationError {
    use aura_invitation::enrollment_setup::{EnrollmentIssuanceError, EnrollmentIssuanceStage};
    let code = match error {
        EnrollmentIssuanceError::Unavailable => SemanticFailureCode::Unavailable,
        EnrollmentIssuanceError::OutsideValidity
        | EnrollmentIssuanceError::CurrentIdentity
        | EnrollmentIssuanceError::InvalidPolicy => SemanticFailureCode::InvalidArgument,
        EnrollmentIssuanceError::AlreadyEnrolled => SemanticFailureCode::InvalidState,
        EnrollmentIssuanceError::MissingPackage(_)
        | EnrollmentIssuanceError::EmptyPendingPackage
        | EnrollmentIssuanceError::EmptyPendingConfig
        | EnrollmentIssuanceError::Time(_) => SemanticFailureCode::CeremonyRuntimeFailed,
        EnrollmentIssuanceError::Failure { stage, source } => {
            let stage_code = match stage {
                EnrollmentIssuanceStage::PrestateValidation => SemanticFailureCode::InvalidState,
                EnrollmentIssuanceStage::InvitationService => SemanticFailureCode::Unavailable,
                EnrollmentIssuanceStage::TreeRead
                | EnrollmentIssuanceStage::Rotation
                | EnrollmentIssuanceStage::PendingPackageRead
                | EnrollmentIssuanceStage::PendingConfigRead
                | EnrollmentIssuanceStage::PrestateEncoding
                | EnrollmentIssuanceStage::OperationEncoding
                | EnrollmentIssuanceStage::Supersession
                | EnrollmentIssuanceStage::CeremonyRegistration
                | EnrollmentIssuanceStage::SetupVerifierRetention
                | EnrollmentIssuanceStage::BaselineExport
                | EnrollmentIssuanceStage::BaselineEncoding
                | EnrollmentIssuanceStage::InvitationCreation
                | EnrollmentIssuanceStage::InvitationExport => {
                    SemanticFailureCode::CeremonyRuntimeFailed
                }
            };
            let boundary_code = enrollment_boundary_code(source);
            if boundary_code == SemanticFailureCode::CeremonyRuntimeFailed {
                stage_code
            } else {
                boundary_code
            }
        }
    };
    SemanticOperationError::new(SemanticFailureDomain::Ceremony, code)
        .with_detail(error.to_string())
}

fn enrollment_runtime_failure(error: &AuraError) -> SemanticOperationError {
    SemanticOperationError::new(
        SemanticFailureDomain::Ceremony,
        enrollment_boundary_code(error),
    )
    .with_detail(error.to_string())
}

async fn prepare_device_enrollment_start(
    app_core: &Arc<RwLock<AppCore>>,
    nickname_suggestion: String,
    setup: UserTransferredEnrollmentSetup,
    owner: &SemanticWorkflowOwner,
) -> Result<crate::runtime_bridge::DeviceEnrollmentStart, AuraError> {
    let runtime = app_core.read().await.runtime().cloned();
    let Some(runtime) = runtime else {
        let cause = WorkflowError::RuntimeUnavailable;
        let detail = cause.to_string();
        owner
            .publish_failure(
                SemanticOperationError::new(
                    SemanticFailureDomain::Ceremony,
                    SemanticFailureCode::Unavailable,
                )
                .with_detail(detail.clone()),
            )
            .await?;
        return Err(AuraError::Internal {
            message: detail,
            source: Some(Arc::new(cause)),
        });
    };
    let start =
        match start_device_enrollment_from_runtime(runtime, nickname_suggestion, setup).await {
            Ok(Ok(start)) => start,
            Ok(Err(error)) => {
                let detail = error.to_string();
                owner
                    .publish_failure(enrollment_issuance_failure(&error))
                    .await?;
                return Err(AuraError::Internal {
                    message: detail,
                    source: Some(Arc::new(error)),
                });
            }
            Err(error) => {
                let detail = error.to_string();
                owner
                    .publish_failure(enrollment_runtime_failure(&error))
                    .await?;
                return Err(AuraError::Internal {
                    message: detail,
                    source: Some(Arc::new(error)),
                });
            }
        };
    Ok(start)
}

fn device_enrollment_handle_from_start(
    start: crate::runtime_bridge::DeviceEnrollmentStart,
) -> DeviceEnrollmentCeremonyStart {
    let handle = CeremonyHandle::new(
        start.ceremony_id.clone(),
        crate::runtime_bridge::CeremonyKind::DeviceEnrollment,
    );
    let status_handle = handle.status_handle();
    DeviceEnrollmentCeremonyStart {
        ceremony_id: start.ceremony_id,
        enrollment_code: start.enrollment_code,
        manifest_transfer: start.manifest_transfer.map(|transfer| {
            crate::ui_contract::EnrollmentManifestTransferInput {
                manifest_code: transfer.manifest_code,
                initiator_verifier_code: transfer.initiator_verifier_code,
            }
        }),
        pending_epoch: start.pending_epoch,
        device_id: start.device_id,
        handle,
        status_handle,
    }
}

pub async fn start_device_enrollment_ceremony_with_terminal_status(
    app_core: &Arc<RwLock<AppCore>>,
    nickname_suggestion: String,
    setup: UserTransferredEnrollmentSetup,
    instance_id: Option<OperationInstanceId>,
) -> crate::ui_contract::WorkflowTerminalOutcome<DeviceEnrollmentCeremonyStart> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::device_enrollment(),
        instance_id,
        SemanticOperationKind::StartDeviceEnrollment,
    );
    let result =
        start_device_enrollment_ceremony_owned(app_core, nickname_suggestion, setup, &owner, None)
            .await;
    crate::ui_contract::WorkflowTerminalOutcome {
        result,
        terminal: owner.terminal_status().await,
    }
}

/// Start enrollment from the setup code explicitly transferred by the user.
pub async fn start_device_enrollment_ceremony_from_setup_code(
    app_core: &Arc<RwLock<AppCore>>,
    nickname_suggestion: String,
    setup_code: String,
) -> Result<DeviceEnrollmentCeremonyStart, AuraError> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::device_enrollment(),
        None,
        SemanticOperationKind::StartDeviceEnrollment,
    );
    start_device_enrollment_from_setup_code_owned(
        app_core,
        nickname_suggestion,
        setup_code,
        &owner,
        None,
    )
    .await
}

/// Preserve the frontend handoff instance across setup verification and issuance.
pub async fn start_device_enrollment_ceremony_from_setup_code_with_terminal_status(
    app_core: &Arc<RwLock<AppCore>>,
    nickname_suggestion: String,
    setup_code: String,
    instance_id: Option<OperationInstanceId>,
) -> crate::ui_contract::WorkflowTerminalOutcome<DeviceEnrollmentCeremonyStart> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::device_enrollment(),
        instance_id,
        SemanticOperationKind::StartDeviceEnrollment,
    );
    let result = start_device_enrollment_from_setup_code_owned(
        app_core,
        nickname_suggestion,
        setup_code,
        &owner,
        None,
    )
    .await;
    crate::ui_contract::WorkflowTerminalOutcome {
        result,
        terminal: owner.terminal_status().await,
    }
}

#[aura_macros::semantic_owner(
    owner = "start_device_enrollment_from_setup_code_owned",
    wrapper = "start_device_enrollment_ceremony_from_setup_code_with_terminal_status",
    terminal = "publish_success_with",
    postcondition = "device_enrollment_started",
    proof = crate::workflows::semantic_facts::DeviceEnrollmentStartedProof,
    authoritative_inputs = "runtime,authoritative_source",
    depends_on = "runtime_device_enrollment_started",
    child_ops = "",
    category = "move_owned"
)]
async fn start_device_enrollment_from_setup_code_owned(
    app_core: &Arc<RwLock<AppCore>>,
    nickname_suggestion: String,
    setup_code: String,
    owner: &SemanticWorkflowOwner,
    _operation_context: Option<
        &mut OperationContext<OperationId, OperationInstanceId, TraceContext>,
    >,
) -> Result<DeviceEnrollmentCeremonyStart, AuraError> {
    owner
        .publish_phase(SemanticOperationPhase::WorkflowDispatched)
        .await?;
    let setup = match pin_user_transferred_device_enrollment_setup(app_core, setup_code).await {
        Ok(setup) => setup,
        Err(error) => {
            let detail = error.to_string();
            owner
                .publish_failure(enrollment_setup_failure(&error))
                .await?;
            return Err(AuraError::Invalid {
                message: detail,
                source: Some(Arc::new(error)),
            });
        }
    };
    let start =
        prepare_device_enrollment_start(app_core, nickname_suggestion, setup, owner).await?;
    owner
        .publish_success_with(issue_device_enrollment_started_proof(
            start.ceremony_id.clone(),
        ))
        .await?;
    Ok(device_enrollment_handle_from_start(start))
}

fn device_enrollment_completion_failure(reason: CeremonyFailureReason) -> SemanticOperationError {
    let code = match reason {
        CeremonyFailureReason::Rejected => SemanticFailureCode::CeremonyRejected,
        CeremonyFailureReason::Cancelled => {
            unreachable!("cancellation is a distinct terminal phase")
        }
        CeremonyFailureReason::TimedOut => SemanticFailureCode::OperationTimedOut,
        CeremonyFailureReason::ChoreographyFailed => {
            SemanticFailureCode::CeremonyChoreographyFailed
        }
        CeremonyFailureReason::RuntimeFailed => SemanticFailureCode::CeremonyRuntimeFailed,
        CeremonyFailureReason::Superseded => SemanticFailureCode::CeremonySuperseded,
    };
    SemanticOperationError::new(SemanticFailureDomain::Ceremony, code)
}

/// Publish a separately tracked enrollment completion from the runtime owner's
/// terminal outcome. A pending runtime result never becomes UI success.
pub async fn observe_device_enrollment_completion_with_terminal_status(
    app_core: &Arc<RwLock<AppCore>>,
    ceremony_id: &CeremonyId,
    instance_id: OperationInstanceId,
) -> crate::ui_contract::WorkflowTerminalOutcome<Option<CeremonyTerminalOutcome>> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::device_enrollment_completion_for(ceremony_id),
        Some(instance_id),
        SemanticOperationKind::CompleteDeviceEnrollment,
    );
    let result =
        observe_device_enrollment_completion_owned(app_core, ceremony_id, &owner, None).await;
    crate::ui_contract::WorkflowTerminalOutcome {
        result,
        terminal: owner.terminal_status().await,
    }
}

#[aura_macros::semantic_owner(
    owner = "observe_device_enrollment_completion_owned",
    wrapper = "observe_device_enrollment_completion_with_terminal_status",
    terminal = "publish_success_with",
    postcondition = "device_enrollment_completed",
    proof = crate::workflows::semantic_facts::DeviceEnrollmentCompletedProof,
    authoritative_inputs = "runtime,ceremony_terminal_outcome",
    depends_on = "runtime_ceremony_terminal",
    child_ops = "",
    category = "move_owned"
)]
async fn observe_device_enrollment_completion_owned(
    app_core: &Arc<RwLock<AppCore>>,
    ceremony_id: &CeremonyId,
    owner: &SemanticWorkflowOwner,
    _operation_context: Option<
        &mut OperationContext<OperationId, OperationInstanceId, TraceContext>,
    >,
) -> Result<Option<CeremonyTerminalOutcome>, AuraError> {
    owner
        .publish_phase(SemanticOperationPhase::WorkflowDispatched)
        .await?;
    let runtime = {
        let core = app_core.read().await;
        core.runtime()
            .cloned()
            .ok_or_else(|| AuraError::from(WorkflowError::RuntimeUnavailable))?
    };
    let outcome = timeout_runtime_call(
        &runtime,
        "observe_device_enrollment_completion",
        "get_ceremony_terminal_outcome",
        DEVICE_ENROLLMENT_TERMINAL_QUERY_TIMEOUT,
        || runtime.get_ceremony_terminal_outcome(ceremony_id),
    )
    .await?
    .map_err(|error| ceremony_op("get device enrollment completion", error))?;
    match outcome {
        None => Ok(None),
        Some(CeremonyTerminalOutcome::Committed) => {
            owner
                .publish_success_with(issue_device_enrollment_completed_proof(ceremony_id.clone()))
                .await?;
            Ok(outcome)
        }
        Some(CeremonyTerminalOutcome::Failed(CeremonyFailureReason::Cancelled)) => {
            owner
                .publish_phase(SemanticOperationPhase::Cancelled)
                .await?;
            Ok(outcome)
        }
        Some(CeremonyTerminalOutcome::Failed(reason)) => {
            owner
                .publish_failure(device_enrollment_completion_failure(reason))
                .await?;
            Ok(outcome)
        }
    }
}

/// Reconcile every runtime-retained enrollment with the app-owned semantic
/// lifecycle. This is called by the runtime hook group on attach and retry.
pub(crate) async fn refresh_device_enrollment_completions(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<(), AuraError> {
    let runtime = {
        let core = app_core.read().await;
        core.runtime()
            .cloned()
            .ok_or_else(|| AuraError::from(WorkflowError::RuntimeUnavailable))?
    };
    let ceremonies = runtime
        .list_device_enrollment_ceremonies()
        .await
        .map_err(|error| ceremony_op("list device enrollment ceremonies", error))?;
    for ceremony_id in ceremonies {
        let instance_id =
            OperationInstanceId(format!("device-enrollment-completion-{ceremony_id}"));
        let already_terminal = {
            let core = app_core.read().await;
            core.authoritative_semantic_facts().iter().any(|fact| {
                matches!(fact,
                    crate::ui_contract::AuthoritativeSemanticFact::OperationStatus {
                        operation_id,
                        instance_id: Some(existing_instance),
                        status,
                        ..
                    } if operation_id == &OperationId::device_enrollment_completion_for(&ceremony_id)
                        && existing_instance == &instance_id
                        && matches!(status.phase,
                            SemanticOperationPhase::Succeeded
                                | SemanticOperationPhase::Failed
                                | SemanticOperationPhase::Cancelled))
            })
        };
        if already_terminal {
            continue;
        }
        observe_device_enrollment_completion_with_terminal_status(
            app_core,
            &ceremony_id,
            instance_id,
        )
        .await
        .result?;
    }
    Ok(())
}

/// Start a device removal ("remove device") ceremony.
pub async fn start_device_removal_ceremony(
    app_core: &Arc<RwLock<AppCore>>,
    device_id: String,
) -> Result<CeremonyHandle, AuraError> {
    let runtime = {
        let core = app_core.read().await;
        core.runtime()
            .cloned()
            .ok_or_else(|| AuraError::from(WorkflowError::RuntimeUnavailable))?
    };
    start_device_removal_from_runtime(runtime, device_id)
        .await?
        .map(|ceremony_id| {
            CeremonyHandle::new(
                ceremony_id,
                crate::runtime_bridge::CeremonyKind::DeviceRemoval,
            )
        })
        .map_err(|e| ceremony_op("start device removal", e).into())
}

/// Polling policy for ceremonies.
#[derive(Debug, Clone)]
pub struct CeremonyPollPolicy {
    /// Delay between polls.
    pub interval: Duration,
    /// Max number of poll attempts.
    pub max_attempts: u32,
    /// Whether to attempt rollback on failure (key rotation only).
    pub rollback_on_failure: bool,
    /// Whether to refresh settings after completion.
    pub refresh_settings_on_complete: bool,
}

impl CeremonyPollPolicy {
    pub fn with_interval(interval: Duration) -> Self {
        Self {
            interval,
            ..Default::default()
        }
    }

    pub fn for_kind(kind: crate::runtime_bridge::CeremonyKind, interval: Duration) -> Self {
        Self {
            interval,
            max_attempts: ceremony_monitor_attempts(kind, interval),
            rollback_on_failure: matches!(
                kind,
                crate::runtime_bridge::CeremonyKind::GuardianRotation
                    | crate::runtime_bridge::CeremonyKind::DeviceRotation
            ),
            refresh_settings_on_complete: true,
        }
    }
}

impl Default for CeremonyPollPolicy {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(1),
            max_attempts: 60,
            rollback_on_failure: true,
            refresh_settings_on_complete: true,
        }
    }
}

/// Lifecycle outcome for a ceremony monitor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CeremonyLifecycleState {
    Completed,
    Failed,
    /// The ceremony failed and the best-effort rollback also failed.
    /// The account may be in a partially-committed state that requires
    /// manual intervention or a fresh ceremony to resolve.
    FailedRollbackIncomplete,
    TimedOut,
}

/// Lifecycle result for a ceremony monitor.
#[derive(Debug, Clone)]
pub struct CeremonyLifecycle<T> {
    pub state: CeremonyLifecycleState,
    pub status: T,
    pub attempts: u32,
}

/// Common interface for ceremony status values.
pub trait CeremonyStatusLike {
    fn is_complete(&self) -> bool;
    fn has_failed(&self) -> bool;
}

impl CeremonyStatusLike for KeyRotationCeremonyStatus {
    fn is_complete(&self) -> bool {
        self.is_complete
    }

    fn has_failed(&self) -> bool {
        self.has_failed
    }
}

/// Get status of a key rotation ceremony (generic form).
pub async fn get_key_rotation_ceremony_status(
    app_core: &Arc<RwLock<AppCore>>,
    handle: &CeremonyStatusHandle,
) -> Result<KeyRotationCeremonyStatus, AuraError> {
    let core = app_core.read().await;
    core.get_key_rotation_ceremony_status(handle.ceremony_id())
        .await
        .map_err(|e| ceremony_op("get ceremony status", e).into())
}

/// Observe a ceremony the runtime tracks, given only its id (for example one a
/// user typed). The returned status handle takes its kind from the runtime's
/// own record, never from the caller; the status read alongside it saves a
/// second query.
// OWNERSHIP: observed
pub async fn observe_key_rotation_ceremony(
    app_core: &Arc<RwLock<AppCore>>,
    ceremony_id: CeremonyId,
) -> Result<(CeremonyStatusHandle, KeyRotationCeremonyStatus), AuraError> {
    let status = {
        let core = app_core.read().await;
        core.get_key_rotation_ceremony_status(&ceremony_id)
            .await
            .map_err(|e| AuraError::from(ceremony_op("get ceremony status", e)))?
    };
    Ok((CeremonyStatusHandle::new(ceremony_id, status.kind), status))
}

/// Cancel a key rotation ceremony (best effort).
///
/// # Ownership contract
///
/// Today this accepts a bare `CeremonyId`, which means multiple callers can
/// race cancel against poll or status queries.  The target ownership model
/// requires a `MoveOwned` ceremony handle returned by the start function that
/// is consumed on cancel — preventing concurrent cancel/poll races by
/// construction.  Until that migration is complete, callers must coordinate
/// externally to avoid issuing cancel and poll concurrently on the same
/// ceremony.
pub async fn cancel_key_rotation_ceremony(
    app_core: &Arc<RwLock<AppCore>>,
    handle: CeremonyHandle,
) -> Result<(), AuraError> {
    let core = app_core.read().await;
    core.cancel_key_rotation_ceremony(handle.ceremony_id())
        .await
        .map_err(|e| ceremony_op("cancel ceremony", e).into())
}

/// Cancel a key-rotation ceremony using a stored ceremony id.
pub async fn cancel_key_rotation_ceremony_by_id(
    app_core: &Arc<RwLock<AppCore>>,
    ceremony_id: CeremonyId,
) -> Result<(), AuraError> {
    let core = app_core.read().await;
    core.cancel_key_rotation_ceremony(&ceremony_id)
        .await
        .map_err(|e| ceremony_op("cancel ceremony", e).into())
}

/// Poll a key rotation ceremony until completion or failure using a policy.
///
/// This is a portable (frontend-agnostic) helper for driving ceremony progress UIs.
/// Callers provide an `on_update` hook to receive intermediate statuses.
pub async fn monitor_key_rotation_ceremony_with_policy<SleepFn, SleepFut>(
    app_core: &Arc<RwLock<AppCore>>,
    handle: &CeremonyStatusHandle,
    policy: CeremonyPollPolicy,
    mut on_update: impl FnMut(&KeyRotationCeremonyStatus),
    mut sleep_fn: SleepFn,
) -> Result<CeremonyLifecycle<KeyRotationCeremonyStatus>, AuraError>
where
    SleepFn: FnMut(Duration) -> SleepFut,
    SleepFut: Future<Output = ()>,
{
    // Bounded polling to avoid infinite loops; UIs can re-invoke if desired.
    let mut attempts = AttemptBudget::new(policy.max_attempts);
    while attempts.can_attempt() {
        let attempt = attempts
            .record_attempt()
            .map_err(AuraError::from)?
            .saturating_add(1);
        sleep_fn(policy.interval).await;

        let status = get_key_rotation_ceremony_status(app_core, handle).await?;
        on_update(&status);

        if status.has_failed {
            // Best-effort rollback for rotations (until runtime owns this fully).
            let mut rollback_failed = false;
            if policy.rollback_on_failure {
                if let Some(epoch) = status.pending_epoch {
                    if matches!(
                        status.kind,
                        crate::runtime_bridge::CeremonyKind::GuardianRotation
                            | crate::runtime_bridge::CeremonyKind::DeviceRotation
                    ) {
                        let core = app_core.read().await;
                        if let Err(e) = core.rollback_guardian_key_rotation(epoch).await {
                            #[cfg(feature = "instrumented")]
                            tracing::error!(
                                error = %e,
                                ceremony_id = %handle.ceremony_id(),
                                epoch = ?epoch,
                                "ceremony rollback failed — account may be in partially-committed state"
                            );
                            let _ = &e;
                            rollback_failed = true;
                        }
                    }
                }
            }
            return Ok(CeremonyLifecycle {
                state: if rollback_failed {
                    CeremonyLifecycleState::FailedRollbackIncomplete
                } else {
                    CeremonyLifecycleState::Failed
                },
                status,
                attempts: attempt,
            });
        }

        if status.is_complete {
            // Best-effort: refresh settings so device list / counts update after a commit.
            if policy.refresh_settings_on_complete {
                if let Err(_e) =
                    crate::workflows::settings::refresh_settings_from_runtime(app_core).await
                {
                    #[cfg(feature = "instrumented")]
                    tracing::warn!(
                        error = %_e,
                        ceremony_id = %handle.ceremony_id(),
                        "settings refresh failed after ceremony completion — UI may show stale device counts"
                    );
                }
            }
            return Ok(CeremonyLifecycle {
                state: CeremonyLifecycleState::Completed,
                status,
                attempts: attempt,
            });
        }
    }

    // Timed out; return the latest status we can fetch.
    let status = get_key_rotation_ceremony_status(app_core, handle).await?;
    Ok(CeremonyLifecycle {
        state: CeremonyLifecycleState::TimedOut,
        status,
        attempts: policy.max_attempts,
    })
}

/// Poll a key rotation ceremony until completion or failure.
///
/// This is a portable (frontend-agnostic) helper for driving ceremony progress UIs.
/// Callers provide an `on_update` hook to receive intermediate statuses.
pub async fn monitor_key_rotation_ceremony<SleepFn, SleepFut>(
    app_core: &Arc<RwLock<AppCore>>,
    handle: &CeremonyStatusHandle,
    poll_interval: Duration,
    mut on_update: impl FnMut(&KeyRotationCeremonyStatus),
    mut sleep_fn: SleepFn,
) -> Result<KeyRotationCeremonyStatus, AuraError>
where
    SleepFn: FnMut(Duration) -> SleepFut,
    SleepFut: Future<Output = ()>,
{
    let policy = CeremonyPollPolicy::for_kind(handle.kind(), poll_interval);
    let lifecycle = monitor_key_rotation_ceremony_with_policy(
        app_core,
        handle,
        policy,
        &mut on_update,
        &mut sleep_fn,
    )
    .await?;

    Ok(lifecycle.status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enrollment_setup_failure_codes_are_structural() {
        use aura_invitation::enrollment_setup::{
            EnrollmentSetupError as S, EnrollmentSetupVerificationError as V,
        };
        let cases = [
            (V::Unavailable, SemanticFailureCode::Unavailable),
            (
                V::Time(aura_core::effects::time::TimeError::ServiceUnavailable),
                SemanticFailureCode::CeremonyRuntimeFailed,
            ),
            (
                V::Setup(S::InvalidFormat),
                SemanticFailureCode::InvalidArgument,
            ),
            (V::Setup(S::SizeLimit), SemanticFailureCode::InvalidArgument),
            (
                V::Setup(S::UnsupportedVersion(42)),
                SemanticFailureCode::InvalidArgument,
            ),
            (
                V::Setup(S::InvalidValidity),
                SemanticFailureCode::InvalidArgument,
            ),
            (
                V::Setup(S::OutsideValidity),
                SemanticFailureCode::InvalidArgument,
            ),
            (
                V::Setup(S::InvalidSigningPolicy),
                SemanticFailureCode::InvalidArgument,
            ),
            (
                V::Setup(S::ProofBinding),
                SemanticFailureCode::PermissionDenied,
            ),
            (
                V::Setup(S::InvalidSignature),
                SemanticFailureCode::PermissionDenied,
            ),
            (
                V::Setup(S::Codec(serde_json::from_str::<u8>("broken").unwrap_err())),
                SemanticFailureCode::InvalidArgument,
            ),
            (
                V::Setup(S::Crypto(AuraError::agent("OperationTimedOut"))),
                SemanticFailureCode::CeremonyRuntimeFailed,
            ),
            (
                V::Boundary(AuraError::agent("PermissionDenied")),
                SemanticFailureCode::CeremonyRuntimeFailed,
            ),
        ];
        for (error, expected) in cases {
            let mapped = enrollment_setup_failure(&error);
            assert_eq!(mapped.domain, SemanticFailureDomain::Ceremony);
            assert_eq!(mapped.code, expected);
            assert_eq!(mapped.detail.as_deref(), Some(error.to_string().as_str()));
        }
    }

    #[test]
    fn enrollment_issuance_failure_codes_cover_every_stage() {
        use aura_invitation::enrollment_setup::{
            EnrollmentIssuanceError as E, EnrollmentIssuanceStage as S,
        };
        let cases = [
            (E::Unavailable, SemanticFailureCode::Unavailable),
            (E::OutsideValidity, SemanticFailureCode::InvalidArgument),
            (E::CurrentIdentity, SemanticFailureCode::InvalidArgument),
            (E::AlreadyEnrolled, SemanticFailureCode::InvalidState),
            (E::InvalidPolicy, SemanticFailureCode::InvalidArgument),
            (
                E::MissingPackage(aura_core::DeviceId::new_from_entropy([7; 32])),
                SemanticFailureCode::CeremonyRuntimeFailed,
            ),
            (
                E::EmptyPendingPackage,
                SemanticFailureCode::CeremonyRuntimeFailed,
            ),
            (
                E::EmptyPendingConfig,
                SemanticFailureCode::CeremonyRuntimeFailed,
            ),
            (
                E::Time(aura_core::effects::time::TimeError::ServiceUnavailable),
                SemanticFailureCode::CeremonyRuntimeFailed,
            ),
        ];
        for (error, expected) in cases {
            assert_eq!(enrollment_issuance_failure(&error).code, expected);
        }
        for stage in [
            S::TreeRead,
            S::Rotation,
            S::PendingPackageRead,
            S::PendingConfigRead,
            S::PrestateEncoding,
            S::OperationEncoding,
            S::PrestateValidation,
            S::Supersession,
            S::CeremonyRegistration,
            S::SetupVerifierRetention,
            S::InvitationService,
            S::BaselineExport,
            S::BaselineEncoding,
            S::InvitationCreation,
            S::InvitationExport,
        ] {
            let expected = match stage {
                S::PrestateValidation => SemanticFailureCode::InvalidState,
                S::InvitationService => SemanticFailureCode::Unavailable,
                S::TreeRead
                | S::Rotation
                | S::PendingPackageRead
                | S::PendingConfigRead
                | S::PrestateEncoding
                | S::OperationEncoding
                | S::Supersession
                | S::CeremonyRegistration
                | S::SetupVerifierRetention
                | S::BaselineExport
                | S::BaselineEncoding
                | S::InvitationCreation
                | S::InvitationExport => SemanticFailureCode::CeremonyRuntimeFailed,
            };
            let error = E::Failure {
                stage,
                source: AuraError::agent("OperationTimedOut"),
            };
            let mapped = enrollment_issuance_failure(&error);
            assert_eq!(mapped.domain, SemanticFailureDomain::Ceremony);
            assert_eq!(mapped.code, expected);
        }
    }

    #[test]
    fn enrollment_timeout_mapping_preserves_original_source_chain() {
        use aura_invitation::enrollment_setup::{
            EnrollmentIssuanceError as E, EnrollmentIssuanceStage as S,
            EnrollmentSetupVerificationError as V,
        };
        use std::error::Error;
        fn timeout() -> AuraError {
            AuraError::Internal {
                message: "bounded enrollment call".into(),
                source: Some(Arc::new(WorkflowError::TimedOut {
                    operation: "enrollment",
                    stage: "verification",
                    timeout_ms: 30_000,
                })),
            }
        }
        let setup = V::Boundary(timeout());
        assert_eq!(
            enrollment_setup_failure(&setup).code,
            SemanticFailureCode::OperationTimedOut
        );
        let returned = AuraError::Invalid {
            message: setup.to_string(),
            source: Some(Arc::new(setup)),
        };
        let original = returned.source().unwrap().downcast_ref::<V>().unwrap();
        let boundary = original
            .source()
            .unwrap()
            .downcast_ref::<AuraError>()
            .unwrap();
        assert!(matches!(
            boundary.source().unwrap().downcast_ref::<WorkflowError>(),
            Some(WorkflowError::TimedOut { .. })
        ));
        let issuance = E::Failure {
            stage: S::Rotation,
            source: timeout(),
        };
        assert_eq!(
            enrollment_issuance_failure(&issuance).code,
            SemanticFailureCode::OperationTimedOut
        );
        let returned = AuraError::Internal {
            message: issuance.to_string(),
            source: Some(Arc::new(issuance)),
        };
        let original = returned.source().unwrap().downcast_ref::<E>().unwrap();
        assert!(original
            .source()
            .unwrap()
            .downcast_ref::<AuraError>()
            .unwrap()
            .source()
            .unwrap()
            .is::<WorkflowError>());
        assert_eq!(
            enrollment_runtime_failure(&timeout()).code,
            SemanticFailureCode::OperationTimedOut
        );
        let unavailable = AuraError::Internal {
            message: "runtime".into(),
            source: Some(Arc::new(WorkflowError::RuntimeUnavailable)),
        };
        assert_eq!(
            enrollment_runtime_failure(&unavailable).code,
            SemanticFailureCode::Unavailable
        );
        assert_eq!(
            enrollment_setup_failure(&V::Boundary(unavailable)).code,
            SemanticFailureCode::Unavailable
        );
    }

    #[test]
    fn device_enrollment_terminal_failures_keep_stable_codes() {
        let cases = [
            (
                CeremonyFailureReason::Rejected,
                SemanticFailureCode::CeremonyRejected,
            ),
            (
                CeremonyFailureReason::TimedOut,
                SemanticFailureCode::OperationTimedOut,
            ),
            (
                CeremonyFailureReason::ChoreographyFailed,
                SemanticFailureCode::CeremonyChoreographyFailed,
            ),
            (
                CeremonyFailureReason::RuntimeFailed,
                SemanticFailureCode::CeremonyRuntimeFailed,
            ),
            (
                CeremonyFailureReason::Superseded,
                SemanticFailureCode::CeremonySuperseded,
            ),
        ];
        for (reason, code) in cases {
            let failure = device_enrollment_completion_failure(reason);
            assert_eq!(failure.domain, SemanticFailureDomain::Ceremony);
            assert_eq!(failure.code, code);
        }
    }

    #[test]
    fn ceremony_monitor_policy_scales_by_kind() {
        let interval = Duration::from_millis(250);

        let enrollment = CeremonyPollPolicy::for_kind(
            crate::runtime_bridge::CeremonyKind::DeviceEnrollment,
            interval,
        );
        let recovery =
            CeremonyPollPolicy::for_kind(crate::runtime_bridge::CeremonyKind::Recovery, interval);

        assert_eq!(enrollment.max_attempts, 2402);
        assert_eq!(recovery.max_attempts, 2402);
        let removal = CeremonyPollPolicy::for_kind(
            crate::runtime_bridge::CeremonyKind::DeviceRemoval,
            interval,
        );
        assert_eq!(removal.max_attempts, 182);
        assert!(!enrollment.rollback_on_failure);
        assert!(!recovery.rollback_on_failure);
    }

    #[test]
    fn ceremony_start_timeout_is_kind_specific() {
        assert_eq!(
            ceremony_start_timeout(crate::runtime_bridge::CeremonyKind::DeviceEnrollment),
            Duration::from_millis(30_000)
        );
        assert_eq!(
            ceremony_start_timeout(crate::runtime_bridge::CeremonyKind::DeviceRemoval),
            Duration::from_millis(20_000)
        );
        assert_eq!(
            ceremony_monitor_timeout(crate::runtime_bridge::CeremonyKind::Recovery),
            Duration::from_secs(600)
        );
        assert_eq!(
            ceremony_monitor_timeout(crate::runtime_bridge::CeremonyKind::OtaActivation),
            Duration::from_secs(90)
        );
    }

    #[test]
    fn only_transient_intent_errors_retry_for_ceremony_start() {
        assert!(retryable_ceremony_intent_error(
            &IntentError::network_error("timeout")
        ));
        assert!(retryable_ceremony_intent_error(
            &IntentError::service_error("busy")
        ));
        assert!(!retryable_ceremony_intent_error(
            &IntentError::validation_failed("bad input")
        ));
    }
}
