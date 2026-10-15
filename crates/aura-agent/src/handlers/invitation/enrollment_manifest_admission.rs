//! Runtime-owned independent user-transfer admission. Secure records retain the
//! selected verifier rather than promoting the invitation's embedded key.
use super::{Invitation, InvitationType, ShareableInvitation};
use crate::runtime::AuraEffectSystem;
use aura_core::effects::{
    PhysicalTimeEffects, SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
};
use aura_core::{AuthorityId, DeviceId, InvitationId};
use aura_invitation::enrollment_manifest::{
    EnrollmentManifestError, SignedEnrollmentTrustManifest, VerifiedEnrollmentBaseline,
};
use serde::{Deserialize, Serialize};
const MAX_RECORD_BYTES: usize =
    aura_invitation::enrollment_manifest::EnrollmentTrustManifest::MAX_BYTES * 2
        + aura_core::envelope::MAX_INVITE_CODE_CHARS
        + 4096;

const MAX_CONFIRMATION_RECORD_BYTES: usize =
    MAX_RECORD_BYTES + super::enrollment_vm_admission::MAX_CONTROL_FRAME_BYTES;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AdmissionRecord {
    version: u16,
    provisional: AuthorityId,
    device: DeviceId,
    invitation: InvitationId,
    code: String,
    manifest_code: String,
    selected_verifier: Vec<u8>,
    admitted_at_ms: u64,
}
/// Non-clone admission evidence for ceremony-scoped bootstrap ingress. It
/// grants no membership, global transport identity or historical signing key.
pub(crate) struct AdmittedEnrollmentManifest {
    canonical_invitation: Invitation,
    original_code: String,
    baseline: VerifiedEnrollmentBaseline,
    local_setup: aura_invitation::enrollment_setup::VerifiedEnrollmentSetupPossession,
    admitted_at_ms: u64,
}
/// Move-only evidence that this owner just committed a new explicit admission.
/// Only the immutable-store Created branch below can mint it.
pub(crate) struct NewEnrollmentAdmissionCapability<'a> {
    witness: &'a AdmittedEnrollmentManifest,
}
impl NewEnrollmentAdmissionCapability<'_> {
    pub(crate) fn witness(&self) -> &AdmittedEnrollmentManifest {
        self.witness
    }
}
impl AdmittedEnrollmentManifest {
    pub(crate) fn matches_original_transfer(
        &self,
        code: &str,
        pin: &aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentManifest,
    ) -> bool {
        self.original_code == code && self.manifest_digest() == pin.digest()
    }
    pub(crate) fn admitted_at_ms(&self) -> u64 {
        self.admitted_at_ms
    }
    pub(crate) fn manifest(
        &self,
    ) -> &aura_invitation::enrollment_manifest::EnrollmentTrustManifest {
        self.baseline.manifest()
    }
    pub(crate) fn manifest_digest(&self) -> [u8; 32] {
        self.baseline.manifest_digest()
    }
    pub(crate) fn canonical_invitation(&self) -> &Invitation {
        &self.canonical_invitation
    }
    pub(crate) fn local_setup(
        &self,
    ) -> &aura_invitation::enrollment_setup::VerifiedEnrollmentSetupPossession {
        &self.local_setup
    }
    pub(crate) fn baseline(&self) -> &VerifiedEnrollmentBaseline {
        &self.baseline
    }
}
fn location(authority: AuthorityId, id: &InvitationId) -> SecureStorageLocation {
    SecureStorageLocation::new(
        "enrollment_manifest_admission_v2",
        format!("{authority}:{id}"),
    )
}
fn boundary(error: impl std::error::Error + Send + Sync + 'static) -> EnrollmentManifestError {
    EnrollmentManifestError::Runtime(Box::new(error))
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum EnrollmentAdmissionBirthError {
    #[error("original enrollment admission was already published")]
    AlreadyPublished,
    #[error("original enrollment admission publication lost fresh ownership")]
    LostFreshPublication,
}

fn birth_refused(source: EnrollmentAdmissionBirthError) -> EnrollmentManifestError {
    boundary(aura_core::AuraError::PermissionDenied {
        message: "original enrollment admission birth refused".into(),
        source: Some(std::sync::Arc::new(source)),
    })
}

/// Admit only the opaque explicit app transfer selection. Discovery and raw
/// cached invitations cannot call this API with a manufactured pin.
pub(crate) async fn admit_user_transfer(
    effects: &std::sync::Arc<AuraEffectSystem>,
    provisional: AuthorityId,
    code: &str,
    pin: &aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentManifest,
    original_tasks: &crate::task_registry::TaskGroup,
) -> Result<
    crate::runtime::services::enrollment_window::EnrollmentExecutionRoot<
        AdmittedEnrollmentManifest,
    >,
    EnrollmentManifestError,
> {
    let _admission_owner = effects.lock_enrollment_manifest_admission().await;
    let key = location(provisional, &pin.manifest().invitation);
    // A retained record is negative restart evidence, never a source of another
    // original execution owner. Refuse before observing a new physical clock.
    if effects.secure_exists(&key).await.map_err(boundary)? {
        return Err(birth_refused(
            EnrollmentAdmissionBirthError::AlreadyPublished,
        ));
    }
    let now = effects.physical_time().await?.ts_ms;
    let record = AdmissionRecord {
        version: 2,
        provisional,
        device: effects.device_id(),
        invitation: pin.manifest().invitation.clone(),
        code: code.to_string(),
        manifest_code: pin.signed_code().to_string(),
        selected_verifier: pin.manifest().initiator_confirmation_verifier.clone(),
        admitted_at_ms: now,
    };
    let admitted = validate(effects, &record, now).await?;
    let encoded = aura_core::util::serialization::to_vec(&record).map_err(boundary)?;
    if encoded.len() > MAX_RECORD_BYTES {
        return Err(EnrollmentManifestError::Shape);
    }
    let caps = [
        SecureStorageCapability::Read,
        SecureStorageCapability::Write,
    ];
    let outcome = effects
        .secure_store_immutable(&key, &encoded, &caps)
        .await
        .map_err(boundary)?;
    if matches!(
        outcome,
        aura_core::effects::secure::ImmutableSecureStoreOutcome::AlreadyExists
    ) {
        return Err(birth_refused(
            EnrollmentAdmissionBirthError::LostFreshPublication,
        ));
    }
    let birth = crate::runtime::services::enrollment_window::EnrollmentExecutionChild::retain_new_admitted_window(
        effects,
        NewEnrollmentAdmissionCapability { witness: &admitted },
    )
    .await
    .map_err(boundary)?;
    // Only this fresh publication ACK carries execution custody; restart and
    // duplicate imports cannot reconstruct it from matching encrypted bytes.
    retain_import_generation_owner(effects, &admitted).await?;
    birth
        .into_root(effects.clone(), admitted, original_tasks)
        .map_err(boundary)
}

/// Enumerated keys select candidates only. Every candidate must revalidate its
/// protected independent transfer and native verifier before notice admission.
pub(crate) async fn load_recoverable_negative_admissions(
    effects: &AuraEffectSystem,
) -> Result<Vec<AdmittedEnrollmentManifest>, EnrollmentManifestError> {
    let keys = effects
        .secure_list_keys(
            "enrollment_manifest_admission_v2",
            &[SecureStorageCapability::List],
        )
        .await
        .map_err(boundary)?;
    if keys.len() > 1024 {
        return Err(EnrollmentManifestError::Shape);
    }
    let mut admitted = Vec::new();
    for key in keys {
        let bytes = effects
            .secure_retrieve(
                &SecureStorageLocation::new("enrollment_manifest_admission_v2", &key),
                &[SecureStorageCapability::Read],
            )
            .await
            .map_err(boundary)?;
        if bytes.len() > MAX_RECORD_BYTES {
            return Err(EnrollmentManifestError::Shape);
        }
        let record: AdmissionRecord =
            aura_core::util::serialization::from_slice(&bytes).map_err(boundary)?;
        if record.device != effects.device_id()
            || location(record.provisional, &record.invitation).key != key
        {
            return Err(EnrollmentManifestError::Pin);
        }
        let now = effects.physical_time().await?.ts_ms;
        if record.admitted_at_ms > now {
            return Err(EnrollmentManifestError::Pin);
        }
        // Authenticate the original transfer at its retained admission time.
        // The sealed recovery window independently enforces its fixed endpoint;
        // completed historical evidence must not become a new active admission.
        let baseline = validate(effects, &record, record.admitted_at_ms).await?;
        if effects
            .secure_exists(&confirmation_location(
                record.provisional,
                &record.invitation,
            ))
            .await
            .map_err(boundary)?
        {
            let confirmed =
                load_confirmed_enrollment(effects, record.provisional, &record.invitation).await?;
            // Only the independently verified durable confirmation admits the
            // same physical profile's transition from provisional to subject.
            if effects.runtime_authority_id() != record.provisional
                && effects.runtime_authority_id() != confirmed.confirmation().manifest().subject
            {
                return Err(EnrollmentManifestError::Pin);
            }
            continue;
        }
        if record.provisional != effects.runtime_authority_id() {
            return Err(EnrollmentManifestError::Pin);
        }
        if load_failed_enrollment_for_ceremony(
            effects,
            record.provisional,
            &baseline.manifest().ceremony,
        )
        .await?
        .is_some()
        {
            continue;
        }
        admitted.push(baseline);
    }
    Ok(admitted)
}

pub(crate) async fn load_admitted_baseline(
    effects: &AuraEffectSystem,
    provisional: AuthorityId,
    invitation: &Invitation,
) -> Result<AdmittedEnrollmentManifest, EnrollmentManifestError> {
    let _admission_owner = effects.lock_enrollment_manifest_admission().await;
    let key = location(provisional, &invitation.invitation_id);
    if !effects.secure_exists(&key).await.map_err(boundary)? {
        return Err(EnrollmentManifestError::MissingPin);
    }
    let bytes = effects
        .secure_retrieve(&key, &[SecureStorageCapability::Read])
        .await
        .map_err(boundary)?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(EnrollmentManifestError::Shape);
    }
    let record: AdmissionRecord =
        aura_core::util::serialization::from_slice(&bytes).map_err(boundary)?;
    if record.provisional != provisional
        || record.device != effects.device_id()
        || record.invitation != invitation.invitation_id
    {
        #[cfg(test)]
        eprintln!("enrollment admission Pin branch at {}:{}", file!(), line!());
        return Err(EnrollmentManifestError::Pin);
    }
    let admitted = validate(effects, &record, effects.physical_time().await?.ts_ms).await?;
    let canonical = admitted.canonical_invitation();
    // Status is runtime lifecycle state; compare canonical signed payload fields.
    if canonical.sender_id != invitation.sender_id
        || canonical.receiver_id != invitation.receiver_id
        || canonical.invitation_type != invitation.invitation_type
        || canonical.expires_at != invitation.expires_at
        || canonical.message != invitation.message
    {
        #[cfg(test)]
        eprintln!("enrollment admission Pin branch at {}:{}", file!(), line!());
        return Err(EnrollmentManifestError::Pin);
    }
    Ok(admitted)
}

