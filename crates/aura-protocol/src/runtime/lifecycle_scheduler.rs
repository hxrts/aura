//! Lightweight scheduler that drives protocol-core lifecycles.
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use super::shared_adapters::EnvironmentBundle;
use crate::core::capabilities::{EffectsProvider, ProtocolEffects};
use crate::core::lifecycle::{ProtocolInput, ProtocolLifecycle};
use crate::protocols::{
    CounterLifecycle, DkdLifecycle, LockingLifecycle, RecoveryLifecycle, ResharingLifecycle,
    StorageLifecycle,
};
use crate::tracing::protocol::ProtocolTracer;
use crate::{MemoryTransport, Transport};
use aura_authentication::EventAuthorization;
use aura_crypto::Effects;
use aura_journal::{
    events::{IncrementCounterEvent, RelationshipId, ReserveCounterRangeEvent},
    protocols::events::{Event, EventType},
    AccountLedger, AccountState, DeviceMetadata, DeviceType,
};
use aura_types::{AccountId, DeviceId, GuardianId, SessionId as CoreSessionId};
use aura_types::{AuraError, AuraResult};
use ed25519_dalek::VerifyingKey;
use tokio::sync::RwLock;
use tracing::{info, warn};
use uuid::Uuid;

use super::super::protocol_results::{
    CounterProtocolResult, DkdProtocolResult, LockingProtocolResult, RecoveryProtocolResult,
    ResharingProtocolResult,
};

/// Error returned when driving a lifecycle fails.
#[derive(Debug, thiserror::Error)]
pub enum LifecycleSchedulerError {
    #[error("protocol lifecycle emitted no outcome")]
    MissingOutcome,
    #[error("protocol lifecycle failed: {0}")]
    LifecycleFailure(String),
    #[error("transport error: {0}")]
    Transport(String),
    #[error("internal coordination error: {0}")]
    Coordination(String),
}

impl From<AuraError> for LifecycleSchedulerError {
    fn from(err: AuraError) -> Self {
        LifecycleSchedulerError::Coordination(err.to_string())
    }
}

async fn process_effects(
    effects: &[ProtocolEffects],
    ledger_handle: &Arc<RwLock<AccountLedger>>,
    shared_effects: &SharedEffects,
) -> Result<(), LifecycleSchedulerError> {
    if effects.is_empty() {
        return Ok(());
    }

    for effect in effects {
        match effect {
            ProtocolEffects::UpdateCounter {
                relationship_hash,
                previous_value,
                reserved_values,
                ttl_epochs,
                requested_epoch,
                requesting_device,
            } => {
                if reserved_values.is_empty() {
                    continue;
                }
                let mut ledger = ledger_handle.write().await;
                let snapshot_state = ledger.state();

                let account_id = snapshot_state.account_id;
                let parent_hash = snapshot_state.last_event_hash;
                let nonce = shared_effects.next_counter();

                let relationship_id = RelationshipId(*relationship_hash);
                let highest_value = reserved_values
                    .iter()
                    .copied()
                    .max()
                    .unwrap_or(*previous_value);
                let snapshot = shared_effects.snapshot();

                let event_type = if reserved_values.len() == 1 {
                    EventType::IncrementCounter(IncrementCounterEvent {
                        relationship_id,
                        requesting_device: *requesting_device,
                        new_counter_value: highest_value,
                        previous_counter_value: *previous_value,
                        requested_at_epoch: *requested_epoch,
                        ttl_epochs: *ttl_epochs,
                    })
                } else {
                    let start_counter = previous_value.saturating_add(1);
                    EventType::ReserveCounterRange(ReserveCounterRangeEvent {
                        relationship_id,
                        requesting_device: *requesting_device,
                        start_counter,
                        range_size: reserved_values.len() as u64,
                        previous_counter_value: *previous_value,
                        requested_at_epoch: *requested_epoch,
                        ttl_epochs: *ttl_epochs,
                    })
                };

                let event = Event::new(
                    account_id,
                    nonce,
                    parent_hash,
                    *requested_epoch,
                    event_type,
                    EventAuthorization::LifecycleInternal,
                    &snapshot,
                )
                .map_err(|e| LifecycleSchedulerError::Coordination(e))?;

                ledger
                    .append_event(event, &snapshot)
                    .map_err(|e| LifecycleSchedulerError::Coordination(format!("{}", e)))?;
            }
            _ => {
                warn!("Unhandled protocol effect: {:?}", effect);
            }
        }
    }

    Ok(())
}

/// Scheduler that drives protocol lifecycles using the new unified abstractions.
#[derive(Clone)]
pub struct LifecycleScheduler {
    effects: SharedEffects,
    environment: Option<EnvironmentBundle>,
    tracer: Option<Arc<RwLock<ProtocolTracer>>>,
}

impl LifecycleScheduler {
    /// Create a new scheduler with production effects and a no-op transport.
    pub fn new() -> Self {
        Self::with_effects(Effects::production())
    }

    /// Create a scheduler using custom effects (useful for tests).
    pub fn with_effects(effects: Effects) -> Self {
        Self::with_shared_effects(Arc::new(effects))
    }

    /// Create a scheduler backed by shared effects.
    pub fn with_shared_effects(effects: Arc<Effects>) -> Self {
        Self {
            effects: SharedEffects::new(effects),
            environment: None,
            tracer: None,
        }
    }

