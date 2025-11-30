//! # TUI Reactive Module
//!
//! Reactive data bindings for the TUI. This module provides:
//! - Query types that generate Biscuit Datalog queries
//! - View dynamics that subscribe to database changes
//! - Type-safe conversion from query results to TUI data
//! - Query executor for executing queries and managing subscriptions

pub mod executor;
pub mod queries;
pub mod views;

pub use executor::{DataUpdate, QueryExecutor};
pub use queries::{
    Channel, ChannelType, ChannelsQuery, Guardian, GuardianApproval, GuardianStatus,
    GuardiansQuery, Invitation, InvitationDirection, InvitationStatus, InvitationType,
    InvitationsQuery, Message, MessagesQuery, RecoveryQuery, RecoveryState, RecoveryStatus,
    TuiQuery,
};
pub use views::{ChatView, GuardiansView, InvitationsView, RecoveryView, ViewState};