async fn validate(
    effects: &AuraEffectSystem,
    record: &AdmissionRecord,
    now: u64,
) -> Result<AdmittedEnrollmentManifest, EnrollmentManifestError> {
    if record.version != 2
        || record.device != effects.device_id()
        || record.selected_verifier.len() != 32
        || record.admitted_at_ms > now
    {
        #[cfg(test)]
        eprintln!("enrollment admission Pin branch at {}:{}", file!(), line!());
        return Err(EnrollmentManifestError::Pin);
    }
    let signed = SignedEnrollmentTrustManifest::decode(&record.manifest_code)?;
    if now >= signed.manifest.expires_at_ms {
        return Err(EnrollmentManifestError::Expired);
    }
    let verified = signed
        .manifest
        .verify_signature(effects, &record.selected_verifier, &signed.signature)
        .await?;
    let manifest = verified.manifest();
    manifest.final_inventory()?;
    let (shareable, proof, transport) =
        ShareableInvitation::from_code_with_proof_and_transport(&record.code).map_err(boundary)?;
    let proof = proof.ok_or(EnrollmentManifestError::Signature)?;
    if proof.public_key != record.selected_verifier
        || transport.sender_device_id != Some(manifest.initiator_device)
        || proof.sender_device_id != Some(manifest.initiator_device)
        || !aura_signature::verify_ed25519_transcript(
            effects,
            &shareable.signing_transcript_with_transport(&transport),
            &proof.signature,
            &record.selected_verifier,
        )
        .await?
    {
        return Err(EnrollmentManifestError::Signature);
    }
    let InvitationType::DeviceEnrollment {
        subject_authority,
        invitee_authority,
        initiator_device_id,
        device_id,
        ceremony_id,
        pending_epoch,
        setup_binding,
        key_package,
        public_key_package,
        threshold_config,
        baseline_tree_ops,
        ..
    } = &shareable.invitation_type
    else {
        return Err(EnrollmentManifestError::Shape);
    };
    if *subject_authority != manifest.subject
        || shareable.sender_id != manifest.subject
        || *invitee_authority != record.provisional
        || manifest.invitee_authority != record.provisional
        || *device_id != record.device
        || manifest.invitee_device != record.device
        || *initiator_device_id != manifest.initiator_device
        || *ceremony_id != manifest.ceremony
        || shareable.invitation_id != manifest.invitation
        || shareable.invitation_id != record.invitation
        || *pending_epoch != manifest.pending_epoch
        || setup_binding != &manifest.setup
        || aura_core::hash::hash(key_package) != manifest.pending_share_digest
        || aura_core::hash::hash(public_key_package) != manifest.pending_public_key_package_digest
        || aura_core::hash::hash(threshold_config)
            != *manifest.pending_threshold_config_digest.as_bytes()
    {
        #[cfg(test)]
        eprintln!("enrollment admission Pin branch at {}:{}", file!(), line!());
        return Err(EnrollmentManifestError::Pin);
    }
    aura_invitation::enrollment_manifest::EnrollmentTrustManifest::validate_pending_policy(
        threshold_config,
    )?;
    let setup_location = SecureStorageLocation::with_sub_key(
        "device_enrollment_setup",
        format!(
            "{}:{}",
            record.provisional,
            hex::encode(manifest.setup.nonce)
        ),
        "request",
    );
    let setup_bytes = effects
        .secure_retrieve(&setup_location, &[SecureStorageCapability::Read])
        .await
        .map_err(boundary)?;
    let setup_code = std::str::from_utf8(&setup_bytes).map_err(boundary)?;
    let setup = aura_invitation::enrollment_setup::DeviceEnrollmentSetupRequest::decode(setup_code)
        .map_err(boundary)?
        .verify_possession(effects, record.admitted_at_ms)
        .await
        .map_err(boundary)?;
    if setup.digest() != manifest.setup.digest
        || setup.statement().device != record.device
        || setup.statement().authority != record.provisional
        || setup.statement().nonce != manifest.setup.nonce
        || manifest.expires_at_ms > setup.statement().expires_at_ms
        || manifest.expires_at_ms <= setup.statement().issued_at_ms
    {
        #[cfg(test)]
        eprintln!("enrollment admission Pin branch at {}:{}", file!(), line!());
        return Err(EnrollmentManifestError::Pin);
    }
    let baseline = verified.clone().verify_baseline(baseline_tree_ops)?;
    let imported = aura_invitation::shareable::ValidatedImportedInvitation::verify_code(
        effects,
        &record.code,
        record.provisional,
        shareable.context_id.ok_or(EnrollmentManifestError::Shape)?,
        now,
    )
    .await
    .map_err(boundary)?;
    Ok(AdmittedEnrollmentManifest {
        canonical_invitation: imported.invitation().clone(),
        original_code: record.code.clone(),
        baseline,
        local_setup: setup,
        admitted_at_ms: record.admitted_at_ms,
    })
}

/// Historical failed receipt evidence is distinct from committed admission.
/// It never authorizes adoption, fresh signing or a renewed execution window.
pub(crate) struct RetainedFailureAdmissionCapability {
    admitted: AdmittedEnrollmentManifest,
    frame: super::enrollment_vm_admission::EnrollmentControlFrame,
    observed_at_ms: u64,
    acknowledged_at_ms: u64,
    frozen_budget: Vec<u8>,
}
impl RetainedFailureAdmissionCapability {
    pub(crate) fn admitted(&self) -> &AdmittedEnrollmentManifest {
        &self.admitted
    }
    pub(super) fn frame(&self) -> &super::enrollment_vm_admission::EnrollmentControlFrame {
        &self.frame
    }
    pub(crate) fn observed_at_ms(&self) -> u64 {
        self.observed_at_ms
    }
    pub(crate) fn acknowledged_at_ms(&self) -> u64 {
        self.acknowledged_at_ms
    }
    pub(crate) fn budget_bytes(&self) -> &[u8] {
        &self.frozen_budget
    }
}
pub(crate) struct DurableFailedEnrollmentCapability {
    evidence: super::enrollment_vm_admission::VerifiedEnrollmentFailureCapability,
}
impl DurableFailedEnrollmentCapability {
    pub(crate) fn evidence(
        &self,
    ) -> &super::enrollment_vm_admission::VerifiedEnrollmentFailureCapability {
        &self.evidence
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FailureRecord {
    version: u16,
    provisional: AuthorityId,
    device: DeviceId,
    invitation: Invitation,
    manifest_digest: [u8; 32],
    frame: super::enrollment_vm_admission::EnrollmentControlFrame,
    observed_at_ms: u64,
    acknowledged_at_ms: u64,
    frozen_budget: Vec<u8>,
}
fn failure_location(
    provisional: AuthorityId,
    ceremony: &aura_core::CeremonyId,
) -> SecureStorageLocation {
    SecureStorageLocation::with_sub_key(
        "device_enrollment_failed_import_v1",
        provisional.to_string(),
        ceremony.to_string(),
    )
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "authenticated_enrollment_failure_receipt",
    capability_type = VerifiedEnrollmentFailureCapability,
    family = "runtime_helper"
)]
pub(super) async fn retain_verified_failure(
    effects: &AuraEffectSystem,
    failure: super::enrollment_vm_admission::VerifiedEnrollmentFailureCapability,
    acknowledged: crate::runtime::services::enrollment_window::AcknowledgedEnrollmentWindow,
) -> Result<DurableFailedEnrollmentCapability, EnrollmentManifestError> {
    persist_verified_failure(effects, &failure, &acknowledged).await?;
    Ok(DurableFailedEnrollmentCapability { evidence: failure })
}
async fn persist_verified_failure(
    effects: &AuraEffectSystem,
    failure: &super::enrollment_vm_admission::VerifiedEnrollmentFailureCapability,
    acknowledged: &crate::runtime::services::enrollment_window::AcknowledgedEnrollmentWindow,
) -> Result<(), EnrollmentManifestError> {
    let manifest = failure.manifest();
    if effects.device_id() != manifest.invitee_device
        || acknowledged.manifest_digest() != failure.manifest_digest()
        || acknowledged.ceremony() != &manifest.ceremony
        || acknowledged.invitation() != &manifest.invitation
        || acknowledged.device() != manifest.invitee_device
        || failure.observed_at_ms() > acknowledged.acknowledged_at_ms()
    {
        return Err(EnrollmentManifestError::Pin);
    }
    // The actual admitted execution lease remains held through this write.
    if effects
        .secure_exists(&confirmation_location(
            manifest.invitee_authority,
            &manifest.invitation,
        ))
        .await
        .map_err(boundary)?
    {
        return Err(EnrollmentManifestError::Pin);
    }
    let record = FailureRecord {
        version: 1,
        provisional: manifest.invitee_authority,
        device: manifest.invitee_device,
        invitation: failure.canonical_invitation().clone(),
        manifest_digest: failure.manifest_digest(),
        frame: failure.frame().clone(),
        observed_at_ms: failure.observed_at_ms(),
        acknowledged_at_ms: acknowledged.acknowledged_at_ms(),
        frozen_budget: acknowledged.frozen_budget_bytes().to_vec(),
    };
    let bytes = aura_core::util::serialization::to_vec(&record).map_err(boundary)?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(EnrollmentManifestError::Shape);
    }
    let key = failure_location(record.provisional, &manifest.ceremony);
    let outcome = effects
        .secure_store_immutable(
            &key,
            &bytes,
            &[
                SecureStorageCapability::Read,
                SecureStorageCapability::Write,
            ],
        )
        .await
        .map_err(boundary)?;
    if outcome == aura_core::effects::secure::ImmutableSecureStoreOutcome::Created {
        let retained = effects
            .secure_retrieve(&key, &[SecureStorageCapability::Read])
            .await
            .map_err(boundary)?;
        if retained != bytes {
            return Err(EnrollmentManifestError::Pin);
        }
        return Ok(());
    }
    let retained =
        load_failed_enrollment_for_ceremony(effects, record.provisional, &manifest.ceremony)
            .await?
            .ok_or(EnrollmentManifestError::Pin)?;
    if retained.evidence().manifest_digest() != record.manifest_digest
        || retained.evidence().reason() != failure.reason()
    {
        return Err(EnrollmentManifestError::Pin);
    }
    Ok(())
}

