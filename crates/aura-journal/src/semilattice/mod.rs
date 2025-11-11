//! Journal CRDT implementations using harmonized architecture
//!
//! This module provides journal-specific CRDTs built on the harmonized
//! foundation from `aura-core`. All types implement the standard CRDT
//! traits and can participate in choreographic synchronization.

pub use account_state::{AccountState as ModernAccountState, GuardianRegistry, MaxCounter};
pub use concrete_types::{DeviceRegistry, EpochLog, IntentPool};
pub use invitations::{InvitationLedger, InvitationRecord, InvitationStatus};
pub use journal_map::JournalMap;
pub use meet_types::{
    CapabilitySet, ConsensusConstraint, DeviceCapability, ResourceQuota, SecurityPolicy, TimeWindow,
};
pub use op_log::{OpLog, OpLogSummary};

pub mod account_state;
pub mod concrete_types;
pub mod invitations;
pub mod journal_map;
pub mod meet_types;
pub mod op_log;

// Re-export foundation types for convenience
pub use aura_core::semilattice::{
    Bottom, ConsistencyProof, ConstraintMsg, ConstraintScope, CvState, DeltaMsg, JoinSemilattice,
    MeetSemiLattice, MeetStateMsg, MsgKind, MvState, OpWithCtx, StateMsg, Top,
};

// TODO: Re-export effect handlers when aura_protocol is available
// pub use aura_protocol::effects::semilattice::{CvHandler, CmHandler, DeltaHandler};

// TODO: Define type aliases when effect handlers are available
// /// Type alias for journal CRDT handler
// ///
// /// This provides a convenient type for handling `JournalMap` synchronization
// /// in choreographic protocols.
// pub type JournalHandler = CvHandler<JournalMap>;

// /// Type alias for intent pool handler
// pub type IntentPoolHandler = CvHandler<IntentPool>;

// /// Type alias for device registry handler
// pub type DeviceRegistryHandler = CvHandler<DeviceRegistry>;

// TODO: Implement factory when effect handlers are available
// /// Factory for creating journal CRDT handlers
// pub struct JournalCRDTFactory;

// impl JournalCRDTFactory {
//     /// Create a new journal handler
//     pub fn journal_handler() -> JournalHandler {
//         CvHandler::new()
//     }

//     /// Create a journal handler with initial state
//     pub fn journal_handler_with_state(journal: JournalMap) -> JournalHandler {
//         CvHandler::with_state(journal)
//     }

//     /// Create an intent pool handler
//     pub fn intent_pool_handler() -> IntentPoolHandler {
//         CvHandler::new()
//     }

//     /// Create a device registry handler
//     pub fn device_registry_handler() -> DeviceRegistryHandler {
//         CvHandler::new()
//     }
// }

/// Integration utilities for journal CRDTs
///
/// Note: These utilities will be enabled once the choreographic runtime
/// and CRDT protocols are fully implemented in aura-choreography.
pub mod integration {
    // Removed unused super::* import
    use aura_core::identifiers::{DeviceId, SessionId};

    // TODO: Uncomment when aura-choreography CRDT modules are implemented
    // use aura_protocol::choreography::semilattice::{execute_cv_sync, MultiCRDTCoordinator};
    // use aura_protocol::choreography::AuraHandlerAdapter;
    // use aura_protocol::choreography::types::ChoreographicRole;
    // use rumpsteak_choreography::ChoreographyError;

    /// Placeholder error type until choreography is ready
    pub type ChoreographyError = Box<dyn std::error::Error + Send + Sync>;

    /// Synchronize journal state across replicas
    ///
    /// This is a high-level utility that coordinates journal synchronization
    /// using the choreographic CRDT protocols.
    // TODO: Implement synchronization functions when handlers are available
    #[allow(unused_variables)]
    pub async fn sync_journal(
        // adapter: &mut AuraHandlerAdapter,
        // journal_handler: &mut JournalHandler,
        participants: Vec<DeviceId>,
        device_id: DeviceId,
        session_id: SessionId,
    ) -> Result<(), ChoreographyError> {
        // TODO: Implement once choreographic runtime is ready
        Ok(())
    }

    /// Synchronize intent pool across replicas
    #[allow(unused_variables)]
    pub async fn sync_intent_pool(
        // adapter: &mut AuraHandlerAdapter,
        // intent_handler: &mut IntentPoolHandler,
        participants: Vec<DeviceId>,
        device_id: DeviceId,
        session_id: SessionId,
    ) -> Result<(), ChoreographyError> {
        // TODO: Implement once choreographic runtime is ready
        Ok(())
    }

    /// Synchronize device registry across replicas
    #[allow(unused_variables)]
    pub async fn sync_device_registry(
        // adapter: &mut AuraHandlerAdapter,
        // registry_handler: &mut DeviceRegistryHandler,
        participants: Vec<DeviceId>,
        device_id: DeviceId,
        session_id: SessionId,
    ) -> Result<(), ChoreographyError> {
        // TODO: Implement once choreographic runtime is ready
        Ok(())
    }

    /// Comprehensive journal system synchronization
    ///
    /// Synchronizes all journal components (journal map, intent pool, device registry)
    /// in a coordinated fashion.
    #[allow(unused_variables)]
    pub async fn sync_journal_system(
        // adapter: &mut AuraHandlerAdapter,
        // journal_handler: &mut JournalHandler,
        // intent_handler: &mut IntentPoolHandler,
        // registry_handler: &mut DeviceRegistryHandler,
        participants: Vec<DeviceId>,
        device_id: DeviceId,
        session_id: SessionId,
    ) -> Result<(), ChoreographyError> {
        // TODO: Implement once choreographic runtime is ready
        Ok(())
    }
}
