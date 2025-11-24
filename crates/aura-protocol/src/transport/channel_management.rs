//! Channel Management Choreographic Protocols
//!
//! Layer 4: Multi-party channel coordination using choreographic protocols.
//! YES choreography - complex setup/teardown coordination with multiple participants.
//! Target: <250 lines, focused on choreographic channel lifecycle.

use super::{ChoreographicConfig, ChoreographicError, ChoreographicResult};
use aura_core::effects::PhysicalTimeEffects;
use aura_core::{identifiers::DeviceId, ContextId};
use aura_effects::time::PhysicalTimeHandler;
use aura_macros::choreography;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// Channel establishment coordinator using choreographic protocols
#[derive(Clone)]
pub struct ChannelEstablishmentCoordinator {
    device_id: DeviceId,
    config: ChoreographicConfig,
    establishing_channels: HashMap<String, ChannelEstablishmentState>,
    time: Arc<dyn PhysicalTimeEffects>,
}

/// Channel teardown coordinator using choreographic protocols
#[derive(Clone)]
pub struct ChannelTeardownCoordinator {
    device_id: DeviceId,
    config: ChoreographicConfig,
    tearing_down_channels: HashMap<String, ChannelTeardownState>,
    time: Arc<dyn PhysicalTimeEffects>,
}

impl std::fmt::Debug for ChannelEstablishmentCoordinator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChannelEstablishmentCoordinator")
            .field("device_id", &self.device_id)
            .field("config", &self.config)
            .field("establishing_channels", &self.establishing_channels)
            .finish()
    }
}

impl std::fmt::Debug for ChannelTeardownCoordinator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChannelTeardownCoordinator")
            .field("device_id", &self.device_id)
            .field("config", &self.config)
            .field("tearing_down_channels", &self.tearing_down_channels)
            .finish()
    }
}

/// Channel establishment state tracking
#[derive(Debug, Clone)]
struct ChannelEstablishmentState {
    channel_id: String,
    participants: Vec<DeviceId>,
    phase: EstablishmentPhase,
    started_at: SystemTime,
    confirmations: HashMap<DeviceId, ChannelConfirmation>,
}

/// Channel teardown state tracking
#[derive(Debug, Clone)]
struct ChannelTeardownState {
    channel_id: String,
    participants: Vec<DeviceId>,
    phase: TeardownPhase,
    started_at: SystemTime,
    acknowledgments: HashMap<DeviceId, TeardownAcknowledgment>,
}

/// Establishment phase enumeration
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EstablishmentPhase {
    Initiating,
    GatheringConfirmations,
    ResourceAllocation,
    Finalizing,
    Established,
    Failed(String),
}

/// Teardown phase enumeration
#[derive(Debug, Clone, PartialEq, Eq)]
enum TeardownPhase {
    Initiating,
    GatheringAcknowledgments,
    ResourceCleanup,
    Finalizing,
    TornDown,
    Failed(String),
}

/// Channel establishment request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelEstablishmentRequest {
    pub channel_id: String,
    pub coordinator_id: DeviceId,
    pub participants: Vec<DeviceId>,
    pub channel_type: ChannelType,
    pub context_id: ContextId,
    pub resource_requirements: ResourceRequirements,
}

/// Channel confirmation response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelConfirmation {
    pub channel_id: String,
    pub participant_id: DeviceId,
    pub confirmation_result: ConfirmationResult,
    pub allocated_resources: AllocatedResources,
    pub timestamp: SystemTime,
}

/// Channel finalization message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelFinalization {
    pub channel_id: String,
    pub coordinator_id: DeviceId,
    pub finalization_result: FinalizationResult,
    pub channel_metadata: ChannelMetadata,
}

/// Channel teardown request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelTeardownRequest {
    pub channel_id: String,
    pub initiator_id: DeviceId,
    pub teardown_reason: TeardownReason,
    pub graceful: bool,
    pub cleanup_deadline: Option<SystemTime>,
}

/// Teardown acknowledgment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeardownAcknowledgment {
    pub channel_id: String,
    pub participant_id: DeviceId,
    pub acknowledgment_result: AcknowledgmentResult,
    pub cleanup_status: CleanupStatus,
    pub timestamp: SystemTime,
}

/// Channel type enumeration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChannelType {
    SecureMessaging,
    FileTransfer,
    StreamingData,
    Control,
}

/// Resource requirements
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceRequirements {
    pub bandwidth_mbps: u32,
    pub storage_mb: u32,
    pub cpu_cores: u8,
    pub memory_mb: u32,
}