/// Raw IDs select a secure record; only full pinned replay mints readout evidence.
pub(crate) async fn load_failed_enrollment_for_ceremony(
    effects: &AuraEffectSystem,
    provisional: AuthorityId,
    ceremony: &aura_core::CeremonyId,
) -> Result<Option<DurableFailedEnrollmentCapability>, EnrollmentManifestError> {
    let key = failure_location(provisional, ceremony);
    if !effects.secure_exists(&key).await.map_err(boundary)? {
        return Ok(None);
    }
    let bytes = effects
        .secure_retrieve(&key, &[SecureStorageCapability::Read])
        .await
        .map_err(boundary)?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(EnrollmentManifestError::Shape);
    }
    let record: FailureRecord =
        aura_core::util::serialization::from_slice(&bytes).map_err(boundary)?;
    let now = effects.physical_time().await?.ts_ms;
    if record.version != 1
        || record.provisional != provisional
        || record.device != effects.device_id()
        || record.observed_at_ms > record.acknowledged_at_ms
        || record.acknowledged_at_ms > now
        || record.frozen_budget.len() > 16_384
    {
        return Err(EnrollmentManifestError::Pin);
    }
    let admission = effects
        .secure_retrieve(
            &location(provisional, &record.invitation.invitation_id),
            &[SecureStorageCapability::Read],
        )
        .await
        .map_err(boundary)?;
    if admission.len() > MAX_RECORD_BYTES {
        return Err(EnrollmentManifestError::Shape);
    }
    let original: AdmissionRecord =
        aura_core::util::serialization::from_slice(&admission).map_err(boundary)?;
    if original.provisional != provisional
        || original.device != effects.device_id()
        || original.invitation != record.invitation.invitation_id
        || original.admitted_at_ms > record.observed_at_ms
    {
        return Err(EnrollmentManifestError::Pin);
    }
    let admitted = validate(effects, &original, original.admitted_at_ms).await?;
    if admitted.manifest().ceremony != *ceremony
        || admitted.manifest_digest() != record.manifest_digest
        || !same_signed_invitation(admitted.canonical_invitation(), &record.invitation)
        || record.observed_at_ms >= admitted.manifest().expires_at_ms
    {
        return Err(EnrollmentManifestError::Pin);
    }
    if effects
        .secure_exists(&confirmation_location(
            provisional,
            &record.invitation.invitation_id,
        ))
        .await
        .map_err(boundary)?
    {
        return Err(EnrollmentManifestError::Pin);
    }
    let retained = RetainedFailureAdmissionCapability {
        admitted,
        frame: record.frame,
        observed_at_ms: record.observed_at_ms,
        acknowledged_at_ms: record.acknowledged_at_ms,
        frozen_budget: record.frozen_budget,
    };
    crate::runtime::services::enrollment_window::EnrollmentExecutionChild::verify_retained_failure_clock(effects, &retained).await.map_err(boundary)?;
    let evidence = super::enrollment_vm_admission::verify_retained_failure(effects, retained)
        .await
        .map_err(boundary)?;
    Ok(Some(DurableFailedEnrollmentCapability { evidence }))
}

