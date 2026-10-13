//! Runtime-private bounded custody of genuine prepared issuer and participant tasks.
//! Capacity is admitted before task spawn, and collision returns ownership for
//! explicit original-window cancellation/drain rather than dropping live owners.
type OriginalPeerContinuationObservation = (
    Arc<tokio::sync::Mutex<super::OriginalEnrollmentPeerContinuation>>,
    Arc<crate::runtime::services::enrollment_window::EnrollmentExecutionChild>,
    Arc<crate::runtime::AuraEffectSystem>,
);

use super::StartedEnrollmentManifestParticipant;
use crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent;
use crate::{
    runtime::{services::enrollment_window::HeldIssuerCompletionObserver, AuraEffectSystem},
    task_registry::TaskGroup,
};
use aura_app::runtime_bridge::DeviceEnrollmentStart;
use aura_core::{AuraError, AuthorityId, CeremonyId, DeviceId};
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

pub(super) type OriginalIssuerCompletion = Result<
    (
        DeviceEnrollmentStart,
        crate::runtime_bridge::OriginalIssuedEnrollmentContinuation,
    ),
    AuraError,
>;

type OriginalReadyResult =
    Result<crate::runtime_bridge::enrollment_quorum::OriginalPreparedIssuerReady, AuraError>;
struct OriginalInitialReadiness {
    receiver: Option<oneshot::Receiver<OriginalReadyResult>>,
    retained: Option<OriginalReadyResult>,
}

pub(super) struct PreparedIssuerEntry {
    #[cfg(test)]
    registered_owner: std::sync::OnceLock<
        std::sync::Weak<
            crate::runtime::services::ceremony_tracker::RegisteredEnrollmentWindowCapability,
        >,
    >,
    _capacity: QuorumCapacityPermit,
    preparation: CeremonyId,
    subject: AuthorityId,
    metadata: std::sync::OnceLock<PreparedIssuerMetadata>,
    effects: Arc<AuraEffectSystem>,
    resume: Mutex<Option<oneshot::Sender<RuntimeApprovedEnrollmentSigningIntent>>>,
    initial_ready: Mutex<OriginalInitialReadiness>,
    launch_observation: super::super::enrollment_window::OriginalIssuerLaunchObservation,
    completed: Mutex<Option<oneshot::Receiver<()>>>,
    #[allow(
        clippy::disallowed_types,
        reason = "Synchronous runtime custody deposit; the lock guard never crosses an await"
    )]
    completed_result: Arc<parking_lot::Mutex<Option<OriginalIssuerCompletion>>>,
    group: TaskGroup,
    #[allow(
        clippy::disallowed_types,
        reason = "Synchronous runtime custody deposit; the lock guard never crosses an await"
    )]
    task: parking_lot::Mutex<Option<crate::task_registry::OriginalRequiredTaskHandle>>,
    #[cfg(test)]
    resources: std::sync::OnceLock<
        std::sync::Weak<
            tokio::sync::Mutex<
                crate::runtime_bridge::enrollment_quorum::OriginalPreparedIssuerResources,
            >,
        >,
    >,
}

/// Issued only after the same required control is retained in the original
/// bounded entry. Weak references prevent a completed custody back-cycle.
pub(crate) struct OriginalPreparedIssuerRegistration {
    original: std::sync::Weak<PreparedIssuerEntry>,
    registry: std::sync::Weak<EnrollmentQuorumRegistry>,
}
impl OriginalPreparedIssuerRegistration {
    pub(super) fn new(
        original: &Arc<PreparedIssuerEntry>,
        registry: &Arc<EnrollmentQuorumRegistry>,
    ) -> Self {
        Self {
            original: Arc::downgrade(original),
            registry: Arc::downgrade(registry),
        }
    }
    pub(crate) fn publish_registered_running(
        &self,
        running: &mut crate::runtime::services::ceremony_tracker::PreparedRunningIssuerObservation,
    ) -> Result<(), AuraError> {
        let original = self
            .original
            .upgrade()
            .ok_or_else(|| rejected(RegistryFailure::Missing))?;
        let result = running.publish_required(&mut original.task.lock());
        #[cfg(test)]
        if result.is_ok() {
            original
                .registered_owner
                .set(running.original_owner_observation_for_test())
                .map_err(|_| rejected(RegistryFailure::Collision))?;
        }
        result
    }

