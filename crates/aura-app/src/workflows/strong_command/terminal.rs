#![allow(missing_docs)]

use super::execution_model::{
    CommandTerminalClassification, CommandTerminalOutcomeStatus, CommandTerminalReasonCode,
};
use aura_core::AuraError;

fn message_contains_any(message: &str, needles: &[&str]) -> bool {
    let lowered = message.to_ascii_lowercase();
    needles.iter().any(|needle| lowered.contains(needle))
}

fn classify_invalid_terminal_command_message(message: &str) -> CommandTerminalClassification {
    if message_contains_any(
        message,
        &[
            "no active home",
            "missing current channel",
            "missing channel scope",
        ],
    ) {
        CommandTerminalClassification::new(
            CommandTerminalOutcomeStatus::Invalid,
            CommandTerminalReasonCode::MissingActiveContext,
        )
    } else if message_contains_any(
        message,
        &[
            "unknown authority target",
            "unknown channel target",
            "unknown context target",
            "not found",
        ],
    ) {
        CommandTerminalClassification::new(
            CommandTerminalOutcomeStatus::Invalid,
            CommandTerminalReasonCode::NotFound,
        )
    } else if message_contains_any(message, &["parse error", "missing required argument"]) {
        CommandTerminalClassification::new(
            CommandTerminalOutcomeStatus::Invalid,
            CommandTerminalReasonCode::InvalidArgument,
        )
    } else if message_contains_any(
        message,
        &["stale snapshot", "invalid state", "precondition failed"],
    ) {
        let reason = CommandTerminalReasonCode::InvalidState;
        CommandTerminalClassification::new(CommandTerminalOutcomeStatus::Failed, reason)
    } else {
        CommandTerminalClassification::new(
            CommandTerminalOutcomeStatus::Invalid,
            CommandTerminalReasonCode::InvalidArgument,
        )
    }
}

fn classify_permission_terminal_command_message(message: &str) -> CommandTerminalClassification {
    let reason = if message_contains_any(message, &["not a member"]) {
        CommandTerminalReasonCode::NotMember
    } else if message_contains_any(message, &["muted"]) {
        CommandTerminalReasonCode::Muted
    } else if message_contains_any(message, &["banned", "ban "]) {
        CommandTerminalReasonCode::Banned
    } else {
        CommandTerminalReasonCode::PermissionDenied
    };
    CommandTerminalClassification::new(CommandTerminalOutcomeStatus::Denied, reason)
}

/// Classify strong command execution failures for terminal-facing outcome
/// rendering without requiring Layer 7 string parsing.
#[must_use]
pub fn classify_terminal_execution_error(error: &AuraError) -> CommandTerminalClassification {
    match error {
        AuraError::Invalid { message } => classify_invalid_terminal_command_message(message),
        AuraError::NotFound { .. } => CommandTerminalClassification::new(
            CommandTerminalOutcomeStatus::Invalid,
            CommandTerminalReasonCode::NotFound,
        ),
        AuraError::PermissionDenied { message } => {
            classify_permission_terminal_command_message(message)
        }
        AuraError::Crypto { .. }
        | AuraError::Network { .. }
        | AuraError::Serialization { .. }
        | AuraError::Storage { .. }
        | AuraError::Internal { .. }
        | AuraError::Terminal(_) => CommandTerminalClassification::new(
            CommandTerminalOutcomeStatus::Failed,
            CommandTerminalReasonCode::Internal,
        ),
    }
}
