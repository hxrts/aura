//! # Callback Type Aliases
//!
//! Common callback type patterns used across TUI screens.

use std::sync::Arc;

// =============================================================================
// Base Callback Types
// =============================================================================

/// Callback that takes no arguments.
pub type NoArgCallback = Arc<dyn Fn() + Send + Sync>;

/// Callback that takes a single string ID.
pub type IdCallback = Arc<dyn Fn(String) + Send + Sync>;

/// Callback that takes two string arguments.
pub type TwoStringCallback = Arc<dyn Fn(String, String) + Send + Sync>;

/// Callback that takes three string arguments.
pub type ThreeStringCallback = Arc<dyn Fn(String, String, String) + Send + Sync>;

/// Callback that takes a string and optional string.
pub type StringOptStringCallback = Arc<dyn Fn(String, Option<String>) + Send + Sync>;

/// Callback that takes two u8 values.
pub type ThresholdCallback = Arc<dyn Fn(u8, u8) + Send + Sync>;

/// Callback that takes three arguments: String, Option<String>, Option<u64>.
pub type CreateInvitationCallbackType =
    Arc<dyn Fn(String, Option<String>, Option<u64>) + Send + Sync>;

// =============================================================================
// Semantic Type Aliases
// =============================================================================

// --- Chat Screen ---
pub type SendCallback = TwoStringCallback;
pub type ChannelSelectCallback = IdCallback;
pub type CreateChannelCallback = StringOptStringCallback;
pub type RetryMessageCallback = ThreeStringCallback;
pub type SetTopicCallback = TwoStringCallback;

// --- Contacts Screen ---
pub type UpdatePetnameCallback = TwoStringCallback;
pub type StartChatCallback = IdCallback;

// --- Recovery Screen ---
pub type RecoveryCallback = NoArgCallback;
pub type ApprovalCallback = IdCallback;

// --- Settings Screen ---
pub type UpdateNicknameCallback = IdCallback;
pub type AddDeviceCallback = IdCallback;
pub type RemoveDeviceCallback = IdCallback;
pub type UpdateThresholdCallback = ThresholdCallback;

// --- Invitations Screen ---
pub type InvitationCallback = IdCallback;
pub type CreateInvitationCallback = CreateInvitationCallbackType;
pub type ExportInvitationCallback = IdCallback;
pub type ImportInvitationCallback = IdCallback;

// --- Block Screen ---
pub type BlockSendCallback = IdCallback;
pub type BlockInviteCallback = IdCallback;
pub type BlockNavCallback = NoArgCallback;
pub type GrantStewardCallback = IdCallback;
pub type RevokeStewardCallback = IdCallback;

// --- Neighborhood Screen ---
pub type GoHomeCallback = NoArgCallback;

// --- App Screen ---
pub type CreateAccountCallback = IdCallback;
pub type GuardianSelectCallback = IdCallback;
