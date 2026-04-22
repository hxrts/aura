//! Runtime Services
//!
//! Service components per Layer-6 spec.
//!
//! All service managers implement the `RuntimeService` trait for unified
//! lifecycle management. See `traits.rs` for the trait definition.

pub mod auth_manager;
pub mod authority_manager;
pub mod authority_state;
pub mod bootstrap_broker;
pub mod ceremony_runner;
pub mod ceremony_tracker;
mod config_profiles;
pub mod context_manager;
pub mod cover_traffic_generator;
pub mod flow_budget_manager;
pub mod hold_manager;
pub mod invariant;
pub mod invitation_manager;
pub mod key_resolution;
pub mod lan_discovery;
pub mod lan_listener_service;
pub mod lan_transport;
pub mod local_health_observer;
pub mod logical_clock_manager;
pub mod maintenance_service;
pub mod move_manager;
pub mod path_manager;
pub mod reactive_pipeline_service;
pub mod receipt_manager;
pub mod reconfiguration_manager;
pub mod recovery_manager;
pub mod rendezvous_manager;
pub mod selection_manager;
mod service_actor;
pub mod service_registry;
pub mod session_manager;
pub mod social_manager;
pub mod state;
pub mod sync_manager;
pub mod threshold_signing;
pub mod traits;

pub(crate) use auth_manager::AuthManager;
pub use authority_manager::AuthorityManager;
pub use authority_state::AuthorityStatus;
pub use ceremony_tracker::CeremonyTracker;
pub use context_manager::ContextManager;
#[allow(unused_imports)]
pub use cover_traffic_generator::{
    CoverTrafficGeneratorConfig, CoverTrafficGeneratorService as CoverTrafficGenerator,
    CoverTrafficPlan,
};
pub use flow_budget_manager::FlowBudgetManager;
pub use hold_manager::{
    AccountabilityWitness, AccountabilityWitnessKind, HoldBudgetSnapshot, HoldDepositOutcome,
    HoldGcOutcome, HoldLocalIndexEntry, HoldManager, HoldManagerConfig, HoldProjection,
    HoldRetrievalOutcome, HoldRetrievalStatus, HoldSelectionPlan, HoldSyncBatch,
    QueuedAccountabilityReply, QueuedSyncRetrieval, VerifiedServiceWitness, VerifierRole,
};
pub(crate) use invitation_manager::InvitationManager;
pub use key_resolution::{
    KeyResolutionError, TrustedKeyDomain, TrustedKeyResolutionService, TrustedKeyStatus,
    TrustedPublicKey,
};
pub use lan_listener_service::LanTransportListenerService;
pub use lan_transport::LanTransportService;
pub use local_health_observer::{
    LocalHealthObserverConfig, LocalHealthObserverService as LocalHealthObserver,
};
pub use logical_clock_manager::LogicalClockManager;
pub use maintenance_service::RuntimeMaintenanceService;
#[allow(unused_imports)]
pub use move_manager::{MoveDeliveryPlan, MoveManager, MoveManagerConfig, MoveProjection};
#[allow(unused_imports)] // Public runtime service API.
pub use path_manager::{
    AnonymousPathManager, AnonymousPathManagerConfig, AnonymousPathManagerError,
    AnonymousPathProjection,
};
pub use reactive_pipeline_service::ReactivePipelineService;
pub use receipt_manager::{ReceiptManager, ReceiptManagerConfig};
#[allow(unused_imports)]
pub use reconfiguration_manager::{
    ActiveSessionDelegationError, ReconfigurationManager, ReconfigurationManagerError,
    SessionDelegationOutcome, SessionDelegationTransfer,
};
pub(crate) use recovery_manager::RecoveryManager;
pub use rendezvous_manager::{RendezvousManager, RendezvousManagerConfig};
#[allow(unused_imports)]
pub use selection_manager::{
    SelectionManagerConfig, SelectionManagerError, SelectionManagerService as SelectionManager,
};
pub use service_registry::ServiceRegistry;
pub(crate) use session_manager::SessionManager;
pub use social_manager::{SocialManager, SocialManagerConfig, SocialManagerState};
pub use sync_manager::{SyncManagerConfig, SyncManagerState, SyncServiceManager};
pub use threshold_signing::ThresholdSigningService;
pub use traits::{
    RuntimeService, RuntimeServiceContext, ServiceError, ServiceErrorKind, ServiceHealth,
};
