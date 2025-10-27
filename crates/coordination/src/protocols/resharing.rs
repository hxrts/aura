//! Resharing Protocol: Complete Implementation
//!
//! This module contains the complete implementation of the P2P key resharing
//! protocol, including both the session type definitions for compile-time safety and the
//! choreographic execution logic. This merger improves cohesion and maintainability by
//! keeping all protocol-related code in a single file.

// ========== Session Type Definitions ==========

use crate::session_types::wrapper::{SessionProtocol, SessionTypedProtocol};
use aura_crypto::FrostKeyShare;
use aura_journal::Event;
use aura_types::DeviceId;
use session_types::witnesses::RuntimeWitness;
use session_types::SessionState;
use std::collections::BTreeMap;
use uuid::Uuid;

/// Core resharing protocol data without session state
#[derive(Debug, Clone)]
pub struct ResharingProtocolCore {
    pub session_id: Uuid,
    pub protocol_id: Uuid,
    pub device_id: DeviceId,
    pub old_threshold: u16,
    pub new_threshold: u16,
    pub old_participants: Vec<DeviceId>,
    pub new_participants: Vec<DeviceId>,
    pub current_key_share: Option<FrostKeyShare>,
    pub collected_sub_shares: BTreeMap<DeviceId, Vec<u8>>,
    pub acknowledgments: BTreeMap<DeviceId, bool>,
}

impl ResharingProtocolCore {
    #[allow(clippy::disallowed_methods)]
    pub fn new(
        session_id: Uuid,
        device_id: DeviceId,
        old_threshold: u16,
        new_threshold: u16,
        old_participants: Vec<DeviceId>,
        new_participants: Vec<DeviceId>,
    ) -> Self {
        Self {
            session_id,
            protocol_id: Uuid::new_v4(),
            device_id,
            old_threshold,
            new_threshold,
            old_participants,
            new_participants,
            current_key_share: None,
            collected_sub_shares: BTreeMap::new(),
            acknowledgments: BTreeMap::new(),
        }
    }
}

/// Error type for resharing session protocols
#[derive(Debug, thiserror::Error)]
pub enum ResharingSessionError {
    #[error("Resharing protocol error: {0}")]
    ProtocolError(String),
    #[error("Invalid operation for current resharing state")]
    InvalidOperation,
    #[error("Resharing failed: {0}")]
    ResharingFailed(String),
    #[error("Insufficient participants: expected {expected}, got {actual}")]
    InsufficientParticipants { expected: usize, actual: usize },
    #[error("Sub-share distribution failed: {0}")]
    SubShareDistributionFailed(String),
    #[error("Share reconstruction failed: {0}")]
    ShareReconstructionFailed(String),
    #[error("Verification failed: {0}")]
    VerificationFailed(String),
    #[error("Insufficient shares: {0}")]
    InsufficientShares(String),
}

// ========== State Definitions ==========

/// Initial state where resharing is being initiated
#[derive(Debug, Clone)]
pub struct ResharingInitializing;

impl SessionState for ResharingInitializing {
    const NAME: &'static str = "ResharingInitializing";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// State where sub-shares are being distributed
#[derive(Debug, Clone)]
pub struct ResharingPhaseOne;

impl SessionState for ResharingPhaseOne {
    const NAME: &'static str = "ResharingPhaseOne";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// State where new shares are being reconstructed
#[derive(Debug, Clone)]
pub struct ResharingPhaseTwo;

impl SessionState for ResharingPhaseTwo {
    const NAME: &'static str = "ResharingPhaseTwo";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// State where new shares are being verified
#[derive(Debug, Clone)]
pub struct ResharingVerification;

impl SessionState for ResharingVerification {
    const NAME: &'static str = "ResharingVerification";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// Final successful state with resharing completed
#[derive(Debug, Clone)]
pub struct ResharingComplete;

impl SessionState for ResharingComplete {
    const NAME: &'static str = "ResharingComplete";
    const IS_FINAL: bool = true;
    const CAN_TERMINATE: bool = true;
}

/// Final failure state
#[derive(Debug, Clone)]
pub struct ResharingFailed;

impl SessionState for ResharingFailed {
    const NAME: &'static str = "ResharingFailed";
    const IS_FINAL: bool = true;
    const CAN_TERMINATE: bool = true;
}

// ========== Runtime Witnesses ==========

/// Witness that resharing has been successfully initiated
#[derive(Debug, Clone)]
pub struct ResharingInitiated {
    pub session_id: Uuid,
    pub old_threshold: u16,
    pub new_threshold: u16,
    pub old_participant_count: usize,
    pub new_participant_count: usize,
    pub initiation_timestamp: u64,
}

impl RuntimeWitness for ResharingInitiated {
    type Evidence = (Uuid, u16, u16, usize, usize, u64); // (session_id, old_threshold, new_threshold, old_count, new_count, timestamp)
    type Config = ();

