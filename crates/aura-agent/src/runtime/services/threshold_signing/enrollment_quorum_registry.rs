//! Runtime-private bounded custody of genuine prepared issuer and participant tasks.
//! Capacity is admitted before task spawn, and collision returns ownership for
//! explicit original-window cancellation/drain rather than dropping live owners.
use super::StartedEnrollmentManifestParticipant;
use crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent;
use crate::{
    runtime::{services::enrollment_window::HeldIssuerCompletionObserver, AuraEffectSystem},
    task_registry::TaskGroup,
};
use aura_app::runtime_bridge::DeviceEnrollmentStart;
use aura_core::{AuraError, AuthorityId, CeremonyId, DeviceId, OwnedTaskHandle};
use aura_invitation::enrollment_setup::EnrollmentIssuanceError;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{oneshot, Mutex, OwnedSemaphorePermit, Semaphore};

#[derive(Debug, thiserror::Error)]
enum RegistryFailure {
    #[error("original enrollment quorum registry owner already exists")]
    Collision,
    #[error("original enrollment quorum registry owner binding differs")]
    Binding,
    #[error("original enrollment quorum registry owner is absent")]
    Missing,
}
fn rejected(source: RegistryFailure) -> AuraError {
    AuraError::Internal {
        message: "owned enrollment quorum registry refused admission".into(),
        source: Some(Arc::new(source)),
    }
}

pub(super) struct QuorumCapacityPermit {
    _permit: OwnedSemaphorePermit,
}

pub(super) struct PreparedIssuerEntry {
    _capacity: QuorumCapacityPermit,
    subject: AuthorityId,
    ceremony: CeremonyId,
    approved_intent_digest: [u8; 32],
    effects: Arc<AuraEffectSystem>,
    original_window: HeldIssuerCompletionObserver,
    resume: oneshot::Sender<RuntimeApprovedEnrollmentSigningIntent>,
    completed: oneshot::Receiver<Result<DeviceEnrollmentStart, EnrollmentIssuanceError>>,
    group: TaskGroup,
    task: OwnedTaskHandle<u64>,
}

#[derive(Debug, thiserror::Error)]
#[error("original prepared issuer failed: {primary}; original task drainage failed: {cleanup}")]
struct PreparedIssuerAndDrainFailure {
    #[source]
    primary: AuraError,
    cleanup: AuraError,
}
pub(super) fn joined(primary: AuraError, cleanup: AuraError) -> AuraError {
    AuraError::Internal {
        message: "original enrollment issuer and task drainage failed".into(),
        source: Some(Arc::new(PreparedIssuerAndDrainFailure { primary, cleanup })),
    }
}

impl PreparedIssuerEntry {
    pub(super) fn new(
        capacity: QuorumCapacityPermit,
        subject: AuthorityId,
        ceremony: CeremonyId,
        approved_intent_digest: [u8; 32],
        effects: Arc<AuraEffectSystem>,
        original_window: HeldIssuerCompletionObserver,
        resume: oneshot::Sender<RuntimeApprovedEnrollmentSigningIntent>,
        completed: oneshot::Receiver<Result<DeviceEnrollmentStart, EnrollmentIssuanceError>>,
        group: TaskGroup,
        task: OwnedTaskHandle<u64>,
    ) -> Self {
        Self {
            _capacity: capacity,
            subject,
            ceremony,
            approved_intent_digest,
            effects,
            original_window,
            resume,
            completed,
            group,
            task,
        }
    }

    fn require_original(
        &self,
        approval: &RuntimeApprovedEnrollmentSigningIntent,
    ) -> Result<(), AuraError> {
        if !Arc::ptr_eq(&self.effects, approval.effects())
            || self.subject != approval.manifest().subject
            || self.ceremony != approval.manifest().ceremony
            || self.approved_intent_digest != approval.canonical_intent_digest()
        {
            return Err(rejected(RegistryFailure::Binding));
        }
        Ok(())
    }

