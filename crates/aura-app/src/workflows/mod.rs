//! # Workflows - Portable Business Logic
//!
//! This module contains workflow coordinators that implement multi-step
//! operations and business logic that is portable across all frontends
//! (CLI, TUI, iOS, Android, Web).
//!
//! ## Architecture
//!
//! Workflows follow the "what to do" / "how to display it" separation:
//! - **Workflows (aura-app)**: Pure business logic, returns domain types
//! - **Handlers (aura-terminal)**: Terminal-specific formatting
//!
//! ## Design Patterns
//!
//! All workflows follow these patterns:
//!
//! **1. ViewState-First Pattern**
//! - Read current state from ViewState (`app_core.views().snapshot()`)
//! - Perform operations via AppCore's runtime bridge
//! - Update ViewState via AppCore methods (signals auto-forward)
//! - Return domain types (not UI types)
//!
//! **2. AppCore Integration**
//! - All workflows take `&Arc<RwLock<AppCore>>` reference
//! - Use AppCore's runtime bridge for effect execution
//! - Update ViewState via `core.views().set_*()` or AppCore helper methods
//! - ReactiveEffects signals update automatically via signal forwarding
//!
//! **3. Error Handling**
//! - Return `Result<T, AppError>` (not terminal-specific errors)
//! - Errors propagate to handlers for display formatting
//!
//! ## Example
//!
//! ```rust,ignore
//! // Workflow (aura-app) - "what to do"
//! pub async fn create_invitation(
//!     app_core: &Arc<RwLock<AppCore>>,
//!     receiver: AuthorityId,
//!     role: Role,
//! ) -> Result<Invitation, AppError> {
//!     // Business logic - portable across all frontends
//!     let invitation = /* ... */;
//!
//!     // Update ViewState - signal forwarding handles ReactiveEffects
//!     app_core.add_invitation(invitation.clone());
//!
//!     Ok(invitation)
//! }
//!
//! // Handler (aura-terminal) - "how to display it"
//! pub async fn handle_invitation(
//!     app_core: &Arc<RwLock<AppCore>>,
//!     action: &InvitationAction,
//! ) -> TerminalResult<CliOutput> {
//!     let invitation = create_invitation(app_core, receiver, role).await?;
//!
//!     // Terminal-specific formatting
//!     let mut output = CliOutput::new();
//!     output.section("Invitation Created");
//!     output.kv("ID", &invitation.id);
//!     Ok(output)
//! }
//! ```

pub mod budget;
pub mod ceremonies;
pub mod contacts;
pub mod context;
pub mod invitation;
#[cfg(feature = "signals")]
pub mod messaging;
pub mod moderation;
pub mod network;
pub mod query;
#[cfg(feature = "signals")]
pub mod recovery;
pub mod settings;
pub mod steward;
pub mod sync;
pub mod system;

// Re-export budget types and workflow functions
pub use budget::{
    // Workflow functions
    can_add_resident,
    can_join_neighborhood,
    can_pin_content,
    get_budget_breakdown,
    get_current_budget,
    update_budget,
    // Types
    BlockFlowBudget,
    BudgetBreakdown,
    BudgetError,
    // Constants
    BLOCK_TOTAL_SIZE,
    BYTE,
    KB,
    MAX_NEIGHBORHOODS,
    MAX_RESIDENTS,
    MB,
    NEIGHBORHOOD_DONATION,
    RESIDENT_ALLOCATION,
};
pub use ceremonies::{
    cancel_key_rotation_ceremony, get_key_rotation_ceremony_status, monitor_key_rotation_ceremony,
    start_device_threshold_ceremony, start_guardian_ceremony,
};
pub use contacts::update_contact_nickname;
pub use context::{get_current_position, get_neighborhood_state, move_position, set_context};
pub use invitation::{
    accept_invitation, accept_pending_block_invitation, cancel_invitation,
    create_channel_invitation, create_contact_invitation, create_guardian_invitation,
    decline_invitation, export_invitation, import_invitation, import_invitation_details,
    list_invitations, list_pending_invitations,
};
#[cfg(feature = "signals")]
pub use messaging::{
    create_channel, get_chat_state, invite_user_to_channel, send_action, send_direct_message,
    send_message, start_direct_chat,
};
pub use moderation::{
    ban_user, kick_user, mute_user, pin_message, unban_user, unmute_user, unpin_message,
};
pub use network::{
    discover_peers, get_discovered_peers, list_lan_peers, list_peers, update_connection_status,
};
pub use query::{get_user_info, list_contacts, list_participants};
#[cfg(feature = "signals")]
pub use recovery::{approve_recovery, dispute_recovery, get_recovery_status, start_recovery};
pub use settings::{get_settings, set_channel_mode, update_mfa_policy, update_nickname};
pub use steward::{grant_steward, is_admin, revoke_steward};
pub use sync::{force_sync, get_sync_status, request_state};
pub use system::{is_available, ping, refresh_account};
