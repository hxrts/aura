//! Simulated participants
//!
//! This module wraps participant agents with simulation infrastructure,
//! injecting controlled time and randomness sources and routing effects
//! through the simulation runtime.

use crate::{
    Effect, EffectContext, EffectSink, Interceptors, Operation, ParticipantId, Result, SimError,
    SimulatedTransport, Tick,
};
use aura_coordination::choreography::{dkd_choreography, resharing_choreography};
use aura_coordination::execution::{ProtocolContext, ProtocolError};
use aura_coordination::execution::{SimulatedTimeSource, SimulationScheduler};
use aura_coordination::Transport;
use aura_crypto::Effects;
use aura_journal::{AccountLedger, DeviceId};
use ed25519_dalek::SigningKey;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Simulated participant
///
/// This wraps a participant's agent with simulation infrastructure.
/// It injects controlled time/randomness and routes effects through interceptors.
pub struct SimulatedParticipant {
    /// Unique participant ID
    pub id: ParticipantId,

    /// Device ID (for protocol context)
    device_id: DeviceId,

    /// Device signing key (for event authentication)
    device_key: SigningKey,

    /// The participant's ledger (their local view of state)
    ledger: Arc<RwLock<AccountLedger>>,

    /// Simulated transport for protocol communication
    transport: Arc<SimulatedTransport>,

    /// Injected effects (time + randomness)
    effects: Effects,

    /// Effect sink to emit effects to the runtime
    effect_sink: EffectSink,

    /// Effect interceptors for Byzantine testing
    interceptors: Interceptors,

    /// Simulation scheduler for time-based coordination
    scheduler: Arc<RwLock<SimulationScheduler>>,

    /// Current tick (for context)
    current_tick: Arc<RwLock<Tick>>,
}

