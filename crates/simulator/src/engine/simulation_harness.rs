//! Main simulation harness
//!
//! This is the top-level API for running deterministic, in-process simulations
//! of distributed protocols.

use crate::{
    Effect, EffectSink, Interceptors, ParticipantId, Result, SideEffectRuntime, SimError,
    SimulatedNetwork, SimulatedParticipant, Tick,
};
use aura_coordination::execution::SimulationScheduler;
use aura_crypto::Effects;
use aura_journal::{
    AccountId, AccountLedger, AccountState, DeviceId, DeviceMetadata, DeviceType, SessionEpoch,
};
use ed25519_dalek::SigningKey;
use indexmap::IndexMap;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Main simulation harness
///
/// This is the entry point for all simulation tests. It owns the simulated world
/// and provides methods to:
/// - Add participants (honest or Byzantine)
/// - Advance time
/// - Inspect state
/// - Configure network conditions
pub struct Simulation {
    /// All participants in the simulation
    participants: IndexMap<ParticipantId, Arc<SimulatedParticipant>>,

    /// The runtime that processes effects
    runtime: SideEffectRuntime,

    /// Shared effects bundle for all participants
    effects: Effects,

    /// Simulation scheduler for time-based wake conditions
    scheduler: Arc<RwLock<SimulationScheduler>>,

    /// Current tick
    current_tick: Tick,

    /// Seed for reproducibility
    seed: u64,
}

impl Simulation {
    /// Create a new simulation with a given seed
    ///
    /// The seed ensures deterministic execution for reproducible tests.
    pub fn new(seed: u64) -> Self {
        let network = SimulatedNetwork::new(seed);
        let runtime = SideEffectRuntime::new(network);

        // Create shared effects bundle with simulated time and seeded randomness
        let effects = Effects::deterministic(seed, 1735689600); // Start at 2025-01-01

        // Create simulation scheduler for time-based coordination
        let scheduler = Arc::new(RwLock::new(SimulationScheduler::new()));

        Simulation {
            participants: IndexMap::new(),
            runtime,
            effects,
            scheduler,
            current_tick: 0,
            seed,
        }
    }

    /// Get a reference to the simulation scheduler
    pub fn scheduler(&self) -> Arc<RwLock<SimulationScheduler>> {
        self.scheduler.clone()
    }

    /// Add a new honest participant
    ///
    /// Returns the participant's ID.
    pub async fn add_participant(&mut self, name: &str) -> ParticipantId {
        let id = ParticipantId::from_name(name);
        self.add_participant_with_id(id, Interceptors::honest())
            .await
    }

    /// Create a shared account with multiple devices (for P2P protocols)
    ///
    /// All participants will share the same AccountId and ledger, simulating
    /// instant CRDT sync. This is the correct setup for testing P2P protocols.
    pub async fn add_account_with_devices(
        &mut self,
        device_names: &[&str],
    ) -> (AccountId, Vec<(ParticipantId, DeviceId)>) {
        let account_id = AccountId::new_with_effects(&self.effects);
        let timestamp = self.effects.now().unwrap_or(0);

        // Generate account keypair
        let account_signing_key = SigningKey::from_bytes(&self.effects.random_bytes::<32>());
        let group_public_key = account_signing_key.verifying_key();

        // Create all devices and their metadata
        let mut devices = BTreeMap::new();
        let mut device_info = Vec::new();
        let mut signing_keys = Vec::new();

        for name in device_names {
            let participant_id = ParticipantId::from_name(name);
            let device_id = DeviceId::new_with_effects(&self.effects);
            let signing_key = SigningKey::from_bytes(&self.effects.random_bytes::<32>());

            let device_metadata = DeviceMetadata {
                device_id,
                device_name: format!("{}-device", name),
                device_type: DeviceType::Native,
                public_key: signing_key.verifying_key(),
                added_at: timestamp,
                last_seen: timestamp,
                dkd_commitment_proofs: BTreeMap::new(),
            };

            devices.insert(device_id, device_metadata);
            device_info.push((participant_id, device_id));
            signing_keys.push((participant_id, device_id, signing_key));
        }

        // Create shared account state
        let initial_state = AccountState {
            account_id,
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
            visibility_index: aura_journal::capability::visibility::VisibilityIndex::new(aura_journal::capability::authority_graph::AuthorityGraph::new(), &self.effects),
            threshold: (device_names.len() as u16).div_ceil(2), // Simple majority
            total_participants: device_names.len() as u16,
            used_nonces: BTreeSet::new(),
            next_nonce: 0,
            last_event_hash: None,
            updated_at: timestamp,
        };

        // Create shared ledger
        let shared_ledger = Arc::new(RwLock::new(
            AccountLedger::new(initial_state).expect("Failed to create ledger"),
        ));

        // Create all participants with shared ledger
        for (participant_id, device_id, signing_key) in signing_keys {
            let effect_sink_tx = self.runtime.register_participant(participant_id);
            let effect_sink = EffectSink::new(effect_sink_tx, participant_id);

            // Create participant with shared ledger
            let participant = SimulatedParticipant::new_with_shared_ledger(
                participant_id,
                device_id,
                signing_key,
                Arc::clone(&shared_ledger),
                self.effects.clone(),
                effect_sink,
                Interceptors::honest(),
                self.scheduler(),
            );

            participant.update_tick(self.current_tick).await;

            self.participants
                .insert(participant_id, Arc::new(participant));
        }

        (account_id, device_info)
    }