    /// Set environment bundle for production execution.
    ///
    /// This replaces the temporary ledger/transport fallbacks with real implementations.
    pub fn set_environment(&mut self, environment: EnvironmentBundle) {
        info!("Setting environment bundle for scheduler");

        // Trace environment injection if tracer is available
        if let Some(tracer) = &self.tracer {
            if let Ok(tracer) = tracer.try_read() {
                tracer.trace_environment_injection(
                    true, // journal is always present in EnvironmentBundle
                    true, // transport is always present in EnvironmentBundle
                );
            }
        }

        self.environment = Some(environment);
    }

    /// Set tracer for lifecycle events
    pub fn set_tracer(&mut self, tracer: Arc<RwLock<ProtocolTracer>>) {
        self.tracer = Some(tracer);
    }

    /// Create a scheduler with environment bundle already configured.
    pub fn with_environment(effects: Arc<Effects>, environment: EnvironmentBundle) -> Self {
        info!("Creating scheduler with pre-configured environment");
        Self {
            effects: SharedEffects::new(effects),
            environment: Some(environment),
            tracer: None,
        }
    }

    /// Get environment metrics if available.
    pub async fn environment_metrics(&self) -> Option<super::shared_adapters::EnvironmentMetrics> {
        match &self.environment {
            Some(env) => {
                let metrics = env.combined_metrics().await;

                // Trace peer metrics if tracer is available
                if let Some(tracer) = &self.tracer {
                    if let Ok(tracer) = tracer.try_read() {
                        tracer.trace_peer_metrics(
                            0, // TODO: Add active sessions tracking
                            metrics.transport.connected_peers as u64,
                            0, // TODO: Add completed sessions tracking
                        );
                    }
                }

                Some(metrics)
            }
            None => None,
        }
    }

    /// Execute the counter reservation lifecycle to completion.
    #[allow(clippy::too_many_arguments)]
    pub async fn execute_counter_reservation(
        &self,
        session_id: Option<CoreSessionId>,
        account_id: AccountId,
        device_id: DeviceId,
        relationship_id: RelationshipId,
        requesting_device: DeviceId,
        count: u64,
        ttl_epochs: u64,
        ledger: Option<Arc<RwLock<AccountLedger>>>,
        transport_override: Option<Arc<dyn Transport>>,
    ) -> Result<CounterProtocolResult, LifecycleSchedulerError> {
        let session_uuid = session_id
            .map(|id| id.uuid())
            .unwrap_or_else(|| self.effects.gen_uuid());

        let group_public_key = VerifyingKey::from_bytes(&[0u8; 32]).expect("default verifying key");
        let quorum = count.max(1) as u16;

        let ledger_handle = match (ledger, &self.environment) {
            (Some(existing), _) => existing,
            (None, Some(env)) => {
                info!("Using shared journal adapter for counter execution");
                env.journal.ledger()
            }
            (None, None) => {
                warn!(
                    "No ledger provided and no environment configured, creating temporary ledger"
                );
                let initial_device = DeviceMetadata {
                    device_id,
                    device_name: format!("device-{}", device_id.0),
                    device_type: DeviceType::Native,
                    public_key: group_public_key,
                    added_at: self.effects.now().unwrap_or(0),
                    last_seen: self.effects.now().unwrap_or(0),
                    dkd_commitment_proofs: BTreeMap::new(),
                    next_nonce: 1,
                    used_nonces: BTreeSet::new(),
                };

                let initial_state =
                    AccountState::new(account_id, group_public_key, initial_device, quorum, quorum);

                Arc::new(RwLock::new(AccountLedger::new(initial_state).map_err(
                    |e| LifecycleSchedulerError::Coordination(format!("{:?}", e)),
                )?))
            }
        };

        let _transport = match (transport_override, &self.environment) {
            (Some(override_transport), _) => override_transport,
            (None, Some(env)) => {
                info!("Using shared transport adapter for counter execution");
                env.transport.transport()
            }
            (None, None) => {
                warn!("No transport provided and no environment configured, using stub transport");
                Arc::new(MemoryTransport::default()) as Arc<dyn Transport>
            }
        };

        let session_id_core = aura_types::SessionId::from_uuid(session_uuid);
        let (base_counter, current_epoch, participants_snapshot) = {
            let ledger = ledger_handle.read().await;
            let state = ledger.state();
            let base = state
                .relationship_counters
                .get(&relationship_id)
                .map(|(counter, _ttl)| *counter)
                .unwrap_or(0);
            let epoch = state.session_epoch.0;
            let participants = state.devices.keys().copied().collect::<Vec<_>>();
            (base, epoch, participants)
        };

        let mut counter_lifecycle = CounterLifecycle::new(
            device_id,
            session_id_core,
            relationship_id,
            requesting_device,
            participants_snapshot.clone(),
            count,
            ttl_epochs,
            current_epoch,
            base_counter,
        );

        if let Some(tracer) = &self.tracer {
            if let Ok(mut tracer) = tracer.try_write() {
                let _ = tracer.trace_lifecycle_start(counter_lifecycle.descriptor());
            }
        }

        let effects_provider = &self.effects as &dyn crate::core::capabilities::EffectsProvider;
        let stub_transport = StubProtocolTransportAdapter::new();

        let mut capabilities = crate::core::capabilities::ProtocolCapabilities {
            effects: effects_provider,
            transport: &stub_transport,
            storage: None,
            access: None,
            ledger: None,
        };

        let input = ProtocolInput::LocalSignal {
            signal: "complete",
            data: Some(&serde_json::json!({
                "relationship_id": hex::encode(relationship_id.0),
                "requesting_device": requesting_device.to_string(),
                "count": count,
                "ttl_epochs": ttl_epochs,
            })),
        };

        loop {
            let step = counter_lifecycle.step(input.clone(), &mut capabilities);

            if let Some(tracer) = &self.tracer {
                if let Ok(tracer) = tracer.try_read() {
                    tracer.trace_lifecycle_step(counter_lifecycle.descriptor(), &step);
                }
            }

            process_effects(&step.effects, &ledger_handle, &self.effects).await?;

            match step.outcome {
                Some(Ok(result)) => {
                    if let Some(tracer) = &self.tracer {
                        if let Ok(tracer) = tracer.try_read() {
                            tracer.trace_lifecycle_complete(counter_lifecycle.descriptor(), true);
                        }
                    }
                    return Ok(result);
                }
                Some(Err(e)) => {
                    if let Some(tracer) = &self.tracer {
                        if let Ok(tracer) = tracer.try_read() {
                            tracer.trace_lifecycle_complete(counter_lifecycle.descriptor(), false);
                        }
                    }
                    return Err(LifecycleSchedulerError::LifecycleFailure(format!(
                        "{:?}",
                        e
                    )));
                }
                None => {
                    if counter_lifecycle.is_final() {
                        return Err(LifecycleSchedulerError::MissingOutcome);
                    }
                    break;
                }
            }
        }

        let final_step = counter_lifecycle.step(input, &mut capabilities);
        process_effects(&final_step.effects, &ledger_handle, &self.effects).await?;
        match final_step.outcome {
            Some(Ok(result)) => Ok(result),
            Some(Err(e)) => Err(LifecycleSchedulerError::LifecycleFailure(format!(
                "{:?}",
                e
            ))),
            None => Err(LifecycleSchedulerError::MissingOutcome),
        }
    }