/// Authenticated local secure receipt input; fields cannot be supplied by peer
/// frames, JSON callers, cached status, or an arbitrary recovery timestamp.
pub(crate) struct RetainedConfirmationAdmission {
    admitted: AdmittedEnrollmentManifest,
    frame: super::enrollment_vm_admission::EnrollmentControlFrame,
    confirmed_at_ms: u64,
    acknowledged_at_ms: u64,
    frozen_budget: Vec<u8>,
}
impl RetainedConfirmationAdmission {
    pub(crate) fn admitted(&self) -> &AdmittedEnrollmentManifest {
        &self.admitted
    }
    pub(super) fn frame(&self) -> &super::enrollment_vm_admission::EnrollmentControlFrame {
        &self.frame
    }
    pub(crate) fn confirmed_at_ms(&self) -> u64 {
        self.confirmed_at_ms
    }
    pub(crate) fn acknowledged_at_ms(&self) -> u64 {
        self.acknowledged_at_ms
    }
    pub(crate) fn budget_bytes(&self) -> &[u8] {
        &self.frozen_budget
    }
}
/// Only immutable receipt publication/reverification may produce the durable
/// capability. Serialization never reconstructs this activation input.
pub(crate) struct DurableConfirmedEnrollmentCapability {
    evidence: super::enrollment_vm_admission::VerifiedEnrollmentConfirmation,
}
impl DurableConfirmedEnrollmentCapability {
    pub(crate) fn confirmation(
        &self,
    ) -> &super::enrollment_vm_admission::VerifiedEnrollmentConfirmation {
        &self.evidence
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfirmationRecord {
    version: u16,
    provisional: AuthorityId,
    device: DeviceId,
    invitation: Invitation,
    manifest_digest: [u8; 32],
    frame: super::enrollment_vm_admission::EnrollmentControlFrame,
    confirmed_at_ms: u64,
    acknowledged_at_ms: u64,
    frozen_budget: Vec<u8>,
}
fn confirmation_location(provisional: AuthorityId, id: &InvitationId) -> SecureStorageLocation {
    SecureStorageLocation::with_sub_key(
        "device_enrollment_confirmed_import_v1",
        provisional.to_string(),
        id.to_string(),
    )
}
fn same_signed_invitation(left: &Invitation, right: &Invitation) -> bool {
    left.invitation_id == right.invitation_id
        && left.sender_id == right.sender_id
        && left.receiver_id == right.receiver_id
        && left.context_id == right.context_id
        && left.invitation_type == right.invitation_type
        && left.expires_at == right.expires_at
        && left.message == right.message
}

pub(super) async fn retain_verified_confirmation(
    effects: &AuraEffectSystem,
    confirmed: super::enrollment_vm_admission::VerifiedEnrollmentConfirmation,
    acknowledged: crate::runtime::services::enrollment_window::AcknowledgedEnrollmentWindow,
) -> Result<DurableConfirmedEnrollmentCapability, EnrollmentManifestError> {
    let manifest = confirmed.manifest();
    if effects.device_id() != manifest.invitee_device
        || acknowledged.manifest_digest() != confirmed.manifest_digest()
        || acknowledged.ceremony() != &manifest.ceremony
        || acknowledged.invitation() != &manifest.invitation
        || acknowledged.device() != manifest.invitee_device
        || confirmed.confirmed_at_ms() > acknowledged.acknowledged_at_ms()
    {
        return Err(EnrollmentManifestError::Pin);
    }
    if effects
        .secure_exists(&failure_location(
            manifest.invitee_authority,
            &manifest.ceremony,
        ))
        .await
        .map_err(boundary)?
    {
        return Err(EnrollmentManifestError::Pin);
    }
    let record = ConfirmationRecord {
        version: 1,
        provisional: manifest.invitee_authority,
        device: manifest.invitee_device,
        invitation: confirmed.canonical_invitation().clone(),
        manifest_digest: confirmed.manifest_digest(),
        frame: confirmed.frame().clone(),
        confirmed_at_ms: confirmed.confirmed_at_ms(),
        acknowledged_at_ms: acknowledged.acknowledged_at_ms(),
        frozen_budget: acknowledged.frozen_budget_bytes().to_vec(),
    };
    let bytes = aura_core::util::serialization::to_vec(&record).map_err(boundary)?;
    if bytes.len() > MAX_CONFIRMATION_RECORD_BYTES {
        return Err(EnrollmentManifestError::Shape);
    }
    let key = confirmation_location(record.provisional, &record.invitation.invitation_id);
    let outcome = effects
        .secure_store_immutable(
            &key,
            &bytes,
            &[
                SecureStorageCapability::Read,
                SecureStorageCapability::Write,
            ],
        )
        .await
        .map_err(boundary)?;
    if outcome == aura_core::effects::secure::ImmutableSecureStoreOutcome::Created {
        return Ok(DurableConfirmedEnrollmentCapability {
            evidence: confirmed,
        });
    }
    // Existing is not success. Reverify actual original pinned commitment and
    // require the same canonical confirmation before returning its capability.
    let retained = load_confirmed_enrollment(
        effects,
        record.provisional,
        &record.invitation.invitation_id,
    )
    .await?;
    let previous = retained.confirmation();
    if previous.manifest_digest() != record.manifest_digest
        || !same_signed_invitation(previous.canonical_invitation(), &record.invitation)
        || aura_core::util::serialization::to_vec(previous.frame()).map_err(boundary)?
            != aura_core::util::serialization::to_vec(&record.frame).map_err(boundary)?
    {
        return Err(EnrollmentManifestError::Pin);
    }
    Ok(retained)
}

/// Locator IDs identify the receipt; they do not authorize activation. This
/// owner loads both immutable secure records and cryptographically revalidates
/// the original independent transfer plus the exact issuer committed frame.
pub(crate) async fn load_confirmed_enrollment(
    effects: &AuraEffectSystem,
    provisional: AuthorityId,
    id: &InvitationId,
) -> Result<DurableConfirmedEnrollmentCapability, EnrollmentManifestError> {
    let bytes = effects
        .secure_retrieve(
            &confirmation_location(provisional, id),
            &[SecureStorageCapability::Read],
        )
        .await
        .map_err(boundary)?;
    if bytes.len() > MAX_CONFIRMATION_RECORD_BYTES {
        return Err(EnrollmentManifestError::Shape);
    }
    let record: ConfirmationRecord =
        aura_core::util::serialization::from_slice(&bytes).map_err(boundary)?;
    let now = effects.physical_time().await?.ts_ms;
    if record.version != 1
        || record.provisional != provisional
        || record.device != effects.device_id()
        || record.invitation.invitation_id != *id
        || record.confirmed_at_ms > record.acknowledged_at_ms
        || record.acknowledged_at_ms > now
        || record.frozen_budget.len() > 16_384
    {
        return Err(EnrollmentManifestError::Pin);
    }
    let admission_bytes = effects
        .secure_retrieve(&location(provisional, id), &[SecureStorageCapability::Read])
        .await
        .map_err(boundary)?;
    if admission_bytes.len() > MAX_RECORD_BYTES {
        return Err(EnrollmentManifestError::Shape);
    }
    let original: AdmissionRecord =
        aura_core::util::serialization::from_slice(&admission_bytes).map_err(boundary)?;
    if original.provisional != provisional
        || original.device != effects.device_id()
        || original.invitation != *id
        || original.admitted_at_ms > record.confirmed_at_ms
    {
        return Err(EnrollmentManifestError::Pin);
    }
    // Historical verification is scoped inside the secure receipt owner. Live
    // admission remains strict at current time; arbitrary callers cannot pass
    // a chosen timestamp to the control verifier or mint a durable capability.
    let admitted = validate(effects, &original, original.admitted_at_ms).await?;
    if admitted.manifest_digest() != record.manifest_digest
        || !same_signed_invitation(admitted.canonical_invitation(), &record.invitation)
        || record.confirmed_at_ms >= admitted.manifest().expires_at_ms
    {
        return Err(EnrollmentManifestError::Pin);
    }
    let retained = RetainedConfirmationAdmission {
        admitted,
        frame: record.frame,
        confirmed_at_ms: record.confirmed_at_ms,
        acknowledged_at_ms: record.acknowledged_at_ms,
        frozen_budget: record.frozen_budget,
    };
    crate::runtime::services::enrollment_window::EnrollmentExecutionChild::verify_retained_confirmation_clock(effects,&retained)
        .await.map_err(boundary)?;
    let evidence = super::enrollment_vm_admission::verify_retained_confirmation(effects, retained)
        .await
        .map_err(boundary)?;
    Ok(DurableConfirmedEnrollmentCapability { evidence })
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ImportedGenerationOwner {
    version: u16,
    provisional: AuthorityId,
    subject: AuthorityId,
    device: DeviceId,
    invitation: InvitationId,
    ceremony: aura_core::CeremonyId,
    epoch: u64,
    manifest_digest: [u8; 32],
    setup_digest: [u8; 32],
    share_digest: [u8; 32],
    package_digest: [u8; 32],
    config_digest: [u8; 32],
}
impl ImportedGenerationOwner {
    fn from_admitted(admitted: &AdmittedEnrollmentManifest) -> Self {
        Self::from_manifest(admitted.manifest(), admitted.manifest_digest())
    }
    fn from_manifest(
        manifest: &aura_invitation::enrollment_manifest::EnrollmentTrustManifest,
        digest: [u8; 32],
    ) -> Self {
        Self {
            version: 1,
            provisional: manifest.invitee_authority,
            subject: manifest.subject,
            device: manifest.invitee_device,
            invitation: manifest.invitation.clone(),
            ceremony: manifest.ceremony.clone(),
            epoch: manifest.pending_epoch,
            manifest_digest: digest,
            setup_digest: manifest.setup.digest,
            share_digest: manifest.pending_share_digest,
            package_digest: manifest.pending_public_key_package_digest,
            config_digest: *manifest.pending_threshold_config_digest.as_bytes(),
        }
    }
}
pub(crate) fn imported_generation_location(
    authority: &AuthorityId,
    epoch: u64,
) -> SecureStorageLocation {
    SecureStorageLocation::with_sub_key(
        "device_enrollment_import_generation_v1",
        authority.to_string(),
        epoch.to_string(),
    )
}
async fn retain_import_generation_owner(
    effects: &AuraEffectSystem,
    admitted: &AdmittedEnrollmentManifest,
) -> Result<(), EnrollmentManifestError> {
    let owner = ImportedGenerationOwner::from_admitted(admitted);
    if owner.device != effects.device_id() {
        return Err(EnrollmentManifestError::Pin);
    }
    let bytes = aura_core::util::serialization::to_vec(&owner).map_err(boundary)?;
    let key = imported_generation_location(&owner.subject, owner.epoch);
    let result = effects
        .secure_store_immutable(
            &key,
            &bytes,
            &[
                SecureStorageCapability::Read,
                SecureStorageCapability::Write,
            ],
        )
        .await
        .map_err(boundary)?;
    if result == aura_core::effects::secure::ImmutableSecureStoreOutcome::AlreadyExists {
        let original = effects
            .secure_retrieve(&key, &[SecureStorageCapability::Read])
            .await
            .map_err(boundary)?;
        if original.len() > 4096 {
            return Err(EnrollmentManifestError::Shape);
        }
        let original: ImportedGenerationOwner =
            aura_core::util::serialization::from_slice(&original).map_err(boundary)?;
        if original != owner {
            return Err(EnrollmentManifestError::Pin);
        }
    }
    Ok(())
}
/// Optional absence is not completion; any existing receipt must fully reverify.
pub(crate) async fn load_optional_confirmed_enrollment(
    effects: &AuraEffectSystem,
    admitted: &AdmittedEnrollmentManifest,
) -> Result<Option<DurableConfirmedEnrollmentCapability>, EnrollmentManifestError> {
    let manifest = admitted.manifest();
    if !effects
        .secure_exists(&confirmation_location(
            manifest.invitee_authority,
            &manifest.invitation,
        ))
        .await
        .map_err(boundary)?
    {
        return Ok(None);
    }
    let confirmed =
        load_confirmed_enrollment(effects, manifest.invitee_authority, &manifest.invitation)
            .await?;
    if confirmed.confirmation().manifest_digest() != admitted.manifest_digest() {
        return Err(EnrollmentManifestError::Pin);
    }
    Ok(Some(confirmed))
}
pub(crate) async fn require_admitted_import_generation(
    effects: &AuraEffectSystem,
    admitted: &AdmittedEnrollmentManifest,
) -> Result<(), EnrollmentManifestError> {
    require_import_generation_owner(effects, ImportedGenerationOwner::from_admitted(admitted)).await
}
async fn require_import_generation_owner(
    effects: &AuraEffectSystem,
    expected: ImportedGenerationOwner,
) -> Result<(), EnrollmentManifestError> {
    if expected.device != effects.device_id() {
        return Err(EnrollmentManifestError::Pin);
    }
    let bytes = effects
        .secure_retrieve(
            &imported_generation_location(&expected.subject, expected.epoch),
            &[SecureStorageCapability::Read],
        )
        .await
        .map_err(boundary)?;
    if bytes.len() > 4096 {
        return Err(EnrollmentManifestError::Shape);
    }
    let actual: ImportedGenerationOwner =
        aura_core::util::serialization::from_slice(&bytes).map_err(boundary)?;
    if actual != expected {
        return Err(EnrollmentManifestError::Pin);
    }
    Ok(())
}

/// Completion authorizes only its already retained exact imported generation;
/// neither an arbitrary cached invitation nor generic rotation can activate it.
pub(crate) async fn require_confirmed_import_generation(
    effects: &AuraEffectSystem,
    confirmed: &DurableConfirmedEnrollmentCapability,
) -> Result<(), EnrollmentManifestError> {
    let proof = confirmed.confirmation();
    let expected =
        ImportedGenerationOwner::from_manifest(proof.manifest(), proof.manifest_digest());
    if expected.device != effects.device_id() {
        return Err(EnrollmentManifestError::Pin);
    }
    let bytes = effects
        .secure_retrieve(
            &imported_generation_location(&expected.subject, expected.epoch),
            &[SecureStorageCapability::Read],
        )
        .await
        .map_err(boundary)?;
    if bytes.len() > 4096 {
        return Err(EnrollmentManifestError::Shape);
    }
    let actual: ImportedGenerationOwner =
        aura_core::util::serialization::from_slice(&bytes).map_err(boundary)?;
    if actual != expected {
        return Err(EnrollmentManifestError::Pin);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_bridge::AgentRuntimeBridge;
    use aura_app::runtime_bridge::RuntimeBridge;
    use std::sync::Arc;

    // Real runtime construction uses the same explicit stack budget as the
    // neighboring enrollment integration fixtures; no global environment is needed.
    macro_rules! runtime_admission_test {
        ($name:ident, $body:block) => {
            #[test]
            fn $name() {
                crate::handlers::invitation::tests::run_async_test_on_large_stack(async move $body);
            }
        };
    }

    runtime_admission_test!(
        actual_transfer_immutable_duplicate_preserves_original_and_corruption_fails_closed,
        {
            let (_issuer, invitee, invitation, start, _accept, _verified) = Box::pin(
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
                    "immutable-admission",
                ),
            )
            .await;
            let effects = invitee.runtime().effects();
            let key = location(invitee.authority_id(), &invitation.invitation_id);
            let read = [SecureStorageCapability::Read];
            let original = effects.secure_retrieve(&key, &read).await.unwrap();
            let app = std::sync::Arc::new(async_lock::RwLock::new(
                aura_app::AppCore::with_runtime(
                    aura_app::AppConfig::default(),
                    std::sync::Arc::new(AgentRuntimeBridge::new(invitee.clone())),
                )
                .unwrap(),
            ));
            let transfer = start.manifest_transfer.as_ref().unwrap();
            let pin =
                aura_app::ui::workflows::ceremonies::pin_user_transferred_enrollment_manifest(
                    &app,
                    transfer.manifest_code.clone(),
                    transfer.initiator_verifier_code.clone(),
                )
                .await
                .unwrap();
            let duplicate = invitee
                .invitations()
                .unwrap()
                .import_enrollment_and_cache(&start.enrollment_code, &pin)
                .await
                .unwrap();
            assert_eq!(duplicate.invitation_id, invitation.invitation_id);
            assert_eq!(
                effects.secure_retrieve(&key, &read).await.unwrap(),
                original,
                "idempotent transfer cannot replace original admission time or pinned evidence"
            );
            // Corrupt the actual backing ciphertext: the record is immutable and
            // production admission has no replacement authority.
            effects
                .fault_corrupt_secure_record_for_test(&key)
                .await
                .unwrap();
            assert!(matches!(
                admit_user_transfer(
                    &effects,
                    invitee.authority_id(),
                    &start.enrollment_code,
                    &pin,
                    &invitee
                        .runtime()
                        .tasks()
                        .group("duplicate-admission-refusal"),
                )
                .await,
                Err(EnrollmentManifestError::Runtime(_))
            ));
            assert!(
                effects.secure_retrieve(&key, &read).await.is_err(),
                "corrupt admission evidence fails closed and is not replaced"
            );
        }
    );

    runtime_admission_test!(
        actual_admitted_setup_signs_exact_pending_mailbox_registration_and_refuses_substitution,
        {
            let (_issuer, invitee, invitation, _start, _accept, _verified) = Box::pin(
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
                    "signed-pending-mailbox-registration",
                ),
            )
            .await;
            let effects = invitee.runtime().effects();
            let admitted = load_recoverable_negative_admissions(effects.as_ref())
                .await
                .expect("actual protected original admission");
            assert_eq!(admitted.len(), 1);
            let original = &admitted[0];
            assert_eq!(original.manifest().invitation, invitation.invitation_id);
            let commitment = aura_core::hash::hash(b"independent one-time retrieval credential");
            let registration = invitee
                .threshold_signing()
                .sign_pending_enrollment_mailbox_registration(
                    original.baseline(),
                    original.local_setup(),
                    commitment,
                )
                .await
                .expect("actual provisional setup key signs original route");
            let selected_now = effects.physical_time().await.unwrap().ts_ms;
            let verified = registration
                .clone()
                .verify_for_admitted_original(
                    effects.as_ref(),
                    original.baseline(),
                    original.local_setup(),
                    selected_now,
                )
                .await
                .expect("exact signed setup route verifies against original transfer");
            assert_eq!(verified.statement().invitation(), &invitation.invitation_id);
            assert_eq!(verified.statement().subject(), original.manifest().subject);
            assert_eq!(verified.statement().invitee_device(), effects.device_id());
            for (field, replacement) in [
                ("invitation", serde_json::json!("foreign-invitation")),
                ("manifest_digest", serde_json::json!(vec![0_u8; 32])),
                (
                    "invitee_device",
                    serde_json::json!(aura_core::DeviceId::from_bytes([49; 32])),
                ),
                (
                    "expires_at_ms",
                    serde_json::json!(original.manifest().expires_at_ms - 1),
                ),
            ] {
                let mut altered = serde_json::to_value(&registration).unwrap();
                altered["statement"][field] = replacement;
                let altered = serde_json::from_value(altered).unwrap();
                assert!(
                    aura_invitation::enrollment_mailbox::PendingEnrollmentMailboxRegistration::verify_for_admitted_original(
                        altered,
                        effects.as_ref(),
                        original.baseline(),
                        original.local_setup(),
                        selected_now,
                    )
                    .await
                    .is_err(),
                    "foreign {field} cannot select original mailbox"
                );
            }
            let mut foreign_key = registration.clone();
            foreign_key.proof.public_key_package = vec![7; 32];
            assert!(foreign_key
                .verify_for_admitted_original(
                    effects.as_ref(),
                    original.baseline(),
                    original.local_setup(),
                    selected_now,
                )
                .await
                .is_err());
            assert!(registration
                .verify_for_admitted_original(
                    effects.as_ref(),
                    original.baseline(),
                    original.local_setup(),
                    original.manifest().expires_at_ms,
                )
                .await
                .is_err());
        }
    );

    runtime_admission_test!(
        authenticated_pending_mailbox_first_frame_retries_interrupted_take_until_receiver_ack,
        {
            let (issuer, invitee, invitation, _start, _accept, _verified) = Box::pin(
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
                    "authenticated-first-mailbox-frame",
                ),
            )
            .await;
            let issuer_effects = issuer.runtime().effects();
            let invitee_effects = invitee.runtime().effects();
            let admitted = load_recoverable_negative_admissions(invitee_effects.as_ref())
                .await
                .expect("actual protected original admission");
            assert_eq!(admitted.len(), 1);
            let admitted = &admitted[0];
            let selected_now = invitee_effects
                .physical_time()
                .await
                .expect("selected receiver time")
                .ts_ms;
            let broker_time = Arc::new(aura_testkit::time::ManualPhysicalClock::new(selected_now));
            let broker = crate::runtime::services::bootstrap_broker::LocalBootstrapBrokerService::bind(
                "127.0.0.1:0",
                std::time::Duration::from_secs(60),
                "first-frame-global-broker-auth",
                crate::runtime::services::bootstrap_broker::BootstrapBrokerLanBindPolicy::LoopbackOnly,
            ).await.expect("loopback broker")
                .with_mailbox_time_provider(broker_time.clone());
            broker.start(
                &issuer
                    .runtime()
                    .tasks()
                    .group("original-first-mailbox-broker"),
            );
            let config =
                crate::runtime::services::bootstrap_broker::BootstrapBrokerConfig::default()
                    .enabled(true)
                    .with_base_url(broker.public_url())
                    .with_auth_token("first-frame-global-broker-auth");
            let issuer_manager = crate::runtime::services::RendezvousManager::new_with_default_udp(
                issuer.authority_id(),
                crate::runtime::services::RendezvousManagerConfig::default()
                    .with_bootstrap_broker(config.clone()),
                Arc::new(issuer_effects.time_effects().clone()),
            );
            let invitee_manager = crate::runtime::services::RendezvousManager::new_with_default_udp(
                invitee.authority_id(),
                crate::runtime::services::RendezvousManagerConfig::default()
                    .with_bootstrap_broker(config),
                Arc::new(invitee_effects.time_effects().clone()),
            );
            issuer_effects.attach_rendezvous_manager(issuer_manager.clone());
            invitee_effects.attach_rendezvous_manager(invitee_manager.clone());
            assert!(
                selected_now < admitted.manifest().expires_at_ms,
                "the original signed admission is still live before upkeep"
            );
            assert!(
                matches!(
                    issuer_manager
                        .original_pending_mailbox_registration(
                            admitted.manifest().subject,
                            admitted.manifest().invitee_device,
                            admitted.manifest_digest(),
                        )
                        .await,
                    Err(crate::runtime::services::rendezvous_manager::RendezvousManagerError::PendingMailboxRegistrationUnavailable)
                ),
                "a live original admission has no broker registration before receiver owner upkeep"
            );
            let invitee_bridge: Arc<dyn aura_app::runtime_bridge::RuntimeBridge> =
                Arc::new(AgentRuntimeBridge::new(invitee.clone()).with_sync_failure_for_test());
            let app = Arc::new(async_lock::RwLock::new(
                aura_app::AppCore::with_runtime(
                    aura_app::AppConfig::default(),
                    invitee_bridge.clone(),
                )
                .expect("actual Bob app owns its runtime"),
            ));
            let sync_error =
                aura_app::ui::workflows::runtime::run_harness_runtime_maintenance_pass(
                    &app,
                    &invitee_bridge,
                )
                .await
                .expect_err("required sibling sync failure remains observable");
            assert!(sync_error.to_string().contains("converge sync"));
            let session =
                aura_invitation::enrollment_mailbox_session::original_enrollment_session_id(
                    &invitation.invitation_id,
                );
            let retained = issuer_effects
                .original_mailbox_issuer(session)
                .expect("actual original issuer owns the live enrollment session");
            let registration = issuer_manager
                .original_pending_mailbox_registration(
                    admitted.manifest().subject,
                    admitted.manifest().invitee_device,
                    admitted.manifest_digest(),
                )
                .await
                .expect("Bob's actual setup-key registration reached broker");
            let progress = crate::runtime::services::bootstrap_broker::fetch_pending_progress_witness_for_test(
                broker.public_url(), "first-frame-global-broker-auth",
                admitted.manifest().subject, admitted.manifest().invitee_device,
                admitted.manifest_digest(),
            ).await.expect("authenticated registration stage");
            assert_eq!(progress["registration_present"], true);
            assert_eq!(progress["messages"].as_array().map(Vec::len), Some(0));
            let route =
                super::super::enrollment_trust::verify_original_pending_mailbox_sender_route(
                    retained.as_ref(),
                    registration,
                )
                .await
                .expect("Alice independently verifies Bob's pinned setup key");
            let context = aura_core::ContextId::new_from_entropy([84; 32]);
            let mut envelope = aura_core::effects::transport::TransportEnvelope {
                source: issuer.authority_id(),
                destination: issuer.authority_id(),
                context,
                payload: vec![0x44, 0x65, 0x76, 0x69, 0x63, 0x65],
                metadata: std::collections::HashMap::from([
                    (
                        "content-type".into(),
                        "application/aura-choreography".into(),
                    ),
                    ("session-id".into(), session.to_string()),
                    (
                        "aura-destination-device-id".into(),
                        invitee.runtime().device_id().to_string(),
                    ),
                    (
                        "aura-message-id".into(),
                        uuid::Uuid::from_bytes([17; 16]).to_string(),
                    ),
                ]),
                receipt: None,
            };
            envelope.receipt = Some(aura_core::effects::transport::TransportReceipt {
                context,
                src: envelope.source,
                dst: envelope.destination,
                epoch: 1,
                cost: 1,
                nonce: 1,
                prev: [0; 32],
                sig: vec![1],
            });
            issuer_manager
                .send_original_pending_mailbox_envelope(
                    issuer_effects.as_ref(),
                    &route,
                    envelope.clone(),
                )
                .await
                .expect("only sealed original issuer route enqueues exact frame");
            let progress = crate::runtime::services::bootstrap_broker::fetch_pending_progress_witness_for_test(
                broker.public_url(), "first-frame-global-broker-auth",
                admitted.manifest().subject, admitted.manifest().invitee_device,
                admitted.manifest_digest(),
            ).await.expect("authenticated first queue stage");
            assert_eq!(progress["messages"][0]["stage"], "queued");
            assert_eq!(
                progress["messages"][0]["message_id"],
                envelope.metadata["aura-message-id"]
            );
            let retained_record = invitee_effects
                .secure_retrieve(
                    &crate::core::agent::pending_enrollment_mailbox::retained_location(admitted),
                    &[SecureStorageCapability::Read],
                )
                .await
                .expect("Bob's scoped credential is encrypted at rest");
            let credential: crate::core::agent::pending_enrollment_mailbox::RetainedMailboxCredential =
                aura_core::util::serialization::from_slice(&retained_record).unwrap();
            assert!(
                invitee_manager
                    .take_original_pending_mailbox(
                        admitted.manifest().subject,
                        admitted.manifest().invitee_device,
                        admitted.manifest_digest(),
                        "foreign-retrieval-credential",
                    )
                    .await
                    .is_err(),
                "broker rejects a caller without Bob's scoped credential"
            );
            let mut foreign_device = envelope.clone();
            foreign_device.metadata.insert(
                "aura-destination-device-id".into(),
                aura_core::DeviceId::from_bytes([99; 32]).to_string(),
            );
            assert!(
                issuer_manager
                    .send_original_pending_mailbox_envelope(
                        issuer_effects.as_ref(),
                        &route,
                        foreign_device,
                    )
                    .await
                    .is_err(),
                "verified sender route rejects foreign recipient device"
            );
            let first = invitee_manager
                .take_original_pending_mailbox(
                    admitted.manifest().subject,
                    admitted.manifest().invitee_device,
                    admitted.manifest_digest(),
                    &credential.credential,
                )
                .await
                .expect("first authenticated take");
            assert_eq!(first.len(), 1);
            let retried = invitee_manager
                .take_original_pending_mailbox(
                    admitted.manifest().subject,
                    admitted.manifest().invitee_device,
                    admitted.manifest_digest(),
                    &credential.credential,
                )
                .await
                .expect("interrupted take returns the original frame");
            assert_eq!(retried.len(), 1);
            assert_eq!(retried[0].message_id, first[0].message_id);
            assert_eq!(retried[0].envelope.payload, envelope.payload);
            let progress = crate::runtime::services::bootstrap_broker::fetch_pending_progress_witness_for_test(
                broker.public_url(), "first-frame-global-broker-auth",
                admitted.manifest().subject, admitted.manifest().invitee_device,
                admitted.manifest_digest(),
            ).await.expect("authenticated receiver selection stage");
            assert_eq!(
                progress["messages"][0]["stage"],
                "retrieval_selected_for_receiver_credential"
            );
            assert_eq!(progress["messages"][0]["message_id"], first[0].message_id);
            broker
                .interrupt_next_pending_enrollment_ack_for_test()
                .await;
            assert!(
                aura_app::ui::workflows::runtime::run_harness_runtime_maintenance_pass(
                    &app,
                    &invitee_bridge,
                )
                .await
                .is_err(),
                "injected ACK interruption preserves Bob's encrypted frame custody"
            );
            let frame_key = crate::core::agent::pending_enrollment_mailbox::frame_location(
                admitted.manifest_digest(),
                &first[0].message_id,
            );
            assert!(invitee_effects.secure_exists(&frame_key).await.unwrap());
            let sync_error =
                aura_app::ui::workflows::runtime::run_harness_runtime_maintenance_pass(
                    &app,
                    &invitee_bridge,
                )
                .await
                .expect_err("required sibling sync failure remains observable after mailbox ACK");
            assert!(sync_error.to_string().contains("converge sync"));
            let progress = crate::runtime::services::bootstrap_broker::fetch_pending_progress_witness_for_test(
                broker.public_url(), "first-frame-global-broker-auth",
                admitted.manifest().subject, admitted.manifest().invitee_device,
                admitted.manifest_digest(),
            ).await.expect("authenticated exact receiver ACK stage");
            assert_eq!(
                progress["messages"][0]["stage"],
                "receiver_acknowledged_retained_frame"
            );
            assert_eq!(progress["messages"][0]["message_id"], first[0].message_id);
            let queued = invitee_effects
                .take_unclaimed_choreography_envelope_retained(|candidate| {
                    candidate.metadata.get("aura-message-id")
                        == envelope.metadata.get("aura-message-id")
                })
                .expect("actual Bob runtime owns original frame after broker ACK");
            assert_eq!(queued.payload, envelope.payload);
            assert!(
                invitee_effects
                    .take_unclaimed_choreography_envelope_retained(|candidate| candidate
                        .metadata
                        .get("aura-message-id")
                        == envelope.metadata.get("aura-message-id"),)
                    .is_none(),
                "ambiguous ACK does not duplicate the actual runtime frame"
            );
            assert!(
                invitee_manager
                    .take_original_pending_mailbox(
                        admitted.manifest().subject,
                        admitted.manifest().invitee_device,
                        admitted.manifest_digest(),
                        &credential.credential,
                    )
                    .await
                    .unwrap()
                    .is_empty(),
                "ACK follows actual runtime admission"
            );
            broker_time.advance(60_000);
            let expired = crate::runtime::services::bootstrap_broker::fetch_pending_progress_witness_for_test(
                broker.public_url(), "first-frame-global-broker-auth",
                admitted.manifest().subject, admitted.manifest().invitee_device,
                admitted.manifest_digest(),
            ).await.expect("authenticated post-retention observation");
            assert_eq!(expired["registration_present"], false);
            assert_eq!(expired["messages"].as_array().map(Vec::len), Some(0));
            let config = invitee_effects.config().clone();
            let clock = Arc::new(invitee_effects.time_effects().clone());
            let authority = invitee.authority_id();
            let device = invitee.runtime().device_id();
            let message_id = first[0].message_id.clone();
            let _consumed_before_reopen = queued.consume();
            if let Err(source) = invitee
                .runtime()
                .tasks()
                .shutdown_gracefully(std::time::Duration::from_secs(5))
                .await
            {
                assert!(
                    matches!(
                        source,
                        crate::task_registry::TaskSupervisionError::TaskFailed { .. }
                    ),
                    "same-profile shutdown retains concrete required-task failure: {source:?}"
                );
            }
            drop(app);
            drop(invitee_bridge);
            drop(invitee_effects);
            drop(invitee);
            let profile = crate::runtime::builder::TestingOwnedProfileCapability::acquire(&config)
                .expect("same protected Bob profile reopens after process teardown");
            let context = aura_core::context::EffectContext::new(
                authority,
                aura_core::ContextId::new_from_entropy([156; 32]),
                aura_core::effects::ExecutionMode::Testing,
            );
            let reopened_runtime =
                crate::runtime::EffectSystemBuilder::testing_with_owned_profile(profile)
                    .with_authority(authority)
                    .with_config(config)
                    .with_physical_time_provider(clock)
                    .build(&context)
                    .await
                    .expect("actual Bob runtime reopens encrypted original custody");
            assert_eq!(reopened_runtime.device_id(), device);
            let reopened = crate::AuraAgent::new(reopened_runtime, authority);
            assert_eq!(
                reopened
                    .drain_pending_enrollment_vm_mailbox()
                    .await
                    .unwrap(),
                1,
                "same-profile process restart replays encrypted frame after broker ACK"
            );
            let reopened_effects = reopened.runtime().effects();
            let replayed = reopened_effects
                .take_unclaimed_choreography_envelope_retained(|candidate| {
                    candidate.metadata.get("aura-message-id") == Some(&message_id)
                })
                .expect("reopened original VM queue owns exact retained frame");
            assert_eq!(replayed.payload, envelope.payload);
        }
    );

    runtime_admission_test!(
        actual_export_transfer_admission_rejects_missing_and_corrupt_secure_record,
        {
            let (_issuer, invitee, invitation, _start, _accept, _verified) = Box::pin(
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
                    "manifest-admission-record",
                ),
            )
            .await;
            let effects = invitee.runtime().effects();
            let key = location(invitee.authority_id(), &invitation.invitation_id);
            let candidates = load_recoverable_negative_admissions(effects.as_ref())
                .await
                .unwrap();
            assert_eq!(candidates.len(), 1);
            assert_eq!(
                candidates[0].canonical_invitation().invitation_id,
                invitation.invitation_id
            );
            let scopes = invitee
                .pending_enrollment_transport_scopes()
                .await
                .expect("retained original admission authorizes mailbox observation");
            assert_eq!(scopes.len(), 1);
            assert_eq!(scopes[0].subject(), candidates[0].manifest().subject);
            assert_eq!(scopes[0].device(), invitee.runtime().device_id());
            assert_eq!(scopes[0].invitation(), &invitation.invitation_id);
            assert_eq!(scopes[0].manifest_digest(), candidates[0].manifest_digest());
            let original = effects
                .secure_retrieve(&key, &[SecureStorageCapability::Read])
                .await
                .unwrap();
            let foreign =
                SecureStorageLocation::new("enrollment_manifest_admission_v2", "foreign-candidate");
            effects
                .secure_store(&foreign, &original, &[SecureStorageCapability::Write])
                .await
                .unwrap();
            assert!(matches!(
                load_recoverable_negative_admissions(effects.as_ref()).await,
                Err(EnrollmentManifestError::Pin)
            ));
            assert!(
                invitee.pending_enrollment_transport_scopes().await.is_err(),
                "foreign protected admission candidate cannot mint a poll scope"
            );
            assert!(effects
                .fault_remove_secure_record_for_test(&foreign)
                .await
                .unwrap());
            assert!(effects
                .fault_remove_secure_record_for_test(&key)
                .await
                .unwrap());
            assert!(matches!(
                load_admitted_baseline(effects.as_ref(), invitee.authority_id(), &invitation).await,
                Err(EnrollmentManifestError::MissingPin)
            ));
            effects
                .secure_store(&key, b"corrupt", &[SecureStorageCapability::Write])
                .await
                .unwrap();
            assert!(matches!(
                load_admitted_baseline(effects.as_ref(), invitee.authority_id(), &invitation).await,
                Err(EnrollmentManifestError::Runtime(_))
            ));
        }
    );