    /// Add a participant with custom interceptors (for Byzantine testing)
    ///
    /// Returns the participant's ID.
    pub async fn add_participant_with_interceptors(
        &mut self,
        name: &str,
        interceptors: Interceptors,
    ) -> ParticipantId {
        let id = ParticipantId::from_name(name);
        self.add_participant_with_id(id, interceptors).await
    }

    /// Add a malicious participant (alias for Byzantine testing)
    pub async fn add_malicious_participant(
        &mut self,
        name: &str,
        interceptors: Interceptors,
    ) -> ParticipantId {
        self.add_participant_with_interceptors(name, interceptors)
            .await
    }

    /// Internal: Add a participant with a specific ID and interceptors
    async fn add_participant_with_id(
        &mut self,
        id: ParticipantId,
        interceptors: Interceptors,
    ) -> ParticipantId {
        // Register with runtime and get effect sink
        let effect_sink_tx = self.runtime.register_participant(id);
        let effect_sink = EffectSink::new(effect_sink_tx, id);

        // Create a minimal ledger for this participant
        let device_id = DeviceId::new_with_effects(&self.effects);
        let timestamp = self.effects.now().unwrap_or(0);

        // Generate a keypair for the simulated account
        let signing_key = SigningKey::from_bytes(&self.effects.random_bytes::<32>());
        let group_public_key = signing_key.verifying_key();

        // Create device metadata
        let device_metadata = DeviceMetadata {
            device_id,
            device_name: format!("{}-device", id),
            device_type: DeviceType::Native,
            public_key: group_public_key,
            added_at: timestamp,
            last_seen: timestamp,
            dkd_commitment_proofs: BTreeMap::new(),
        };

        let mut devices = BTreeMap::new();
        devices.insert(device_id, device_metadata);

        // Create initial account state
        let initial_state = AccountState {
            account_id: AccountId::new_with_effects(&self.effects),
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
            visibility_index: aura_journal::capability::visibility::VisibilityIndex::new(aura_journal::capability::authority_graph::AuthorityGraph::new(), &self.effects),
            threshold: 1,
            total_participants: 1,
            used_nonces: BTreeSet::new(),
            next_nonce: 0,
            last_event_hash: None,
            updated_at: timestamp,
        };

        let ledger = AccountLedger::new(initial_state).expect("Failed to create ledger");

        // Create participant with cloned effects
        let participant = SimulatedParticipant::new(
            id,
            device_id,
            signing_key, // Pass the device signing key
            ledger,
            self.effects.clone(),
            effect_sink,
            interceptors,
            self.scheduler(),
        );

        participant.update_tick(self.current_tick).await;

        self.participants.insert(id, Arc::new(participant));

        id
    }

    /// Remove a participant from the simulation
    pub fn remove_participant(&mut self, id: ParticipantId) -> Result<()> {
        self.participants
            .shift_remove(&id)
            .ok_or(SimError::ParticipantNotFound(id))?;
        self.runtime.unregister_participant(id);
        Ok(())
    }

