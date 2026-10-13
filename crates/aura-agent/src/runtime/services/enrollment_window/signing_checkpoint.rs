//! Private signing clock allocation. A previous anchor is a consumed allocation,
//! never permission to mint a refreshed original window after restart.
use super::*;
use aura_core::effects::secure::ImmutableSecureStoreOutcome;
use aura_signature::SecurityTranscript;
use serde::{Deserialize, Serialize};

const ORIGINAL_SIGNING_POLICY: Duration = Duration::from_secs(30);
const MAX_RECORD_BYTES: usize = 16_384;

fn continues_original_shortcut(left: &TimeoutBudget, original: &TimeoutBudget) -> bool {
    left.validate_checkpoint_continuation_from(original).is_ok()
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct SigningBinding {
    subject: aura_core::AuthorityId,
    device: aura_core::DeviceId,
    coordinator: aura_core::DeviceId,
    ceremony: aura_core::CeremonyId,
    invitation: aura_core::InvitationId,
    domain: String,
    transcript_digest: [u8; 32],
    approved_intent_digest: [u8; 32],
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SigningClockRecord {
    negative_cleanup_budget: TimeoutBudget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shortcut_budget: Option<TimeoutBudget>,
    original_policy: OriginalEnrollmentWindowPolicy,
    version: u16,
    binding: SigningBinding,
    budget: TimeoutBudget,
    execution: DurableEnrollmentExecutionState,
}

#[derive(Debug, thiserror::Error)]
enum SigningWindowError {
    #[error("original signing approval allocation already exists or was retired")]
    AlreadyAllocated,
    #[error("original signing clock binding or protected record changed")]
    Binding,
    #[error("original signing clock record exceeds bounds")]
    Bounds,
}
fn rejected(source: SigningWindowError) -> AuraError {
    AuraError::PermissionDenied {
        message: "original local signing clock admission refused".into(),
        source: Some(Arc::new(source)),
    }
}

pub(super) struct ApprovedSigningCheckpoint {
    negative_cleanup_budget: TimeoutBudget,
    shortcut_budget: TimeoutBudget,
    original_policy: OriginalEnrollmentWindowPolicy,
    effects: Arc<AuraEffectSystem>,
    binding: SigningBinding,
    allocation_key: String,
    anchor: Vec<u8>,
    writes: Mutex<()>,
}

// Distinct from a checkpoint observation: owns actual domain completion
// and the original Closed write/readback. No serialization or cloning.
pub(super) struct OriginalSigningTerminalReceipt {
    owner: Arc<ApprovedSigningCheckpoint>,
    frozen: Vec<u8>,
}

pub(super) struct OriginalSigningNegativeCleanupReceipt {
    owner: Arc<ApprovedSigningCheckpoint>,
    frozen: Vec<u8>,
    // Observed first durable terminal decision; never execution authority.
    retained_decision: DurableEnrollmentExecutionState,
}

impl OriginalSigningTerminalReceipt {
    pub(super) fn require_original(
        &self,
        owner: &Arc<ApprovedSigningCheckpoint>,
    ) -> Result<(), AuraError> {
        if !Arc::ptr_eq(&self.owner, owner) {
            return Err(rejected(SigningWindowError::Binding));
        }
        let record: SigningClockRecord =
            serde_json::from_slice(&self.frozen).map_err(|source| AuraError::Serialization {
                message: "decode retained original Closed signing receipt".into(),
                source: Some(Arc::new(source)),
            })?;
        if record.original_policy != owner.original_policy
            || !record.shortcut_budget.as_ref().is_some_and(|shortcut| {
                continues_original_shortcut(shortcut, &owner.shortcut_budget)
            })
        {
            return Err(rejected(SigningWindowError::Binding));
        }
        let anchor: SigningClockRecord =
            serde_json::from_slice(&owner.anchor).map_err(|source| AuraError::Serialization {
                message: "decode original Closed signing anchor".into(),
                source: Some(Arc::new(source)),
            })?;
        ApprovedSigningCheckpoint::validate_record_phase(
            &owner.binding,
            &self.frozen,
            &anchor.budget,
            DurableEnrollmentExecutionState::Closed,
        )
    }
}

impl OriginalSigningNegativeCleanupReceipt {
    pub(super) fn require_original(
        &self,
        owner: &Arc<ApprovedSigningCheckpoint>,
    ) -> Result<(), AuraError> {
        if !Arc::ptr_eq(&self.owner, owner) {
            return Err(rejected(SigningWindowError::Binding));
        }
        let record: SigningClockRecord =
            serde_json::from_slice(&self.frozen).map_err(|source| AuraError::Serialization {
                message: "decode actual negative signing receipt".into(),
                source: Some(Arc::new(source)),
            })?;
        if record.original_policy != owner.original_policy
            || !record.shortcut_budget.as_ref().is_some_and(|shortcut| {
                continues_original_shortcut(shortcut, &owner.shortcut_budget)
            })
            || record.execution != self.retained_decision
            || !matches!(
                record.execution,
                DurableEnrollmentExecutionState::Closed | DurableEnrollmentExecutionState::Refused
            )
        {
            return Err(rejected(SigningWindowError::Binding));
        }
        let anchor: SigningClockRecord =
            serde_json::from_slice(&owner.anchor).map_err(|source| AuraError::Serialization {
                message: "decode original negative signing anchor".into(),
                source: Some(Arc::new(source)),
            })?;
        ApprovedSigningCheckpoint::validate_record_phase(
            &owner.binding,
            &self.frozen,
            &anchor.budget,
            self.retained_decision,
        )
    }
    pub(super) fn retained_decision(&self) -> DurableEnrollmentExecutionState {
        self.retained_decision
    }
}

impl ApprovedSigningCheckpoint {
    pub(super) async fn require_pending(&self, original: &TimeoutBudget) -> Result<(), AuraError> {
        for namespace in [
            "enrollment_signing_clock_anchor_v1",
            "enrollment_signing_clock_live_v1",
        ] {
            let bytes = self
                .effects
                .secure_retrieve(&self.location(namespace), &[SecureStorageCapability::Read])
                .await?;
            if bytes != self.anchor {
                return Err(rejected(SigningWindowError::Binding));
            }
        }
        let bytes = self
            .effects
            .secure_retrieve(
                &self.location("enrollment_signing_clock_checkpoint_v1"),
                &[SecureStorageCapability::Read],
            )
            .await?;
        Self::validate_record(&self.binding, &bytes, original)?;
        let record: SigningClockRecord =
            serde_json::from_slice(&bytes).map_err(|source| AuraError::Serialization {
                message: "decode original pending signing checkpoint".into(),
                source: Some(Arc::new(source)),
            })?;
        if record.original_policy != self.original_policy
            || !record.shortcut_budget.as_ref().is_some_and(|shortcut| {
                continues_original_shortcut(shortcut, &self.shortcut_budget)
            })
        {
            return Err(rejected(SigningWindowError::Binding));
        }
        Ok(())
    }
    pub(super) fn require_approved_manifest(
        &self,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
    ) -> Result<(), AuraError> {
        self.require_effects(approval.effects().as_ref())?;
        if self.binding.approved_intent_digest != approval.canonical_intent_digest() {
            return Err(rejected(SigningWindowError::Binding));
        }
        Ok(())
    }
    pub(super) fn require_effects(&self, effects: &AuraEffectSystem) -> Result<(), AuraError> {
        if !std::ptr::eq(self.effects.as_ref(), effects) {
            return Err(rejected(SigningWindowError::Binding));
        }
        Ok(())
    }
    fn location(&self, namespace: &str) -> SecureStorageLocation {
        SecureStorageLocation::new(namespace, self.allocation_key.clone())
    }

    pub(super) async fn allocate(
        effects: Arc<AuraEffectSystem>,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
    ) -> Result<(Self, TimeoutBudget, Arc<OwnedSemaphorePermit>), AuraError> {
        if !Arc::ptr_eq(&effects, approval.effects())
            || aura_guards::GuardContextProvider::authority_id(effects.as_ref())
                != approval.manifest().subject
        {
            return Err(rejected(SigningWindowError::Binding));
        }
        // Consent grants only a bounded local clock allocation. No signing or
        // membership authority is inferred here. Actual native tree/material
        // admission occurs under this window before the grant can be minted.
        let manifest = approval.manifest();
        let message = manifest.transcript_bytes().map_err(|source| {
            AuraError::crypto_with_source("bind original approved signing clock", Arc::new(source))
        })?;
        let transcript_digest = aura_core::hash::hash(&message);
        let binding = SigningBinding {
            subject: manifest.subject,
            device: effects.device_id(),
            coordinator: manifest.initiator_device,
            ceremony: manifest.ceremony.clone(),
            invitation: manifest.invitation.clone(),
            domain: <aura_invitation::enrollment_manifest::EnrollmentTrustManifest as SecurityTranscript>::DOMAIN_SEPARATOR.into(),
            transcript_digest,
            approved_intent_digest: approval.canonical_intent_digest(),
        };
        // Match the existing private approval retirement domain exactly. A
        // retirement from a prior actor cannot be bypassed by another consent.
        let retirement_digest = aura_core::hash::hash(&aura_core::util::serialization::to_vec(&(
            "aura.enrollment.manifest-approval-retirement.v1",
            manifest.subject,
            &manifest.ceremony,
            &manifest.invitation,
            transcript_digest,
        ))?);
        let retirement = SecureStorageLocation::with_sub_key(
            "enrollment_signing_approval_retirement",
            manifest.subject.to_string(),
            hex::encode(retirement_digest),
        );
        if effects.secure_exists(&retirement).await? {
            return Err(rejected(SigningWindowError::AlreadyAllocated));
        }
        let allocation_key = hex::encode(aura_core::hash::hash(
            &aura_core::util::serialization::to_vec(&binding)?,
        ));
        let anchor_location = SecureStorageLocation::new(
            "enrollment_signing_clock_anchor_v1",
            allocation_key.clone(),
        );
        // Refuse any existing allocation BEFORE a new physical read. Atomic
        // creation below independently prevents concurrent first admissions.
        if effects.secure_exists(&anchor_location).await? {
            return Err(rejected(SigningWindowError::AlreadyAllocated));
        }
        let now = effects.physical_time().await?;
        let shortcut_policy = OriginalShortcutWindowPolicy::bounded_extension(
            ORIGINAL_SIGNING_POLICY,
            ORIGINAL_SIGNING_POLICY,
        )?;
        let birth = OriginalEnrollmentWindowBirth::allocate(
            &now,
            ORIGINAL_SIGNING_POLICY,
            shortcut_policy,
        )?;
        let budget = birth.execution;
        let shortcut_budget = birth.shortcut;
        let negative_cleanup_budget = birth.negative_cleanup;
        let original_policy = birth.policy;
        let anchor = serde_json::to_vec(&SigningClockRecord {
            negative_cleanup_budget: negative_cleanup_budget.clone(),
            shortcut_budget: Some(shortcut_budget.clone()),
            original_policy: original_policy.clone(),
            version: 4,
            binding: binding.clone(),
            budget: budget.clone(),
            execution: DurableEnrollmentExecutionState::Pending,
        })
        .map_err(|source| AuraError::Serialization {
            message: "encode original local signing clock".into(),
            source: Some(Arc::new(source)),
        })?;
        let publication_effects = Arc::clone(&effects);
        let owner = aura_core::time::timeout::acknowledge_initial_publication_with_timeout_budget(
            effects.as_ref(),
            &budget,
            || async move {
                Self::publish_original_birth(
                    publication_effects,
                    binding,
                    allocation_key,
                    anchor,
                    original_policy,
                    negative_cleanup_budget,
                    shortcut_budget,
                )
                .await
            },
        )
        .await
        .map_err(|source| match source {
            TimeoutRunError::Operation(source) => source,
            TimeoutRunError::Timeout(source) => AuraError::from(source),
        })?;
        let lease = Arc::new(tokio::sync::Semaphore::new(1))
            .try_acquire_owned()
            .map_err(|source| AuraError::Internal {
                message: "retain new original signing clock lease".into(),
                source: Some(Arc::new(source)),
            })?;
        Ok((owner, budget, Arc::new(lease)))
    }

    // Only this actual fresh protected publication can create the retained
    // signing owner. All records are acknowledged before the first validation
    // read in the canonical initial-publication boundary.
    async fn publish_original_birth(
        effects: Arc<AuraEffectSystem>,
        binding: SigningBinding,
        allocation_key: String,
        anchor: Vec<u8>,
        original_policy: OriginalEnrollmentWindowPolicy,
        negative_cleanup_budget: TimeoutBudget,
        shortcut_budget: TimeoutBudget,
    ) -> Result<Self, AuraError> {
        let permissions = [
            SecureStorageCapability::Read,
            SecureStorageCapability::Write,
        ];
        let anchor_location = SecureStorageLocation::new(
            "enrollment_signing_clock_anchor_v1",
            allocation_key.clone(),
        );
        if effects
            .secure_store_immutable(&anchor_location, &anchor, &permissions)
            .await?
            != ImmutableSecureStoreOutcome::Created
        {
            return Err(rejected(SigningWindowError::AlreadyAllocated));
        }
        if effects
            .secure_retrieve(&anchor_location, &[SecureStorageCapability::Read])
            .await?
            != anchor
        {
            return Err(rejected(SigningWindowError::Binding));
        }
        let checkpoint_location = SecureStorageLocation::new(
            "enrollment_signing_clock_checkpoint_v1",
            allocation_key.clone(),
        );
        if effects
            .secure_create_mutable(&checkpoint_location, &anchor, &permissions)
            .await?
            != ImmutableSecureStoreOutcome::Created
        {
            return Err(rejected(SigningWindowError::AlreadyAllocated));
        }
        if effects
            .secure_retrieve(&checkpoint_location, &[SecureStorageCapability::Read])
            .await?
            != anchor
        {
            return Err(rejected(SigningWindowError::Binding));
        }
        let live_location =
            SecureStorageLocation::new("enrollment_signing_clock_live_v1", allocation_key.clone());
        if effects
            .secure_store_immutable(&live_location, &anchor, &permissions)
            .await?
            != ImmutableSecureStoreOutcome::Created
        {
            return Err(rejected(SigningWindowError::AlreadyAllocated));
        }
        if effects
            .secure_retrieve(&live_location, &[SecureStorageCapability::Read])
            .await?
            != anchor
        {
            return Err(rejected(SigningWindowError::Binding));
        }
        Ok(Self {
            negative_cleanup_budget,
            shortcut_budget,
            original_policy,
            effects,
            binding,
            allocation_key,
            anchor,
            writes: Mutex::new(()),
        })
    }

    fn validate_record(
        binding: &SigningBinding,
        bytes: &[u8],
        original: &TimeoutBudget,
    ) -> Result<(), AuraError> {
        Self::validate_record_phase(
            binding,
            bytes,
            original,
            DurableEnrollmentExecutionState::Pending,
        )
    }
    fn validate_record_phase(
        binding: &SigningBinding,
        bytes: &[u8],
        original: &TimeoutBudget,
        expected: DurableEnrollmentExecutionState,
    ) -> Result<(), AuraError> {
        if bytes.len() > MAX_RECORD_BYTES {
            return Err(rejected(SigningWindowError::Bounds));
        }
        let record: SigningClockRecord =
            serde_json::from_slice(bytes).map_err(|source| AuraError::Serialization {
                message: "decode original signing checkpoint".into(),
                source: Some(Arc::new(source)),
            })?;
        if !matches!(
            (record.version, record.shortcut_budget.as_ref()),
            (3, None) | (4, Some(_))
        ) || &record.binding != binding
        {
            return Err(rejected(SigningWindowError::Binding));
        }
        if record.execution != expected {
            return Err(rejected(SigningWindowError::Binding));
        }
        record.original_policy.require_original(original)?;
        if let Some(shortcut) = &record.shortcut_budget {
            if shortcut.started_at_ms() != original.started_at_ms()
                || shortcut.deadline_at_ms() <= original.deadline_at_ms()
                || shortcut.timeout_ms() > original.timeout_ms().saturating_mul(2)
            {
                return Err(rejected(SigningWindowError::Binding));
            }
        }
        original
            .validate_checkpoint_continuation_from(&record.budget)
            .map_err(AuraError::from)
    }

    // Only the consuming root invokes this after actual native domain
    // completion and its exact sealed descendant disposal. The callback's
    // receipt is then forwarded by the shared resource-ACK boundary.
    pub(super) async fn close_original_participant(
        self: &Arc<Self>,
        original: &TimeoutBudget,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        completion: &super::super::threshold_signing::OriginalParticipantCompletion,
    ) -> Result<OriginalSigningTerminalReceipt, TimeoutBudgetError> {
        completion
            .require_original(approval)
            .map_err(TimeoutBudgetError::checkpoint_failure)?;
        self.require_effects(approval.effects().as_ref())
            .map_err(TimeoutBudgetError::checkpoint_failure)?;
        if self.binding.approved_intent_digest != approval.canonical_intent_digest() {
            return Err(TimeoutBudgetError::checkpoint_failure(rejected(
                SigningWindowError::Binding,
            )));
        }
        let frozen = self
            .persist_checkpoint(original, true, DurableEnrollmentExecutionState::Closed)
            .await?;
        Ok(OriginalSigningTerminalReceipt {
            owner: self.clone(),
            frozen,
        })
    }

    pub(super) fn negative_cleanup_budget(&self) -> &TimeoutBudget {
        &self.negative_cleanup_budget
    }

    // No signing or nonce generation is available on this path. The consuming
    // original root has already cancelled and drained its actual descendants.
    pub(super) async fn refuse_original_participant(
        self: &Arc<Self>,
        original: &TimeoutBudget,
    ) -> Result<OriginalSigningNegativeCleanupReceipt, TimeoutBudgetError> {
        let frozen = self
            .persist_checkpoint(original, true, DurableEnrollmentExecutionState::Refused)
            .await?;
        let retained: SigningClockRecord =
            serde_json::from_slice(&frozen).map_err(TimeoutBudgetError::checkpoint_failure)?;
        if !matches!(
            retained.execution,
            DurableEnrollmentExecutionState::Closed | DurableEnrollmentExecutionState::Refused
        ) {
            return Err(TimeoutBudgetError::checkpoint_failure(rejected(
                SigningWindowError::Binding,
            )));
        }
        Ok(OriginalSigningNegativeCleanupReceipt {
            owner: self.clone(),
            frozen,
            retained_decision: retained.execution,
        })
    }

    async fn persist_checkpoint(
        &self,
        original: &TimeoutBudget,
        require_live: bool,
        execution: DurableEnrollmentExecutionState,
    ) -> Result<Vec<u8>, TimeoutBudgetError> {
        let _write = self.writes.lock().await;
        let anchor = self
            .effects
            .secure_retrieve(
                &self.location("enrollment_signing_clock_anchor_v1"),
                &[SecureStorageCapability::Read],
            )
            .await
            .map_err(TimeoutBudgetError::checkpoint_failure)?;
        if anchor != self.anchor {
            return Err(TimeoutBudgetError::checkpoint_failure(rejected(
                SigningWindowError::Binding,
            )));
        }
        if require_live {
            let live = self
                .effects
                .secure_retrieve(
                    &self.location("enrollment_signing_clock_live_v1"),
                    &[SecureStorageCapability::Read],
                )
                .await
                .map_err(TimeoutBudgetError::checkpoint_failure)?;
            if live != self.anchor {
                return Err(TimeoutBudgetError::checkpoint_failure(rejected(
                    SigningWindowError::Binding,
                )));
            }
        }
        // No missing-record fallback, including after ever-live acknowledgment.
        let retained = self
            .effects
            .secure_retrieve(
                &self.location("enrollment_signing_clock_checkpoint_v1"),
                &[SecureStorageCapability::Read],
            )
            .await
            .map_err(TimeoutBudgetError::checkpoint_failure)?;
        let retained_record: SigningClockRecord =
            serde_json::from_slice(&retained).map_err(TimeoutBudgetError::checkpoint_failure)?;
        let expected = if execution == DurableEnrollmentExecutionState::Refused {
            match retained_record.execution {
                DurableEnrollmentExecutionState::Closed => DurableEnrollmentExecutionState::Closed,
                DurableEnrollmentExecutionState::Refused => {
                    DurableEnrollmentExecutionState::Refused
                }
                _ => DurableEnrollmentExecutionState::Pending,
            }
        } else {
            DurableEnrollmentExecutionState::Pending
        };
        Self::validate_record_phase(&self.binding, &retained, original, expected)
            .map_err(TimeoutBudgetError::checkpoint_failure)?;
        if retained_record.original_policy != self.original_policy {
            return Err(TimeoutBudgetError::checkpoint_failure(rejected(
                SigningWindowError::Binding,
            )));
        }
        if !retained_record
            .shortcut_budget
            .as_ref()
            .is_some_and(|shortcut| continues_original_shortcut(shortcut, &self.shortcut_budget))
        {
            return Err(TimeoutBudgetError::checkpoint_failure(rejected(
                SigningWindowError::Binding,
            )));
        }
        if execution == DurableEnrollmentExecutionState::Refused
            && matches!(
                expected,
                DurableEnrollmentExecutionState::Closed | DurableEnrollmentExecutionState::Refused
            )
        {
            // First durable terminal decision wins. Actual original cleanup
            // custody may acknowledge disposal, but cannot rewrite committed
            // Closed as Refused or issue any positive execution root.
            return Ok(retained);
        }
        let bytes = serde_json::to_vec(&SigningClockRecord {
            negative_cleanup_budget: self.negative_cleanup_budget.clone(),
            shortcut_budget: Some(self.shortcut_budget.clone()),
            original_policy: self.original_policy.clone(),
            version: 4,
            binding: self.binding.clone(),
            budget: original.clone(),
            execution,
        })
        .map_err(TimeoutBudgetError::checkpoint_failure)?;
        self.effects
            .secure_store(
                &self.location("enrollment_signing_clock_checkpoint_v1"),
                &bytes,
                &[SecureStorageCapability::Write],
            )
            .await
            .map_err(TimeoutBudgetError::checkpoint_failure)?;
        let readback = self
            .effects
            .secure_retrieve(
                &self.location("enrollment_signing_clock_checkpoint_v1"),
                &[SecureStorageCapability::Read],
            )
            .await
            .map_err(TimeoutBudgetError::checkpoint_failure)?;
        if readback != bytes {
            return Err(TimeoutBudgetError::checkpoint_failure(rejected(
                SigningWindowError::Binding,
            )));
        }
        Ok(readback)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding() -> SigningBinding {
        SigningBinding {
            subject: aura_core::AuthorityId::new_from_entropy([1; 32]),
            device: aura_core::DeviceId::from_uuid(uuid::Uuid::from_u128(1)),
            coordinator: aura_core::DeviceId::from_uuid(uuid::Uuid::from_u128(2)),
            ceremony: aura_core::CeremonyId::new("original-clock"),
            invitation: aura_core::InvitationId::new("original-invitation"),
            domain: "original-manifest-domain".into(),
            transcript_digest: [3; 32],
            approved_intent_digest: [4; 32],
        }
    }
    fn record(binding: SigningBinding, budget: &TimeoutBudget) -> Vec<u8> {
        let birth = OriginalEnrollmentWindowBirth::allocate(
            &PhysicalTime::exact(budget.started_at_ms()),
            ORIGINAL_SIGNING_POLICY,
            OriginalShortcutWindowPolicy::bounded_extension(
                ORIGINAL_SIGNING_POLICY,
                ORIGINAL_SIGNING_POLICY,
            )
            .expect("original shortcut policy"),
        )
        .expect("actual original signing birth");
        serde_json::to_vec(&SigningClockRecord {
            version: 4,
            binding,
            budget: budget.clone(),
            negative_cleanup_budget: birth.negative_cleanup,
            shortcut_budget: Some(birth.shortcut),
            original_policy: birth.policy,
            execution: DurableEnrollmentExecutionState::Pending,
        })
        .expect("encode actual original checkpoint")
    }
    #[test]
    fn original_checkpoint_refuses_renewed_interval_foreign_intent_and_missing_record() {
        let binding = binding();
        let original = TimeoutBudget::from_start_and_timeout(
            &PhysicalTime::exact(100),
            ORIGINAL_SIGNING_POLICY,
        )
        .expect("original allocation");
        let frozen = record(binding.clone(), &original);
        assert!(ApprovedSigningCheckpoint::validate_record(&binding, &frozen, &original).is_ok());
        let renewed = TimeoutBudget::from_start_and_timeout(
            &PhysicalTime::exact(200),
            ORIGINAL_SIGNING_POLICY,
        )
        .expect("renewed interval is valid but unauthorized");
        assert!(ApprovedSigningCheckpoint::validate_record(&binding, &frozen, &renewed).is_err());
        let mut foreign = binding.clone();
        foreign.transcript_digest[0] ^= 1;
        assert!(ApprovedSigningCheckpoint::validate_record(&foreign, &frozen, &original).is_err());
        assert!(ApprovedSigningCheckpoint::validate_record(&binding, &[], &original).is_err());
    }
    #[test]
    fn protected_highwater_and_expiration_cannot_be_reset_by_fresh_clock_or_child() {
        let binding = binding();
        let original = TimeoutBudget::from_start_and_timeout(
            &PhysicalTime::exact(100),
            ORIGINAL_SIGNING_POLICY,
        )
        .expect("original allocation");
        original
            .remaining_at(&PhysicalTime::exact(200))
            .expect("original observation");
        let durable = record(binding.clone(), &original);
        let reset = TimeoutBudget::from_start_and_timeout(
            &PhysicalTime::exact(100),
            ORIGINAL_SIGNING_POLICY,
        )
        .expect("lookalike original interval");
        assert!(ApprovedSigningCheckpoint::validate_record(&binding, &durable, &reset).is_err());
        assert!(original.remaining_at(&PhysicalTime::exact(30_101)).is_err());
        let exhausted = record(binding.clone(), &original);
        assert!(ApprovedSigningCheckpoint::validate_record(&binding, &exhausted, &reset).is_err());
        assert!(original
            .child_budget(&PhysicalTime::exact(30_101), ORIGINAL_SIGNING_POLICY)
            .is_err());
    }
    #[test]
    fn protected_shortcut_endpoint_survives_restart_and_expires_without_renewal() {
        let original = TimeoutBudget::from_start_and_timeout(
            &PhysicalTime::exact(100),
            ORIGINAL_SIGNING_POLICY,
        )
        .expect("original allocation");
        let frozen = record(binding(), &original);
        let restored: SigningClockRecord =
            serde_json::from_slice(&frozen).expect("restore protected allocation");
        let restored_shortcut = restored.shortcut_budget.as_ref().expect("new shortcut");
        assert_eq!(restored_shortcut.started_at_ms(), original.started_at_ms());
        assert_eq!(restored_shortcut.deadline_at_ms(), 60_100);
        let retained_original = restored_shortcut.clone();
        retained_original
            .remaining_at(&PhysicalTime::exact(500))
            .expect("original shortcut observation");
        let reset: SigningClockRecord =
            serde_json::from_slice(&frozen).expect("lookalike with reset observation");
        assert!(!continues_original_shortcut(
            reset.shortcut_budget.as_ref().expect("new shortcut"),
            &retained_original
        ));
        assert!(restored_shortcut
            .remaining_at(&PhysicalTime::exact(60_101))
            .is_err());
        let expired = serde_json::to_vec(&restored).expect("retain expired allocation");
        let again: SigningClockRecord =
            serde_json::from_slice(&expired).expect("restore same allocation");
        let again_shortcut = again.shortcut_budget.as_ref().expect("new shortcut");
        assert_eq!(again_shortcut.deadline_at_ms(), 60_100);
        assert!(again
            .shortcut_budget
            .as_ref()
            .expect("new shortcut")
            .remaining_at(&PhysicalTime::exact(60_101))
            .is_err());
    }
    #[test]
    fn legacy_v3_signing_record_resumes_enrollment_without_shortcut_authority() {
        let binding = binding();
        let original = TimeoutBudget::from_start_and_timeout(
            &PhysicalTime::exact(100),
            ORIGINAL_SIGNING_POLICY,
        )
        .expect("original allocation");
        let mut legacy: SigningClockRecord =
            serde_json::from_slice(&record(binding.clone(), &original))
                .expect("new protected record");
        legacy.version = 3;
        legacy.shortcut_budget = None;
        let frozen = serde_json::to_vec(&legacy).expect("legacy protected record");
        let resumed: SigningClockRecord = serde_json::from_slice(&frozen).expect("legacy decode");
        assert!(ApprovedSigningCheckpoint::validate_record(&binding, &frozen, &original).is_ok());
        assert!(
            resumed.shortcut_budget.is_none(),
            "legacy cannot deliver shortcut"
        );
        let mut stripped_v4 = resumed;
        stripped_v4.version = 4;
        assert!(ApprovedSigningCheckpoint::validate_record(
            &binding,
            &serde_json::to_vec(&stripped_v4).expect("stripped v4"),
            &original
        )
        .is_err());
    }
}
