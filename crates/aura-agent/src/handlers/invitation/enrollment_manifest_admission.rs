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
    SecureStorageLocation::with_sub_key(
        "enrollment_manifest_admission_v1",
        authority.to_string(),
        id.to_string(),
    )
}
fn boundary(error: impl std::error::Error + Send + Sync + 'static) -> EnrollmentManifestError {
    EnrollmentManifestError::Runtime(Box::new(error))
}

/// Admit only the opaque explicit app transfer selection. Discovery and raw
/// cached invitations cannot call this API with a manufactured pin.
pub(crate) async fn admit_user_transfer(
    effects: &AuraEffectSystem,
    provisional: AuthorityId,
    code: &str,
    pin: &aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentManifest,
) -> Result<AdmittedEnrollmentManifest, EnrollmentManifestError> {
    let _admission_owner = effects.lock_enrollment_manifest_admission().await;
    let now = effects.physical_time().await?.ts_ms;
    let record = AdmissionRecord {
        version: 1,
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
    let key = location(provisional, &record.invitation);
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
        let old = effects
            .secure_retrieve(&key, &caps)
            .await
            .map_err(boundary)?;
        if old.len() > MAX_RECORD_BYTES {
            return Err(EnrollmentManifestError::Shape);
        }
        let previous: AdmissionRecord =
            aura_core::util::serialization::from_slice(&old).map_err(boundary)?;
        // Preserve original owned admission time on an idempotent transfer.
        if previous.provisional != record.provisional
            || previous.device != record.device
            || previous.invitation != record.invitation
            || previous.code != record.code
            || previous.manifest_code != record.manifest_code
            || previous.selected_verifier != record.selected_verifier
        {
            #[cfg(test)]
            eprintln!("enrollment admission Pin branch at {}:{}", file!(), line!());
            return Err(EnrollmentManifestError::Pin);
        }
        let admitted = validate(effects, &previous, now).await?;
        crate::runtime::services::enrollment_window::EnrollmentWindowCapability::require_retained_admitted_window(effects, &admitted).await.map_err(boundary)?;
        retain_import_generation_owner(effects, &admitted).await?;
        return Ok(admitted);
    }
    crate::runtime::services::enrollment_window::EnrollmentWindowCapability::retain_new_admitted_window(
        effects,
        NewEnrollmentAdmissionCapability { witness: &admitted },
    )
    .await
    .map_err(boundary)?;
    // Created means the provider durably published complete encrypted bytes
    // without replacement. AlreadyExists above requires the original pinned
    // record's exact binding and cryptographic revalidation, never existence.
    retain_import_generation_owner(effects, &admitted).await?;
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
    if record.version != 1
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
        || *invitee_authority != Some(record.provisional)
        || manifest.invitee_authority != record.provisional
        || *device_id != record.device
        || manifest.invitee_device != record.device
        || *initiator_device_id != manifest.initiator_device
        || *ceremony_id != manifest.ceremony
        || shareable.invitation_id != manifest.invitation
        || shareable.invitation_id != record.invitation
        || *pending_epoch != manifest.pending_epoch
        || setup_binding.as_ref() != Some(&manifest.setup)
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
    family = "runtime_helper"
)]
pub(super) async fn retain_verified_failure(
    effects: &AuraEffectSystem,
    failure: super::enrollment_vm_admission::VerifiedEnrollmentFailureCapability,
    acknowledged: crate::runtime::services::enrollment_window::AcknowledgedEnrollmentWindow,
) -> Result<DurableFailedEnrollmentCapability, EnrollmentManifestError> {
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
        return Ok(DurableFailedEnrollmentCapability { evidence: failure });
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
    Ok(retained)
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
    crate::runtime::services::enrollment_window::EnrollmentWindowCapability::verify_retained_failure_clock(effects, &retained).await.map_err(boundary)?;
    let evidence = super::enrollment_vm_admission::verify_retained_failure(effects, retained)
        .await
        .map_err(boundary)?;
    Ok(Some(DurableFailedEnrollmentCapability { evidence }))
}

/// A raw invitation selects the independently retained admission. Absence alone
/// may select a different invitation family; failed reads never become absence.
pub(crate) async fn load_admitted_enrollment_for_id(
    effects: &AuraEffectSystem,
    provisional: AuthorityId,
    id: &InvitationId,
) -> Result<Option<AdmittedEnrollmentManifest>, EnrollmentManifestError> {
    let key = location(provisional, id);
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
    let record: AdmissionRecord =
        aura_core::util::serialization::from_slice(&bytes).map_err(boundary)?;
    if record.provisional != provisional || record.invitation != *id {
        return Err(EnrollmentManifestError::Pin);
    }
    let now = effects.physical_time().await?.ts_ms;
    validate(effects, &record, now).await.map(Some)
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
    crate::runtime::services::enrollment_window::EnrollmentWindowCapability::verify_retained_confirmation_clock(effects,&retained)
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
            let duplicate = admit_user_transfer(
                effects.as_ref(),
                invitee.authority_id(),
                &start.enrollment_code,
                &pin,
            )
            .await
            .unwrap();
            assert_eq!(
                duplicate.canonical_invitation().invitation_id,
                invitation.invitation_id
            );
            assert_eq!(
                effects.secure_retrieve(&key, &read).await.unwrap(),
                original,
                "idempotent transfer cannot replace original admission time or pinned evidence"
            );
            // Explicit test corruption models damaged existing ciphertext plaintext;
            // production admission has no replacement authority.
            effects
                .secure_store(&key, b"corrupt", &[SecureStorageCapability::Write])
                .await
                .unwrap();
            assert!(matches!(
                admit_user_transfer(
                    effects.as_ref(),
                    invitee.authority_id(),
                    &start.enrollment_code,
                    &pin
                )
                .await,
                Err(EnrollmentManifestError::Runtime(_))
            ));
            assert_eq!(
                effects.secure_retrieve(&key, &read).await.unwrap(),
                b"corrupt"
            );
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
            effects
                .secure_delete(&key, &[SecureStorageCapability::Delete])
                .await
                .unwrap();
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