    /// Get a reference to a participant
    pub fn get_participant(&self, id: ParticipantId) -> Result<Arc<SimulatedParticipant>> {
        self.participants
            .get(&id)
            .cloned()
            .ok_or(SimError::ParticipantNotFound(id))
    }

    /// Get all participant IDs
    pub fn get_all_participants(&self) -> Vec<ParticipantId> {
        self.participants.keys().copied().collect()
    }

    /// Get a snapshot of a participant's ledger
    pub async fn ledger_snapshot(&self, id: ParticipantId) -> Result<AccountLedger> {
        let participant = self.get_participant(id)?;
        Ok(participant.ledger_snapshot().await)
    }

    /// Advance the simulation by one tick
    ///
    /// This:
    /// 1. Processes all pending effects
    /// 2. Advances the network (delivers messages)
    /// 3. Updates tick counter
    /// 4. Notifies scheduler about new events immediately
    /// 5. Advances scheduler time
    /// 6. Updates participant ticks
    ///
    /// Returns the number of messages delivered.
    pub async fn tick(&mut self) -> Result<usize> {
        // Process all pending effects
        self.runtime.process_effects().await?;

        // Advance network
        let delivered = self.runtime.advance_tick()?;

        // Update tick counter
        self.current_tick += 1;

        // Immediately notify scheduler that events might be available globally
        // This should happen BEFORE advancing time to ensure contexts see events
        {
            let mut scheduler = self.scheduler.write().await;
            scheduler.notify_events_available_globally();
        }

        // Advance the scheduler's time to coordinate with waiting contexts
        {
            let mut scheduler = self.scheduler.write().await;
            scheduler.advance_time(1); // Advance by 1 tick
        }

        // Update all participants' tick
        for (_participant_id, participant) in &self.participants {
            participant.update_tick(self.current_tick).await;
        }

        // Deliver messages to participants
        for (participant_id, participant) in &self.participants {
            let messages = self.runtime.deliver_messages_to(*participant_id);
            for envelope in messages {
                // Convert envelope to effect and process through incoming interceptor
                let effect = Effect::Send(envelope);
                if let Some(_intercepted) = participant.receive_effect(effect, None).await? {
                    // In a full implementation, we'd trigger protocol logic here
                    // For now, just acknowledge receipt
                }
            }
        }

        Ok(delivered)
    }

    /// Run the simulation until no more effects, messages, or waiting protocols
    ///
    /// This repeatedly calls `tick()` until the system is quiescent.
    /// Returns the total number of ticks executed.
    pub async fn run_until_idle(&mut self) -> Result<u64> {
        let mut ticks_executed = 0;
        const MAX_TICKS: u64 = 10000; // Safety limit to prevent infinite loops

        println!("run_until_idle starting");

        while !self.is_idle().await && ticks_executed < MAX_TICKS {
            if ticks_executed % 100 == 0 {
                let scheduler = self.scheduler.read().await;
                println!(
                    "Tick {}: runtime_idle={}, waiting_contexts={}, active_contexts={}",
                    ticks_executed,
                    self.runtime.is_idle(),
                    scheduler.waiting_context_count(),
                    scheduler.active_context_count()
                );
            }
            self.tick().await?;
            ticks_executed += 1;
        }

        if ticks_executed >= MAX_TICKS {
            let scheduler = self.scheduler.read().await;
            eprintln!(
                "Simulation timeout: runtime_idle={}, waiting_contexts={}, active_contexts={}",
                self.runtime.is_idle(),
                scheduler.waiting_context_count(),
                scheduler.active_context_count()
            );
            return Err(SimError::RuntimeError(
                "Simulation did not converge after max ticks".to_string(),
            ));
        }

        println!("run_until_idle completed after {} ticks", ticks_executed);
        Ok(ticks_executed)
    }

    /// Run for a specific number of ticks
    pub async fn run_for_ticks(&mut self, n: u64) -> Result<()> {
        for _ in 0..n {
            self.tick().await?;
        }
        Ok(())
    }

    /// Check if the simulation is idle (no pending effects, messages, or waiting protocols)
    pub async fn is_idle(&self) -> bool {
        // Check runtime for pending effects/messages
        if !self.runtime.is_idle() {
            return false;
        }

        // Check scheduler for waiting protocol contexts
        let scheduler = self.scheduler.read().await;
        !scheduler.has_waiting_contexts()
    }

