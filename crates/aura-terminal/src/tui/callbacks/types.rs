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

/// Callback that takes a string, optional string, and a list of strings.
pub type StringOptStringVecU8Callback =
    Arc<dyn Fn(String, Option<String>, Vec<String>, u8) + Send + Sync>;

/// Callback that takes two u8 values.
pub type ThresholdCallback = Arc<dyn Fn(u8, u8) + Send + Sync>;

/// Callback for creating an invitation and (usually) surfacing a shareable code.
///
/// Arguments:
/// - receiver authority ID
/// - invitation type string (e.g. "contact", "guardian", "channel")
/// - optional message
/// - optional TTL (seconds)
pub type CreateInvitationCallbackType =
    Arc<dyn Fn(String, String, Option<String>, Option<u64>) + Send + Sync>;

// =============================================================================
// Semantic Type Aliases
// =============================================================================

// --- Chat Screen ---
/// Send callback takes channel_id and content - channel is obtained from TUI's selected_channel
/// to avoid race conditions with async channel selection updates.
pub type SendCallback = TwoStringCallback;
pub type ChannelSelectCallback = IdCallback;
pub type CreateChannelCallback = StringOptStringVecU8Callback;
pub type RetryMessageCallback = ThreeStringCallback;
pub type SetTopicCallback = TwoStringCallback;

// --- Contacts Screen ---
pub type UpdateNicknameCallback = TwoStringCallback;
pub type StartChatCallback = IdCallback;

// --- Recovery Screen ---
pub type RecoveryCallback = NoArgCallback;
pub type ApprovalCallback = IdCallback;

// --- Settings Screen ---
pub type UpdateDisplayNameCallback = IdCallback;
pub type AddDeviceCallback = IdCallback;
pub type RemoveDeviceCallback = IdCallback;
pub type UpdateThresholdCallback = ThresholdCallback;
pub type ImportDeviceEnrollmentCallback = IdCallback;

// --- Invitations Screen ---
pub type InvitationCallback = IdCallback;
pub type CreateInvitationCallback = CreateInvitationCallbackType;
pub type ExportInvitationCallback = IdCallback;
pub type ImportInvitationCallback = IdCallback;

// --- Home Messaging ---
pub type HomeSendCallback = IdCallback;

// --- Neighborhood Screen ---
pub type GoHomeCallback = NoArgCallback;

// --- App Screen ---
pub type CreateAccountCallback = IdCallback;
pub type GuardianSelectCallback = IdCallback;