    /// Execute the locking lifecycle to completion.
    pub async fn execute_locking(
        &self,
        session_id: Option<CoreSessionId>,
        account_id: AccountId,
        device_id: DeviceId,
        operation_type: aura_journal::OperationType,
        contenders: Vec<DeviceId>,
        ledger: Option<Arc<RwLock<AccountLedger>>>,
        transport_override: Option<Arc<dyn Transport>>,
    ) -> Result<LockingProtocolResult, LifecycleSchedulerError> {
        let session_uuid = session_id
            .map(|id| id.uuid())
            .unwrap_or_else(|| self.effects.gen_uuid());

        let group_public_key = VerifyingKey::from_bytes(&[0u8; 32]).expect("default verifying key");

        let threshold = contenders.len().max(1) as u16;

        let _ledger = match (ledger, &self.environment) {
            (Some(existing), _) => existing,
            (None, Some(env)) => {
                info!("Using shared journal adapter for locking execution");
                env.journal.ledger()
            }
            (None, None) => {
                warn!(
                    "No ledger provided and no environment configured, creating temporary ledger"
                );
                let initial_device = DeviceMetadata {
                    device_id,
                    device_name: format!("device-{}", device_id.0),
                    device_type: DeviceType::Native,
                    public_key: group_public_key,
                    added_at: self.effects.now().unwrap_or(0),
                    last_seen: self.effects.now().unwrap_or(0),
                    dkd_commitment_proofs: BTreeMap::new(),
                    next_nonce: 1,
                    used_nonces: BTreeSet::new(),
                };

                let initial_state = AccountState::new(
                    account_id,
                    group_public_key,
                    initial_device,
                    threshold,
                    threshold,
                );

                Arc::new(RwLock::new(AccountLedger::new(initial_state).map_err(
                    |e| LifecycleSchedulerError::Coordination(format!("{:?}", e)),
                )?))
            }
        };

        let _transport = match (transport_override, &self.environment) {
            (Some(override_transport), _) => override_transport,
            (None, Some(env)) => {
                info!("Using shared transport adapter for locking execution");
                env.transport.transport()
            }
            (None, None) => {
                warn!("No transport provided and no environment configured, using stub transport");
                Arc::new(MemoryTransport::default()) as Arc<dyn Transport>
            }
        };

        let session_id_core = aura_types::SessionId::from_uuid(session_uuid);

        let mut locking_lifecycle = LockingLifecycle::new(
            device_id,
            session_id_core,
            operation_type,
            contenders.clone(),
        );

        if let Some(tracer) = &self.tracer {
            if let Ok(mut tracer) = tracer.try_write() {
                let _ = tracer.trace_lifecycle_start(locking_lifecycle.descriptor());
            }
        }

        let effects_provider = &self.effects as &dyn crate::core::capabilities::EffectsProvider;
        let stub_transport = StubProtocolTransportAdapter::new();

        let mut capabilities = crate::core::capabilities::ProtocolCapabilities {
            effects: effects_provider,
            transport: &stub_transport,
            storage: None,
            access: None,
            ledger: None,
        };

        let input = ProtocolInput::LocalSignal {
            signal: "complete",
            data: Some(&serde_json::json!({
                "operation_type": format!("{:?}", operation_type),
                "contenders": contenders.iter().map(|d| d.to_string()).collect::<Vec<_>>(),
            })),
        };

        loop {
            let step = locking_lifecycle.step(input.clone(), &mut capabilities);

            if let Some(tracer) = &self.tracer {
                if let Ok(tracer) = tracer.try_read() {
                    tracer.trace_lifecycle_step(locking_lifecycle.descriptor(), &step);
                }
            }

            match step.outcome {
                Some(Ok(result)) => {
                    if let Some(tracer) = &self.tracer {
                        if let Ok(tracer) = tracer.try_read() {
                            tracer.trace_lifecycle_complete(locking_lifecycle.descriptor(), true);
                        }
                    }
                    return Ok(result);
                }
                Some(Err(e)) => {
                    if let Some(tracer) = &self.tracer {
                        if let Ok(tracer) = tracer.try_read() {
                            tracer.trace_lifecycle_complete(locking_lifecycle.descriptor(), false);
                        }
                    }
                    return Err(LifecycleSchedulerError::LifecycleFailure(format!(
                        "{:?}",
                        e
                    )));
                }
                None => {
                    if locking_lifecycle.is_final() {
                        return Err(LifecycleSchedulerError::MissingOutcome);
                    }
                    break;
                }
            }
        }

        let final_step = locking_lifecycle.step(input, &mut capabilities);
        match final_step.outcome {
            Some(Ok(result)) => Ok(result),
            Some(Err(e)) => Err(LifecycleSchedulerError::LifecycleFailure(format!(
                "{:?}",
                e
            ))),
            None => Err(LifecycleSchedulerError::MissingOutcome),
        }
    }

