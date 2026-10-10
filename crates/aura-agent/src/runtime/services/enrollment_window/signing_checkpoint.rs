//! Private signing clock allocation. A previous anchor is a consumed allocation,
//! never permission to mint a refreshed original window after restart.
use super::*;
use aura_core::effects::secure::ImmutableSecureStoreOutcome;
use aura_signature::SecurityTranscript;
use serde::{Deserialize, Serialize};

const ORIGINAL_SIGNING_POLICY: Duration = Duration::from_secs(30);
const MAX_RECORD_BYTES: usize = 16_384;

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
    effects: Arc<AuraEffectSystem>,
    binding: SigningBinding,
    allocation_key: String,
    anchor: Vec<u8>,
    writes: Mutex<()>,
}
impl ApprovedSigningCheckpoint {
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
        let budget = TimeoutBudget::from_start_and_timeout(&now, ORIGINAL_SIGNING_POLICY)
            .map_err(AuraError::from)?;
        let anchor = serde_json::to_vec(&SigningClockRecord {
            version: 2,
            binding: binding.clone(),
            budget: budget.clone(),
            execution: DurableEnrollmentExecutionState::Allocated,
        })
        .map_err(|source| AuraError::Serialization {
            message: "encode original local signing clock".into(),
            source: Some(Arc::new(source)),
        })?;
        if effects
            .secure_store_immutable(&anchor_location, &anchor, &[SecureStorageCapability::Write])
            .await?
            != ImmutableSecureStoreOutcome::Created
        {
            return Err(rejected(SigningWindowError::AlreadyAllocated));
        }
        let owner = Self {
            effects,
            binding,
            allocation_key,
            anchor,
            writes: Mutex::new(()),
        };
        owner
            .effects
            .secure_store(
                &owner.location("enrollment_signing_clock_checkpoint_v1"),
                &owner.anchor,
                &[SecureStorageCapability::Write],
            )
            .await?;
        // Required first observation and persisted acknowledgment precede live
        // owner publication. A crash anywhere leaves the immutable allocation
        // consumed; missing checkpoint cannot authorize a new clock.
        let observation = budget.acquire_observation().await;
        let observed = owner.effects.physical_time().await?;
        let eligible = budget.remaining_at(&observed);
        owner
            .persist_checkpoint(&budget, false)
            .await
            .map_err(AuraError::from)?;
        eligible.map_err(AuraError::from)?;
        if owner
            .effects
            .secure_store_immutable(
                &owner.location("enrollment_signing_clock_live_v1"),
                &owner.anchor,
                &[SecureStorageCapability::Write],
            )
            .await?
            != ImmutableSecureStoreOutcome::Created
        {
            return Err(rejected(SigningWindowError::AlreadyAllocated));
        }
        drop(observation);
        let lease = Arc::new(tokio::sync::Semaphore::new(1))
            .try_acquire_owned()
            .map_err(|source| AuraError::Internal {
                message: "retain new original signing clock lease".into(),
                source: Some(Arc::new(source)),
            })?;
        Ok((owner, budget, Arc::new(lease)))
    }

    fn validate_record(
        binding: &SigningBinding,
        bytes: &[u8],
        original: &TimeoutBudget,
    ) -> Result<(), AuraError> {
        if bytes.len() > MAX_RECORD_BYTES {
            return Err(rejected(SigningWindowError::Bounds));
        }
        let record: SigningClockRecord =
            serde_json::from_slice(bytes).map_err(|source| AuraError::Serialization {
                message: "decode original signing checkpoint".into(),
                source: Some(Arc::new(source)),
            })?;
        if record.version != 2 || &record.binding != binding {
            return Err(rejected(SigningWindowError::Binding));
        }
        record.execution.require_unstarted_recovery()?;
        original
            .validate_checkpoint_continuation_from(&record.budget)
            .map_err(AuraError::from)
    }

    pub(super) async fn checkpoint(
        &self,
        original: &TimeoutBudget,
    ) -> Result<(), TimeoutBudgetError> {
        self.persist_checkpoint(original, true).await
    }

    async fn persist_checkpoint(
        &self,
        original: &TimeoutBudget,
        require_live: bool,
    ) -> Result<(), TimeoutBudgetError> {
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
        Self::validate_record(&self.binding, &retained, original)
            .map_err(TimeoutBudgetError::checkpoint_failure)?;
        let bytes = serde_json::to_vec(&SigningClockRecord {
            version: 2,
            binding: self.binding.clone(),
            budget: original.clone(),
            execution: DurableEnrollmentExecutionState::Allocated,
        })
        .map_err(TimeoutBudgetError::checkpoint_failure)?;
        self.effects
            .secure_store(
                &self.location("enrollment_signing_clock_checkpoint_v1"),
                &bytes,
                &[SecureStorageCapability::Write],
            )
            .await
            .map_err(TimeoutBudgetError::checkpoint_failure)
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
        serde_json::to_vec(&SigningClockRecord {
            version: 2,
            binding,
            budget: budget.clone(),
            execution: DurableEnrollmentExecutionState::Allocated,
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
}