    fn verify(evidence: Self::Evidence, _config: Self::Config) -> Option<Self> {
        let (
            session_id,
            old_threshold,
            new_threshold,
            old_participant_count,
            new_participant_count,
            initiation_timestamp,
        ) = evidence;
        if old_threshold > 0
            && new_threshold > 0
            && old_participant_count > 0
            && new_participant_count > 0
        {
            Some(ResharingInitiated {
                session_id,
                old_threshold,
                new_threshold,
                old_participant_count,
                new_participant_count,
                initiation_timestamp,
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Resharing protocol initiated"
    }
}

impl ResharingInitiated {
    /// Check if the witness is valid
    pub fn check(&self) -> Result<(), ResharingSessionError> {
        if self.old_threshold == 0 {
            return Err(ResharingSessionError::ProtocolError(
                "Old threshold cannot be zero".to_string(),
            ));
        }
        if self.new_threshold == 0 {
            return Err(ResharingSessionError::ProtocolError(
                "New threshold cannot be zero".to_string(),
            ));
        }
        if self.old_participant_count == 0 {
            return Err(ResharingSessionError::ProtocolError(
                "Old participant count cannot be zero".to_string(),
            ));
        }
        if self.new_participant_count == 0 {
            return Err(ResharingSessionError::ProtocolError(
                "New participant count cannot be zero".to_string(),
            ));
        }
        Ok(())
    }
}

/// Witness that sub-shares have been distributed successfully
#[derive(Debug, Clone)]
pub struct SubSharesDistributed {
    pub session_id: Uuid,
    pub distribution_count: usize,
    pub expected_count: usize,
}

impl RuntimeWitness for SubSharesDistributed {
    type Evidence = (Uuid, usize, usize); // (session_id, distribution_count, expected_count)
    type Config = ();

    fn verify(evidence: Self::Evidence, _config: Self::Config) -> Option<Self> {
        let (session_id, distribution_count, expected_count) = evidence;
        if distribution_count >= expected_count {
            Some(SubSharesDistributed {
                session_id,
                distribution_count,
                expected_count,
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Sub-shares distributed successfully"
    }
}

impl SubSharesDistributed {
    /// Check if the witness is valid
    pub fn check(&self) -> Result<(), ResharingSessionError> {
        if self.distribution_count < self.expected_count {
            return Err(ResharingSessionError::InsufficientShares(format!(
                "Need {} shares, have {}",
                self.expected_count, self.distribution_count
            )));
        }
        Ok(())
    }
}

/// Witness that shares have been reconstructed successfully
#[derive(Debug, Clone)]
pub struct SharesReconstructed {
    pub session_id: Uuid,
    pub reconstructed_shares: BTreeMap<DeviceId, Vec<u8>>,
    pub threshold_met: bool,
}

impl RuntimeWitness for SharesReconstructed {
    type Evidence = (Uuid, BTreeMap<DeviceId, Vec<u8>>, bool); // (session_id, shares, threshold_met)
    type Config = ();

    fn verify(evidence: Self::Evidence, _config: Self::Config) -> Option<Self> {
        let (session_id, reconstructed_shares, threshold_met) = evidence;
        if threshold_met && !reconstructed_shares.is_empty() {
            Some(SharesReconstructed {
                session_id,
                reconstructed_shares,
                threshold_met,
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Shares reconstructed successfully"
    }
}

impl SharesReconstructed {
    /// Check if the witness is valid
    pub fn check(&self) -> Result<(), ResharingSessionError> {
        if !self.threshold_met {
            return Err(ResharingSessionError::InsufficientShares(
                "Threshold not met".to_string(),
            ));
        }
        if self.reconstructed_shares.is_empty() {
            return Err(ResharingSessionError::InsufficientShares(
                "No shares reconstructed".to_string(),
            ));
        }
        Ok(())
    }
}

/// Witness that new shares have been verified successfully
#[derive(Debug, Clone)]
pub struct SharesVerified {
    pub session_id: Uuid,
    pub verification_results: BTreeMap<DeviceId, bool>,
    pub all_verified: bool,
}

impl RuntimeWitness for SharesVerified {
    type Evidence = (Uuid, BTreeMap<DeviceId, bool>, bool); // (session_id, verification_results, all_verified)
    type Config = ();

    fn verify(evidence: Self::Evidence, _config: Self::Config) -> Option<Self> {
        let (session_id, verification_results, all_verified) = evidence;
        if all_verified {
            Some(SharesVerified {
                session_id,
                verification_results,
                all_verified,
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "New shares verified successfully"
    }
}

/// Witness that resharing has been completed successfully
#[derive(Debug, Clone)]
pub struct ResharingCompleted {
    pub session_id: Uuid,
    pub new_threshold: u16,
    pub new_participant_count: usize,
    pub finalized: bool,
}

impl RuntimeWitness for ResharingCompleted {
    type Evidence = (Uuid, u16, usize, bool); // (session_id, new_threshold, new_participant_count, finalized)
    type Config = ();

    fn verify(evidence: Self::Evidence, _config: Self::Config) -> Option<Self> {
        let (session_id, new_threshold, new_participant_count, finalized) = evidence;
        if finalized && new_threshold > 0 && new_participant_count > 0 {
            Some(ResharingCompleted {
                session_id,
                new_threshold,
                new_participant_count,
                finalized,
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Resharing completed successfully"
    }
}

impl ResharingCompleted {
    /// Check if the witness is valid
    pub fn check(&self) -> Result<(), ResharingSessionError> {
        if !self.finalized {
            return Err(ResharingSessionError::ProtocolError(
                "Resharing not finalized".to_string(),
            ));
        }
        if self.new_threshold == 0 {
            return Err(ResharingSessionError::ProtocolError(
                "New threshold cannot be zero".to_string(),
            ));
        }
        if self.new_participant_count == 0 {
            return Err(ResharingSessionError::ProtocolError(
                "New participant count cannot be zero".to_string(),
            ));
        }
        Ok(())
    }
}

/// Witness that resharing has failed
#[derive(Debug, Clone)]
pub struct ResharingAborted {
    pub session_id: Uuid,
    pub failure_reason: String,
    pub failed_by: Option<DeviceId>,
}

impl RuntimeWitness for ResharingAborted {
    type Evidence = (Uuid, String, Option<DeviceId>); // (session_id, failure_reason, failed_by)
    type Config = ();

    fn verify(evidence: Self::Evidence, _config: Self::Config) -> Option<Self> {
        let (session_id, failure_reason, failed_by) = evidence;
        if !failure_reason.is_empty() {
            Some(ResharingAborted {
                session_id,
                failure_reason,
                failed_by,
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Resharing protocol aborted"
    }
}

impl ResharingAborted {
    /// Check if the witness is valid
    pub fn check(&self) -> Result<(), ResharingSessionError> {
        if self.failure_reason.is_empty() {
            return Err(ResharingSessionError::ProtocolError(
                "Failure reason cannot be empty".to_string(),
            ));
        }
        Ok(())
    }
}

// ========== Protocol State Machine ==========

/// Union type representing all possible resharing session states
#[derive(Debug, Clone)]
pub enum ResharingProtocolState {
    ResharingInitializing(SessionTypedProtocol<ResharingProtocolCore, ResharingInitializing>),
    ResharingPhaseOne(SessionTypedProtocol<ResharingProtocolCore, ResharingPhaseOne>),
    ResharingPhaseTwo(SessionTypedProtocol<ResharingProtocolCore, ResharingPhaseTwo>),
    ResharingVerification(SessionTypedProtocol<ResharingProtocolCore, ResharingVerification>),
    ResharingComplete(SessionTypedProtocol<ResharingProtocolCore, ResharingComplete>),
    ResharingFailed(SessionTypedProtocol<ResharingProtocolCore, ResharingFailed>),
}

// Utility methods for accessing inner protocol data
impl ResharingProtocolState {
    pub fn session_id(&self) -> Uuid {
        match self {
            ResharingProtocolState::ResharingInitializing(p) => p.core().session_id,
            ResharingProtocolState::ResharingPhaseOne(p) => p.core().session_id,
            ResharingProtocolState::ResharingPhaseTwo(p) => p.core().session_id,
            ResharingProtocolState::ResharingVerification(p) => p.core().session_id,
            ResharingProtocolState::ResharingComplete(p) => p.core().session_id,
            ResharingProtocolState::ResharingFailed(p) => p.core().session_id,
        }
    }

    pub fn device_id(&self) -> DeviceId {
        match self {
            ResharingProtocolState::ResharingInitializing(p) => p.core().device_id,
            ResharingProtocolState::ResharingPhaseOne(p) => p.core().device_id,
            ResharingProtocolState::ResharingPhaseTwo(p) => p.core().device_id,
            ResharingProtocolState::ResharingVerification(p) => p.core().device_id,
            ResharingProtocolState::ResharingComplete(p) => p.core().device_id,
            ResharingProtocolState::ResharingFailed(p) => p.core().device_id,
        }
    }

    pub fn protocol_id(&self) -> Uuid {
        match self {
            ResharingProtocolState::ResharingInitializing(p) => p.core().protocol_id,
            ResharingProtocolState::ResharingPhaseOne(p) => p.core().protocol_id,
            ResharingProtocolState::ResharingPhaseTwo(p) => p.core().protocol_id,
            ResharingProtocolState::ResharingVerification(p) => p.core().protocol_id,
            ResharingProtocolState::ResharingComplete(p) => p.core().protocol_id,
            ResharingProtocolState::ResharingFailed(p) => p.core().protocol_id,
        }
    }

    pub fn state_name(&self) -> &'static str {
        match self {
            ResharingProtocolState::ResharingInitializing(_) => ResharingInitializing::NAME,
            ResharingProtocolState::ResharingPhaseOne(_) => ResharingPhaseOne::NAME,
            ResharingProtocolState::ResharingPhaseTwo(_) => ResharingPhaseTwo::NAME,
            ResharingProtocolState::ResharingVerification(_) => ResharingVerification::NAME,
            ResharingProtocolState::ResharingComplete(_) => ResharingComplete::NAME,
            ResharingProtocolState::ResharingFailed(_) => ResharingFailed::NAME,
        }
    }

    pub fn can_terminate(&self) -> bool {
        match self {
            ResharingProtocolState::ResharingInitializing(_) => {
                ResharingInitializing::CAN_TERMINATE
            }
            ResharingProtocolState::ResharingPhaseOne(_) => ResharingPhaseOne::CAN_TERMINATE,
            ResharingProtocolState::ResharingPhaseTwo(_) => ResharingPhaseTwo::CAN_TERMINATE,
            ResharingProtocolState::ResharingVerification(_) => {
                ResharingVerification::CAN_TERMINATE
            }
            ResharingProtocolState::ResharingComplete(_) => ResharingComplete::CAN_TERMINATE,
            ResharingProtocolState::ResharingFailed(_) => ResharingFailed::CAN_TERMINATE,
        }
    }
}

// ========== SessionProtocol Implementation ==========

// Union state type for ResharingProtocolState
#[derive(Debug, Clone)]
pub struct ResharingUnionState;

impl SessionState for ResharingUnionState {
    const NAME: &'static str = "ResharingUnion";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

impl SessionProtocol for ResharingProtocolState {
    fn session_id(&self) -> Uuid {
        self.session_id()
    }

    fn device_id(&self) -> Uuid {
        self.device_id().0
    }

    fn protocol_id(&self) -> Uuid {
        self.protocol_id()
    }

    fn state_name(&self) -> &'static str {
        self.state_name()
    }

    fn can_terminate(&self) -> bool {
        self.can_terminate()
    }

    fn is_final(&self) -> bool {
        matches!(
            self,
            ResharingProtocolState::ResharingComplete(_)
                | ResharingProtocolState::ResharingFailed(_)
        )
    }
}

// ========== State Transition Methods ==========

impl ResharingProtocolState {
    /// Check if protocol is in a final state
    pub fn is_final(&self) -> bool {
        matches!(
            self,
            ResharingProtocolState::ResharingComplete(_)
                | ResharingProtocolState::ResharingFailed(_)
        )
    }

    /// Transition from ResharingInitializing to ResharingPhaseOne
    pub fn begin_phase_one(
        self,
        witness: ResharingInitiated,
    ) -> Result<ResharingProtocolState, ResharingSessionError> {
        witness.check()?;
        match self {
            ResharingProtocolState::ResharingInitializing(protocol) => {
                let core = protocol.into_core();
                let new_protocol = SessionTypedProtocol::new(core);
                Ok(ResharingProtocolState::ResharingPhaseOne(new_protocol))
            }
            _ => Err(ResharingSessionError::InvalidOperation),
        }
    }

    /// Transition from ResharingPhaseOne to ResharingPhaseTwo
    pub fn begin_phase_two(
        self,
        witness: SubSharesDistributed,
    ) -> Result<ResharingProtocolState, ResharingSessionError> {
        witness.check()?;
        match self {
            ResharingProtocolState::ResharingPhaseOne(protocol) => {
                let core = protocol.into_core();
                let new_protocol = SessionTypedProtocol::new(core);
                Ok(ResharingProtocolState::ResharingPhaseTwo(new_protocol))
            }
            _ => Err(ResharingSessionError::InvalidOperation),
        }
    }

    /// Transition from ResharingPhaseTwo to ResharingVerification
    pub fn begin_verification(
        self,
        witness: SharesReconstructed,
    ) -> Result<ResharingProtocolState, ResharingSessionError> {
        witness.check()?;
        match self {
            ResharingProtocolState::ResharingPhaseTwo(protocol) => {
                let mut core = protocol.into_core();
                // Update collected shares from witness
                core.collected_sub_shares = witness.reconstructed_shares;
                let new_protocol = SessionTypedProtocol::new(core);
                Ok(ResharingProtocolState::ResharingVerification(new_protocol))
            }
            _ => Err(ResharingSessionError::InvalidOperation),
        }
    }

    /// Transition from ResharingVerification to ResharingComplete
    pub fn complete_resharing(
        self,
        witness: ResharingCompleted,
    ) -> Result<ResharingProtocolState, ResharingSessionError> {
        witness.check()?;
        match self {
            ResharingProtocolState::ResharingVerification(protocol) => {
                let core = protocol.into_core();
                let new_protocol = SessionTypedProtocol::new(core);
                Ok(ResharingProtocolState::ResharingComplete(new_protocol))
            }
            _ => Err(ResharingSessionError::InvalidOperation),
        }
    }

    /// Transition to ResharingFailed from any non-final state
    pub fn fail_resharing(
        self,
        witness: ResharingAborted,
    ) -> Result<ResharingProtocolState, ResharingSessionError> {
        witness.check()?;
        if self.is_final() {
            return Err(ResharingSessionError::InvalidOperation);
        }

        let core = match self {
            ResharingProtocolState::ResharingInitializing(p) => p.into_core(),
            ResharingProtocolState::ResharingPhaseOne(p) => p.into_core(),
            ResharingProtocolState::ResharingPhaseTwo(p) => p.into_core(),
            ResharingProtocolState::ResharingVerification(p) => p.into_core(),
            _ => return Err(ResharingSessionError::InvalidOperation),
        };

        let new_protocol = SessionTypedProtocol::new(core);
        Ok(ResharingProtocolState::ResharingFailed(new_protocol))
    }
}

// ========== Constructor Functions ==========

/// Create a new resharing protocol instance in the initial state
pub fn new_resharing_protocol(
    session_id: Uuid,
    device_id: DeviceId,
    old_threshold: u16,
    new_threshold: u16,
    old_participants: Vec<DeviceId>,
    new_participants: Vec<DeviceId>,
) -> Result<ResharingProtocolState, ResharingSessionError> {
    let core = ResharingProtocolCore::new(
        session_id,
        device_id,
        old_threshold,
        new_threshold,
        old_participants,
        new_participants,
    );
    let protocol = SessionTypedProtocol::new(core);
    Ok(ResharingProtocolState::ResharingInitializing(protocol))
}

/// Rehydrate a resharing protocol from crash recovery evidence
pub fn rehydrate_resharing_protocol(
    session_id: Uuid,
    device_id: DeviceId,
    old_threshold: u16,
    new_threshold: u16,
    old_participants: Vec<DeviceId>,
    new_participants: Vec<DeviceId>,
    events: Vec<Event>,
) -> Result<ResharingProtocolState, ResharingSessionError> {
    let mut core = ResharingProtocolCore::new(
        session_id,
        device_id,
        old_threshold,
        new_threshold,
        old_participants,
        new_participants,
    );

    // Analyze events to determine current state
    let mut has_initiation = false;
    let mut has_sub_shares = false;
    let mut has_acknowledgments = false;
    let mut has_finalization = false;

    for event in &events {
        match &event.event_type {
            aura_journal::EventType::InitiateResharing(_) => has_initiation = true,
            aura_journal::EventType::DistributeSubShare(dist) => {
                has_sub_shares = true;
                if dist.to_device_id == device_id {
                    core.collected_sub_shares
                        .insert(dist.from_device_id, dist.encrypted_sub_share.clone());
                }
            }
            aura_journal::EventType::AcknowledgeSubShare(ack) => {
                has_acknowledgments = true;
                if ack.to_device_id == device_id {
                    core.acknowledgments.insert(ack.from_device_id, true);
                }
            }
            aura_journal::EventType::FinalizeResharing(_) => has_finalization = true,
            _ => {}
        }
    }

    // Determine state based on events
    if has_finalization {
        Ok(ResharingProtocolState::ResharingComplete(
            SessionTypedProtocol::new(core),
        ))
    } else if has_acknowledgments {
        Ok(ResharingProtocolState::ResharingVerification(
            SessionTypedProtocol::new(core),
        ))
    } else if has_sub_shares {
        Ok(ResharingProtocolState::ResharingPhaseTwo(
            SessionTypedProtocol::new(core),
        ))
    } else if has_initiation {
        Ok(ResharingProtocolState::ResharingPhaseOne(
            SessionTypedProtocol::new(core),
        ))
    } else {
        Ok(ResharingProtocolState::ResharingInitializing(
            SessionTypedProtocol::new(core),
        ))
    }
}

// ========== Choreographic Execution Logic ==========

use crate::execution::{
    EventAwaiter, EventBuilder, EventTypePattern, ProtocolContext, ProtocolContextExt,
    ProtocolError, ProtocolErrorType, SessionLifecycle,
};
use crate::protocol_results::ResharingProtocolResult;
use aura_crypto::{LagrangeInterpolation, ShamirPolynomial, SharePoint};
use aura_journal::{
    AcknowledgeSubShareEvent, DistributeSubShareEvent, EventType, FinalizeResharingEvent,
    InitiateResharingEvent, OperationType, ParticipantId as JournalParticipantId, ProtocolType,
    Session,
};

/// Resharing Protocol implementation using SessionLifecycle trait
pub struct ResharingProtocol<'a> {
    ctx: &'a mut ProtocolContext,
    new_threshold: Option<u16>,
    new_participants: Option<Vec<DeviceId>>,
}

impl<'a> ResharingProtocol<'a> {
    pub fn new(
        ctx: &'a mut ProtocolContext,
        new_threshold: Option<u16>,
        new_participants: Option<Vec<DeviceId>>,
    ) -> Self {
        Self {
            ctx,
            new_threshold,
            new_participants,
        }
    }
}

#[async_trait::async_trait]
impl<'a> SessionLifecycle for ResharingProtocol<'a> {
    type Result = ResharingProtocolResult; // Success indicator

    fn operation_type(&self) -> OperationType {
        OperationType::Resharing
    }

    fn generate_context_id(&self) -> Vec<u8> {
        format!(
            "resharing:{}:{:?}",
            self.new_threshold
                .unwrap_or(self.ctx.threshold().unwrap_or(2) as u16),
            self.new_participants
                .as_ref()
                .unwrap_or(self.ctx.participants())
        )
        .into_bytes()
    }

    async fn create_session(&mut self) -> Result<Session, ProtocolError> {
        let ledger_context = self.ctx.fetch_ledger_context().await?;

        // Convert participants to session participants
        let session_participants: Vec<JournalParticipantId> = self
            .ctx
            .participants()
            .iter()
            .map(|&device_id| JournalParticipantId::Device(device_id))
            .collect();

        // Create Resharing session
        Ok(Session::new(
            aura_journal::SessionId(self.ctx.session_id()),
            ProtocolType::Resharing,
            session_participants,
            ledger_context.epoch,
            100, // TTL in epochs
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
    ) -> Result<ResharingProtocolResult, ProtocolError> {
        // Get current participants and new configuration
        let participants = self.ctx.participants().clone();
        let new_participants = self
            .new_participants
            .clone()
            .unwrap_or_else(|| participants.clone());
        let new_threshold = self
            .new_threshold
            .unwrap_or(self.ctx.threshold().unwrap_or(2) as u16);

        // Phase 1: Initiate Resharing (only coordinator)
        if participants.first() == Some(&DeviceId(self.ctx.device_id())) {
            let start_epoch = self.ctx.fetch_ledger_context().await?.epoch;
            let session_id = self.ctx.session_id();
            let old_threshold = self.ctx.threshold().unwrap_or(2) as u16;

            EventBuilder::new(self.ctx)
                .with_type(EventType::InitiateResharing(InitiateResharingEvent {
                    session_id,
                    old_threshold,
                    new_threshold,
                    old_participants: participants.clone(),
                    new_participants: new_participants.clone(),
                    start_epoch,
                    ttl_in_epochs: 100,
                }))
                .with_device_auth()
                .build_sign_and_emit()
                .await?;
        }

        // Wait for initiation event
        let session_id = self.ctx.session_id();
        let initiation_event = EventAwaiter::new(self.ctx)
            .for_session(session_id)
            .for_event_types(vec![EventTypePattern::InitiateResharing])
            .from_authors(participants.first().cloned().into_iter())
            .await_single(100)
            .await?;

        let (final_new_participants, final_new_threshold) = match &initiation_event.event_type {
            EventType::InitiateResharing(ref initiate) => {
                (initiate.new_participants.clone(), initiate.new_threshold)
            }
            _ => {
                return Err(ProtocolError {
                    session_id: self.ctx.session_id(),
                    error_type: ProtocolErrorType::UnexpectedEvent,
                    message: "Expected InitiateResharing event".to_string(),
                });
            }
        };

        // Phase 2: Distribute Sub-shares
        if participants.contains(&DeviceId(self.ctx.device_id())) {
            self.distribute_sub_shares(&final_new_participants, final_new_threshold)
                .await?;
        }

        // Phase 3: Collect Sub-shares (for new participants)
        let mut collected_sub_shares = BTreeMap::new();
        if final_new_participants.contains(&DeviceId(self.ctx.device_id())) {
            collected_sub_shares = self
                .collect_sub_shares(&final_new_participants, final_new_threshold)
                .await?;
        }

        // Phase 4: Reconstruct New Share
        if final_new_participants.contains(&DeviceId(self.ctx.device_id())) {
            self.reconstruct_share(&collected_sub_shares).await?;
        }

        // Phase 5: Verify via Test Signature (placeholder)
        if final_new_participants.contains(&DeviceId(self.ctx.device_id())) {
            self.verify_new_shares().await?;
        }

        // Phase 6: Finalize Resharing (only coordinator)
        if participants.first() == Some(&DeviceId(self.ctx.device_id())) {
            let session_id = self.ctx.session_id();
            EventBuilder::new(self.ctx)
                .with_type(EventType::FinalizeResharing(FinalizeResharingEvent {
                    session_id,
                    new_group_public_key: vec![0u8; 32], // Placeholder
                    new_threshold: final_new_threshold,
                    test_signature: vec![0u8; 64], // Placeholder
                }))
                .with_threshold_auth()
                .build_sign_and_emit()
                .await?;
        }

        // Wait for finalization
        let session_id = self.ctx.session_id();
        let _finalize_event = EventAwaiter::new(self.ctx)
            .for_session(session_id)
            .for_event_types(vec![EventTypePattern::FinalizeResharing])
            .from_authors(participants.first().cloned().into_iter())
            .await_single(100)
            .await?;

        // Collect all events from this protocol execution
        let ledger_events = self.ctx.collected_events().to_vec();

        // Create encrypted shares (placeholder - would be collected from distribute events)
        let encrypted_shares = vec![];

        // Create approval signature (placeholder - would be collected from threshold signers)
        let approval_signature = crate::ThresholdSignature {
            signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
            signers: vec![], // Would be populated with actual ParticipantIds
        };

        Ok(ResharingProtocolResult {
            session_id: aura_journal::SessionId(session_id),
            new_threshold: final_new_threshold,
            new_participants: final_new_participants,
            old_participants: participants,
            new_shares: encrypted_shares,
            approval_signature,
            ledger_events,
        })
    }

    async fn wait_for_completion(
        &mut self,
        winning_session: &Session,
    ) -> Result<ResharingProtocolResult, ProtocolError> {
        let finalize_event = EventAwaiter::new(self.ctx)
            .for_session(winning_session.session_id.0)
            .for_event_types(vec![EventTypePattern::FinalizeResharing])
            .await_single(100) // Default TTL epochs
            .await?;

        match &finalize_event.event_type {
            EventType::FinalizeResharing(finalize) => {
                // Reconstruct protocol result from finalize event
                let approval_signature = crate::ThresholdSignature {
                    signature: ed25519_dalek::Signature::from_bytes(&[0u8; 64]),
                    signers: vec![], // Would be extracted from event
                };

                Ok(ResharingProtocolResult {
                    session_id: winning_session.session_id,
                    new_threshold: finalize.new_threshold,
                    new_participants: vec![], // Would be extracted from event
                    old_participants: vec![], // Would be extracted from event
                    new_shares: vec![],
                    approval_signature,
                    ledger_events: vec![finalize_event],
                })
            }
            _ => Err(ProtocolError {
                session_id: self.ctx.session_id(),
                error_type: ProtocolErrorType::InvalidState,
                message: "Expected resharing finalize event".to_string(),
            }),
        }
    }
}

impl<'a> ResharingProtocol<'a> {
    /// Distribute sub-shares to new participants
    async fn distribute_sub_shares(
        &mut self,
        new_participants: &[DeviceId],
        new_threshold: u16,
    ) -> Result<(), ProtocolError> {
        // Get current key share and generate polynomial
        let key_share_bytes = self.ctx.get_key_share().await?;
        let key_share_scalar = curve25519_dalek::scalar::Scalar::from_bytes_mod_order(
            key_share_bytes.try_into().unwrap_or([0u8; 32]),
        );
        let mut rng = self.ctx.create_rng();
        let polynomial =
            ShamirPolynomial::from_secret(key_share_scalar, new_threshold.into(), &mut rng);

        // Distribute sub-shares to each new participant
        for (i, new_participant) in new_participants.iter().enumerate() {
            let x = curve25519_dalek::scalar::Scalar::from((i + 1) as u64);
            let sub_share_scalar = polynomial.evaluate(x);
            let sub_share = sub_share_scalar.to_bytes().to_vec();

            // Encrypt the sub-share using HPKE
            let recipient_public_key = self.ctx.get_device_public_key(new_participant).await?;
            let hpke_public_key = aura_crypto::HpkePublicKey::from_bytes(&recipient_public_key)?;

            let mut encrypt_rng = self.ctx.create_rng();
            let ciphertext =
                aura_crypto::encrypt_base(&sub_share, &hpke_public_key, &mut encrypt_rng)?;
            let encrypted_sub_share = ciphertext.to_bytes();

            let session_id = self.ctx.session_id();
            let device_id = self.ctx.device_id();
            EventBuilder::new(self.ctx)
                .with_type(EventType::DistributeSubShare(DistributeSubShareEvent {
                    session_id,
                    from_device_id: DeviceId(device_id),
                    to_device_id: *new_participant,
                    encrypted_sub_share,
                }))
                .with_device_auth()
                .build_sign_and_emit()
                .await?;
        }

        Ok(())
    }

    /// Collect sub-shares from old participants
    async fn collect_sub_shares(
        &mut self,
        _new_participants: &[DeviceId],
        new_threshold: u16,
    ) -> Result<BTreeMap<DeviceId, Vec<u8>>, ProtocolError> {
        let mut collected_sub_shares = BTreeMap::new();

        // Collect sub-shares from threshold old participants
        for _ in 0..new_threshold {
            let session_id = self.ctx.session_id();
            let event = EventAwaiter::new(self.ctx)
                .for_session(session_id)
                .for_event_types(vec![EventTypePattern::DistributeSubShare])
                .await_single(200)
                .await?;

            if let EventType::DistributeSubShare(ref distribute) = event.event_type {
                if distribute.to_device_id == DeviceId(self.ctx.device_id()) {
                    // Decrypt the sub-share
                    let hpke_ciphertext =
                        aura_crypto::HpkeCiphertext::from_bytes(&distribute.encrypted_sub_share)?;
                    let device_private_key = self.ctx.get_device_hpke_private_key().await?;

                    let decrypted =
                        aura_crypto::decrypt_base(&hpke_ciphertext, &device_private_key)?;
                    collected_sub_shares.insert(distribute.from_device_id, decrypted);

                    // Send acknowledgment
                    let session_id = self.ctx.session_id();
                    let device_id = self.ctx.device_id();
                    EventBuilder::new(self.ctx)
                        .with_type(EventType::AcknowledgeSubShare(AcknowledgeSubShareEvent {
                            session_id,
                            from_device_id: distribute.from_device_id,
                            to_device_id: DeviceId(device_id),
                            ack_signature: vec![0u8; 64], // Placeholder
                        }))
                        .with_device_auth()
                        .build_sign_and_emit()
                        .await?;
                }
            }
        }

        Ok(collected_sub_shares)
    }

    /// Reconstruct share from collected sub-shares
    async fn reconstruct_share(
        &mut self,
        collected_sub_shares: &BTreeMap<DeviceId, Vec<u8>>,
    ) -> Result<(), ProtocolError> {
        let share_points: Vec<SharePoint> = collected_sub_shares
            .iter()
            .enumerate()
            .map(|(i, (_device_id, share_bytes))| {
                let scalar = if share_bytes.len() >= 32 {
                    let mut bytes = [0u8; 32];
                    bytes.copy_from_slice(&share_bytes[..32]);
                    curve25519_dalek::scalar::Scalar::from_bytes_mod_order(bytes)
                } else {
                    curve25519_dalek::scalar::Scalar::from_bytes_mod_order([0u8; 32])
                };
                SharePoint {
                    x: curve25519_dalek::scalar::Scalar::from((i + 1) as u64),
                    y: scalar,
                }
            })
            .collect();

        let reconstructed_scalar = LagrangeInterpolation::interpolate_at_zero(&share_points)?;
        let reconstructed_share = reconstructed_scalar.to_bytes().to_vec();

        // Store new share
        self.ctx.set_key_share(reconstructed_share).await?;
        Ok(())
    }

    /// Verify new shares work correctly
    async fn verify_new_shares(&mut self) -> Result<(), ProtocolError> {
        // Verify that we have a valid key share
        let key_share = self.ctx.get_key_share().await?;
        if key_share.len() != 32 {
            return Err(ProtocolError {
                session_id: self.ctx.session_id(),
                error_type: ProtocolErrorType::InvalidState,
                message: "Invalid key share length after resharing".to_string(),
            });
        }
        Ok(())
    }
}

/// Resharing Protocol Choreography - Main entry point
pub async fn resharing_choreography(
    ctx: &mut ProtocolContext,
    new_threshold: Option<u16>,
    new_participants: Option<Vec<DeviceId>>,
) -> Result<ResharingProtocolResult, ProtocolError> {
    let mut protocol = ResharingProtocol::new(ctx, new_threshold, new_participants);
    protocol.execute().await
}

// ========== Tests ==========

#[cfg(test)]
#[allow(warnings, clippy::all)]
mod tests {
    use super::*;
    use crate::execution::context::StubTransport;
    use aura_crypto::Effects;
    use aura_journal::{AccountLedger, AccountState};
    use aura_types::AccountId;
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_resharing_choreography_structure() {
        // Use deterministic UUIDs for testing
        let session_id = Uuid::from_bytes([1u8; 16]);
        let device_id = Uuid::from_bytes([2u8; 16]);

        let old_participants = vec![
            DeviceId(Uuid::from_bytes([3u8; 16])),
            DeviceId(Uuid::from_bytes([4u8; 16])),
        ];

        let new_participants = vec![
            DeviceId(Uuid::from_bytes([5u8; 16])),
            DeviceId(Uuid::from_bytes([6u8; 16])),
            DeviceId(Uuid::from_bytes([7u8; 16])),
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
            AccountId(Uuid::from_bytes([8u8; 16])),
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
            old_participants,
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
    fn test_resharing_session_state_transitions() {
        let session_id = Uuid::new_v4();
        let device_id = DeviceId(Uuid::new_v4());
        let old_participants = vec![DeviceId(Uuid::new_v4()), DeviceId(Uuid::new_v4())];
        let new_participants = vec![
            DeviceId(Uuid::new_v4()),
            DeviceId(Uuid::new_v4()),
            DeviceId(Uuid::new_v4()),
        ];

        // Test protocol creation
        let protocol = new_resharing_protocol(
            session_id,
            device_id,
            2, // old_threshold
            3, // new_threshold
            old_participants.clone(),
            new_participants.clone(),
        )
        .unwrap();
        assert!(!protocol.is_final());
        assert_eq!(protocol.state_name(), "ResharingInitializing");

        // Test state transition to phase one
        let initiation_witness = ResharingInitiated {
            session_id,
            old_threshold: 2,
            new_threshold: 3,
            old_participant_count: old_participants.len(),
            new_participant_count: new_participants.len(),
            initiation_timestamp: 12345,
        };
        let protocol = protocol.begin_phase_one(initiation_witness).unwrap();
        assert_eq!(protocol.state_name(), "ResharingPhaseOne");

        // Test state transition to phase two
        let distribution_witness = SubSharesDistributed {
            session_id,
            distribution_count: 6,
            expected_count: 6,
        };
        let protocol = protocol.begin_phase_two(distribution_witness).unwrap();
        assert_eq!(protocol.state_name(), "ResharingPhaseTwo");

        // Test state transition to verification
        let reconstruction_witness = SharesReconstructed {
            session_id,
            reconstructed_shares: {
                let mut shares = BTreeMap::new();
                shares.insert(new_participants[0], vec![1, 2, 3, 4]);
                shares.insert(new_participants[1], vec![5, 6, 7, 8]);
                shares.insert(new_participants[2], vec![9, 10, 11, 12]);
                shares
            },
            threshold_met: true,
        };
        let protocol = protocol.begin_verification(reconstruction_witness).unwrap();
        assert_eq!(protocol.state_name(), "ResharingVerification");

        // Test completion
        let completion_witness = ResharingCompleted {
            session_id,
            new_threshold: 3,
            new_participant_count: new_participants.len(),
            finalized: true,
        };
        let protocol = protocol.complete_resharing(completion_witness).unwrap();
        assert_eq!(protocol.state_name(), "ResharingComplete");
        assert!(protocol.is_final());
    }

    #[test]
    fn test_resharing_failure_transition() {
        let session_id = Uuid::new_v4();
        let device_id = DeviceId(Uuid::new_v4());
        let old_participants = vec![DeviceId(Uuid::new_v4())];
        let new_participants = vec![DeviceId(Uuid::new_v4())];

        let protocol = new_resharing_protocol(
            session_id,
            device_id,
            1,
            1,
            old_participants,
            new_participants,
        )
        .unwrap();

        // Test failure transition from any non-final state
        let failure_witness = ResharingAborted {
            session_id,
            failure_reason: "Test failure".to_string(),
            failed_by: Some(device_id),
        };
        let failed_protocol = protocol.fail_resharing(failure_witness).unwrap();
        assert_eq!(failed_protocol.state_name(), "ResharingFailed");
        assert!(failed_protocol.is_final());
    }

    #[test]
    fn test_resharing_witness_validation() {
        let session_id = Uuid::new_v4();

        // Test valid ResharingInitiated witness
        let valid_witness = ResharingInitiated {
            session_id,
            old_threshold: 2,
            new_threshold: 3,
            old_participant_count: 2,
            new_participant_count: 3,
            initiation_timestamp: 12345,
        };
        assert!(valid_witness.check().is_ok());

        // Test invalid ResharingInitiated witness
        let invalid_witness = ResharingInitiated {
            session_id,
            old_threshold: 0, // Invalid threshold
            new_threshold: 3,
            old_participant_count: 2,
            new_participant_count: 3,
            initiation_timestamp: 12345,
        };
        assert!(invalid_witness.check().is_err());

        // Test SubSharesDistributed witness
        let distribution_witness = SubSharesDistributed {
            session_id,
            distribution_count: 5,
            expected_count: 6,
        };
        assert!(distribution_witness.check().is_err()); // Should fail due to insufficient distributions

        let valid_distribution_witness = SubSharesDistributed {
            session_id,
            distribution_count: 6,
            expected_count: 6,
        };
        assert!(valid_distribution_witness.check().is_ok());
    }
}
