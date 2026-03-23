//! # Callback Type Aliases
//!
//! Common callback type patterns used across TUI screens.

use std::sync::Arc;

use crate::tui::semantic_lifecycle::{LocalTerminalOperationOwner, WorkflowHandoffOperationOwner};
use crate::tui::state::DeviceId;
use aura_core::AuthorityId;

// =============================================================================
// Base Callback Types
// =============================================================================

/// Callback that takes no arguments.
pub type NoArgCallback = Arc<dyn Fn() + Send + Sync>;
pub type NoArgOwnedCallback = Arc<dyn Fn(WorkflowHandoffOperationOwner) + Send + Sync>;
pub type NoArgLocalOwnedCallback = Arc<dyn Fn(LocalTerminalOperationOwner) + Send + Sync>;

/// Callback that takes a single string ID.
pub type IdCallback = Arc<dyn Fn(String) + Send + Sync>;
pub type IdHandoffCallback = Arc<dyn Fn(String, WorkflowHandoffOperationOwner) + Send + Sync>;
pub type IdLocalOwnedCallback = Arc<dyn Fn(String, LocalTerminalOperationOwner) + Send + Sync>;

/// Callback that takes two string arguments.
pub type TwoStringCallback = Arc<dyn Fn(String, String) + Send + Sync>;
pub type TwoStringHandoffCallback =
    Arc<dyn Fn(String, String, WorkflowHandoffOperationOwner) + Send + Sync>;
pub type TwoStringContextHandoffCallback =
    Arc<dyn Fn(String, String, Option<String>, WorkflowHandoffOperationOwner) + Send + Sync>;

/// Callback that takes three string arguments.
pub type ThreeStringCallback = Arc<dyn Fn(String, String, String) + Send + Sync>;

/// Callback that takes a string and optional string.
pub type StringOptStringCallback = Arc<dyn Fn(String, Option<String>) + Send + Sync>;

/// Callback that takes a string, optional string, and a list of strings.
pub type StringOptStringVecU8LocalOwnedCallback =
    Arc<dyn Fn(String, Option<String>, Vec<String>, u8, LocalTerminalOperationOwner) + Send + Sync>;

/// Callback that takes two u8 values.
pub type ThresholdCallback = Arc<dyn Fn(u8, u8) + Send + Sync>;

/// Callback for creating an invitation and (usually) surfacing a shareable code.
///
/// Arguments:
/// - receiver authority ID
/// - invitation type string (e.g. "contact", "guardian", "channel")
/// - optional message
/// - optional TTL (seconds)
pub type CreateInvitationCallbackType = Arc<
    dyn Fn(AuthorityId, String, Option<String>, Option<u64>, LocalTerminalOperationOwner)
        + Send
        + Sync,
>;

// =============================================================================
// Semantic Type Aliases
// =============================================================================

// --- Chat Screen ---
/// Send callback takes channel_id and content - channel is obtained from TUI's selected_channel
/// to avoid race conditions with async channel selection updates.
pub(crate) type SendCallback = TwoStringCallback;
#[doc(hidden)]
#[allow(private_interfaces)]
pub type SendOwnedCallback = TwoStringHandoffCallback;
pub type ChannelSelectCallback = IdCallback;
pub type JoinChannelCallback = IdHandoffCallback;
#[doc(hidden)]
#[allow(private_interfaces)]
pub type CreateChannelCallback = StringOptStringVecU8LocalOwnedCallback;
pub type RetryMessageCallback = ThreeStringCallback;
pub type SetTopicCallback = TwoStringCallback;

// --- Contacts Screen ---
pub type UpdateNicknameCallback = TwoStringCallback;
pub type StartChatCallback = IdCallback;

// --- Recovery Screen ---
pub type RecoveryCallback = NoArgCallback;
pub type ApprovalCallback = IdCallback;

// --- Settings Screen ---
pub type UpdateNicknameSuggestionCallback = IdCallback;
/// Callback for adding a device: (nickname, invitee_authority_id, operation)
#[doc(hidden)]
#[allow(private_interfaces)]
pub type AddDeviceCallback =
    Arc<dyn Fn(String, AuthorityId, LocalTerminalOperationOwner) + Send + Sync>;
pub type RemoveDeviceCallback = Arc<dyn Fn(DeviceId) + Send + Sync>;
pub type UpdateThresholdCallback = ThresholdCallback;
pub type ImportDeviceEnrollmentCallback =
    Arc<dyn Fn(String, LocalTerminalOperationOwner) + Send + Sync>;

// --- Invitations Screen ---
pub type InvitationCallback = IdCallback;
pub type CreateInvitationCallback = CreateInvitationCallbackType;
pub type ExportInvitationCallback = IdCallback;
pub(crate) type ImportInvitationOwnedCallback =
    Arc<dyn Fn(String, WorkflowHandoffOperationOwner) + Send + Sync>;

// --- Neighborhood Screen ---
pub type GoHomeCallback = NoArgCallback;
/// Create home callback: (name, optional_description)
pub type CreateHomeCallback = StringOptStringCallback;
/// Create neighborhood callback: (name)
pub type CreateNeighborhoodCallback = IdCallback;
/// Neighborhood home operation callback: (home_id)
pub type NeighborhoodHomeCallback = IdCallback;
/// Set moderator callback: (optional_home_id, target_authority_id, assign)
pub type SetModeratorCallback = Arc<dyn Fn(Option<String>, String, bool) + Send + Sync>;

// --- App Screen ---
#[doc(hidden)]
#[allow(private_interfaces)]
pub type CreateAccountCallback = Arc<dyn Fn(String, LocalTerminalOperationOwner) + Send + Sync>;
pub type GuardianSelectCallback = IdCallback;