impl SimulatedParticipant {
    /// Create a new simulated participant
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: ParticipantId,
        device_id: DeviceId,
        device_key: SigningKey,
        ledger: AccountLedger,
        effects: Effects,
        effect_sink: EffectSink,
        interceptors: Interceptors,
        scheduler: Arc<RwLock<SimulationScheduler>>,
    ) -> Self {
        // Create transport with effect emission
        let effect_sink_clone = effect_sink.clone();
        let transport = Arc::new(SimulatedTransport::new(id, move |effect| {
            let _ = effect_sink_clone.emit(effect);
        }));

        SimulatedParticipant {
            id,
            device_id,
            device_key,
            ledger: Arc::new(RwLock::new(ledger)),
            transport,
            effects,
            effect_sink,
            interceptors,
            scheduler,
            current_tick: Arc::new(RwLock::new(0)),
        }
    }

    /// Create a new simulated participant with a shared ledger
    ///
    /// This is used when multiple participants need to share the same account/ledger,
    /// simulating instant CRDT sync for testing P2P protocols.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_shared_ledger(
        id: ParticipantId,
        device_id: DeviceId,
        device_key: SigningKey,
        shared_ledger: Arc<RwLock<AccountLedger>>,
        effects: Effects,
        effect_sink: EffectSink,
        interceptors: Interceptors,
        scheduler: Arc<RwLock<SimulationScheduler>>,
    ) -> Self {
        // Create transport with effect emission
        let effect_sink_clone = effect_sink.clone();
        let transport = Arc::new(SimulatedTransport::new(id, move |effect| {
            let _ = effect_sink_clone.emit(effect);
        }));

        SimulatedParticipant {
            id,
            device_id,
            device_key,
            ledger: shared_ledger,
            transport,
            effects,
            effect_sink,
            interceptors,
            scheduler,
            current_tick: Arc::new(RwLock::new(0)),
        }
    }

    /// Get the participant's ID
    pub fn id(&self) -> ParticipantId {
        self.id
    }

    /// Get a snapshot of the participant's ledger
    pub async fn ledger_snapshot(&self) -> AccountLedger {
        // AccountLedger doesn't implement Clone, so we need to reconstruct it
        let ledger_guard = self.ledger.read().await;
        let state = ledger_guard.state().clone();
        // Create a new ledger with the same state
        // Note: This loses the event log, but for snapshots we primarily care about state
        AccountLedger::new(state).expect("Failed to create ledger snapshot")
    }

    /// Get the injected effects bundle
    pub fn effects(&self) -> &Effects {
        &self.effects
    }

    /// Update the current tick (called by simulation)
    pub async fn update_tick(&self, tick: Tick) {
        *self.current_tick.write().await = tick;
    }

    /// Emit an effect through the interceptor and into the runtime
    pub async fn emit_effect(&self, effect: Effect, operation: Option<Operation>) -> Result<()> {
        let tick = *self.current_tick.read().await;

        let recipients = match &effect {
            Effect::Send(envelope) => envelope.recipients.clone(),
            _ => vec![],
        };

        let ctx = EffectContext {
            tick,
            sender: self.id,
            recipients,
            operation,
        };

        // Apply outgoing interceptor
        if let Some(intercepted_effect) = self.interceptors.outgoing.apply(&ctx, effect) {
            self.effect_sink.emit(intercepted_effect)?;
        }
        // If interceptor returns None, the effect is dropped (Byzantine behavior)

        Ok(())
    }

    /// Process an incoming effect (applies incoming interceptor)
    pub async fn receive_effect(
        &self,
        effect: Effect,
        operation: Option<Operation>,
    ) -> Result<Option<Effect>> {
        let tick = *self.current_tick.read().await;

        let recipients = match &effect {
            Effect::Send(envelope) => envelope.recipients.clone(),
            _ => vec![],
        };

        let ctx = EffectContext {
            tick,
            sender: self.id,
            recipients,
            operation,
        };

        // Apply incoming interceptor
        Ok(self.interceptors.incoming.apply(&ctx, effect))
    }

    /// Get mutable access to the ledger
    pub async fn ledger_mut(&self) -> tokio::sync::RwLockWriteGuard<'_, AccountLedger> {
        self.ledger.write().await
    }

    /// Get read access to the ledger
    pub async fn ledger(&self) -> tokio::sync::RwLockReadGuard<'_, AccountLedger> {
        self.ledger.read().await
    }

    // ========== High-Level Protocol Actions ==========

    /// Create a protocol context for this participant
    pub fn create_protocol_context(
        &self,
        session_id: Uuid,
        participants: Vec<DeviceId>,
        threshold: Option<usize>,
    ) -> ProtocolContext {
        // Create simulated time source using the scheduler
        let time_source = Box::new(SimulatedTimeSource::new(session_id, self.scheduler.clone()));

        ProtocolContext::new(
            session_id,
            self.device_id.0, // Extract Uuid from DeviceId
            participants,
            threshold,
            Arc::clone(&self.ledger),
            Arc::clone(&self.transport) as Arc<dyn Transport>,
            self.effects.clone(),
            self.device_key.clone(), // Pass device signing key
            time_source,
        )
    }

    /// Initiate a DKD protocol
    ///
    /// This executes the DKD choreography to derive a threshold key.
    pub async fn initiate_dkd(
        &self,
        participants: Vec<DeviceId>,
        threshold: usize,
    ) -> Result<Vec<u8>> {
        let session_id = self.effects.gen_uuid();
        self.initiate_dkd_with_session(session_id, participants, threshold)
            .await
    }

    /// Initiate DKD with a specific session ID (for coordinated testing)
    pub async fn initiate_dkd_with_session(
        &self,
        session_id: Uuid,
        participants: Vec<DeviceId>,
        threshold: usize,
    ) -> Result<Vec<u8>> {
        let mut ctx = self.create_protocol_context(session_id, participants, Some(threshold));

        dkd_choreography(&mut ctx, vec![])
            .await
            .map_err(|e: ProtocolError| SimError::RuntimeError(format!("DKD failed: {:?}", e)))
    }

    /// Initiate a resharing protocol
    ///
    /// This executes the resharing choreography to refresh threshold shares.
    pub async fn initiate_resharing(
        &self,
        _old_participants: Vec<DeviceId>,
        new_participants: Vec<DeviceId>,
        _old_threshold: usize,
        new_threshold: usize,
    ) -> Result<Vec<u8>> {
        let session_id = self.effects.gen_uuid();

        let mut ctx =
            self.create_protocol_context(session_id, new_participants.clone(), Some(new_threshold));

        resharing_choreography(&mut ctx, Some(new_threshold as u16), Some(new_participants))
            .await
            .map_err(|e: ProtocolError| {
                SimError::RuntimeError(format!("Resharing failed: {:?}", e))
            })
    }

    /// Initiate resharing with a specific session ID (for testing)
    pub async fn initiate_resharing_with_session(
        &self,
        session_id: Uuid,
        _old_participants: Vec<DeviceId>,
        new_participants: Vec<DeviceId>,
        _old_threshold: usize,
        new_threshold: usize,
    ) -> Result<Vec<u8>> {
        let mut ctx =
            self.create_protocol_context(session_id, new_participants.clone(), Some(new_threshold));

        resharing_choreography(&mut ctx, Some(new_threshold as u16), Some(new_participants))
            .await
            .map_err(|e: ProtocolError| {
                SimError::RuntimeError(format!("Resharing failed: {:?}", e))
            })
    }

    /// Initiate a recovery protocol
    ///
    /// This executes the recovery choreography for guardian-based recovery.
    pub async fn initiate_recovery(
        &self,
        guardians: Vec<aura_journal::GuardianId>,
        required_threshold: usize,
        cooldown_hours: u64,
        new_device_id: Option<DeviceId>,
    ) -> Result<Vec<u8>> {
        let session_id = self.effects.gen_uuid();

        let mut ctx = self.create_protocol_context(
            session_id,
            vec![], // Recovery doesn't use participants in the same way
            Some(required_threshold),
        );

        // Configure recovery-specific context
        ctx.set_guardians(guardians.clone())
            .map_err(|e| SimError::RuntimeError(format!("Failed to set guardians: {:?}", e)))?;
        ctx.set_guardian_threshold(required_threshold)
            .map_err(|e| {
                SimError::RuntimeError(format!("Failed to set guardian threshold: {:?}", e))
            })?;
        ctx.set_cooldown_hours(cooldown_hours)
            .map_err(|e| SimError::RuntimeError(format!("Failed to set cooldown: {:?}", e)))?;
        ctx.set_recovery_initiator(true).map_err(|e| {
            SimError::RuntimeError(format!("Failed to set recovery initiator: {:?}", e))
        })?;

        if let Some(device_id) = new_device_id {
            ctx.set_new_device_id(device_id).map_err(|e| {
                SimError::RuntimeError(format!("Failed to set new device ID: {:?}", e))
            })?;
        }

        aura_coordination::choreography::recovery_choreography(
            &mut ctx,
            guardians,
            required_threshold as u16,
        )
        .await
        .map_err(|e: ProtocolError| SimError::RuntimeError(format!("Recovery failed: {:?}", e)))
    }

    /// Initiate recovery with a specific session ID (for testing)
    pub async fn initiate_recovery_with_session(
        &self,
        session_id: Uuid,
        guardians: Vec<aura_journal::GuardianId>,
        required_threshold: usize,
        cooldown_hours: u64,
        new_device_id: Option<DeviceId>,
    ) -> Result<Vec<u8>> {
        let mut ctx = self.create_protocol_context(
            session_id,
            vec![], // Recovery doesn't use participants in the same way
            Some(required_threshold),
        );

        // Configure recovery-specific context
        ctx.set_guardians(guardians.clone())
            .map_err(|e| SimError::RuntimeError(format!("Failed to set guardians: {:?}", e)))?;
        ctx.set_guardian_threshold(required_threshold)
            .map_err(|e| {
                SimError::RuntimeError(format!("Failed to set guardian threshold: {:?}", e))
            })?;
        ctx.set_cooldown_hours(cooldown_hours)
            .map_err(|e| SimError::RuntimeError(format!("Failed to set cooldown: {:?}", e)))?;
        ctx.set_recovery_initiator(true).map_err(|e| {
            SimError::RuntimeError(format!("Failed to set recovery initiator: {:?}", e))
        })?;

        if let Some(device_id) = new_device_id {
            ctx.set_new_device_id(device_id).map_err(|e| {
                SimError::RuntimeError(format!("Failed to set new device ID: {:?}", e))
            })?;
        }

        aura_coordination::choreography::recovery_choreography(
            &mut ctx,
            guardians,
            required_threshold as u16,
        )
        .await
        .map_err(|e: ProtocolError| SimError::RuntimeError(format!("Recovery failed: {:?}", e)))
    }

    /// Approve a recovery request as a guardian
    pub async fn approve_recovery(
        &self,
        recovery_id: Uuid,
        guardian_id: aura_journal::GuardianId,
    ) -> Result<()> {
        let mut ctx = self.create_protocol_context(
            recovery_id,
            vec![], // Recovery doesn't use participants in the same way
            None,
        );

        // Configure as guardian
        ctx.set_guardian_id(guardian_id)
            .map_err(|e| SimError::RuntimeError(format!("Failed to set guardian ID: {:?}", e)))?;

        // Execute recovery choreography from guardian perspective
        aura_coordination::choreography::recovery_choreography(&mut ctx, vec![], 0)
            .await
            .map_err(|e: ProtocolError| {
                SimError::RuntimeError(format!("Guardian approval failed: {:?}", e))
            })?;

        Ok(())
    }

    /// Acquire a distributed lock for an operation
    pub async fn acquire_lock_with_session(
        &self,
        session_id: uuid::Uuid,
        operation_type: aura_journal::OperationType,
    ) -> Result<()> {
        // Get device IDs for all participants in this ledger
        let device_ids = {
            let ledger = self.ledger().await;
            ledger.state().devices.keys().cloned().collect::<Vec<_>>()
        };

        // Create a protocol context for locking
        let mut ctx = self.create_protocol_context(
            session_id, device_ids, None, // No threshold needed for locking
        );

        // Execute locking choreography
        aura_coordination::choreography::locking_choreography(&mut ctx, operation_type)
            .await
            .map_err(|e: ProtocolError| {
                SimError::RuntimeError(format!("Lock acquisition failed: {:?}", e))
            })?;

        Ok(())
    }
}