    pub(super) async fn finish(
        self,
        approval: RuntimeApprovedEnrollmentSigningIntent,
    ) -> Result<DeviceEnrollmentStart, AuraError> {
        // Even an erroneous parent invocation cannot lose a live task owner.
        if let Err(primary) = self.require_original(&approval) {
            return match self.cancel_and_drain().await {
                Ok(()) => Err(primary),
                Err(cleanup) => Err(joined(primary, cleanup)),
            };
        }
        let Self {
            _capacity,
            effects,
            original_window,
            resume,
            completed,
            group,
            task,
            ..
        } = self;
        tracing::debug!(
            task_id = *task.handle_id(),
            "Resume original prepared enrollment issuer"
        );
        let primary = match resume.send(approval) {
            Ok(()) => {
                original_window
                    .receive_issuer_result(effects.as_ref(), completed)
                    .await
            }
            Err(_) => Err(rejected(RegistryFailure::Missing)),
        };
        let cleanup = if primary.is_ok() {
            original_window
                .wait_owned_group(effects.as_ref(), &group)
                .await
        } else {
            original_window
                .shutdown_owned_group(effects.as_ref(), &group)
                .await
        }
        .map_err(|source| AuraError::Internal {
            message: "original prepared issuer actual task drainage failed".into(),
            source: Some(Arc::new(source)),
        });
        // Keep native capacity and handle custody through the final actual ACK.
        drop(task);
        drop(_capacity);
        match (primary, cleanup) {
            (Ok(result), Ok(())) => Ok(result),
            (Err(primary), Ok(())) => Err(primary),
            (Ok(_), Err(cleanup)) => Err(cleanup),
            (Err(primary), Err(cleanup)) => Err(joined(primary, cleanup)),
        }
    }

    pub(super) async fn cancel_and_drain(self) -> Result<(), AuraError> {
        let Self {
            _capacity,
            effects,
            original_window,
            resume,
            completed,
            group,
            task,
            ..
        } = self;
        drop(resume);
        drop(completed);
        tracing::debug!(
            task_id = *task.handle_id(),
            "Cancel original prepared enrollment issuer"
        );
        let result = original_window
            .shutdown_owned_group(effects.as_ref(), &group)
            .await
            .map_err(|source| AuraError::Internal {
                message: "original prepared issuer cancellation drainage failed".into(),
                source: Some(Arc::new(source)),
            });
        drop(task);
        drop(_capacity);
        result
    }
}

pub(super) struct RegisteredParticipant {
    owner: StartedEnrollmentManifestParticipant,
    _capacity: QuorumCapacityPermit,
    subject: AuthorityId,
    ceremony: CeremonyId,
    device: DeviceId,
}
impl RegisteredParticipant {
    pub(super) async fn cancel_and_drain(self) -> Result<(), AuraError> {
        let Self {
            owner, _capacity, ..
        } = self;
        let result = owner.cancel_and_drain().await;
        drop(_capacity);
        result
    }
}

