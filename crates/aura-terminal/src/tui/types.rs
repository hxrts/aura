//! # Shared Types
//!
//! TUI-facing shared types and presentation adapters.
//!
//! Authoritative upstream types are consumed directly where the terminal is not
//! the owner. Terminal-local types remain only for display-specific grouping,
//! input helpers, and other shell-owned concerns.

mod chat;
mod contacts;
mod invitations;
mod neighborhood;
mod recovery;
mod settings;
mod shared;

pub use aura_app::ui::types::format_timestamp;

pub use chat::{Channel, DeliveryStatus, Message};
pub use contacts::{
    format_contact_name, Contact, ContactStatus, ReadReceiptPolicy, ReadReceiptPolicyExt,
};
pub use invitations::{Invitation, InvitationDirection, InvitationStatus, InvitationType};
pub use neighborhood::{AccessLevel, HomeBudget, HomeMember, HomeSummary};
pub use recovery::{
    Guardian, GuardianApproval, GuardianStatus, PendingRequest, RecoveryState, RecoveryStatus,
    RecoveryTab,
};
pub use settings::{AuthorityInfo, ChannelMode, Device, MfaPolicy, SettingsSection};
pub use shared::{short_id, KeyHint};