    runtime_admission_test!(
        actual_transfer_rejects_substituted_independent_issuer_identity_before_mutation,
        {
            let (_issuer, invitee, _invitation, start, _accept, _verified) = Box::pin(
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
                    "manifest-independent-pin",
                ),
            )
            .await;
            let transfer = start.manifest_transfer.unwrap();
            let statement =
                aura_invitation::enrollment_manifest::decode_initiator_verifier_transfer(
                    &transfer.initiator_verifier_code,
                )
                .unwrap();
            let substituted =
                aura_invitation::enrollment_manifest::encode_initiator_verifier_transfer(
                    AuthorityId::new_from_entropy([201; 32]),
                    statement.initiator_device,
                    &statement.verifying_key,
                )
                .unwrap();
            let before = invitee.runtime().effects().export_tree_ops().await.unwrap();
            let result = AgentRuntimeBridge::new(invitee.clone())
                .verify_enrollment_manifest_transfer(transfer.manifest_code.clone(), substituted)
                .await;
            assert!(matches!(result, Err(EnrollmentManifestError::Pin)));
            let after = invitee.runtime().effects().export_tree_ops().await.unwrap();
            assert_eq!(
                aura_core::util::serialization::to_vec(&before).unwrap(),
                aura_core::util::serialization::to_vec(&after).unwrap()
            );
        }
    );
}

