//! # Callback Type Aliases
//!
//! Common callback type patterns used across TUI screens.

use std::sync::Arc;

use crate::tui::semantic_lifecycle::{
    CeremonySubmissionOwner, LocalTerminalOperationOwner, WorkflowHandoffOperationOwner,
};
use crate::tui::state::DeviceId;
use crate::tui::types::MfaPolicy;
use aura_core::AuthorityId;

// =============================================================================
// Base Callback Types
// =============================================================================

/// Callback that takes no arguments.
pub type NoArgCallback = Arc<dyn Fn() + Send + Sync>;
pub(crate) type NoArgOwnedCallback = Arc<dyn Fn(WorkflowHandoffOperationOwner) + Send + Sync>;
pub(crate) type NoArgLocalOwnedCallback = Arc<dyn Fn(LocalTerminalOperationOwner) + Send + Sync>;

/// Callback that takes a single string ID.
pub type IdCallback = Arc<dyn Fn(String) + Send + Sync>;
pub(crate) type IdHandoffCallback =
    Arc<dyn Fn(String, WorkflowHandoffOperationOwner) + Send + Sync>;
pub(crate) type IdLocalOwnedCallback =
    Arc<dyn Fn(String, LocalTerminalOperationOwner) + Send + Sync>;
pub(crate) type StringLocalOwnedCallback =
    Arc<dyn Fn(String, LocalTerminalOperationOwner) + Send + Sync>;

/// Callback that takes two string arguments.
pub type TwoStringCallback = Arc<dyn Fn(String, String) + Send + Sync>;
pub(crate) type TwoStringHandoffCallback =
    Arc<dyn Fn(String, String, WorkflowHandoffOperationOwner) + Send + Sync>;
pub(crate) type TwoStringLocalOwnedCallback =
    Arc<dyn Fn(String, String, LocalTerminalOperationOwner) + Send + Sync>;
pub(crate) type TwoStringContextHandoffCallback =
    Arc<dyn Fn(String, String, Option<String>, WorkflowHandoffOperationOwner) + Send + Sync>;

/// Callback that takes three string arguments.
pub type ThreeStringCallback = Arc<dyn Fn(String, String, String) + Send + Sync>;
pub(crate) type ThreeStringHandoffCallback =
    Arc<dyn Fn(String, String, String, WorkflowHandoffOperationOwner) + Send + Sync>;

/// Callback that takes a string and optional string.
pub type StringOptStringCallback = Arc<dyn Fn(String, Option<String>) + Send + Sync>;
pub(crate) type StringOptStringLocalOwnedCallback =
    Arc<dyn Fn(String, Option<String>, LocalTerminalOperationOwner) + Send + Sync>;

/// Callback that takes a string, optional string, and a list of strings.
pub(crate) type StringOptStringVecU8LocalOwnedCallback =
    Arc<dyn Fn(String, Option<String>, Vec<String>, u8, LocalTerminalOperationOwner) + Send + Sync>;

/// Callback that takes two u8 values.
pub type ThresholdCallback = Arc<dyn Fn(u8, u8) + Send + Sync>;
pub(crate) type ThresholdLocalOwnedCallback =
    Arc<dyn Fn(u8, u8, LocalTerminalOperationOwner) + Send + Sync>;

/// Callback for creating an invitation and (usually) surfacing a shareable code.
///
/// Arguments:
/// - optional receiver authority ID
/// - invitation type string (e.g. "contact", "guardian", "channel")
/// - optional message
/// - optional TTL (seconds)
pub type CreateInvitationCallbackType = Arc<
    dyn Fn(Option<AuthorityId>, String, Option<String>, Option<u64>, LocalTerminalOperationOwner)
        + Send
        + Sync,
>;

// =============================================================================
// Semantic Type Aliases
// =============================================================================

// --- Chat Screen ---
/// Slash-command callback takes channel_id and raw command text.
/// Regular parity-critical message sending uses `SendOwnedCallback`.
pub(crate) type SlashCommandCallback = TwoStringCallback;
#[doc(hidden)]
#[allow(private_interfaces)]
pub(crate) type SendOwnedCallback = TwoStringHandoffCallback;
pub type ChannelSelectCallback = IdCallback;
pub(crate) type JoinChannelCallback = IdHandoffCallback;
#[doc(hidden)]
#[allow(private_interfaces)]
pub type CreateChannelCallback = StringOptStringVecU8LocalOwnedCallback;
pub(crate) type RetryMessageCallback = ThreeStringHandoffCallback;
pub(crate) type SetTopicCallback = TwoStringLocalOwnedCallback;

// --- Contacts Screen ---
pub(crate) type UpdateNicknameCallback = TwoStringLocalOwnedCallback;
pub(crate) type StartChatCallback = IdLocalOwnedCallback;

// --- Recovery Screen ---
pub type RecoveryCallback = NoArgCallback;
pub type ApprovalCallback = IdCallback;
pub(crate) type GuardianSelectCallback = IdHandoffCallback;

// --- Settings Screen ---
pub(crate) type UpdateNicknameSuggestionCallback = StringLocalOwnedCallback;
pub(crate) type UpdateMfaCallback =
    Arc<dyn Fn(MfaPolicy, LocalTerminalOperationOwner) + Send + Sync>;
/// Callback for adding a device: (nickname, invitee_authority_id, operation)
#[doc(hidden)]
#[allow(private_interfaces)]
pub type AddDeviceCallback =
    Arc<dyn Fn(String, AuthorityId, LocalTerminalOperationOwner) + Send + Sync>;
#[doc(hidden)]
#[allow(private_interfaces)]
pub type RemoveDeviceCallback = Arc<dyn Fn(DeviceId, CeremonySubmissionOwner) + Send + Sync>;
pub(crate) type UpdateThresholdCallback = ThresholdLocalOwnedCallback;
pub(crate) type ImportDeviceEnrollmentCallback =
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
pub(crate) type CreateHomeCallback = StringOptStringLocalOwnedCallback;
/// Create neighborhood callback: (name)
pub(crate) type CreateNeighborhoodCallback = StringLocalOwnedCallback;
/// Neighborhood home operation callback: (home_id)
pub(crate) type NeighborhoodHomeCallback = IdLocalOwnedCallback;
/// Set moderator callback: (optional_home_id, target_authority_id, assign)
pub(crate) type SetModeratorCallback =
    Arc<dyn Fn(Option<String>, String, bool, LocalTerminalOperationOwner) + Send + Sync>;

// --- App Screen ---
#[doc(hidden)]
#[allow(private_interfaces)]
pub(crate) type CreateAccountCallback =
    Arc<dyn Fn(String, LocalTerminalOperationOwner) + Send + Sync>;