    /// Execute the DKD lifecycle to completion using the provided participants.
    pub async fn execute_dkd(
        &self,
        session_id: Option<CoreSessionId>,
        account_id: AccountId,
        device_id: DeviceId,
        app_id: String,
        context_label: String,
        participants: Vec<DeviceId>,
        threshold: usize,
        context_bytes: Vec<u8>,
        ledger: Option<Arc<RwLock<AccountLedger>>>,
        transport_override: Option<Arc<dyn Transport>>,
    ) -> Result<DkdProtocolResult, LifecycleSchedulerError> {
        let session_uuid = session_id
            .map(|id| id.uuid())
            .unwrap_or_else(|| self.effects.gen_uuid());

        let group_public_key = VerifyingKey::from_bytes(&[0u8; 32]).expect("default verifying key");

        let threshold_u16 = if threshold > u16::MAX as usize {
            u16::MAX
        } else {
            threshold as u16
        };

        info!(
            session = %session_uuid,
            app = %app_id,
            context = %context_label,
            participants = %participants.len(),
            threshold = threshold,
            "Executing DKD lifecycle via scheduler"
        );

        let _ledger = match (ledger, &self.environment) {
            (Some(existing), _) => existing,
            (None, Some(env)) => {
                info!("Using shared journal adapter for DKD execution");
                env.journal.ledger()
            }
            (None, None) => {
                warn!(
                    "No ledger provided and no environment configured, creating temporary ledger"
                );
                let initial_device = DeviceMetadata {
                    device_id,
                    device_name: format!("device-{}", device_id.0),
                    device_type: DeviceType::Native,
                    public_key: group_public_key,
                    added_at: self.effects.now().unwrap_or(0),
                    last_seen: self.effects.now().unwrap_or(0),
                    dkd_commitment_proofs: BTreeMap::new(),
                    next_nonce: 1,
                    used_nonces: BTreeSet::new(),
                };

                let initial_state = AccountState::new(
                    account_id,
                    group_public_key,
                    initial_device,
                    threshold_u16,
                    (participants.len() as u16).max(threshold_u16),
                );

                Arc::new(RwLock::new(AccountLedger::new(initial_state).map_err(
                    |e| LifecycleSchedulerError::Coordination(format!("{:?}", e)),
                )?))
            }
        };

        let _transport = match (transport_override, &self.environment) {
            (Some(override_transport), _) => override_transport,
            (None, Some(env)) => {
                info!("Using shared transport adapter for DKD execution");
                env.transport.transport()
            }
            (None, None) => {
                warn!("No transport provided and no environment configured, using stub transport");
                Arc::new(MemoryTransport::default()) as Arc<dyn Transport>
            }
        };
        let session_id_core = aura_types::SessionId::from_uuid(session_uuid);

        // Create DKD lifecycle instance
        let mut dkd_lifecycle = DkdLifecycle::new(
            device_id,
            session_id_core,
            context_bytes.clone(),
            participants.clone(),
        );

        // Trace lifecycle start if tracer is available
        if let Some(tracer) = &self.tracer {
            if let Ok(mut tracer) = tracer.try_write() {
                let _traced_op = tracer.trace_lifecycle_start(dkd_lifecycle.descriptor());
            }
        }

        // Create capabilities bundle for protocol execution
        let effects_provider = &self.effects as &dyn crate::core::capabilities::EffectsProvider;
        let stub_transport = StubProtocolTransportAdapter::new();

        let mut capabilities = crate::core::capabilities::ProtocolCapabilities {
            effects: effects_provider,
            transport: &stub_transport,
            storage: None,
            access: None,
            ledger: None,
        };

        // Send completion signal to trigger protocol execution
        let input = ProtocolInput::LocalSignal {
            signal: "complete",
            data: Some(&serde_json::json!({
                "app_id": app_id,
                "context_label": context_label,
                "threshold": threshold,
                "context_bytes": context_bytes
            })),
        };

        loop {
            let step = dkd_lifecycle.step(input.clone(), &mut capabilities);

            // Trace the step if tracer is available
            if let Some(tracer) = &self.tracer {
                if let Ok(tracer) = tracer.try_read() {
                    tracer.trace_lifecycle_step(dkd_lifecycle.descriptor(), &step);
                }
            }

            match step.outcome {
                Some(Ok(result)) => {
                    // Trace completion if tracer is available
                    if let Some(tracer) = &self.tracer {
                        if let Ok(tracer) = tracer.try_read() {
                            tracer.trace_lifecycle_complete(dkd_lifecycle.descriptor(), true);
                        }
                    }
                    return Ok(result);
                }
                Some(Err(e)) => {
                    // Trace failure if tracer is available
                    if let Some(tracer) = &self.tracer {
                        if let Ok(tracer) = tracer.try_read() {
                            tracer.trace_lifecycle_complete(dkd_lifecycle.descriptor(), false);
                        }
                    }
                    return Err(LifecycleSchedulerError::LifecycleFailure(format!(
                        "{:?}",
                        e
                    )));
                }
                None => {
                    // Protocol is still running, continue with next step
                    if dkd_lifecycle.is_final() {
                        return Err(LifecycleSchedulerError::MissingOutcome);
                    }
                    // In a real implementation, we would process effects and continue
                    // For now, break to avoid infinite loop since this is a placeholder
                    break;
                }
            }
        }

        // Protocol didn't complete in loop, try one final step
        let final_step = dkd_lifecycle.step(input, &mut capabilities);
        match final_step.outcome {
            Some(Ok(result)) => Ok(result),
            Some(Err(e)) => Err(LifecycleSchedulerError::LifecycleFailure(format!(
                "{:?}",
                e
            ))),
            None => Err(LifecycleSchedulerError::MissingOutcome),
        }
    }