/// Allocated resources
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllocatedResources {
    pub bandwidth_allocated: u32,
    pub storage_allocated: u32,
    pub cpu_allocated: u8,
    pub memory_allocated: u32,
}

/// Channel metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelMetadata {
    pub established_at: SystemTime,
    pub participants: Vec<DeviceId>,
    pub channel_type: ChannelType,
    pub encryption_enabled: bool,
}

/// Confirmation result enumeration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConfirmationResult {
    Confirmed,
    InsufficientResources { missing: ResourceRequirements },
    CapabilityDenied { required: Vec<String> },
    Rejected { reason: String },
}

/// Finalization result enumeration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FinalizationResult {
    Success,
    PartialFailure { failed_participants: Vec<DeviceId> },
    Failed { reason: String },
}

/// Teardown reason enumeration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TeardownReason {
    NormalShutdown,
    ResourceExhaustion,
    SecurityBreach,
    NetworkFailure,
    AdminShutdown,
}

/// Acknowledgment result enumeration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AcknowledgmentResult {
    Acknowledged,
    CleanupInProgress,
    CleanupFailed { reason: String },
}

/// Cleanup status enumeration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CleanupStatus {
    Complete,
    InProgress,
    Failed,
}

impl ChannelEstablishmentCoordinator {
    /// Create new channel establishment coordinator
    pub fn new(device_id: DeviceId, config: ChoreographicConfig) -> Self {
        Self::with_time(device_id, config, Arc::new(PhysicalTimeHandler::new()))
    }

    /// Create coordinator with explicit time provider
    pub fn with_time(
        device_id: DeviceId,
        config: ChoreographicConfig,
        time: Arc<dyn PhysicalTimeEffects>,
    ) -> Self {
        Self {
            device_id,
            config,
            establishing_channels: HashMap::new(),
            time,
        }
    }

    fn now(&self) -> SystemTime {
        let ms = futures::executor::block_on(async {
            self.time
                .physical_time()
                .await
                .map(|p| p.ts_ms)
                .unwrap_or_default()
        });
        SystemTime::UNIX_EPOCH + Duration::from_millis(ms)
    }

    /// Initiate channel establishment
    pub fn initiate_establishment(
        &mut self,
        participants: Vec<DeviceId>,
        channel_type: ChannelType,
        context_id: ContextId,
    ) -> ChoreographicResult<String> {
        if self.establishing_channels.len() >= self.config.max_concurrent_protocols {
            return Err(ChoreographicError::ExecutionFailed(
                "Maximum concurrent establishments exceeded".to_string(),
            ));
        }

        let channel_id = format!(
            "channel-{}-{}",
            &format!("{:?}", self.device_id)[..8],
            self.now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );

        let establishment_state = ChannelEstablishmentState {
            channel_id: channel_id.clone(),
            participants: participants.clone(),
            phase: EstablishmentPhase::Initiating,
            started_at: self.now(),
            confirmations: HashMap::new(),
        };

        self.establishing_channels
            .insert(channel_id.clone(), establishment_state);
        Ok(channel_id)
    }

    /// Process channel confirmation
    pub fn process_confirmation(
        &mut self,
        confirmation: ChannelConfirmation,
    ) -> ChoreographicResult<bool> {
        let establishment = self
            .establishing_channels
            .get_mut(&confirmation.channel_id)
            .ok_or_else(|| {
                ChoreographicError::ExecutionFailed(format!(
                    "Channel establishment not found: {}",
                    confirmation.channel_id
                ))
            })?;

        establishment
            .confirmations
            .insert(confirmation.participant_id, confirmation);

        // Check if we have all confirmations
        let all_confirmed = establishment.confirmations.len() == establishment.participants.len();

        if all_confirmed {
            establishment.phase = EstablishmentPhase::Finalizing;
        }

        Ok(all_confirmed)
    }

    /// Get establishment status
    pub fn get_establishment_status(&self, channel_id: &str) -> Option<&EstablishmentPhase> {
        self.establishing_channels.get(channel_id).map(|e| &e.phase)
    }
}

impl ChannelTeardownCoordinator {
    /// Create new channel teardown coordinator
    pub fn new(device_id: DeviceId, config: ChoreographicConfig) -> Self {
        Self::with_time(device_id, config, Arc::new(PhysicalTimeHandler::new()))
    }

    /// Create coordinator with explicit time provider
    pub fn with_time(
        device_id: DeviceId,
        config: ChoreographicConfig,
        time: Arc<dyn PhysicalTimeEffects>,
    ) -> Self {
        Self {
            device_id,
            config,
            tearing_down_channels: HashMap::new(),
            time,
        }
    }