/// Native verified negative evidence prepared after actual VM disposal. It
/// precedes the Closed checkpoint and never grants positive execution authority.
pub(crate) struct PreparedRecoveredEnrollmentFailure {
    admitted: std::sync::Arc<AdmittedEnrollmentManifest>,
    effects: std::sync::Weak<AuraEffectSystem>,
    proof: std::sync::Arc<super::enrollment_vm_admission::VerifiedEnrollmentFailureCapability>,
}
impl PreparedRecoveredEnrollmentFailure {
    pub(crate) fn require_original(
        &self,
        admitted: &AdmittedEnrollmentManifest,
        effects: &AuraEffectSystem,
    ) -> Result<(), aura_core::AuraError> {
        if !std::ptr::eq(self.admitted.as_ref(), admitted)
            || !self
                .effects
                .upgrade()
                .is_some_and(|actual| std::ptr::eq(actual.as_ref(), effects))
        {
            return Err(aura_core::AuraError::permission_denied(
                "foreign prepared original negative receipt",
            ));
        }
        Ok(())
    }
    pub(crate) fn proof(
        &self,
    ) -> &super::enrollment_vm_admission::VerifiedEnrollmentFailureCapability {
        &self.proof
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreparedRecoveredFailureRecord {
    version: u16,
    provisional: AuthorityId,
    device: DeviceId,
    manifest_digest: [u8; 32],
    frame: super::enrollment_vm_admission::EnrollmentControlFrame,
    observed_at_ms: u64,
}
fn prepared_recovered_failure_location(
    admitted: &AdmittedEnrollmentManifest,
) -> SecureStorageLocation {
    SecureStorageLocation::with_sub_key(
        "device_enrollment_prepared_negative_v1",
        admitted.manifest().invitee_authority.to_string(),
        admitted.manifest().ceremony.to_string(),
    )
}
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "RecoveredEnrollmentNoticeVmDisposal",
    family = "runtime_helper"
)]
pub(crate) async fn prepare_recovered_enrollment_failure(
    effects: &std::sync::Arc<AuraEffectSystem>,
    admitted: &std::sync::Arc<AdmittedEnrollmentManifest>,
    proof: std::sync::Arc<super::enrollment_vm_admission::VerifiedEnrollmentFailureCapability>,
    disposal: &super::enrollment_terminal_notice::RecoveredEnrollmentNoticeVmDisposal,
) -> Result<PreparedRecoveredEnrollmentFailure, aura_core::AuraError> {
    disposal.require_original(admitted.as_ref(), effects.as_ref())?;
    if proof.manifest_digest() != admitted.manifest_digest()
        || aura_core::util::serialization::to_vec(proof.canonical_invitation())
            .map_err(aura_core::AuraError::from)?
            != aura_core::util::serialization::to_vec(admitted.canonical_invitation())
                .map_err(aura_core::AuraError::from)?
    {
        return Err(aura_core::AuraError::permission_denied(
            "foreign prepared failure evidence",
        ));
    }
    let record = PreparedRecoveredFailureRecord {
        version: 1,
        provisional: admitted.manifest().invitee_authority,
        device: effects.device_id(),
        manifest_digest: proof.manifest_digest(),
        frame: proof.frame().clone(),
        observed_at_ms: proof.observed_at_ms(),
    };
    let bytes =
        aura_core::util::serialization::to_vec(&record).map_err(aura_core::AuraError::from)?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(aura_core::AuraError::invalid(
            "prepared negative receipt oversized",
        ));
    }
    let key = prepared_recovered_failure_location(admitted);
    effects
        .secure_store(&key, &bytes, &[SecureStorageCapability::Write])
        .await?;
    if effects
        .secure_retrieve(&key, &[SecureStorageCapability::Read])
        .await?
        != bytes
    {
        return Err(aura_core::AuraError::permission_denied(
            "prepared negative receipt readback mismatch",
        ));
    }
    Ok(PreparedRecoveredEnrollmentFailure {
        admitted: admitted.clone(),
        effects: std::sync::Arc::downgrade(effects),
        proof,
    })
}
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "AdmittedEnrollmentManifest",
    family = "runtime_helper"
)]
pub(crate) async fn load_prepared_recovered_enrollment_failure(
    effects: &std::sync::Arc<AuraEffectSystem>,
    admitted: &std::sync::Arc<AdmittedEnrollmentManifest>,
) -> Result<Option<PreparedRecoveredEnrollmentFailure>, aura_core::AuraError> {
    let key = prepared_recovered_failure_location(admitted);
    if !effects.secure_exists(&key).await? {
        return Ok(None);
    }
    let bytes = effects
        .secure_retrieve(&key, &[SecureStorageCapability::Read])
        .await?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(aura_core::AuraError::invalid(
            "prepared negative receipt oversized",
        ));
    }
    let record: PreparedRecoveredFailureRecord =
        aura_core::util::serialization::from_slice(&bytes).map_err(aura_core::AuraError::from)?;
    let now = effects
        .physical_time()
        .await
        .map_err(aura_core::AuraError::from)?;
    if record.version != 1
        || record.provisional != admitted.manifest().invitee_authority
        || record.device != effects.device_id()
        || record.manifest_digest != admitted.manifest_digest()
        || record.observed_at_ms < admitted.admitted_at_ms()
        || record.observed_at_ms >= admitted.manifest().expires_at_ms
        || record.observed_at_ms > now.ts_ms
    {
        return Err(aura_core::AuraError::permission_denied(
            "prepared negative receipt original binding mismatch",
        ));
    }
    let proof = record
        .frame
        .verify_failure_at(effects.as_ref(), admitted.as_ref(), record.observed_at_ms)
        .await
        .map_err(|source| aura_core::AuraError::Internal {
            message: "verify original prepared negative receipt".into(),
            source: Some(std::sync::Arc::new(source)),
        })?;
    Ok(Some(PreparedRecoveredEnrollmentFailure {
        admitted: admitted.clone(),
        effects: std::sync::Arc::downgrade(effects),
        proof: std::sync::Arc::new(proof),
    }))
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "PreparedRecoveredEnrollmentFailure",
    family = "runtime_helper"
)]
pub(crate) async fn persist_prepared_recovered_enrollment_failure(
    effects: &AuraEffectSystem,
    prepared: &PreparedRecoveredEnrollmentFailure,
    acknowledged: &crate::runtime::services::enrollment_window::AcknowledgedEnrollmentWindow,
) -> Result<(), aura_core::AuraError> {
    prepared.require_original(prepared.admitted.as_ref(), effects)?;
    persist_verified_failure(effects, prepared.proof(), acknowledged)
        .await
        .map_err(|source| aura_core::AuraError::Internal {
            message: "persist original recovered negative receipt".into(),
            source: Some(std::sync::Arc::new(source)),
        })
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod recovered_notice_refusal_tests {
    use super::*;
    use crate::runtime::services::enrollment_window::RecoveredEnrollmentNoticeWindowCapability as Window;
    use std::sync::Arc;

    async fn actual_reopened_receiver(
        phase: &str,
    ) -> (
        Arc<crate::AuraAgent>,
        Arc<AdmittedEnrollmentManifest>,
        Arc<aura_testkit::time::ManualPhysicalClock>,
    ) {
        let boundary = crate::runtime_bridge::tests::actual_enrollment_crash_boundary(phase);
        let (crate::runtime_bridge::tests::EnrollmentCrashDetail::CancelledNotice {
            invitee_authority,
            invitee_context,
            invitee_config,
            ..
        }
        | crate::runtime_bridge::tests::EnrollmentCrashDetail::RecoveredNotice {
            invitee_authority,
            invitee_context,
            invitee_config,
            ..
        }) = boundary.detail
        else {
            panic!("actual original cancellation crash")
        };
        let clock = Arc::new(aura_testkit::time::ManualPhysicalClock::new(5_000));
        let profile =
            crate::runtime::builder::TestingOwnedProfileCapability::acquire(&invitee_config)
                .expect("actual original recovery fixture operation");
        let runtime = crate::runtime::EffectSystemBuilder::testing_with_owned_profile(profile)
            .with_authority(invitee_authority)
            .with_config(invitee_config)
            .with_shared_transport(crate::SharedTransport::new())
            .with_physical_time_provider(clock.clone())
            .build(&crate::runtime::EffectContext::new(
                invitee_authority,
                invitee_context,
                aura_core::effects::ExecutionMode::Testing,
            ))
            .await
            .expect("actual original recovery fixture operation");
        let agent = Arc::new(crate::AuraAgent::new(runtime, invitee_authority));
        let mut admitted = load_recoverable_negative_admissions(agent.runtime().effects().as_ref())
            .await
            .expect("actual original recovery fixture operation");
        assert_eq!(admitted.len(), 1);
        (
            agent,
            Arc::new(
                admitted
                    .pop()
                    .expect("actual original recovery fixture operation"),
            ),
            clock,
        )
    }

    async fn close_actual_receiver(agent: &crate::AuraAgent) {
        agent
            .runtime()
            .tasks()
            .shutdown_gracefully(std::time::Duration::from_secs(5))
            .await
            .expect("refused recovery leaves no unresolved notice task");
    }

    fn actual_notice_tasks(agent: &crate::AuraAgent) -> Vec<String> {
        let mut tasks: Vec<_> = agent
            .runtime()
            .tasks()
            .active_tasks()
            .into_iter()
            .filter(|name| {
                name.contains("recovered-negative-notice")
                    || name.contains("recovered_negative_notice")
            })
            .collect();
        tasks.sort();
        tasks
    }

    #[test]
    fn recovered_negative_notice_missing_or_foreign_original_anchor_refuses() {
        crate::handlers::invitation::tests::run_async_test_on_large_stack(async {
            let (agent, admitted, _) = Box::pin(actual_reopened_receiver("cancelled-notice")).await;
            let effects = agent.runtime().effects();
            let key = Window::original_admitted_anchor_location_for_test(&admitted);
            let original = effects
                .secure_retrieve(&key, &[SecureStorageCapability::Read])
                .await
                .expect("actual original recovery fixture operation");
            let tasks_before = actual_notice_tasks(&agent);
            assert!(effects
                .fault_remove_secure_record_for_test(&key)
                .await
                .expect("actual original recovery fixture operation"));
            let missing = Window::recover(effects.clone(), admitted.clone())
                .await
                .err()
                .expect("missing original anchor refuses");
            let mut source: Option<&(dyn std::error::Error + 'static)> = Some(&missing);
            let mut native = false;
            while let Some(actual) = source {
                native |= actual.is::<aura_core::effects::SecureStorageError>();
                source = actual.source();
            }
            assert!(
                native,
                "missing protected anchor retains concrete storage cause: {missing:?}"
            );
            assert!(
                !effects
                    .secure_exists(&key)
                    .await
                    .expect("actual original recovery fixture operation"),
                "refusal cannot repair missing anchor"
            );
            assert_eq!(actual_notice_tasks(&agent), tasks_before);

            let (other, other_admitted, _) =
                Box::pin(actual_reopened_receiver("cancelled-notice-foreign")).await;
            let other_key = Window::original_admitted_anchor_location_for_test(&other_admitted);
            let foreign = other
                .runtime()
                .effects()
                .secure_retrieve(&other_key, &[SecureStorageCapability::Read])
                .await
                .expect("actual original recovery fixture operation");
            assert_ne!(
                foreign, original,
                "independent genuine issuance has distinct original anchor"
            );
            effects
                .secure_store(&key, &foreign, &[SecureStorageCapability::Write])
                .await
                .expect("actual original recovery fixture operation");
            let refused = Window::recover(effects.clone(), admitted)
                .await
                .err()
                .expect("foreign original scope refuses");
            let mut source: Option<&(dyn std::error::Error + 'static)> = Some(&refused);
            let mut discontinuity = false;
            while let Some(actual) = source {
                discontinuity |= matches!(
                    actual.downcast_ref::<aura_core::TimeoutBudgetError>(),
                    Some(aura_core::TimeoutBudgetError::CheckpointDiscontinuity { .. })
                );
                source = actual.source();
            }
            assert!(
                discontinuity,
                "actual foreign binding is retained: {refused:?}"
            );
            assert_eq!(
                effects
                    .secure_retrieve(&key, &[SecureStorageCapability::Read])
                    .await
                    .expect("actual original recovery fixture operation"),
                foreign,
                "refusal cannot repair foreign anchor"
            );
            assert_eq!(actual_notice_tasks(&agent), tasks_before);
            close_actual_receiver(&agent).await;
            close_actual_receiver(&other).await;
        });
    }

    #[test]
    fn recovered_negative_notice_closed_without_prepared_failure_refuses() {
        crate::handlers::invitation::tests::run_async_test_on_large_stack(async {
            let (agent, admitted, clock) =
                Box::pin(actual_reopened_receiver("prepared-negative-after-close")).await;
            let effects = agent.runtime().effects();
            let checkpoint = Window::original_admitted_checkpoint_location_for_test(&admitted);
            let closed = effects
                .secure_retrieve(&checkpoint, &[SecureStorageCapability::Read])
                .await
                .expect("actual original recovery fixture operation");
            let prepared = prepared_recovered_failure_location(&admitted);
            assert!(
                effects
                    .secure_exists(&prepared)
                    .await
                    .expect("actual original recovery fixture operation"),
                "actual crash follows prepared receipt and Closed readback"
            );
            let failed = failure_location(
                admitted.manifest().invitee_authority,
                &admitted.manifest().ceremony,
            );
            assert!(
                !effects
                    .secure_exists(&failed)
                    .await
                    .expect("actual original recovery fixture operation"),
                "worker stopped before final failure persistence"
            );
            let tasks_before = actual_notice_tasks(&agent);
            assert!(effects
                .fault_remove_secure_record_for_test(&prepared)
                .await
                .expect("actual original recovery fixture operation"));
            let refused = Window::recover(effects.clone(), admitted)
                .await
                .err()
                .expect("Closed without actual prepared negative evidence refuses");
            Window::assert_recovered_notice_missing_checkpoint_for_test(&refused);
            assert_eq!(
                effects
                    .secure_retrieve(&checkpoint, &[SecureStorageCapability::Read])
                    .await
                    .expect("actual original recovery fixture operation"),
                closed,
                "refusal preserves actual original Closed checkpoint"
            );
            assert!(
                !effects
                    .secure_exists(&prepared)
                    .await
                    .expect("actual original recovery fixture operation"),
                "missing preparation is not repaired"
            );
            assert!(
                !effects
                    .secure_exists(&failed)
                    .await
                    .expect("actual original recovery fixture operation"),
                "refusal cannot publish failure from Closed alone"
            );
            assert_eq!(actual_notice_tasks(&agent), tasks_before);
            assert_eq!(
                clock
                    .physical_time()
                    .await
                    .expect("actual original recovery fixture operation")
                    .ts_ms,
                5_000
            );
            close_actual_receiver(&agent).await;
        });
    }

    #[test]
    fn recovered_negative_notice_selected_provider_failure_retains_native_source() {
        crate::handlers::invitation::tests::run_async_test_on_large_stack(async {
            let (agent, admitted, clock) =
                Box::pin(actual_reopened_receiver("cancelled-notice")).await;
            let effects = agent.runtime().effects();
            let key = Window::original_admitted_anchor_location_for_test(&admitted);
            let original = effects
                .secure_retrieve(&key, &[SecureStorageCapability::Read])
                .await
                .expect("actual original recovery fixture operation");
            let tasks_before = actual_notice_tasks(&agent);
            clock
                .fail_next_observation(aura_core::effects::TimeError::OperationFailed {
                    reason: "original recovered receiver clock fault".into(),
                })
                .await;
            let refused = Window::recover(effects.clone(), admitted.clone())
                .await
                .err()
                .expect("selected original clock failure refuses");
            let mut source: Option<&(dyn std::error::Error + 'static)> = Some(&refused);
            let mut native = false;
            while let Some(actual) = source {
                native |= matches!(actual.downcast_ref::<aura_core::effects::TimeError>(), Some(aura_core::effects::TimeError::OperationFailed { reason }) if reason == "original recovered receiver clock fault");
                source = actual.source();
            }
            assert!(
                native,
                "actual selected provider cause retained: {refused:?}"
            );
            assert_eq!(
                effects
                    .secure_retrieve(&key, &[SecureStorageCapability::Read])
                    .await
                    .expect("actual original recovery fixture operation"),
                original
            );
            assert_eq!(actual_notice_tasks(&agent), tasks_before);
            assert_eq!(
                clock
                    .physical_time()
                    .await
                    .expect("actual original recovery fixture operation")
                    .ts_ms,
                5_000
            );
            let recovered = Window::recover(effects, admitted)
                .await
                .expect("same protected endpoint remains recoverable after provider fault");
            drop(recovered);
            close_actual_receiver(&agent).await;
        });
    }
}