    /// Get current tick
    pub fn current_tick(&self) -> Tick {
        self.current_tick
    }

    /// Get the simulation seed
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Advance logical time by N seconds
    pub fn advance_time(&self, seconds: u64) -> Result<()> {
        self.effects
            .advance_time(seconds)
            .map_err(|e| SimError::TimeError(e.to_string()))
    }

    /// Get current timestamp
    pub fn current_timestamp(&self) -> Result<u64> {
        self.effects
            .now()
            .map_err(|e| SimError::TimeError(e.to_string()))
    }

    /// Generate a deterministic UUID using seeded randomness
    pub fn generate_uuid(&self) -> uuid::Uuid {
        // Generate 16 bytes from the seeded RNG
        let bytes = self.effects.random_bytes::<16>();
        uuid::Uuid::from_bytes(bytes)
    }

    // ========== Network Configuration ==========

    /// Configure network latency range (in ticks)
    pub fn set_latency_range(&mut self, min: Tick, max: Tick) {
        self.runtime.network_mut().set_latency_range(min, max);
    }

    /// Set message drop rate (0.0 to 1.0)
    pub fn set_drop_rate(&mut self, rate: f64) {
        self.runtime.network_mut().set_drop_rate(rate);
    }

    /// Create a network partition
    ///
    /// Pass a vector of participant ID sets, where each set is an isolated island.
    pub fn partition_network(&mut self, islands: Vec<Vec<ParticipantId>>) {
        let islands_set: Vec<_> = islands
            .into_iter()
            .map(|island| island.into_iter().collect())
            .collect();
        self.runtime.network_mut().partition(islands_set);
    }

    /// Heal all network partitions
    pub fn heal_partitions(&mut self) {
        self.runtime.network_mut().heal_partitions();
    }

    /// Get network statistics
    pub fn network_stats(&self) -> crate::NetworkStats {
        self.runtime.network().stats()
    }

    /// Get the list of all participant IDs
    pub fn participant_ids(&self) -> Vec<ParticipantId> {
        self.participants.keys().copied().collect()
    }

    /// Get the number of participants
    pub fn participant_count(&self) -> usize {
        self.participants.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_simulation_basic() {
        let mut sim = Simulation::new(42);

        let alice = sim.add_participant("alice").await;
        let _bob = sim.add_participant("bob").await;

        assert_eq!(sim.participant_count(), 2);
        assert!(sim.is_idle().await);

        // Should be able to get ledger snapshots
        let alice_ledger = sim.ledger_snapshot(alice).await.unwrap();
        assert!(alice_ledger.event_log().is_empty()); // Empty ledger initially
    }

    #[tokio::test]
    async fn test_simulation_tick() {
        let mut sim = Simulation::new(42);

        sim.add_participant("alice").await;
        sim.add_participant("bob").await;

        // Tick should succeed even with no activity
        sim.tick().await.unwrap();
        assert_eq!(sim.current_tick(), 1);

        sim.run_for_ticks(10).await.unwrap();
        assert_eq!(sim.current_tick(), 11);
    }

    #[tokio::test]
    async fn test_simulation_time_advancement() {
        let sim = Simulation::new(42);

        let t1 = sim.current_timestamp().unwrap();
        sim.advance_time(3600).unwrap(); // +1 hour
        let t2 = sim.current_timestamp().unwrap();

        assert_eq!(t2, t1 + 3600);
    }

    #[tokio::test]
    async fn test_simulation_network_config() {
        let mut sim = Simulation::new(42);

        sim.set_latency_range(5, 20);
        sim.set_drop_rate(0.1);

        let alice = sim.add_participant("alice").await;
        let bob = sim.add_participant("bob").await;

        // Create a partition
        sim.partition_network(vec![vec![alice], vec![bob]]);

        // Heal partition
        sim.heal_partitions();
    }

    #[tokio::test]
    async fn test_run_until_idle() {
        let mut sim = Simulation::new(42);

        sim.add_participant("alice").await;
        sim.add_participant("bob").await;

        // Should immediately be idle (no activity)
        let ticks = sim.run_until_idle().await.unwrap();
        assert_eq!(ticks, 0);
    }
}
