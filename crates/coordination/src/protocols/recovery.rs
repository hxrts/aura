//! Recovery Protocol: Complete Implementation
//!
//! This module contains the complete implementation of the guardian-based recovery
//! protocol, including both the session type definitions for compile-time safety and the
//! choreographic execution logic. This merger improves cohesion and maintainability by
//! keeping all protocol-related code in a single file.

// ========== Session Type Definitions ==========

use crate::session_types::wrapper::{SessionProtocol, SessionTypedProtocol};
use aura_types::{DeviceId, GuardianId};
use aura_journal::{Event, EventType};
use session_types::witnesses::RuntimeWitness;
use session_types::SessionState;
use std::collections::BTreeMap;
use uuid::Uuid;

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

// ========== State Definitions ==========

/// Initial state where recovery is being initiated
#[derive(Debug, Clone)]
pub struct RecoveryInitialized;

impl SessionState for RecoveryInitialized {
    const NAME: &'static str = "RecoveryInitialized";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// State where guardian approvals are being collected
#[derive(Debug, Clone)]
pub struct CollectingApprovals;

impl SessionState for CollectingApprovals {
    const NAME: &'static str = "CollectingApprovals";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// State where cooldown period is being enforced
#[derive(Debug, Clone)]
pub struct EnforcingCooldown;

impl SessionState for EnforcingCooldown {
    const NAME: &'static str = "EnforcingCooldown";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// State where recovery shares are being collected
#[derive(Debug, Clone)]
pub struct CollectingShares;

impl SessionState for CollectingShares {
    const NAME: &'static str = "CollectingShares";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// State where recovery key is being reconstructed
#[derive(Debug, Clone)]
pub struct ReconstructingKey;

impl SessionState for ReconstructingKey {
    const NAME: &'static str = "ReconstructingKey";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// Final successful state with recovery completed
#[derive(Debug, Clone)]
pub struct RecoveryProtocolCompleted;

impl SessionState for RecoveryProtocolCompleted {
    const NAME: &'static str = "RecoveryProtocolCompleted";
    const IS_FINAL: bool = true;
    const CAN_TERMINATE: bool = true;
}

/// Final failure state
#[derive(Debug, Clone)]
pub struct RecoveryAborted;

impl SessionState for RecoveryAborted {
    const NAME: &'static str = "RecoveryAborted";
    const IS_FINAL: bool = true;
    const CAN_TERMINATE: bool = true;
}

// ========== Runtime Witnesses ==========

/// Witness that recovery has been successfully initiated
#[derive(Debug, Clone)]
pub struct RecoveryInitiated {
    pub recovery_id: Uuid,
    pub initiation_timestamp: u64,
    pub guardian_count: usize,
}

impl RuntimeWitness for RecoveryInitiated {
    type Evidence = (DeviceId, u64, usize); // (device_id, timestamp, guardian_count)
    type Config = ();

