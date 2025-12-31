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
//! - Return `Result<T, AuraError>` (not terminal-specific errors)
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
//! ) -> Result<Invitation, AuraError> {
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

pub mod account;
pub mod admin;
pub mod authority;
pub mod budget;
pub mod amp;
pub mod chat_commands;
pub mod config;
pub mod ceremonies;
pub(crate) mod channel_ref;
pub mod contacts;
pub mod context;
pub mod demo_config;
pub mod ids;
pub mod invitation;
pub(crate) mod journal;
#[cfg(feature = "signals")]
pub mod messaging;
pub mod moderation;
pub mod network;
pub(crate) mod parse;
pub mod privacy;
pub mod query;
#[cfg(feature = "signals")]
pub mod recovery;
pub mod recovery_cli;
pub(crate) mod runtime;
pub(crate) mod signals;
pub(crate) mod state_helpers;
pub mod settings;
pub mod snapshot;
pub(crate) mod snapshot_policy;
pub mod steward;
pub mod sync;
pub mod system;
pub(crate) mod time;
