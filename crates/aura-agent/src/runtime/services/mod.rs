//! Runtime Services
//!
//! Service components per Layer-6 spec.
//!
//! All service managers implement the `RuntimeService` trait for unified
//! lifecycle management. See `traits.rs` for the trait definition.

pub mod authority_manager;
pub mod auth_manager;
pub mod invitation_manager;
pub mod recovery_manager;
pub mod rendezvous_cache_manager;
pub mod traits;
pub mod ceremony_tracker;
pub mod context_manager;
pub mod flow_budget_manager;
pub mod lan_discovery;
pub mod logical_clock_manager;
pub mod ota_manager;
pub mod receipt_manager;
pub mod rendezvous_manager;
pub mod runtime_tasks;
pub mod session_manager;
pub mod state;
pub mod social_manager;
pub mod sync_manager;
pub mod threshold_signing;

pub use authority_manager::{
    AuthorityError, AuthorityManager, AuthorityState, AuthorityStatus, SharedAuthorityManager,
};
pub(crate) use auth_manager::AuthManager;
pub(crate) use invitation_manager::InvitationManager;
pub(crate) use recovery_manager::RecoveryManager;
pub(crate) use rendezvous_cache_manager::RendezvousCacheManager;
pub use ceremony_tracker::CeremonyTracker;
pub use context_manager::ContextManager;
pub use flow_budget_manager::FlowBudgetManager;
pub use logical_clock_manager::{LogicalClockManager, LogicalClockState};
pub use ota_manager::UpdateStatus;
pub use receipt_manager::{ReceiptManager, ReceiptManagerConfig};
pub use rendezvous_manager::{RendezvousManager, RendezvousManagerConfig};
pub use runtime_tasks::RuntimeTaskRegistry;
pub(crate) use ota_manager::OtaManager;
pub(crate) use session_manager::SessionManager;
pub use social_manager::{SocialManager, SocialManagerConfig, SocialManagerState};
pub use sync_manager::{SyncManagerConfig, SyncManagerState, SyncServiceManager};
pub use threshold_signing::ThresholdSigningService;
pub use traits::{RuntimeService, ServiceError, ServiceErrorKind, ServiceHealth};