    /// Execute the resharing lifecycle to completion.
    pub async fn execute_resharing(
        &self,
        session_id: Option<CoreSessionId>,
        account_id: AccountId,
        device_id: DeviceId,
        old_participants: Vec<DeviceId>,
        new_participants: Vec<DeviceId>,
        new_threshold: u16,
        ledger: Option<Arc<RwLock<AccountLedger>>>,
        transport_override: Option<Arc<dyn Transport>>,
    ) -> Result<ResharingProtocolResult, LifecycleSchedulerError> {
        let session_uuid = session_id
            .map(|id| id.uuid())
            .unwrap_or_else(|| self.effects.gen_uuid());

        let group_public_key = VerifyingKey::from_bytes(&[0u8; 32]).expect("default verifying key");

        let _ledger = match (ledger, &self.environment) {
            (Some(existing), _) => existing,
            (None, Some(env)) => {
                info!("Using shared journal adapter for resharing execution");
                env.journal.ledger()
            }
            (None, None) => {
                warn!(
                    "No ledger provided and no environment configured, creating temporary ledger"
                );
                let initial_device = DeviceMetadata {
                    device_id,
                    device_name: format!("device-{}", device_id.0),
                    device_type: DeviceType::Native,
                    public_key: group_public_key,
                    added_at: self.effects.now().unwrap_or(0),
                    last_seen: self.effects.now().unwrap_or(0),
                    dkd_commitment_proofs: BTreeMap::new(),
                    next_nonce: 1,
                    used_nonces: BTreeSet::new(),
                };

                let initial_state = AccountState::new(
                    account_id,
                    group_public_key,
                    initial_device,
                    new_threshold,
                    (old_participants.len() as u16).max(new_threshold),
                );

                Arc::new(RwLock::new(AccountLedger::new(initial_state).map_err(
                    |e| LifecycleSchedulerError::Coordination(format!("{:?}", e)),
                )?))
            }
        };

        let _transport = match (transport_override, &self.environment) {
            (Some(override_transport), _) => override_transport,
            (None, Some(env)) => {
                info!("Using shared transport adapter for resharing execution");
                env.transport.transport()
            }
            (None, None) => {
                warn!("No transport provided and no environment configured, using stub transport");
                Arc::new(MemoryTransport::default()) as Arc<dyn Transport>
            }
        };
        let session_id_core = aura_types::SessionId::from_uuid(session_uuid);

        // Create Resharing lifecycle instance
        let mut resharing_lifecycle = ResharingLifecycle::new(
            device_id,
            session_id_core,
            old_participants.clone(),
            new_participants.clone(),
            new_threshold,
        );

        // Trace lifecycle start if tracer is available
        if let Some(tracer) = &self.tracer {
            if let Ok(mut tracer) = tracer.try_write() {
                let _traced_op = tracer.trace_lifecycle_start(resharing_lifecycle.descriptor());
            }
        }

        // Create capabilities bundle for protocol execution
        let effects_provider = &self.effects as &dyn crate::core::capabilities::EffectsProvider;
        let stub_transport = StubProtocolTransportAdapter::new();

        let mut capabilities = crate::core::capabilities::ProtocolCapabilities {
            effects: effects_provider,
            transport: &stub_transport,
            storage: None,
            access: None,
            ledger: None,
        };

        // Send completion signal to trigger protocol execution
        let input = ProtocolInput::LocalSignal {
            signal: "complete",
            data: Some(&serde_json::json!({
                "old_participants": old_participants.iter().map(|d| d.to_string()).collect::<Vec<_>>(),
                "new_participants": new_participants.iter().map(|d| d.to_string()).collect::<Vec<_>>(),
                "new_threshold": new_threshold
            })),
        };

        loop {
            let step = resharing_lifecycle.step(input.clone(), &mut capabilities);

            // Trace the step if tracer is available
            if let Some(tracer) = &self.tracer {
                if let Ok(tracer) = tracer.try_read() {
                    tracer.trace_lifecycle_step(resharing_lifecycle.descriptor(), &step);
                }
            }

            match step.outcome {
                Some(Ok(result)) => {
                    // Trace completion if tracer is available
                    if let Some(tracer) = &self.tracer {
                        if let Ok(tracer) = tracer.try_read() {
                            tracer.trace_lifecycle_complete(resharing_lifecycle.descriptor(), true);
                        }
                    }
                    return Ok(result);
                }
                Some(Err(e)) => {
                    // Trace failure if tracer is available
                    if let Some(tracer) = &self.tracer {
                        if let Ok(tracer) = tracer.try_read() {
                            tracer
                                .trace_lifecycle_complete(resharing_lifecycle.descriptor(), false);
                        }
                    }
                    return Err(LifecycleSchedulerError::LifecycleFailure(format!(
                        "{:?}",
                        e
                    )));
                }
                None => {
                    // Protocol is still running, continue with next step
                    if resharing_lifecycle.is_final() {
                        return Err(LifecycleSchedulerError::MissingOutcome);
                    }
                    // In a real implementation, we would process effects and continue
                    // For now, break to avoid infinite loop since this is a placeholder
                    break;
                }
            }
        }

        // Protocol didn't complete in loop, try one final step
        let final_step = resharing_lifecycle.step(input, &mut capabilities);
        match final_step.outcome {
            Some(Ok(result)) => Ok(result),
            Some(Err(e)) => Err(LifecycleSchedulerError::LifecycleFailure(format!(
                "{:?}",
                e
            ))),
            None => Err(LifecycleSchedulerError::MissingOutcome),
        }
    }