    pub(crate) fn publish_original_window(
        &self,
        effects: &Arc<AuraEffectSystem>,
        ceremony: CeremonyId,
        window: HeldIssuerCompletionObserver,
    ) -> Result<(), AuraError> {
        let entry = self
            .original
            .upgrade()
            .ok_or_else(|| rejected(RegistryFailure::Missing))?;
        if !Arc::ptr_eq(effects, &entry.effects) {
            return Err(rejected(RegistryFailure::Binding));
        }
        entry.publish_original_window(ceremony, window)
    }
    pub(crate) fn bind_original_approval(
        &self,
        effects: &Arc<AuraEffectSystem>,
        ceremony: &CeremonyId,
        digest: [u8; 32],
    ) -> Result<(), AuraError> {
        let entry = self
            .original
            .upgrade()
            .ok_or_else(|| rejected(RegistryFailure::Missing))?;
        let metadata = entry
            .metadata
            .get()
            .ok_or_else(|| rejected(RegistryFailure::Missing))?;
        if !Arc::ptr_eq(effects, &entry.effects) || &metadata.ceremony != ceremony {
            return Err(rejected(RegistryFailure::Binding));
        }
        metadata
            .approved_intent_digest
            .set(digest)
            .map_err(|_| rejected(RegistryFailure::Collision))
    }
    pub(crate) fn acknowledge_original_removal(&self) -> Result<(), AuraError> {
        let Some(entry) = self.original.upgrade() else {
            return Ok(());
        };
        let registry = self
            .registry
            .upgrade()
            .ok_or_else(|| rejected(RegistryFailure::Missing))?;
        registry.remove_original_preparation(&entry)
    }
}
struct PreparedIssuerMetadata {
    ceremony: CeremonyId,
    approved_intent_digest: std::sync::OnceLock<[u8; 32]>,
    original_window: HeldIssuerCompletionObserver,
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

#[cfg(test)]
pub(crate) struct OriginalPreparedIssuerProbe {
    original: Arc<PreparedIssuerEntry>,
}
#[cfg(test)]
impl OriginalPreparedIssuerProbe {
    pub(crate) fn original_registered_owner_for_test(
        &self,
    ) -> Result<
        Arc<crate::runtime::services::ceremony_tracker::RegisteredEnrollmentWindowCapability>,
        AuraError,
    > {
        self.original
            .registered_owner
            .get()
            .and_then(std::sync::Weak::upgrade)
            .ok_or_else(|| rejected(RegistryFailure::Missing))
    }
    pub(crate) async fn observe_initial_readiness_for_test(&self) -> Result<(), AuraError> {
        self.original.receive_initial_ready().await.map(|_| ())
    }
    pub(crate) fn request_cancel(&self) -> Result<(), AuraError> {
        use aura_core::effects::task::RequiredTaskControl;
        self.original
            .task
            .lock()
            .as_mut()
            .ok_or_else(|| rejected(RegistryFailure::Missing))?
            .request_cancel();
        Ok(())
    }
    pub(crate) fn disposal_observer(
        &self,
    ) -> Result<crate::task_registry::OriginalRequiredTaskDisposalObserver, AuraError> {
        Ok(self
            .original
            .task
            .lock()
            .as_ref()
            .ok_or_else(|| rejected(RegistryFailure::Missing))?
            .disposal_observer())
    }
    pub(crate) fn require_original_disposal_for_test(
        &self,
        disposal: &crate::task_registry::OriginalRequiredTaskDisposalObservation,
    ) -> Result<(), AuraError> {
        let original = self.original.task.lock();
        disposal.require_original(
            original
                .as_ref()
                .ok_or_else(|| rejected(RegistryFailure::Missing))?,
        )
    }
    pub(crate) fn with_retained_resources_for_test<R>(
        &self,
        observe: impl FnOnce(
            &crate::runtime_bridge::enrollment_quorum::OriginalPreparedIssuerResources,
        ) -> R,
    ) -> Result<R, AuraError> {
        let resources = self
            .original
            .resources
            .get()
            .and_then(std::sync::Weak::upgrade)
            .ok_or_else(|| rejected(RegistryFailure::Missing))?;
        let borrowed = resources.try_lock().map_err(|source| AuraError::Internal {
            message: "original prepared resources remain borrowed".into(),
            source: Some(Arc::new(source)),
        })?;
        Ok(observe(&borrowed))
    }
}

impl PreparedIssuerEntry {
    #[cfg(test)]
    pub(super) fn retain_resources_observation_for_test(
        &self,
        original: std::sync::Weak<
            tokio::sync::Mutex<
                crate::runtime_bridge::enrollment_quorum::OriginalPreparedIssuerResources,
            >,
        >,
    ) {
        assert!(
            self.resources.set(original).is_ok(),
            "original resources observation installed once before start"
        );
    }
    #[cfg(test)]
    pub(super) fn probe_for_test(self: Arc<Self>) -> OriginalPreparedIssuerProbe {
        OriginalPreparedIssuerProbe { original: self }
    }
    #[allow(
        clippy::disallowed_types,
        reason = "Synchronous runtime custody deposit; the lock guard never crosses an await"
    )]
    pub(super) fn new(
        capacity: QuorumCapacityPermit,
        preparation: CeremonyId,
        subject: AuthorityId,
        effects: Arc<AuraEffectSystem>,
        resume: Option<oneshot::Sender<RuntimeApprovedEnrollmentSigningIntent>>,
        initial_ready: oneshot::Receiver<OriginalReadyResult>,
        launch_observation: super::super::enrollment_window::OriginalIssuerLaunchObservation,
        completed: oneshot::Receiver<()>,
        #[allow(
            clippy::disallowed_types,
            reason = "Synchronous runtime custody deposit; the lock guard never crosses an await"
        )]
        completed_result: Arc<parking_lot::Mutex<Option<OriginalIssuerCompletion>>>,
        group: TaskGroup,
        task: crate::task_registry::OriginalRequiredTaskHandle,
    ) -> Self {
        Self {
            #[cfg(test)]
            registered_owner: std::sync::OnceLock::new(),
            _capacity: capacity,
            preparation,
            subject,
            metadata: std::sync::OnceLock::new(),
            effects,
            resume: Mutex::new(resume),
            initial_ready: Mutex::new(OriginalInitialReadiness {
                receiver: Some(initial_ready),
                retained: None,
            }),
            launch_observation,
            completed: Mutex::new(Some(completed)),
            completed_result,
            group,
            task: parking_lot::Mutex::new(Some(task)),
            #[cfg(test)]
            resources: std::sync::OnceLock::new(),
        }
    }

    pub(crate) fn publish_original_window(
        &self,
        ceremony: CeremonyId,
        original_window: HeldIssuerCompletionObserver,
    ) -> Result<(), AuraError> {
        self.metadata
            .set(PreparedIssuerMetadata {
                ceremony,
                approved_intent_digest: std::sync::OnceLock::new(),
                original_window,
            })
            .map_err(|_| rejected(RegistryFailure::Collision))
    }

    pub(super) async fn receive_initial_ready(&self) -> OriginalReadyResult {
        let mut ready = self
            .initial_ready
            .try_lock()
            .map_err(|source| AuraError::Internal {
                message: "original initial readiness observer is borrowed".into(),
                source: Some(Arc::new(source)),
            })?;
        let OriginalInitialReadiness { receiver, retained } = &mut *ready;
        self.launch_observation
            .receive_initial_ready(&self.effects, receiver, retained)
            .await?;
        // The slot remains entry-owned through all awaited provider checks.
        // No await follows extraction of this readiness projection.
        retained
            .take()
            .ok_or_else(|| rejected(RegistryFailure::Missing))?
    }

    pub(super) async fn receive_owned_result(
        &self,
    ) -> Result<
        (
            DeviceEnrollmentStart,
            crate::runtime_bridge::OriginalIssuedEnrollmentContinuation,
        ),
        AuraError,
    > {
        // Native execution failure is already authoritative and keeps its exact
        // source; observing it cannot be replaced by a later clock failure.
        if let Some(Err(source)) = self.completed_result.lock().as_ref() {
            return Err(source.clone());
        }
        let metadata = self
            .metadata
            .get()
            .ok_or_else(|| rejected(RegistryFailure::Missing))?;
        let mut completed = self
            .completed
            .try_lock()
            .map_err(|source| AuraError::Internal {
                message: "original issuance result observer is borrowed".into(),
                source: Some(Arc::new(source)),
            })?;
        metadata
            .original_window
            .receive_issuer_result(self.effects.as_ref(), &mut completed)
            .await?;
        // No await follows exact-once extraction from retained entry custody.
        self.completed_result
            .lock()
            .take()
            .ok_or_else(|| rejected(RegistryFailure::Missing))?
    }

    fn require_original(
        &self,
        approval: &RuntimeApprovedEnrollmentSigningIntent,
    ) -> Result<(), AuraError> {
        let metadata = self
            .metadata
            .get()
            .ok_or_else(|| rejected(RegistryFailure::Missing))?;
        if !Arc::ptr_eq(&self.effects, approval.effects())
            || self.subject != approval.manifest().subject
            || metadata.ceremony != approval.manifest().ceremony
            || metadata.approved_intent_digest.get().copied()
                != Some(approval.canonical_intent_digest())
        {
            return Err(rejected(RegistryFailure::Binding));
        }
        Ok(())
    }

    pub(super) async fn finish(
        self: Arc<Self>,
        approval: RuntimeApprovedEnrollmentSigningIntent,
    ) -> Result<DeviceEnrollmentStart, AuraError> {
        // Even an erroneous parent invocation cannot lose a live task owner.
        if let Err(primary) = self.require_original(&approval) {
            return match self.cancel_and_drain().await {
                Ok(()) => Err(primary),
                Err(cleanup) => Err(joined(primary, cleanup)),
            };
        }
        let original_window = &self
            .metadata
            .get()
            .ok_or_else(|| rejected(RegistryFailure::Missing))?
            .original_window;
        let resume = self
            .resume
            .try_lock()
            .map_err(|source| AuraError::Internal {
                message: "original approval ingress is borrowed".into(),
                source: Some(Arc::new(source)),
            })?
            .take()
            .ok_or_else(|| rejected(RegistryFailure::Missing))?;
        let primary = match resume.send(approval) {
            Ok(()) => self
                .receive_owned_result()
                .await
                .map(|(observed, _original)| observed),
            Err(_) => Err(rejected(RegistryFailure::Missing)),
        };
        // Issuance readiness does not dispose the original task: the same
        // admitted custodian continues the registered protocol. Its handle
        // remains in this entry until actual terminal cleanup acknowledges it.
        if primary.is_ok() {
            return primary;
        }
        let cleanup = original_window
            .shutdown_owned_group(self.effects.as_ref(), &self.group)
            .await
            .map_err(|source| AuraError::Internal {
                message: "original prepared issuer actual task drainage failed".into(),
                source: Some(Arc::new(source)),
            });
        // Keep native capacity and handle custody through the final actual ACK.
        match (primary, cleanup) {
            (Ok(result), Ok(())) => Ok(result),
            (Err(primary), Ok(())) => Err(primary),
            (Ok(_), Err(cleanup)) => Err(cleanup),
            (Err(primary), Err(cleanup)) => Err(joined(primary, cleanup)),
        }
    }

    pub(super) async fn cancel_and_drain(self: Arc<Self>) -> Result<(), AuraError> {
        use aura_core::effects::task::RequiredTaskControl;
        if let Some(task) = self.task.lock().as_mut() {
            task.request_cancel();
        }
        self.resume
            .try_lock()
            .map_err(|source| AuraError::Internal {
                message: "original approval ingress is borrowed".into(),
                source: Some(Arc::new(source)),
            })?
            .take();
        self.completed
            .try_lock()
            .map_err(|source| AuraError::Internal {
                message: "original preparation result is borrowed".into(),
                source: Some(Arc::new(source)),
            })?
            .take();
        let Some(metadata) = self.metadata.get() else {
            return Err(rejected(RegistryFailure::Missing));
        };
        let original_window = &metadata.original_window;
        let result = original_window
            .shutdown_owned_group(self.effects.as_ref(), &self.group)
            .await
            .map_err(|source| AuraError::Internal {
                message: "original prepared issuer cancellation drainage failed".into(),
                source: Some(Arc::new(source)),
            });
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
    prepared: Mutex<HashMap<CeremonyId, Arc<PreparedIssuerEntry>>>,
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
    pub(super) fn insert_prepared(
        &self,
        entry: Arc<PreparedIssuerEntry>,
    ) -> Result<(), (AuraError, Arc<PreparedIssuerEntry>)> {
        if !Arc::ptr_eq(entry._capacity._permit.semaphore(), &self.capacity) {
            return Err((rejected(RegistryFailure::Binding), entry));
        }
        let mut entries = match self.prepared.try_lock() {
            Ok(entries) => entries,
            Err(source) => {
                return Err((
                    AuraError::Internal {
                        message: "original preparation registry admission is contended".into(),
                        source: Some(Arc::new(source)),
                    },
                    entry,
                ))
            }
        };
        if self.capacity.is_closed() {
            return Err((rejected(RegistryFailure::Missing), entry));
        }
        if entries.contains_key(&entry.preparation) {
            return Err((rejected(RegistryFailure::Collision), entry));
        }
        entries.insert(entry.preparation.clone(), entry);
        Ok(())
    }
    pub(super) async fn take_for_original_approval(
        &self,
        approval: &RuntimeApprovedEnrollmentSigningIntent,
    ) -> Result<Arc<PreparedIssuerEntry>, AuraError> {
        let entries = self.prepared.lock().await;
        let entry = entries
            .values()
            .find(|entry| {
                entry
                    .metadata
                    .get()
                    .is_some_and(|metadata| metadata.ceremony == approval.manifest().ceremony)
            })
            .ok_or_else(|| rejected(RegistryFailure::Missing))?;
        entry.require_original(approval)?;
        Ok(Arc::clone(entry))
    }

    pub(super) fn remove_original_preparation(
        &self,
        original: &Arc<PreparedIssuerEntry>,
    ) -> Result<(), AuraError> {
        let mut entries = self
            .prepared
            .try_lock()
            .map_err(|source| AuraError::Internal {
                message: "original preparation settlement is contended".into(),
                source: Some(Arc::new(source)),
            })?;
        match entries.get(&original.preparation) {
            Some(actual) if Arc::ptr_eq(actual, original) => {
                entries.remove(&original.preparation);
                Ok(())
            }
            None => Ok(()),
            Some(_) => Err(rejected(RegistryFailure::Binding)),
        }
    }
    #[cfg(test)]
    pub(super) fn original_peer_disposal_for_test(
        &self,
        subject: AuthorityId,
        ceremony: &CeremonyId,
        device: DeviceId,
    ) -> Result<crate::task_registry::OriginalRequiredTaskDisposalObserver, AuraError> {
        let entries = self.participants.try_lock().map_err(|source| {
            AuraError::crypto_with_source("observe original peer disposal", Arc::new(source))
        })?;
        let entry = entries
            .get(&(subject, ceremony.clone(), device))
            .ok_or_else(|| rejected(RegistryFailure::Missing))?;
        Ok(entry.owner.task.disposal_observer())
    }
    pub(super) fn original_peer_continuation(
        &self,
        subject: AuthorityId,
        ceremony: &CeremonyId,
        device: DeviceId,
    ) -> Result<OriginalPeerContinuationObservation, AuraError> {
        let entries = self
            .participants
            .try_lock()
            .map_err(|source| AuraError::Internal {
                message: "original participant registry remains borrowed".into(),
                source: Some(Arc::new(source)),
            })?;
        let entry = entries
            .get(&(subject, ceremony.clone(), device))
            .ok_or_else(|| rejected(RegistryFailure::Missing))?;
        Ok((
            Arc::clone(&entry.owner.peer_continuation),
            Arc::clone(&entry.owner.original_window),
            Arc::clone(&entry.owner.effects),
        ))
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