    #[allow(clippy::disallowed_methods)]
    fn verify(evidence: Self::Evidence, _config: Self::Config) -> Option<Self> {
        let (_device_id, initiation_timestamp, guardian_count) = evidence;
        if guardian_count > 0 {
            Some(RecoveryInitiated {
                recovery_id: Uuid::new_v4(),
                initiation_timestamp,
                guardian_count,
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Recovery protocol initiated successfully"
    }
}

impl RecoveryInitiated {
    /// Check if the witness is valid
    pub fn check(&self) -> Result<(), RecoverySessionError> {
        if self.guardian_count == 0 {
            return Err(RecoverySessionError::ProtocolError(
                "No guardians available".to_string(),
            ));
        }
        Ok(())
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
    type Evidence = (Uuid, Vec<GuardianId>, u16); // (recovery_id, guardians, threshold)
    type Config = ();

    fn verify(evidence: Self::Evidence, _config: Self::Config) -> Option<Self> {
        let (recovery_id, approved_guardians, required_threshold) = evidence;
        let approval_count = approved_guardians.len();
        if approval_count >= required_threshold as usize {
            Some(RecoveryApprovalThresholdMet {
                recovery_id,
                approved_guardians,
                approval_count,
                required_threshold,
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Recovery approval threshold met"
    }
}

impl RecoveryApprovalThresholdMet {
    /// Check if the witness is valid
    pub fn check(&self) -> Result<(), RecoverySessionError> {
        if self.approval_count < self.required_threshold as usize {
            return Err(RecoverySessionError::InsufficientApprovals);
        }
        Ok(())
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
    type Evidence = (Uuid, u64, u64, bool); // (recovery_id, cooldown_start, cooldown_end, no_vetoes)
    type Config = ();

    fn verify(evidence: Self::Evidence, _config: Self::Config) -> Option<Self> {
        let (recovery_id, cooldown_start, cooldown_end, no_vetoes) = evidence;
        if no_vetoes && cooldown_end > cooldown_start {
            Some(CooldownCompleted {
                recovery_id,
                cooldown_start,
                cooldown_end,
                no_vetoes,
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Recovery cooldown completed without vetoes"
    }
}

impl CooldownCompleted {
    /// Check if the witness is valid
    pub fn check(&self) -> Result<(), RecoverySessionError> {
        if !self.no_vetoes {
            return Err(RecoverySessionError::RecoveryAborted(
                "Recovery was vetoed".to_string(),
            ));
        }
        if self.cooldown_end <= self.cooldown_start {
            return Err(RecoverySessionError::ProtocolError(
                "Invalid cooldown period".to_string(),
            ));
        }
        Ok(())
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
    type Evidence = (Uuid, BTreeMap<GuardianId, Vec<u8>>, u16); // (recovery_id, shares, threshold)
    type Config = ();

    fn verify(evidence: Self::Evidence, _config: Self::Config) -> Option<Self> {
        let (recovery_id, collected_shares, required_threshold) = evidence;
        let share_count = collected_shares.len();
        if share_count >= required_threshold as usize {
            Some(RecoverySharesCollected {
                recovery_id,
                collected_shares,
                share_count,
                required_threshold,
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Sufficient recovery shares collected"
    }
}

impl RecoverySharesCollected {
    /// Check if the witness is valid
    pub fn check(&self) -> Result<(), RecoverySessionError> {
        if self.share_count < self.required_threshold as usize {
            return Err(RecoverySessionError::SharesCollectionFailed(format!(
                "Need {} shares, have {}",
                self.required_threshold, self.share_count
            )));
        }
        Ok(())
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
    type Evidence = (Uuid, Vec<u8>, usize); // (recovery_id, key, shares_used)
    type Config = ();

    fn verify(evidence: Self::Evidence, _config: Self::Config) -> Option<Self> {
        let (recovery_id, reconstructed_key, shares_used) = evidence;
        if reconstructed_key.len() == 32 && shares_used > 0 {
            Some(KeyReconstructed {
                recovery_id,
                reconstructed_key,
                shares_used,
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Recovery key successfully reconstructed"
    }
}

impl KeyReconstructed {
    /// Check if the witness is valid
    pub fn check(&self) -> Result<(), RecoverySessionError> {
        if self.reconstructed_key.len() != 32 {
            return Err(RecoverySessionError::KeyReconstructionFailed(
                "Invalid key length".to_string(),
            ));
        }
        if self.shares_used == 0 {
            return Err(RecoverySessionError::KeyReconstructionFailed(
                "No shares used in reconstruction".to_string(),
            ));
        }
        Ok(())
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
    type Evidence = (Uuid, String, Option<GuardianId>); // (recovery_id, reason, aborted_by)
    type Config = ();

    fn verify(evidence: Self::Evidence, _config: Self::Config) -> Option<Self> {
        let (recovery_id, abort_reason, aborted_by) = evidence;
        if !abort_reason.is_empty() {
            Some(RecoveryAbort {
                recovery_id,
                abort_reason,
                aborted_by,
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Recovery protocol aborted"
    }
}

impl RecoveryAbort {
    /// Check if the witness is valid
    pub fn check(&self) -> Result<(), RecoverySessionError> {
        if self.abort_reason.is_empty() {
            return Err(RecoverySessionError::ProtocolError(
                "Abort reason cannot be empty".to_string(),
            ));
        }
        Ok(())
    }
}

// ========== Protocol State Machine ==========

/// Union type representing all possible recovery session states
#[derive(Debug, Clone)]
pub enum RecoveryProtocolState {
    RecoveryInitialized(SessionTypedProtocol<RecoveryProtocolCore, RecoveryInitialized>),
    CollectingApprovals(SessionTypedProtocol<RecoveryProtocolCore, CollectingApprovals>),
    EnforcingCooldown(SessionTypedProtocol<RecoveryProtocolCore, EnforcingCooldown>),
    CollectingShares(SessionTypedProtocol<RecoveryProtocolCore, CollectingShares>),
    ReconstructingKey(SessionTypedProtocol<RecoveryProtocolCore, ReconstructingKey>),
    RecoveryProtocolCompleted(
        SessionTypedProtocol<RecoveryProtocolCore, RecoveryProtocolCompleted>,
    ),
    RecoveryAborted(SessionTypedProtocol<RecoveryProtocolCore, RecoveryAborted>),
}

// Marker type for recovery union state
#[derive(Debug, Clone)]
pub struct RecoveryUnionState;

impl SessionState for RecoveryUnionState {
    const NAME: &'static str = "RecoveryUnion";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = true;
}

impl SessionProtocol for RecoveryProtocolState {
    fn session_id(&self) -> Uuid {
        match self {
            RecoveryProtocolState::RecoveryInitialized(p) => p.core().recovery_id,
            RecoveryProtocolState::CollectingApprovals(p) => p.core().recovery_id,
            RecoveryProtocolState::EnforcingCooldown(p) => p.core().recovery_id,
            RecoveryProtocolState::CollectingShares(p) => p.core().recovery_id,
            RecoveryProtocolState::ReconstructingKey(p) => p.core().recovery_id,
            RecoveryProtocolState::RecoveryProtocolCompleted(p) => p.core().recovery_id,
            RecoveryProtocolState::RecoveryAborted(p) => p.core().recovery_id,
        }
    }

    fn device_id(&self) -> Uuid {
        match self {
            RecoveryProtocolState::RecoveryInitialized(p) => p.core().new_device_id.0,
            RecoveryProtocolState::CollectingApprovals(p) => p.core().new_device_id.0,
            RecoveryProtocolState::EnforcingCooldown(p) => p.core().new_device_id.0,
            RecoveryProtocolState::CollectingShares(p) => p.core().new_device_id.0,
            RecoveryProtocolState::ReconstructingKey(p) => p.core().new_device_id.0,
            RecoveryProtocolState::RecoveryProtocolCompleted(p) => p.core().new_device_id.0,
            RecoveryProtocolState::RecoveryAborted(p) => p.core().new_device_id.0,
        }
    }

    fn state_name(&self) -> &'static str {
        match self {
            RecoveryProtocolState::RecoveryInitialized(_) => RecoveryInitialized::NAME,
            RecoveryProtocolState::CollectingApprovals(_) => CollectingApprovals::NAME,
            RecoveryProtocolState::EnforcingCooldown(_) => EnforcingCooldown::NAME,
            RecoveryProtocolState::CollectingShares(_) => CollectingShares::NAME,
            RecoveryProtocolState::ReconstructingKey(_) => ReconstructingKey::NAME,
            RecoveryProtocolState::RecoveryProtocolCompleted(_) => RecoveryProtocolCompleted::NAME,
            RecoveryProtocolState::RecoveryAborted(_) => RecoveryAborted::NAME,
        }
    }

    fn can_terminate(&self) -> bool {
        match self {
            RecoveryProtocolState::RecoveryInitialized(_) => RecoveryInitialized::CAN_TERMINATE,
            RecoveryProtocolState::CollectingApprovals(_) => CollectingApprovals::CAN_TERMINATE,
            RecoveryProtocolState::EnforcingCooldown(_) => EnforcingCooldown::CAN_TERMINATE,
            RecoveryProtocolState::CollectingShares(_) => CollectingShares::CAN_TERMINATE,
            RecoveryProtocolState::ReconstructingKey(_) => ReconstructingKey::CAN_TERMINATE,
            RecoveryProtocolState::RecoveryProtocolCompleted(_) => {
                RecoveryProtocolCompleted::CAN_TERMINATE
            }
            RecoveryProtocolState::RecoveryAborted(_) => RecoveryAborted::CAN_TERMINATE,
        }
    }

    fn protocol_id(&self) -> Uuid {
        // For union types, protocol_id is the same as session_id
        self.session_id()
    }

    fn is_final(&self) -> bool {
        matches!(
            self,
            RecoveryProtocolState::RecoveryProtocolCompleted(_)
                | RecoveryProtocolState::RecoveryAborted(_)
        )
    }
}

// ========== State Transition Methods ==========

impl RecoveryProtocolState {
    /// Check if protocol is in a final state
    pub fn is_final(&self) -> bool {
        matches!(
            self,
            RecoveryProtocolState::RecoveryProtocolCompleted(_)
                | RecoveryProtocolState::RecoveryAborted(_)
        )
    }

    /// Transition from RecoveryInitialized to CollectingApprovals
    pub fn begin_collecting_approvals(
        self,
        witness: RecoveryInitiated,
    ) -> Result<RecoveryProtocolState, RecoverySessionError> {
        witness.check()?;
        match self {
            RecoveryProtocolState::RecoveryInitialized(protocol) => {
                let core = protocol.into_core();
                let new_protocol = SessionTypedProtocol::new(core);
                Ok(RecoveryProtocolState::CollectingApprovals(new_protocol))
            }
            _ => Err(RecoverySessionError::InvalidOperation),
        }
    }

    /// Transition from CollectingApprovals to EnforcingCooldown
    pub fn begin_cooldown(
        self,
        witness: RecoveryApprovalThresholdMet,
    ) -> Result<RecoveryProtocolState, RecoverySessionError> {
        witness.check()?;
        match self {
            RecoveryProtocolState::CollectingApprovals(protocol) => {
                let mut core = protocol.into_core();
                // Update collected approvals
                for guardian_id in witness.approved_guardians {
                    core.collected_approvals.insert(guardian_id, true);
                }
                let new_protocol = SessionTypedProtocol::new(core);
                Ok(RecoveryProtocolState::EnforcingCooldown(new_protocol))
            }
            _ => Err(RecoverySessionError::InvalidOperation),
        }
    }

    /// Transition from EnforcingCooldown to CollectingShares
    pub fn begin_collecting_shares(
        self,
        witness: CooldownCompleted,
    ) -> Result<RecoveryProtocolState, RecoverySessionError> {
        witness.check()?;
        match self {
            RecoveryProtocolState::EnforcingCooldown(protocol) => {
                let core = protocol.into_core();
                let new_protocol = SessionTypedProtocol::new(core);
                Ok(RecoveryProtocolState::CollectingShares(new_protocol))
            }
            _ => Err(RecoverySessionError::InvalidOperation),
        }
    }

    /// Transition from CollectingShares to ReconstructingKey
    pub fn begin_key_reconstruction(
        self,
        witness: RecoverySharesCollected,
    ) -> Result<RecoveryProtocolState, RecoverySessionError> {
        witness.check()?;
        match self {
            RecoveryProtocolState::CollectingShares(protocol) => {
                let mut core = protocol.into_core();
                // Update collected shares
                core.collected_shares = witness.collected_shares;
                let new_protocol = SessionTypedProtocol::new(core);
                Ok(RecoveryProtocolState::ReconstructingKey(new_protocol))
            }
            _ => Err(RecoverySessionError::InvalidOperation),
        }
    }

    /// Transition from ReconstructingKey to RecoveryProtocolCompleted
    pub fn complete_recovery(
        self,
        witness: KeyReconstructed,
    ) -> Result<RecoveryProtocolState, RecoverySessionError> {
        witness.check()?;
        match self {
            RecoveryProtocolState::ReconstructingKey(protocol) => {
                let core = protocol.into_core();
                let new_protocol = SessionTypedProtocol::new(core);
                Ok(RecoveryProtocolState::RecoveryProtocolCompleted(
                    new_protocol,
                ))
            }
            _ => Err(RecoverySessionError::InvalidOperation),
        }
    }

    /// Transition to RecoveryAborted from any non-final state
    pub fn abort_recovery(
        self,
        witness: RecoveryAbort,
    ) -> Result<RecoveryProtocolState, RecoverySessionError> {
        witness.check()?;
        if self.is_final() {
            return Err(RecoverySessionError::InvalidOperation);
        }

        let core = match self {
            RecoveryProtocolState::RecoveryInitialized(p) => p.into_core(),
            RecoveryProtocolState::CollectingApprovals(p) => p.into_core(),
            RecoveryProtocolState::EnforcingCooldown(p) => p.into_core(),
            RecoveryProtocolState::CollectingShares(p) => p.into_core(),
            RecoveryProtocolState::ReconstructingKey(p) => p.into_core(),
            _ => return Err(RecoverySessionError::InvalidOperation),
        };

        let new_protocol = SessionTypedProtocol::new(core);
        Ok(RecoveryProtocolState::RecoveryAborted(new_protocol))
    }
}

// ========== Constructor Functions ==========

/// Create a new recovery protocol instance in the initial state
pub fn new_recovery_protocol(
    recovery_id: Uuid,
    new_device_id: DeviceId,
    guardian_ids: Vec<GuardianId>,
    threshold: u16,
    cooldown_hours: Option<u64>,
) -> Result<RecoveryProtocolState, RecoverySessionError> {
    let core = RecoveryProtocolCore::new(
        recovery_id,
        new_device_id,
        guardian_ids,
        threshold,
        cooldown_hours,
    );
    let protocol = SessionTypedProtocol::new(core);
    Ok(RecoveryProtocolState::RecoveryInitialized(protocol))
}

/// Rehydrate a recovery protocol from crash recovery evidence
pub fn rehydrate_recovery_protocol(
    recovery_id: Uuid,
    new_device_id: DeviceId,
    guardian_ids: Vec<GuardianId>,
    threshold: u16,
    cooldown_hours: Option<u64>,
    events: Vec<Event>,
) -> Result<RecoveryProtocolState, RecoverySessionError> {
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

    if has_abort {
        Ok(RecoveryProtocolState::RecoveryAborted(
            SessionTypedProtocol::new(core),
        ))
    } else if has_completion {
        Ok(RecoveryProtocolState::RecoveryProtocolCompleted(
            SessionTypedProtocol::new(core),
        ))
    } else if has_sufficient_shares {
        Ok(RecoveryProtocolState::ReconstructingKey(
            SessionTypedProtocol::new(core),
        ))
    } else if has_sufficient_approvals {
        Ok(RecoveryProtocolState::CollectingShares(
            SessionTypedProtocol::new(core),
        ))
    } else if has_initiation {
        Ok(RecoveryProtocolState::CollectingApprovals(
            SessionTypedProtocol::new(core),
        ))
    } else {
        Ok(RecoveryProtocolState::RecoveryInitialized(
            SessionTypedProtocol::new(core),
        ))
    }
}

// ========== Choreographic Execution Logic ==========

use crate::execution::{
    EventAwaiter, EventBuilder, EventTypePattern, Instruction, InstructionResult, ProtocolContext,
    ProtocolContextExt, ProtocolError, ProtocolErrorType, SessionLifecycle,
};
use crate::protocol_results::{GuardianSignature, RecoveryProtocolResult};
use aura_crypto::{decrypt_with_aad, HpkeCiphertext, LagrangeInterpolation, SharePoint};
use aura_journal::{CompleteRecoveryEvent, InitiateRecoveryEvent, OperationType, ParticipantId as JournalParticipantId, ProtocolType, Session};
use ed25519_dalek::Signer;

/// Recovery Protocol implementation using SessionLifecycle trait
pub struct RecoveryProtocol<'a> {
    ctx: &'a mut ProtocolContext,
    guardian_ids: Vec<GuardianId>,
    threshold: u16,
}

impl<'a> RecoveryProtocol<'a> {
    pub fn new(
        ctx: &'a mut ProtocolContext,
        guardian_ids: Vec<GuardianId>,
        threshold: u16,
    ) -> Self {
        Self {
            ctx,
            guardian_ids,
            threshold,
        }
    }
}

#[async_trait::async_trait]
impl<'a> SessionLifecycle for RecoveryProtocol<'a> {
    type Result = RecoveryProtocolResult; // Recovery result with proof

    fn operation_type(&self) -> OperationType {
        OperationType::Recovery
    }

    fn generate_context_id(&self) -> Vec<u8> {
        format!("recovery:{}:{:?}", self.threshold, self.guardian_ids).into_bytes()
    }

    async fn create_session(&mut self) -> Result<Session, ProtocolError> {
        let ledger_context = self.ctx.fetch_ledger_context().await?;

        // Convert guardians to session participants
        let session_participants: Vec<JournalParticipantId> = self
            .guardian_ids
            .iter()
            .map(|&guardian_id| JournalParticipantId::Guardian(guardian_id))
            .collect();

        // Create Recovery session
        Ok(Session::new(
            aura_journal::SessionId(self.ctx.session_id()),
            ProtocolType::Recovery,
            session_participants,
            ledger_context.epoch,
            200, // TTL in epochs - recovery has longer time limit due to cooldown
            self.ctx.effects().now().map_err(|e| ProtocolError {
                session_id: self.ctx.session_id(),
                error_type: ProtocolErrorType::Other,
                message: format!("Failed to get timestamp: {:?}", e),
            })?,
        ))
    }

    async fn execute_protocol(
        &mut self,
        _session: &Session,
    ) -> Result<RecoveryProtocolResult, ProtocolError> {
        let recovery_id = self.ctx.session_id();
        let new_device_id = DeviceId(self.ctx.device_id());
        let _start_epoch = self.ctx.fetch_ledger_context().await?.epoch;

        // Extract device public key before the mutable borrow
        let new_device_pk = self.ctx.device_key().verifying_key().to_bytes().to_vec();

        // Phase 1: Initiate Recovery
        EventBuilder::new(self.ctx)
            .with_type(EventType::InitiateRecovery(InitiateRecoveryEvent {
                recovery_id,
                new_device_id,
                new_device_pk,
                required_guardians: self.guardian_ids.clone(),
                quorum_threshold: self.threshold,
                cooldown_seconds: 48 * 3600, // 48 hours in seconds
            }))
            .with_device_auth()
            .build_sign_and_emit()
            .await?;

        // Phase 2: Collect Guardian Approvals
        let guardian_approvals = self.collect_guardian_approvals(recovery_id).await?;

        // Phase 3: Enforce Cooldown Period
        self.enforce_cooldown_period(recovery_id).await?;

        // Phase 4: Reconstruct Recovery Share
        let recovered_share = self.reconstruct_recovery_share(&guardian_approvals).await?;

        // Phase 5: Complete Recovery
        // Generate a test signature to prove the device can use the recovered key
        let test_message = format!("recovery_test_{}_{}", recovery_id, new_device_id.0);
        let test_signature = self
            .ctx
            .device_key()
            .sign(test_message.as_bytes())
            .to_bytes()
            .to_vec();

        let _complete_event = EventBuilder::new(self.ctx)
            .with_type(EventType::CompleteRecovery(CompleteRecoveryEvent {
                recovery_id,
                new_device_id,
                test_signature,
            }))
            .with_device_auth()
            .build_sign_and_emit()
            .await?;

        // Collect all events from this protocol execution
        let ledger_events = self.ctx.collected_events().to_vec();

        // Create guardian signatures from approvals
        let guardian_signatures = guardian_approvals
            .keys()
            .map(|guardian_id| GuardianSignature {
                guardian_id: *guardian_id,
                signature: vec![0u8; 64], // Placeholder - would be actual signature
                signed_at: 0,
            })
            .collect();

        // Return complete protocol result
        Ok(RecoveryProtocolResult {
            session_id: aura_journal::SessionId(recovery_id),
            new_device_id,
            approving_guardians: guardian_approvals.keys().cloned().collect(),
            guardian_signatures,
            recovered_share,
            revocation_proof: None, // No revocation in this flow
            ledger_events,
        })
    }

    async fn wait_for_completion(
        &mut self,
        winning_session: &Session,
    ) -> Result<RecoveryProtocolResult, ProtocolError> {
        let complete_event = EventAwaiter::new(self.ctx)
            .for_session(winning_session.session_id.0)
            .for_event_types(vec![EventTypePattern::CompleteRecovery])
            .await_single(100) // Default TTL epochs
            .await?;

        match &complete_event.event_type {
            EventType::CompleteRecovery(complete) => {
                // Reconstruct protocol result from complete event
                Ok(RecoveryProtocolResult {
                    session_id: winning_session.session_id,
                    new_device_id: complete.new_device_id,
                    approving_guardians: vec![], // Would be extracted from event history
                    guardian_signatures: vec![],
                    recovered_share: vec![0u8; 32], // Placeholder
                    revocation_proof: None,
                    ledger_events: vec![complete_event],
                })
            }
            _ => Err(ProtocolError {
                session_id: self.ctx.session_id(),
                error_type: ProtocolErrorType::InvalidState,
                message: "Expected recovery complete event".to_string(),
            }),
        }
    }
}

impl<'a> RecoveryProtocol<'a> {
    /// Collect guardian approvals with encrypted recovery shares
    async fn collect_guardian_approvals(
        &mut self,
        recovery_id: Uuid,
    ) -> Result<BTreeMap<GuardianId, Vec<u8>>, ProtocolError> {
        let mut collected_shares = BTreeMap::new();

        // Wait for threshold guardian recovery shares
        for _ in 0..self.threshold {
            let share_event = EventAwaiter::new(self.ctx)
                .for_session(recovery_id)
                .for_event_types(vec![EventTypePattern::SubmitRecoveryShare])
                .await_single(500)
                .await?;

            if let EventType::SubmitRecoveryShare(ref share) = share_event.event_type {
                // Verify guardian is in approved list
                if !self.guardian_ids.contains(&share.guardian_id) {
                    return Err(ProtocolError {
                        session_id: self.ctx.session_id(),
                        error_type: ProtocolErrorType::UnexpectedEvent,
                        message: format!("Share from unexpected guardian: {:?}", share.guardian_id),
                    });
                }

                // Store encrypted recovery share
                collected_shares.insert(share.guardian_id, share.encrypted_share.clone());
            }
        }

        Ok(collected_shares)
    }

    /// Enforce the cooldown period with periodic checks for vetoes
    async fn enforce_cooldown_period(&mut self, recovery_id: Uuid) -> Result<(), ProtocolError> {
        let cooldown_epochs = 100; // Simplified for MVP

        // Check for vetoes periodically during cooldown
        for _ in 0..5 {
            // Check for abort events
            let abort_check = self
                .ctx
                .execute(Instruction::CheckForEvent {
                    filter: crate::execution::EventFilter {
                        session_id: Some(recovery_id),
                        event_types: Some(vec![EventTypePattern::AbortRecovery]),
                        authors: None,
                        predicate: None,
                    },
                })
                .await?;

            if let InstructionResult::EventReceived(event) = abort_check {
                if let EventType::AbortRecovery(_) = event.event_type {
                    return Err(ProtocolError {
                        session_id: self.ctx.session_id(),
                        error_type: ProtocolErrorType::RecoveryVetoed,
                        message: "Recovery aborted by guardian veto".to_string(),
                    });
                }
            }
            // No veto found, continue

            // Wait for a portion of the cooldown period
            let wait_result = self
                .ctx
                .execute(Instruction::WaitEpochs(cooldown_epochs / 5))
                .await?;

            match wait_result {
                InstructionResult::EpochsElapsed => continue,
                _ => {
                    return Err(ProtocolError {
                        session_id: self.ctx.session_id(),
                        error_type: ProtocolErrorType::InvalidState,
                        message: "Failed to wait epochs".to_string(),
                    })
                }
            }
        }

        Ok(())
    }

    /// Reconstruct the recovery share from guardian shares
    async fn reconstruct_recovery_share(
        &mut self,
        guardian_shares: &BTreeMap<GuardianId, Vec<u8>>,
    ) -> Result<Vec<u8>, ProtocolError> {
        let mut decrypted_points = Vec::new();

        // Decrypt each guardian's share
        for (_guardian_id, encrypted_share) in guardian_shares.iter() {
            // In production, each guardian would have encrypted their share
            // specifically for the recovering device's public key
            // For MVP, we'll simulate this

            // Parse encrypted share as HPKE ciphertext
            let ciphertext = HpkeCiphertext::from_bytes(encrypted_share)?;

            // Get device's HPKE private key for decryption
            let device_private_key = self.ctx.get_device_hpke_private_key().await?;

            // Decrypt with associated data for authenticity
            let aad = format!("recovery:{}", self.ctx.session_id()).into_bytes();
            let decrypted = decrypt_with_aad(&ciphertext, &device_private_key, &aad)?;

            // Parse as scalar point
            if decrypted.len() >= 32 {
                let mut bytes = [0u8; 32];
                bytes.copy_from_slice(&decrypted[..32]);
                let scalar = curve25519_dalek::scalar::Scalar::from_bytes_mod_order(bytes);
                decrypted_points.push(scalar);
            }
        }

        // Create SharePoints for Lagrange interpolation
        let share_points: Vec<SharePoint> = decrypted_points
            .into_iter()
            .enumerate()
            .map(|(i, y)| SharePoint {
                x: curve25519_dalek::scalar::Scalar::from((i + 1) as u64),
                y,
            })
            .collect();

        // Reconstruct the secret via Lagrange interpolation
        let recovered_scalar = LagrangeInterpolation::interpolate_at_zero(&share_points)?;

        Ok(recovered_scalar.to_bytes().to_vec())
    }
}

/// Recovery Protocol Choreography - Main entry point
pub async fn recovery_choreography(
    ctx: &mut ProtocolContext,
    guardian_ids: Vec<GuardianId>,
    threshold: u16,
) -> Result<RecoveryProtocolResult, ProtocolError> {
    let mut protocol = RecoveryProtocol::new(ctx, guardian_ids, threshold);
    protocol.execute().await
}

/// Nudge a guardian to approve a recovery request
pub async fn nudge_guardian(
    _ctx: &mut ProtocolContext,
    guardian_id: GuardianId,
    recovery_session_id: Uuid,
) -> Result<(), ProtocolError> {
    // This is a basic implementation - in production this would
    // send a notification to the guardian via their preferred channel
    tracing::info!(
        "Nudging guardian {} for recovery session {}",
        guardian_id.0,
        recovery_session_id
    );

    // For now, just log the nudge attempt
    Ok(())
}

// ========== Tests ==========

#[cfg(test)]
#[allow(warnings, clippy::all)]
mod tests {
    use super::*;
    use crate::execution::context::StubTransport;
    use aura_crypto::Effects;
    use aura_types::{AccountId};
use aura_journal::{AccountLedger, AccountState, EventAuthorization, EventId, InitiateRecoveryEvent};
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_recovery_choreography_structure() {
        // Use deterministic UUIDs for testing
        let session_id = Uuid::from_bytes([1u8; 16]);
        let device_id = Uuid::from_bytes([2u8; 16]);

        let guardian_ids = vec![
            GuardianId(Uuid::from_bytes([3u8; 16])),
            GuardianId(Uuid::from_bytes([4u8; 16])),
        ];

        // Create minimal context (won't actually execute)
        let device_metadata = aura_journal::DeviceMetadata {
            device_id: DeviceId(device_id),
            device_name: "test-device".to_string(),
            device_type: aura_journal::DeviceType::Native,
            public_key: ed25519_dalek::VerifyingKey::from_bytes(&[0u8; 32]).unwrap(),
            added_at: 0,
            last_seen: 0,
            dkd_commitment_proofs: std::collections::BTreeMap::new(),
            next_nonce: 1,
            used_nonces: std::collections::BTreeSet::new(),
        };

        let state = AccountState::new(
            AccountId(Uuid::from_bytes([6u8; 16])),
            ed25519_dalek::VerifyingKey::from_bytes(&[0u8; 32]).unwrap(),
            device_metadata,
            2,
            3,
        );

        let ledger = Arc::new(RwLock::new(AccountLedger::new(state).unwrap()));

        let device_key = ed25519_dalek::SigningKey::from_bytes(&[0u8; 32]);

        let ctx = ProtocolContext::new(
            session_id,
            device_id,
            vec![], // No participants for recovery
            Some(2),
            ledger,
            Arc::new(StubTransport::default()),
            Effects::test(),
            device_key,
            Box::new(crate::ProductionTimeSource::new()),
        );

        // Verify context is set up correctly
        assert_eq!(ctx.session_id(), session_id);
        assert_eq!(ctx.threshold(), Some(2));
    }

    #[test]
    fn test_recovery_session_state_transitions() {
        let recovery_id = Uuid::new_v4();
        let new_device_id = DeviceId(Uuid::new_v4());
        let guardian_ids = vec![GuardianId(Uuid::new_v4()), GuardianId(Uuid::new_v4())];

        // Test protocol creation
        let protocol = new_recovery_protocol(
            recovery_id,
            new_device_id,
            guardian_ids.clone(),
            2,
            Some(48),
        )
        .unwrap();
        assert!(!protocol.is_final());
        assert_eq!(protocol.state_name(), "RecoveryInitialized");

        // Test state transition to collecting approvals
        let initiation_witness = RecoveryInitiated {
            recovery_id,
            initiation_timestamp: 1000,
            guardian_count: guardian_ids.len(),
        };
        let protocol = protocol
            .begin_collecting_approvals(initiation_witness)
            .unwrap();
        assert_eq!(protocol.state_name(), "CollectingApprovals");

        // Test state transition to cooldown
        let approval_witness = RecoveryApprovalThresholdMet {
            recovery_id,
            approved_guardians: guardian_ids.clone(),
            approval_count: 2,
            required_threshold: 2,
        };
        let protocol = protocol.begin_cooldown(approval_witness).unwrap();
        assert_eq!(protocol.state_name(), "EnforcingCooldown");

        // Test state transition to collecting shares
        let cooldown_witness = CooldownCompleted {
            recovery_id,
            cooldown_start: 1000,
            cooldown_end: 2000,
            no_vetoes: true,
        };
        let protocol = protocol.begin_collecting_shares(cooldown_witness).unwrap();
        assert_eq!(protocol.state_name(), "CollectingShares");

        // Test state transition to key reconstruction
        let shares_witness = RecoverySharesCollected {
            recovery_id,
            collected_shares: {
                let mut shares = BTreeMap::new();
                shares.insert(guardian_ids[0], vec![1, 2, 3, 4]);
                shares.insert(guardian_ids[1], vec![5, 6, 7, 8]);
                shares
            },
            share_count: 2,
            required_threshold: 2,
        };
        let protocol = protocol.begin_key_reconstruction(shares_witness).unwrap();
        assert_eq!(protocol.state_name(), "ReconstructingKey");

        // Test completion
        let reconstruction_witness = KeyReconstructed {
            recovery_id,
            reconstructed_key: vec![0u8; 32],
            shares_used: 2,
        };
        let protocol = protocol.complete_recovery(reconstruction_witness).unwrap();
        assert_eq!(protocol.state_name(), "RecoveryProtocolCompleted");
        assert!(protocol.is_final());
    }

    #[test]
    fn test_recovery_abort_transition() {
        let recovery_id = Uuid::new_v4();
        let new_device_id = DeviceId(Uuid::new_v4());
        let guardian_ids = vec![GuardianId(Uuid::new_v4())];

        let protocol =
            new_recovery_protocol(recovery_id, new_device_id, guardian_ids, 1, Some(48)).unwrap();

        // Test abort transition from any non-final state
        let abort_witness = RecoveryAbort {
            recovery_id,
            abort_reason: "Test abort".to_string(),
            aborted_by: None,
        };
        let failed_protocol = protocol.abort_recovery(abort_witness).unwrap();
        assert_eq!(failed_protocol.state_name(), "RecoveryAborted");
        assert!(failed_protocol.is_final());
    }

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

        // Test witness validation
        let witness = RecoveryInitiated {
            recovery_id,
            initiation_timestamp: 1000,
            guardian_count: 1,
        };
        assert!(witness.check().is_ok());

        // Test invalid witness
        let invalid_witness = RecoveryInitiated {
            recovery_id,
            initiation_timestamp: 1000,
            guardian_count: 0, // Invalid: no guardians
        };
        assert!(invalid_witness.check().is_err());
    }
}