    /// Execute the recovery lifecycle to completion.
    pub async fn execute_recovery(
        &self,
        session_id: Option<CoreSessionId>,
        account_id: AccountId,
        device_id: DeviceId,
        approving_guardians: Vec<GuardianId>,
        _new_device_id: DeviceId,
        guardian_threshold: usize,
        ledger: Option<Arc<RwLock<AccountLedger>>>,
        transport_override: Option<Arc<dyn Transport>>,
    ) -> Result<RecoveryProtocolResult, LifecycleSchedulerError> {
        let session_uuid = session_id
            .map(|id| id.uuid())
            .unwrap_or_else(|| self.effects.gen_uuid());

        let group_public_key = VerifyingKey::from_bytes(&[0u8; 32]).expect("default verifying key");

        let _ledger = match (ledger, &self.environment) {
            (Some(existing), _) => existing,
            (None, Some(env)) => {
                info!("Using shared journal adapter for recovery execution");
                env.journal.ledger()
            }
            (None, None) => {
                warn!(
                    "No ledger provided and no environment configured, creating temporary ledger"
                );
                let initial_device = DeviceMetadata {
                    device_id,
                    device_name: format!("device-{}", device_id.0),
                    device_type: DeviceType::Native,
                    public_key: group_public_key,
                    added_at: self.effects.now().unwrap_or(0),
                    last_seen: self.effects.now().unwrap_or(0),
                    dkd_commitment_proofs: BTreeMap::new(),
                    next_nonce: 1,
                    used_nonces: BTreeSet::new(),
                };

                let initial_state = AccountState::new(
                    account_id,
                    group_public_key,
                    initial_device,
                    approving_guardians.len() as u16,
                    approving_guardians.len() as u16,
                );

                Arc::new(RwLock::new(AccountLedger::new(initial_state).map_err(
                    |e| LifecycleSchedulerError::Coordination(format!("{:?}", e)),
                )?))
            }
        };

        let _transport = match (transport_override, &self.environment) {
            (Some(override_transport), _) => override_transport,
            (None, Some(env)) => {
                info!("Using shared transport adapter for recovery execution");
                env.transport.transport()
            }
            (None, None) => {
                warn!("No transport provided and no environment configured, using stub transport");
                Arc::new(MemoryTransport::default()) as Arc<dyn Transport>
            }
        };
        let session_id_core = aura_types::SessionId::from_uuid(session_uuid);

        let guardian_threshold_u16 = if guardian_threshold > u16::MAX as usize {
            u16::MAX
        } else {
            guardian_threshold as u16
        };

        // Create Recovery lifecycle instance (new_device_id is the device being recovered)
        let new_device_id = DeviceId(self.effects.gen_uuid()); // Generate new device ID for recovery
        let mut recovery_lifecycle = RecoveryLifecycle::new(
            device_id,
            session_id_core,
            approving_guardians.clone(),
            new_device_id,
        );

        // Trace lifecycle start if tracer is available
        if let Some(tracer) = &self.tracer {
            if let Ok(mut tracer) = tracer.try_write() {
                let _traced_op = tracer.trace_lifecycle_start(recovery_lifecycle.descriptor());
            }
        }

        // Create capabilities bundle for protocol execution
        let effects_provider = &self.effects as &dyn crate::core::capabilities::EffectsProvider;
        let stub_transport = StubProtocolTransportAdapter::new();

        let mut capabilities = crate::core::capabilities::ProtocolCapabilities {
            effects: effects_provider,
            transport: &stub_transport,
            storage: None,
            access: None,
            ledger: None,
        };

        // Send completion signal to trigger protocol execution
        let input = ProtocolInput::LocalSignal {
            signal: "complete",
            data: Some(&serde_json::json!({
                "approving_guardians": approving_guardians.iter().map(|g| g.to_string()).collect::<Vec<_>>(),
                "guardian_threshold": guardian_threshold_u16
            })),
        };

        loop {
            let step = recovery_lifecycle.step(input.clone(), &mut capabilities);

            // Trace the step if tracer is available
            if let Some(tracer) = &self.tracer {
                if let Ok(tracer) = tracer.try_read() {
                    tracer.trace_lifecycle_step(recovery_lifecycle.descriptor(), &step);
                }
            }

            match step.outcome {
                Some(Ok(result)) => {
                    // Trace completion if tracer is available
                    if let Some(tracer) = &self.tracer {
                        if let Ok(tracer) = tracer.try_read() {
                            tracer.trace_lifecycle_complete(recovery_lifecycle.descriptor(), true);
                        }
                    }
                    return Ok(result);
                }
                Some(Err(e)) => {
                    // Trace failure if tracer is available
                    if let Some(tracer) = &self.tracer {
                        if let Ok(tracer) = tracer.try_read() {
                            tracer.trace_lifecycle_complete(recovery_lifecycle.descriptor(), false);
                        }
                    }
                    return Err(LifecycleSchedulerError::LifecycleFailure(format!(
                        "{:?}",
                        e
                    )));
                }
                None => {
                    // Protocol is still running, continue with next step
                    if recovery_lifecycle.is_final() {
                        return Err(LifecycleSchedulerError::MissingOutcome);
                    }
                    // In a real implementation, we would process effects and continue
                    // For now, break to avoid infinite loop since this is a placeholder
                    break;
                }
            }
        }

        // Protocol didn't complete in loop, try one final step
        let final_step = recovery_lifecycle.step(input, &mut capabilities);
        match final_step.outcome {
            Some(Ok(result)) => Ok(result),
            Some(Err(e)) => Err(LifecycleSchedulerError::LifecycleFailure(format!(
                "{:?}",
                e
            ))),
            None => Err(LifecycleSchedulerError::MissingOutcome),
        }
    }