/// Builder for creating simulated participants with custom configuration
pub struct ParticipantBuilder {
    id: Option<ParticipantId>,
    device_id: Option<DeviceId>,
    device_key: Option<SigningKey>,
    ledger: Option<AccountLedger>,
    effects: Option<Effects>,
    effect_sink: Option<EffectSink>,
    interceptors: Option<Interceptors>,
}

impl ParticipantBuilder {
    /// Create a new participant builder
    #[allow(clippy::too_many_arguments)]
    pub fn new() -> Self {
        ParticipantBuilder {
            id: None,
            device_id: None,
            device_key: None,
            ledger: None,
            effects: None,
            effect_sink: None,
            interceptors: None,
        }
    }

    /// Set the participant ID
    pub fn with_id(mut self, id: ParticipantId) -> Self {
        self.id = Some(id);
        self
    }

    /// Set the device ID
    pub fn with_device_id(mut self, device_id: DeviceId) -> Self {
        self.device_id = Some(device_id);
        self
    }

    /// Set the device signing key
    pub fn with_device_key(mut self, device_key: SigningKey) -> Self {
        self.device_key = Some(device_key);
        self
    }

    /// Set the ledger
    pub fn with_ledger(mut self, ledger: AccountLedger) -> Self {
        self.ledger = Some(ledger);
        self
    }