    fn now(&self) -> SystemTime {
        let ms = futures::executor::block_on(async {
            self.time
                .physical_time()
                .await
                .map(|p| p.ts_ms)
                .unwrap_or_default()
        });
        SystemTime::UNIX_EPOCH + Duration::from_millis(ms)
    }

    /// Initiate channel teardown
    pub fn initiate_teardown(
        &mut self,
        channel_id: String,
        participants: Vec<DeviceId>,
        reason: TeardownReason,
    ) -> ChoreographicResult<()> {
        let teardown_state = ChannelTeardownState {
            channel_id: channel_id.clone(),
            participants,
            phase: TeardownPhase::Initiating,
            started_at: self.now(),
            acknowledgments: HashMap::new(),
        };

        self.tearing_down_channels
            .insert(channel_id, teardown_state);
        Ok(())
    }

    /// Process teardown acknowledgment
    pub fn process_acknowledgment(
        &mut self,
        acknowledgment: TeardownAcknowledgment,
    ) -> ChoreographicResult<bool> {
        let teardown = self
            .tearing_down_channels
            .get_mut(&acknowledgment.channel_id)
            .ok_or_else(|| {
                ChoreographicError::ExecutionFailed(format!(
                    "Channel teardown not found: {}",
                    acknowledgment.channel_id
                ))
            })?;

        teardown
            .acknowledgments
            .insert(acknowledgment.participant_id, acknowledgment);

        // Check if we have all acknowledgments
        let all_acknowledged = teardown.acknowledgments.len() == teardown.participants.len();

        if all_acknowledged {
            teardown.phase = TeardownPhase::TornDown;
        }

        Ok(all_acknowledged)
    }
}

// Choreographic Protocol Definitions
mod channel_establishment {
    use super::*;

    // Multi-phase channel establishment with resource allocation
    choreography! {
        #[namespace = "channel_establishment"]
        protocol ChannelEstablishmentProtocol {
            roles: Coordinator, Participant1, Participant2;

            // Phase 1: Request channel establishment
            Coordinator[guard_capability = "coordinate_channel_establishment",
                       flow_cost = 200,
                       journal_facts = "channel_establishment_initiated"]
            -> Participant1: ChannelEstablishmentRequest(ChannelEstablishmentRequest);

            Coordinator[guard_capability = "coordinate_channel_establishment",
                       flow_cost = 200]
            -> Participant2: ChannelEstablishmentRequest(ChannelEstablishmentRequest);

            // Phase 2: Participants confirm with resource allocation
            Participant1[guard_capability = "confirm_channel_participation",
                        flow_cost = 150,
                        journal_facts = "channel_participation_confirmed"]
            -> Coordinator: ChannelConfirmation(ChannelConfirmation);

            Participant2[guard_capability = "confirm_channel_participation",
                        flow_cost = 150,
                        journal_facts = "channel_participation_confirmed"]
            -> Coordinator: ChannelConfirmation(ChannelConfirmation);

            // Phase 3: Coordinator finalizes channel establishment
            Coordinator[guard_capability = "finalize_channel_establishment",
                       flow_cost = 100,
                       journal_facts = "channel_establishment_finalized"]
            -> Participant1: ChannelFinalization(ChannelFinalization);

            Coordinator[guard_capability = "finalize_channel_establishment",
                       flow_cost = 100,
                       journal_facts = "channel_establishment_finalized"]
            -> Participant2: ChannelFinalization(ChannelFinalization);
        }
    }
}

mod channel_teardown {
    use super::*;

    // Coordinated channel teardown with cleanup
    choreography! {
    #[namespace = "channel_teardown"]
    protocol ChannelTeardownProtocol {
        roles: Initiator, Participant1, Participant2;

        // Phase 1: Request channel teardown
        Initiator[guard_capability = "initiate_channel_teardown",
                 flow_cost = 120,
                 journal_facts = "channel_teardown_initiated"]
        -> Participant1: ChannelTeardownRequest(ChannelTeardownRequest);

        Initiator[guard_capability = "initiate_channel_teardown",
                 flow_cost = 120]
        -> Participant2: ChannelTeardownRequest(ChannelTeardownRequest);

        // Phase 2: Participants acknowledge and perform cleanup
        Participant1[guard_capability = "acknowledge_channel_teardown",
                    flow_cost = 100,
                    journal_facts = "channel_teardown_acknowledged"]
        -> Initiator: TeardownAcknowledgment(TeardownAcknowledgment);

        Participant2[guard_capability = "acknowledge_channel_teardown",
                    flow_cost = 100,
                    journal_facts = "channel_teardown_acknowledged"]
        -> Initiator: TeardownAcknowledgment(TeardownAcknowledgment);
    }
    }
}