    /// Execute a storage lifecycle (store, retrieve, or delete operation).
    pub async fn execute_storage(
        &self,
        session_id: Option<CoreSessionId>,
        account_id: AccountId,
        device_id: DeviceId,
        operation_type: crate::protocols::StorageOperationType,
        blob_id: aura_types::Cid,
        metadata: crate::protocols::BlobMetadata,
        ledger: Option<Arc<RwLock<AccountLedger>>>,
        transport_override: Option<Arc<dyn Transport>>,
    ) -> Result<(), LifecycleSchedulerError> {
        let session_uuid = session_id
            .map(|id| id.uuid())
            .unwrap_or_else(|| self.effects.gen_uuid());

        let group_public_key = VerifyingKey::from_bytes(&[0u8; 32]).expect("default verifying key");

        let _ledger = match (ledger, &self.environment) {
            (Some(existing), _) => existing,
            (None, Some(env)) => {
                info!("Using shared journal adapter for storage execution");
                env.journal.ledger()
            }
            (None, None) => {
                warn!(
                    "No ledger provided and no environment configured, creating temporary ledger"
                );
                let initial_device = DeviceMetadata {
                    device_id,
                    device_name: format!("device-{}", device_id.0),
                    device_type: DeviceType::Native,
                    public_key: group_public_key,
                    added_at: self.effects.now().unwrap_or(0),
                    last_seen: self.effects.now().unwrap_or(0),
                    dkd_commitment_proofs: BTreeMap::new(),
                    next_nonce: 1,
                    used_nonces: BTreeSet::new(),
                };

                let initial_state = AccountState::new(
                    account_id,
                    group_public_key,
                    initial_device,
                    1, // threshold
                    1, // total devices
                );

                Arc::new(RwLock::new(AccountLedger::new(initial_state).map_err(
                    |e| LifecycleSchedulerError::Coordination(format!("{:?}", e)),
                )?))
            }
        };

        let _transport = match (transport_override, &self.environment) {
            (Some(override_transport), _) => override_transport,
            (None, Some(env)) => {
                info!("Using shared transport adapter for storage execution");
                env.transport.transport()
            }
            (None, None) => {
                warn!("No transport provided and no environment configured, using stub transport");
                Arc::new(MemoryTransport::default()) as Arc<dyn Transport>
            }
        };
        let _session_id_core = aura_types::SessionId::from_uuid(session_uuid);

        // Create Storage lifecycle instance
        let mut storage_lifecycle = match operation_type {
            crate::protocols::StorageOperationType::Store => StorageLifecycle::new_store(
                session_uuid,
                account_id,
                device_id,
                blob_id.clone(),
                metadata,
                self.effects.snapshot(),
            ),
            crate::protocols::StorageOperationType::Retrieve => {
                // Create a placeholder capability token
                let placeholder_token = aura_authorization::CapabilityToken::new(
                    aura_authorization::Subject::Device(device_id),
                    aura_authorization::Resource::StorageObject {
                        object_id: uuid::Uuid::new_v4(),
                        owner: account_id,
                    },
                    vec![aura_authorization::Action::Read],
                    device_id,
                    false, // not delegatable
                    0,     // no delegation depth
                );
                StorageLifecycle::new_retrieve(
                    session_uuid,
                    account_id,
                    device_id,
                    blob_id.clone(),
                    placeholder_token,
                    self.effects.snapshot(),
                )
            }
            crate::protocols::StorageOperationType::Delete => StorageLifecycle::new_delete(
                session_uuid,
                account_id,
                device_id,
                blob_id.clone(),
                Some("User requested deletion".to_string()),
                self.effects.snapshot(),
            ),
        };

        // Trace lifecycle start if tracer is available
        if let Some(tracer) = &self.tracer {
            if let Ok(mut tracer) = tracer.try_write() {
                let _traced_op = tracer.trace_lifecycle_start(&storage_lifecycle.descriptor());
            }
        }

        // Create capabilities bundle for protocol execution
        let effects_provider = &self.effects as &dyn crate::core::capabilities::EffectsProvider;
        let stub_transport = StubProtocolTransportAdapter::new();

        let mut capabilities = crate::core::capabilities::ProtocolCapabilities {
            effects: effects_provider,
            transport: &stub_transport,
            storage: None,
            access: None,
            ledger: None,
        };

        // Send completion signal to trigger protocol execution
        let input = ProtocolInput::LocalSignal {
            signal: "complete",
            data: Some(&serde_json::json!({
                "operation_type": format!("{:?}", operation_type),
                "blob_id": blob_id.to_string(),
            })),
        };

        loop {
            let step = storage_lifecycle.step(input.clone(), &mut capabilities);

            // Trace the step if tracer is available
            if let Some(tracer) = &self.tracer {
                if let Ok(tracer) = tracer.try_read() {
                    tracer.trace_lifecycle_step(&storage_lifecycle.descriptor(), &step);
                }
            }

            match step.outcome {
                Some(Ok(_result)) => {
                    // Trace completion if tracer is available
                    if let Some(tracer) = &self.tracer {
                        if let Ok(tracer) = tracer.try_read() {
                            tracer.trace_lifecycle_complete(&storage_lifecycle.descriptor(), true);
                        }
                    }
                    return Ok(());
                }
                Some(Err(e)) => {
                    // Trace failure if tracer is available
                    if let Some(tracer) = &self.tracer {
                        if let Ok(tracer) = tracer.try_read() {
                            tracer.trace_lifecycle_complete(&storage_lifecycle.descriptor(), false);
                        }
                    }
                    return Err(LifecycleSchedulerError::LifecycleFailure(format!(
                        "{:?}",
                        e
                    )));
                }
                None => {
                    // Protocol is still running, continue with next step
                    if storage_lifecycle.is_final() {
                        return Err(LifecycleSchedulerError::MissingOutcome);
                    }
                    // In a real implementation, we would process effects and continue
                    // For now, break to avoid infinite loop since this is a placeholder
                    break;
                }
            }
        }

        // Protocol didn't complete in loop, try one final step
        let final_step = storage_lifecycle.step(input, &mut capabilities);
        match final_step.outcome {
            Some(Ok(_result)) => Ok(()),
            Some(Err(e)) => Err(LifecycleSchedulerError::LifecycleFailure(format!(
                "{:?}",
                e
            ))),
            None => Err(LifecycleSchedulerError::MissingOutcome),
        }
    }
}