    /// Set the effects bundle
    pub fn with_effects(mut self, effects: Effects) -> Self {
        self.effects = Some(effects);
        self
    }

    /// Set the effect sink
    pub fn with_effect_sink(mut self, effect_sink: EffectSink) -> Self {
        self.effect_sink = Some(effect_sink);
        self
    }

    /// Set the interceptors
    pub fn with_interceptors(mut self, interceptors: Interceptors) -> Self {
        self.interceptors = Some(interceptors);
        self
    }

    /// Build the participant
    pub fn build(self) -> Result<SimulatedParticipant> {
        // Create a default scheduler if none provided
        let scheduler = Arc::new(RwLock::new(SimulationScheduler::new()));

        Ok(SimulatedParticipant::new(
            self.id
                .ok_or_else(|| SimError::RuntimeError("ID is required".to_string()))?,
            self.device_id
                .ok_or_else(|| SimError::RuntimeError("DeviceId is required".to_string()))?,
            self.device_key
                .ok_or_else(|| SimError::RuntimeError("DeviceKey is required".to_string()))?,
            self.ledger
                .ok_or_else(|| SimError::RuntimeError("Ledger is required".to_string()))?,
            self.effects
                .ok_or_else(|| SimError::RuntimeError("Effects is required".to_string()))?,
            self.effect_sink
                .ok_or_else(|| SimError::RuntimeError("EffectSink is required".to_string()))?,
            self.interceptors.unwrap_or_else(Interceptors::honest),
            scheduler,
        ))
    }
}