pub(super) struct EnrollmentQuorumRegistry {
    capacity: Arc<Semaphore>,
    prepared: Mutex<HashMap<CeremonyId, PreparedIssuerEntry>>,
    participants: Mutex<HashMap<(AuthorityId, CeremonyId, DeviceId), RegisteredParticipant>>,
}
impl EnrollmentQuorumRegistry {
    pub(super) fn new() -> Self {
        Self {
            capacity: Arc::new(Semaphore::new(64)),
            prepared: Mutex::new(HashMap::new()),
            participants: Mutex::new(HashMap::new()),
        }
    }
    pub(super) fn reserve(&self) -> Result<QuorumCapacityPermit, AuraError> {
        self.capacity
            .clone()
            .try_acquire_owned()
            .map(|permit| QuorumCapacityPermit { _permit: permit })
            .map_err(|source| AuraError::Internal {
                message: "original enrollment quorum registry capacity refused before spawn".into(),
                source: Some(Arc::new(source)),
            })
    }
    pub(super) async fn insert_prepared(
        &self,
        entry: PreparedIssuerEntry,
    ) -> Result<(), (AuraError, PreparedIssuerEntry)> {
        if !Arc::ptr_eq(entry._capacity._permit.semaphore(), &self.capacity) {
            return Err((rejected(RegistryFailure::Binding), entry));
        }
        let mut entries = self.prepared.lock().await;
        if self.capacity.is_closed() {
            return Err((rejected(RegistryFailure::Missing), entry));
        }
        if entries.contains_key(&entry.ceremony) {
            return Err((rejected(RegistryFailure::Collision), entry));
        }
        entries.insert(entry.ceremony.clone(), entry);
        Ok(())
    }
    pub(super) async fn take_for_original_approval(
        &self,
        approval: &RuntimeApprovedEnrollmentSigningIntent,
    ) -> Result<PreparedIssuerEntry, AuraError> {
        let mut entries = self.prepared.lock().await;
        let entry = entries
            .get(&approval.manifest().ceremony)
            .ok_or_else(|| rejected(RegistryFailure::Missing))?;
        entry.require_original(approval)?;
        entries
            .remove(&approval.manifest().ceremony)
            .ok_or_else(|| rejected(RegistryFailure::Missing))
    }
    pub(super) async fn insert_started_participant(
        &self,
        owner: StartedEnrollmentManifestParticipant,
        capacity: QuorumCapacityPermit,
    ) -> Result<(), AuraError> {
        let subject = owner.subject;
        let ceremony = owner.ceremony.clone();
        let entry = RegisteredParticipant {
            device: owner.effects.device_id(),
            owner,
            _capacity: capacity,
            subject,
            ceremony,
        };
        if entry.subject
            != aura_guards::GuardContextProvider::authority_id(entry.owner.effects.as_ref())
            || !Arc::ptr_eq(entry._capacity._permit.semaphore(), &self.capacity)
        {
            let primary = rejected(RegistryFailure::Binding);
            return match entry.cancel_and_drain().await {
                Ok(()) => Err(primary),
                Err(cleanup) => Err(joined(primary, cleanup)),
            };
        }
        let key = (entry.subject, entry.ceremony.clone(), entry.device);
        let mut entries = self.participants.lock().await;
        if self.capacity.is_closed() || entries.contains_key(&key) {
            drop(entries);
            let primary = rejected(RegistryFailure::Collision);
            return match entry.cancel_and_drain().await {
                Ok(()) => Err(primary),
                Err(cleanup) => Err(joined(primary, cleanup)),
            };
        }
        entries.insert(key, entry);
        Ok(())
    }

    pub(super) async fn drain_all(&self) -> Result<(), AuraError> {
        // Native admission closes before transfer. Lock both maps in one fixed
        // order, transfer their actual owners, then release locks before awaits.
        self.capacity.close();
        let (prepared, participants) = {
            let mut prepared = self.prepared.lock().await;
            let mut participants = self.participants.lock().await;
            (
                prepared.drain().map(|(_, owner)| owner).collect::<Vec<_>>(),
                participants
                    .drain()
                    .map(|(_, owner)| owner)
                    .collect::<Vec<_>>(),
            )
        };
        let mut failures = Vec::new();
        for owner in prepared {
            if let Err(source) = owner.cancel_and_drain().await {
                failures.push(source);
            }
        }
        for owner in participants {
            if let Err(source) = owner.cancel_and_drain().await {
                failures.push(source);
            }
        }
        let mut failures = failures.into_iter();
        let Some(primary) = failures.next() else {
            return Ok(());
        };
        #[derive(Debug, thiserror::Error)]
        #[error("original quorum registry shutdown failed: {primary}; additional native task failures: {additional:?}")]
        struct RegistryDrainFailure {
            #[source]
            primary: AuraError,
            additional: Vec<AuraError>,
        }
        Err(AuraError::Internal {
            message: "original quorum registry owner drainage failed".into(),
            source: Some(Arc::new(RegistryDrainFailure {
                primary,
                additional: failures.collect(),
            })),
        })
    }
}