/// Adapter that exposes `aura_crypto::Effects` as a `crate::core::EffectsProvider`.
#[derive(Clone)]
struct SharedEffects {
    effects: Arc<Effects>,
    counter: Arc<AtomicU64>,
}

impl SharedEffects {
    fn new(effects: Arc<Effects>) -> Self {
        SharedEffects {
            effects,
            counter: Arc::new(AtomicU64::new(0)),
        }
    }

    fn gen_uuid(&self) -> Uuid {
        self.effects.gen_uuid()
    }

    fn snapshot(&self) -> Effects {
        Effects {
            time: self.effects.time.clone(),
            random: self.effects.random.clone(),
        }
    }
}

impl EffectsProvider for SharedEffects {
    fn now(&self) -> AuraResult<u64> {
        self.effects
            .now()
            .map_err(|e| AuraError::coordination_failed(format!("effects::now failed: {}", e)))
    }

    fn gen_uuid(&self) -> Uuid {
        self.effects.gen_uuid()
    }

    // Note: random_bytes<const N: usize> is provided by EffectsExt trait

    fn random_bytes_vec(&self, len: usize) -> Vec<u8> {
        let mut buf = vec![0u8; len];
        self.effects.fill_random(&mut buf);
        buf
    }

    fn counter(&self) -> u64 {
        self.counter.load(Ordering::SeqCst)
    }

    fn next_counter(&self) -> u64 {
        self.counter.fetch_add(1, Ordering::SeqCst) + 1
    }
}

/// Adapter that provides ProtocolTransport interface using a stub implementation.
#[derive(Debug, Clone)]
struct StubProtocolTransportAdapter;

impl StubProtocolTransportAdapter {
    fn new() -> Self {
        StubProtocolTransportAdapter
    }
}

#[async_trait::async_trait]
impl crate::core::capabilities::ProtocolTransport for StubProtocolTransportAdapter {
    async fn send(
        &self,
        _message: crate::core::capabilities::ProtocolMessage,
    ) -> aura_types::Result<()> {
        // Stub implementation - just accept the message
        Ok(())
    }

    async fn broadcast(
        &self,
        _from: aura_types::DeviceId,
        _payload: Vec<u8>,
        _session_id: Option<uuid::Uuid>,
    ) -> aura_types::Result<()> {
        // Stub implementation - just accept the broadcast
        Ok(())
    }

    async fn receive(&self) -> aura_types::Result<crate::core::capabilities::ProtocolMessage> {
        // Stub implementation - return a dummy message
        use aura_types::DeviceId;
        use uuid::Uuid;
        Ok(crate::core::capabilities::ProtocolMessage {
            from: DeviceId(Uuid::nil()),
            to: DeviceId(Uuid::nil()),
            payload: vec![],
            session_id: None,
        })
    }

    async fn is_reachable(&self, _device_id: aura_types::DeviceId) -> bool {
        // Stub implementation - always return false
        false
    }

    async fn connected_peers(&self) -> Vec<aura_types::DeviceId> {
        // Stub implementation - return empty list
        vec![]
    }
}
