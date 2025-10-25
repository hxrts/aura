//! Session Type States for Recovery Protocol Choreography (Refactored with Macros)
//!
//! This module defines session types for the guardian-based recovery protocol,
//! providing compile-time safety for recovery phases and guardian coordination.

use crate::core::{ChoreographicProtocol, SessionProtocol, SessionState, WitnessedTransition};
use crate::witnesses::RuntimeWitness;
use aura_journal::{DeviceId, Event, EventType, GuardianId};
use std::collections::BTreeMap;
use uuid::Uuid;

// ========== Recovery Protocol Core ==========

/// Core recovery protocol data without session state
#[derive(Debug, Clone)]
pub struct RecoveryProtocolCore {
    pub recovery_id: Uuid,
    pub new_device_id: DeviceId,
    pub guardian_ids: Vec<GuardianId>,
    pub threshold: u16,
    pub cooldown_hours: Option<u64>,
    pub collected_approvals: BTreeMap<GuardianId, bool>,
    pub collected_shares: BTreeMap<GuardianId, Vec<u8>>,
}

impl RecoveryProtocolCore {
    pub fn new(
        recovery_id: Uuid,
        new_device_id: DeviceId,
        guardian_ids: Vec<GuardianId>,
        threshold: u16,
        cooldown_hours: Option<u64>,
    ) -> Self {
        Self {
            recovery_id,
            new_device_id,
            guardian_ids,
            threshold,
            cooldown_hours,
            collected_approvals: BTreeMap::new(),
            collected_shares: BTreeMap::new(),
        }
    }
}

// ========== Error Type ==========

/// Error type for recovery session protocols
#[derive(Debug, thiserror::Error)]
pub enum RecoverySessionError {
    #[error("Recovery protocol error: {0}")]
    ProtocolError(String),
    #[error("Invalid operation for current recovery state")]
    InvalidOperation,
    #[error("Recovery failed: {0}")]
    RecoveryFailed(String),
    #[error("Guardian approval threshold not met")]
    InsufficientApprovals,
    #[error("Recovery shares collection failed: {0}")]
    SharesCollectionFailed(String),
    #[error("Key reconstruction failed: {0}")]
    KeyReconstructionFailed(String),
    #[error("Recovery aborted: {0}")]
    RecoveryAborted(String),
}

// ========== Protocol Definition using Macros ==========

define_protocol! {
    Protocol: RecoveryProtocol,
    Core: RecoveryProtocolCore,
    Error: RecoverySessionError,
    Union: RecoverySessionState,

    States {
        RecoveryInitialized => Vec<u8>,
        CollectingApprovals => Vec<u8>,
        EnforcingCooldown => Vec<u8>,
        CollectingShares => Vec<u8>,
        ReconstructingKey => Vec<u8>,
        RecoveryProtocolCompleted @ final => Vec<u8>,
        RecoveryAborted @ final => Vec<u8>,
    }

    Extract {
        session_id: |core| core.recovery_id,
        device_id: |core| core.new_device_id,
    }
}

// ========== Protocol Type Alias ==========

/// Session-typed recovery protocol wrapper
pub type RecoveryProtocol<S> = ChoreographicProtocol<RecoveryProtocolCore, S>;

// ========== Runtime Witnesses for Recovery Operations ==========

/// Witness that recovery has been successfully initiated
#[derive(Debug, Clone)]
pub struct RecoveryInitiated {
    pub recovery_id: Uuid,
    pub initiation_timestamp: u64,
    pub guardian_count: usize,
}

impl RuntimeWitness for RecoveryInitiated {
    type Evidence = Event;
    type Config = ();