impl Default for ParticipantBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_journal::{AccountLedger, DeviceId, DeviceMetadata};
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn test_participant_basic() {
        use aura_journal::{AccountId, AccountState, DeviceType, SessionEpoch};
        use ed25519_dalek::SigningKey;
        use std::collections::{BTreeMap, BTreeSet};

        let effects = aura_crypto::Effects::for_test("test_participant_basic");
        let id = ParticipantId::from_name("alice");

        // Create a minimal ledger
        let device_id = DeviceId::new_with_effects(&effects);
        let signing_key = SigningKey::from_bytes(&[0u8; 32]);
        let group_public_key = signing_key.verifying_key();

        let device_metadata = DeviceMetadata {
            device_id,
            device_name: "alice-device".to_string(),
            device_type: DeviceType::Native,
            public_key: group_public_key,
            added_at: effects.now().unwrap_or(0),
            last_seen: effects.now().unwrap_or(0),
            dkd_commitment_proofs: BTreeMap::new(),
        };

        let mut devices = BTreeMap::new();
        devices.insert(device_id, device_metadata);

        let initial_state = AccountState {
            account_id: AccountId::new_with_effects(&effects),
            group_public_key,
            devices,
            removed_devices: BTreeSet::new(),
            guardians: BTreeMap::new(),
            removed_guardians: BTreeSet::new(),
            session_epoch: SessionEpoch::initial(),
            lamport_clock: 0,
            dkd_commitment_roots: BTreeMap::new(),
            sessions: BTreeMap::new(),
            active_operation_lock: None,
            presence_tickets: BTreeMap::new(),
            cooldowns: BTreeMap::new(),
            authority_graph: aura_journal::capability::authority_graph::AuthorityGraph::new(),
            visibility_index: aura_journal::capability::visibility::VisibilityIndex::new(
                aura_journal::capability::authority_graph::AuthorityGraph::new(),
                &effects,
            ),
            threshold: 1,
            total_participants: 1,
            used_nonces: BTreeSet::new(),
            next_nonce: 0,
            last_event_hash: None,
            updated_at: 0,
        };

        let ledger = AccountLedger::new(initial_state).expect("Failed to create ledger");

        let effects = Effects::test();
        let (tx, _rx) = mpsc::unbounded_channel();
        let effect_sink = EffectSink::new(tx, id);
        let device_key = SigningKey::from_bytes(&[1u8; 32]);

        let scheduler = Arc::new(RwLock::new(SimulationScheduler::new()));

        let participant = SimulatedParticipant::new(
            id,
            device_id,
            device_key,
            ledger,
            effects,
            effect_sink,
            Interceptors::honest(),
            scheduler,
        );

        assert_eq!(participant.id(), id);
    }
}
