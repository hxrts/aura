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
//! **1. Reactive Signal Pattern**
//! - Read current state from signals
//! - Perform operations via AppCore's runtime bridge
//! - Emit updated signals when state changes
//! - Return domain types (not UI types)
//!
//! **2. AppCore Integration**
//! - All workflows take `&Arc<RwLock<AppCore>>` reference
//! - Use AppCore's runtime bridge for effect execution
//! - Emit signals via `core.emit()` after operations
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
//!     // Emit signal for reactive UI updates
//!     emit_invitations_signal(app_core).await;
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
pub mod invitation;
pub mod recovery;

// Re-export workflow functions for convenience
pub use budget::{can_add_resident, can_join_neighborhood, can_pin_content, get_current_budget};
pub use invitation::{
    accept_invitation, cancel_invitation, create_invitation, decline_invitation,
    export_invitation, import_invitation, list_invitations,
};
pub use recovery::{
    approve_recovery, dispute_recovery, get_recovery_status, start_recovery,
};