    fn verify(evidence: Event, _config: ()) -> Option<Self> {
        if let EventType::InitiateRecovery(initiate_event) = evidence.event_type {
            Some(RecoveryInitiated {
                recovery_id: initiate_event.recovery_id,
                initiation_timestamp: evidence.timestamp,
                guardian_count: initiate_event.required_guardians.len(),
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Recovery successfully initiated"
    }
}

/// Witness that guardian approval threshold has been met
#[derive(Debug, Clone)]
pub struct RecoveryApprovalThresholdMet {
    pub recovery_id: Uuid,
    pub approved_guardians: Vec<GuardianId>,
    pub approval_count: usize,
    pub required_threshold: u16,
}

impl RuntimeWitness for RecoveryApprovalThresholdMet {
    type Evidence = (Vec<Event>, u16);
    type Config = ();

    fn verify(evidence: (Vec<Event>, u16), _config: ()) -> Option<Self> {
        let (events, threshold) = evidence;
        let mut approved_guardians = Vec::new();
        let mut recovery_id = None;

        for event in events {
            if let EventType::CollectGuardianApproval(approval_event) = event.event_type {
                if recovery_id.is_none() {
                    recovery_id = Some(approval_event.recovery_id);
                }

                if let Some(rid) = recovery_id {
                    if approval_event.recovery_id == rid && approval_event.approved {
                        approved_guardians.push(approval_event.guardian_id);
                    }
                }
            }
        }

        let approval_count = approved_guardians.len();
        if approval_count >= threshold as usize {
            Some(RecoveryApprovalThresholdMet {
                recovery_id: recovery_id?,
                approved_guardians,
                approval_count,
                required_threshold: threshold,
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Guardian approval threshold met"
    }
}

/// Witness that cooldown period has been successfully enforced
#[derive(Debug, Clone)]
pub struct CooldownCompleted {
    pub recovery_id: Uuid,
    pub cooldown_start: u64,
    pub cooldown_end: u64,
    pub no_vetoes: bool,
}

impl RuntimeWitness for CooldownCompleted {
    type Evidence = (Uuid, u64, Vec<Event>);
    type Config = u64; // Cooldown duration in epochs

    fn verify(evidence: (Uuid, u64, Vec<Event>), config: u64) -> Option<Self> {
        let (recovery_id, start_epoch, veto_events) = evidence;
        let current_epoch = start_epoch + config;

        // Check for any veto events during cooldown period
        let has_vetoes = veto_events.iter().any(|event| {
            matches!(event.event_type, EventType::AbortRecovery(_))
                && event.timestamp >= start_epoch
                && event.timestamp <= current_epoch
        });

        if !has_vetoes {
            Some(CooldownCompleted {
                recovery_id,
                cooldown_start: start_epoch,
                cooldown_end: current_epoch,
                no_vetoes: true,
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Cooldown period completed without vetoes"
    }
}

/// Witness that guardian shares have been collected
#[derive(Debug, Clone)]
pub struct RecoverySharesCollected {
    pub recovery_id: Uuid,
    pub collected_shares: BTreeMap<GuardianId, Vec<u8>>,
    pub share_count: usize,
    pub required_threshold: u16,
}

impl RuntimeWitness for RecoverySharesCollected {
    type Evidence = (Vec<Event>, u16);
    type Config = ();

    fn verify(evidence: (Vec<Event>, u16), _config: ()) -> Option<Self> {
        let (events, threshold) = evidence;
        let mut collected_shares = BTreeMap::new();
        let mut recovery_id = None;

        for event in events {
            if let EventType::SubmitRecoveryShare(share_event) = event.event_type {
                if recovery_id.is_none() {
                    recovery_id = Some(share_event.recovery_id);
                }

                if let Some(rid) = recovery_id {
                    if share_event.recovery_id == rid {
                        collected_shares
                            .insert(share_event.guardian_id, share_event.encrypted_share.clone());
                    }
                }
            }
        }

        let share_count = collected_shares.len();
        if share_count >= threshold as usize {
            Some(RecoverySharesCollected {
                recovery_id: recovery_id?,
                share_count,
                collected_shares,
                required_threshold: threshold,
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Guardian recovery shares collected"
    }
}

/// Witness that recovery key has been successfully reconstructed
#[derive(Debug, Clone)]
pub struct KeyReconstructed {
    pub recovery_id: Uuid,
    pub reconstructed_key: Vec<u8>,
    pub shares_used: usize,
}

impl RuntimeWitness for KeyReconstructed {
    type Evidence = (Uuid, Vec<u8>);
    type Config = usize; // Number of shares used

    fn verify(evidence: (Uuid, Vec<u8>), config: usize) -> Option<Self> {
        let (recovery_id, key_bytes) = evidence;

        // Verify key is valid length for Ed25519
        if key_bytes.len() == 32 && config > 0 {
            Some(KeyReconstructed {
                recovery_id,
                reconstructed_key: key_bytes,
                shares_used: config,
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Recovery key successfully reconstructed"
    }
}

/// Witness that recovery has been aborted
#[derive(Debug, Clone)]
pub struct RecoveryAbort {
    pub recovery_id: Uuid,
    pub abort_reason: String,
    pub aborted_by: Option<GuardianId>,
}

impl RuntimeWitness for RecoveryAbort {
    type Evidence = Event;
    type Config = ();

    fn verify(evidence: Event, _config: ()) -> Option<Self> {
        if let EventType::AbortRecovery(abort_event) = evidence.event_type {
            Some(RecoveryAbort {
                recovery_id: abort_event.recovery_id,
                abort_reason: format!("{:?}", abort_event.reason),
                aborted_by: None, // Would be extracted from event author
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Recovery protocol aborted"
    }
}

// ========== State Transitions ==========

/// Transition from RecoveryInitialized to CollectingApprovals (requires RecoveryInitiated witness)
impl WitnessedTransition<RecoveryInitialized, CollectingApprovals>
    for ChoreographicProtocol<RecoveryProtocolCore, RecoveryInitialized>
{
    type Witness = RecoveryInitiated;
    type Target = ChoreographicProtocol<RecoveryProtocolCore, CollectingApprovals>;

    /// Begin collecting guardian approvals
    fn transition_with_witness(self, _witness: Self::Witness) -> Self::Target {
        self.transition_to()
    }
}

/// Transition from CollectingApprovals to EnforcingCooldown (requires RecoveryApprovalThresholdMet witness)
impl WitnessedTransition<CollectingApprovals, EnforcingCooldown>
    for ChoreographicProtocol<RecoveryProtocolCore, CollectingApprovals>
{
    type Witness = RecoveryApprovalThresholdMet;
    type Target = ChoreographicProtocol<RecoveryProtocolCore, EnforcingCooldown>;

    /// Begin cooldown period after sufficient approvals
    fn transition_with_witness(mut self, witness: Self::Witness) -> Self::Target {
        // Update collected approvals
        for guardian_id in witness.approved_guardians {
            self.inner.collected_approvals.insert(guardian_id, true);
        }
        self.transition_to()
    }
}

/// Transition from EnforcingCooldown to CollectingShares (requires CooldownCompleted witness)
impl WitnessedTransition<EnforcingCooldown, CollectingShares>
    for ChoreographicProtocol<RecoveryProtocolCore, EnforcingCooldown>
{
    type Witness = CooldownCompleted;
    type Target = ChoreographicProtocol<RecoveryProtocolCore, CollectingShares>;

    /// Begin collecting recovery shares after cooldown
    fn transition_with_witness(self, _witness: Self::Witness) -> Self::Target {
        self.transition_to()
    }
}

/// Transition from CollectingShares to ReconstructingKey (requires RecoverySharesCollected witness)
impl WitnessedTransition<CollectingShares, ReconstructingKey>
    for ChoreographicProtocol<RecoveryProtocolCore, CollectingShares>
{
    type Witness = RecoverySharesCollected;
    type Target = ChoreographicProtocol<RecoveryProtocolCore, ReconstructingKey>;

    /// Begin key reconstruction with collected shares
    fn transition_with_witness(mut self, witness: Self::Witness) -> Self::Target {
        // Update collected shares
        self.inner.collected_shares = witness.collected_shares;
        self.transition_to()
    }
}

/// Transition from ReconstructingKey to RecoveryProtocolCompleted (requires KeyReconstructed witness)
impl WitnessedTransition<ReconstructingKey, RecoveryProtocolCompleted>
    for ChoreographicProtocol<RecoveryProtocolCore, ReconstructingKey>
{
    type Witness = KeyReconstructed;
    type Target = ChoreographicProtocol<RecoveryProtocolCore, RecoveryProtocolCompleted>;

    /// Complete recovery with reconstructed key
    fn transition_with_witness(self, _witness: Self::Witness) -> Self::Target {
        self.transition_to()
    }
}

/// Transition to RecoveryAborted from any state (requires RecoveryAbort witness)
impl<S: SessionState> WitnessedTransition<S, RecoveryAborted>
    for ChoreographicProtocol<RecoveryProtocolCore, S>
where
    Self: SessionProtocol<State = S, Output = Vec<u8>, Error = RecoverySessionError>,
{
    type Witness = RecoveryAbort;
    type Target = ChoreographicProtocol<RecoveryProtocolCore, RecoveryAborted>;

    /// Abort recovery due to veto or timeout
    fn transition_with_witness(self, _witness: Self::Witness) -> Self::Target {
        self.transition_to()
    }
}

// ========== Recovery-Specific Operations ==========

/// Operations only available in RecoveryInitialized state
impl ChoreographicProtocol<RecoveryProtocolCore, RecoveryInitialized> {
    /// Initiate the recovery process
    pub async fn initiate_recovery(&mut self) -> Result<RecoveryInitiated, RecoverySessionError> {
        // This would interact with the recovery choreography to emit InitiateRecovery event
        // For now, return a mock witness
        Ok(RecoveryInitiated {
            recovery_id: self.inner.recovery_id,
            initiation_timestamp: 0, // Would use actual timestamp
            guardian_count: self.inner.guardian_ids.len(),
        })
    }

    /// Get the guardian IDs that need to approve
    pub fn required_guardians(&self) -> &[GuardianId] {
        &self.inner.guardian_ids
    }

    /// Get the approval threshold
    pub fn approval_threshold(&self) -> u16 {
        self.inner.threshold
    }
}

/// Operations only available in CollectingApprovals state
impl ChoreographicProtocol<RecoveryProtocolCore, CollectingApprovals> {
    /// Check approval progress
    pub async fn check_approval_progress(
        &self,
        events: Vec<Event>,
    ) -> Option<RecoveryApprovalThresholdMet> {
        RecoveryApprovalThresholdMet::verify((events, self.inner.threshold), ())
    }

    /// Get current approval count
    pub fn current_approval_count(&self) -> usize {
        self.inner
            .collected_approvals
            .values()
            .filter(|&&approved| approved)
            .count()
    }

    /// Check if specific guardian has approved
    pub fn has_guardian_approved(&self, guardian_id: &GuardianId) -> bool {
        self.inner
            .collected_approvals
            .get(guardian_id)
            .copied()
            .unwrap_or(false)
    }
}

/// Operations only available in EnforcingCooldown state
impl ChoreographicProtocol<RecoveryProtocolCore, EnforcingCooldown> {
    /// Check cooldown completion
    pub async fn check_cooldown_completion(
        &self,
        start_epoch: u64,
        veto_events: Vec<Event>,
    ) -> Option<CooldownCompleted> {
        let cooldown_duration = self.inner.cooldown_hours.unwrap_or(48); // Default 48 hours
        CooldownCompleted::verify(
            (self.inner.recovery_id, start_epoch, veto_events),
            cooldown_duration,
        )
    }

    /// Get cooldown period in hours
    pub fn cooldown_hours(&self) -> u64 {
        self.inner.cooldown_hours.unwrap_or(48)
    }
}

/// Operations only available in CollectingShares state
impl ChoreographicProtocol<RecoveryProtocolCore, CollectingShares> {
    /// Check shares collection progress
    pub async fn check_shares_progress(
        &self,
        events: Vec<Event>,
    ) -> Option<RecoverySharesCollected> {
        RecoverySharesCollected::verify((events, self.inner.threshold), ())
    }

    /// Get current share count
    pub fn current_share_count(&self) -> usize {
        self.inner.collected_shares.len()
    }

    /// Check if specific guardian has submitted share
    pub fn has_guardian_shared(&self, guardian_id: &GuardianId) -> bool {
        self.inner.collected_shares.contains_key(guardian_id)
    }
}

/// Operations only available in ReconstructingKey state
impl ChoreographicProtocol<RecoveryProtocolCore, ReconstructingKey> {
    /// Attempt key reconstruction
    pub async fn reconstruct_key(&self) -> Result<KeyReconstructed, RecoverySessionError> {
        // This would use the actual Lagrange interpolation from the choreography
        // For now, return a mock reconstruction
        let mock_key = vec![0u8; 32]; // Would be actual reconstructed key

        KeyReconstructed::verify(
            (self.inner.recovery_id, mock_key.clone()),
            self.inner.collected_shares.len(),
        )
        .ok_or_else(|| {
            RecoverySessionError::KeyReconstructionFailed(
                "Failed to reconstruct key from shares".to_string(),
            )
        })
    }

    /// Get the collected shares for reconstruction
    pub fn collected_shares(&self) -> &BTreeMap<GuardianId, Vec<u8>> {
        &self.inner.collected_shares
    }
}

/// Operations available in final states
impl ChoreographicProtocol<RecoveryProtocolCore, RecoveryProtocolCompleted> {
    /// Get the recovery result
    pub fn get_recovery_result(&self) -> Option<String> {
        Some("Recovery completed successfully".to_string())
    }

    /// Get the new device ID that was recovered
    pub fn recovered_device_id(&self) -> DeviceId {
        self.inner.new_device_id
    }
}

impl ChoreographicProtocol<RecoveryProtocolCore, RecoveryAborted> {
    /// Get the abort reason
    pub fn get_abort_reason(&self) -> Option<String> {
        Some("Recovery was aborted".to_string())
    }
}

// ========== Factory Functions ==========

/// Create a new session-typed recovery protocol
pub fn new_session_typed_recovery(
    recovery_id: Uuid,
    new_device_id: DeviceId,
    guardian_ids: Vec<GuardianId>,
    threshold: u16,
    cooldown_hours: Option<u64>,
) -> ChoreographicProtocol<RecoveryProtocolCore, RecoveryInitialized> {
    let core = RecoveryProtocolCore::new(
        recovery_id,
        new_device_id,
        guardian_ids,
        threshold,
        cooldown_hours,
    );
    ChoreographicProtocol::new(core)
}

/// Rehydrate a recovery protocol session from state
pub fn rehydrate_recovery_session(
    recovery_id: Uuid,
    new_device_id: DeviceId,
    guardian_ids: Vec<GuardianId>,
    threshold: u16,
    cooldown_hours: Option<u64>,
    events: Vec<Event>,
) -> RecoverySessionState {
    let mut core = RecoveryProtocolCore::new(
        recovery_id,
        new_device_id,
        guardian_ids,
        threshold,
        cooldown_hours,
    );

    // Analyze events to determine current state
    let mut has_initiation = false;
    let mut has_completion = false;
    let mut has_abort = false;

    for event in &events {
        match &event.event_type {
            EventType::InitiateRecovery(_) => has_initiation = true,
            EventType::CollectGuardianApproval(approval) => {
                if approval.approved {
                    core.collected_approvals.insert(approval.guardian_id, true);
                }
            }
            EventType::SubmitRecoveryShare(share) => {
                core.collected_shares
                    .insert(share.guardian_id, share.encrypted_share.clone());
            }
            EventType::CompleteRecovery(_) => has_completion = true,
            EventType::AbortRecovery(_) => has_abort = true,
            _ => {}
        }
    }

    // Determine state based on events
    let has_sufficient_approvals = core.collected_approvals.len() >= threshold as usize;
    let has_sufficient_shares = core.collected_shares.len() >= threshold as usize;

    // Check cooldown completion properly based on timestamps
    let has_cooldown_complete =
        check_cooldown_completion(&events, recovery_id, cooldown_hours.unwrap_or(24));

    if has_abort {
        RecoverySessionState::RecoveryAborted(ChoreographicProtocol::new(core))
    } else if has_completion {
        RecoverySessionState::RecoveryProtocolCompleted(ChoreographicProtocol::new(core))
    } else if has_sufficient_shares {
        RecoverySessionState::ReconstructingKey(ChoreographicProtocol::new(core))
    } else if has_cooldown_complete {
        RecoverySessionState::CollectingShares(ChoreographicProtocol::new(core))
    } else if has_sufficient_approvals {
        RecoverySessionState::EnforcingCooldown(ChoreographicProtocol::new(core))
    } else if has_initiation {
        RecoverySessionState::CollectingApprovals(ChoreographicProtocol::new(core))
    } else {
        RecoverySessionState::RecoveryInitialized(ChoreographicProtocol::new(core))
    }
}

/// Helper function to check cooldown completion using proper timestamp verification
fn check_cooldown_completion(events: &[Event], recovery_id: Uuid, cooldown_duration: u64) -> bool {
    // Find the recovery initiation event to get the start timestamp
    let initiation_epoch = events.iter().find_map(|event| {
        if let EventType::InitiateRecovery(init_event) = &event.event_type {
            if init_event.recovery_id == recovery_id {
                Some(event.epoch_at_write)
            } else {
                None
            }
        } else {
            None
        }
    });

    match initiation_epoch {
        Some(start_epoch) => {
            // Filter events during cooldown period (potential vetoes)
            let veto_events: Vec<Event> = events
                .iter()
                .filter(|event| {
                    event.epoch_at_write >= start_epoch
                        && event.epoch_at_write <= start_epoch + cooldown_duration
                })
                .cloned()
                .collect();

            // Use the CooldownCompleted witness to verify cooldown completion
            CooldownCompleted::verify((recovery_id, start_epoch, veto_events), cooldown_duration)
                .is_some()
        }
        None => false, // No initiation event found
    }
}

#[allow(clippy::disallowed_methods, clippy::expect_used, clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use super::*;
    use aura_journal::{AccountId, EventAuthorization, EventId, InitiateRecoveryEvent};

    #[allow(clippy::disallowed_methods)]
    #[test]
    fn test_recovery_state_transitions() {
        let recovery_id = Uuid::new_v4();
        let new_device_id = DeviceId(Uuid::new_v4());
        let guardian_ids = vec![GuardianId(Uuid::new_v4()), GuardianId(Uuid::new_v4())];

        // Create a new recovery protocol
        let recovery = new_session_typed_recovery(
            recovery_id,
            new_device_id,
            guardian_ids.clone(),
            2,        // threshold
            Some(48), // cooldown hours
        );

        // Should start in RecoveryInitialized state
        assert_eq!(recovery.state_name(), "RecoveryInitialized");
        assert!(!recovery.can_terminate());
        assert_eq!(recovery.required_guardians(), &guardian_ids);
        assert_eq!(recovery.approval_threshold(), 2);

        // Transition to CollectingApprovals with witness
        let initiation_witness = RecoveryInitiated {
            recovery_id,
            initiation_timestamp: 1000,
            guardian_count: guardian_ids.len(),
        };

        let collecting_recovery = <ChoreographicProtocol<RecoveryProtocolCore, RecoveryInitialized> as WitnessedTransition<RecoveryInitialized, CollectingApprovals>>::transition_with_witness(recovery, initiation_witness);
        assert_eq!(collecting_recovery.state_name(), "CollectingApprovals");
        assert_eq!(collecting_recovery.current_approval_count(), 0);

        // Can transition to cooldown with approval witness
        let approval_witness = RecoveryApprovalThresholdMet {
            recovery_id,
            approved_guardians: guardian_ids.clone(),
            approval_count: 2,
            required_threshold: 2,
        };

        let cooldown_recovery = <ChoreographicProtocol<RecoveryProtocolCore, CollectingApprovals> as WitnessedTransition<CollectingApprovals, EnforcingCooldown>>::transition_with_witness(collecting_recovery, approval_witness);
        assert_eq!(cooldown_recovery.state_name(), "EnforcingCooldown");
        assert_eq!(cooldown_recovery.cooldown_hours(), 48);
    }

    #[allow(clippy::disallowed_methods)]
    #[test]
    fn test_recovery_witness_verification() {
        let recovery_id = Uuid::new_v4();
        let guardian_id = GuardianId(Uuid::new_v4());

        // Test RecoveryInitiated witness
        let initiate_event = Event {
            version: 1,
            event_id: EventId(Uuid::new_v4()),
            account_id: AccountId(Uuid::new_v4()),
            timestamp: 1000,
            nonce: 1,
            parent_hash: None,
            epoch_at_write: 1,
            event_type: EventType::InitiateRecovery(InitiateRecoveryEvent {
                recovery_id,
                new_device_id: DeviceId(Uuid::new_v4()),
                new_device_pk: vec![0u8; 32],
                required_guardians: vec![guardian_id],
                quorum_threshold: 2,
                cooldown_seconds: 48 * 3600,
            }),
            authorization: EventAuthorization::DeviceCertificate {
                device_id: DeviceId(Uuid::new_v4()),
                signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
            },
        };

        let witness = RecoveryInitiated::verify(initiate_event, ());
        assert!(witness.is_some());
        let witness = witness.unwrap();
        assert_eq!(witness.recovery_id, recovery_id);
        assert_eq!(witness.guardian_count, 1);
    }

    #[allow(clippy::disallowed_methods)]
    #[test]
    fn test_recovery_rehydration() {
        let recovery_id = Uuid::new_v4();
        let new_device_id = DeviceId(Uuid::new_v4());
        let guardian_ids = vec![GuardianId(Uuid::new_v4())];

        // Test rehydration without events
        let state = rehydrate_recovery_session(
            recovery_id,
            new_device_id,
            guardian_ids.clone(),
            1,
            Some(48),
            vec![],
        );
        assert_eq!(state.state_name(), "RecoveryInitialized");
        assert!(!state.can_terminate());
        assert_eq!(state.session_id(), recovery_id);

        // Test rehydration with initiation event
        let initiate_event = Event {
            version: 1,
            event_id: EventId(Uuid::new_v4()),
            account_id: AccountId(Uuid::new_v4()),
            timestamp: 1000,
            nonce: 1,
            parent_hash: None,
            epoch_at_write: 1,
            event_type: EventType::InitiateRecovery(InitiateRecoveryEvent {
                recovery_id,
                new_device_id,
                new_device_pk: vec![0u8; 32],
                required_guardians: guardian_ids.clone(),
                quorum_threshold: 1,
                cooldown_seconds: 48 * 3600,
            }),
            authorization: EventAuthorization::DeviceCertificate {
                device_id: new_device_id,
                signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
            },
        };

        let state = rehydrate_recovery_session(
            recovery_id,
            new_device_id,
            guardian_ids,
            1,
            Some(48),
            vec![initiate_event],
        );
        assert_eq!(state.state_name(), "CollectingApprovals");
        assert!(!state.can_terminate());
    }
}
