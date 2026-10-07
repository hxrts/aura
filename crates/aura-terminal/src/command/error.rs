//! Typed command failures and their process exit codes.
//!
//! Every CLI command and RPC request fails with a [`CommandError`]: a stable
//! [`ErrorCode`] (which fixes the process exit code), a user-facing message
//! worded by `aura_app::ui::workflows::user_errors::classify`, and the raw
//! error chain as copyable detail.

use crate::error::TerminalError;
use aura_app::ui::contract::SemanticOperationError;
use aura_app::ui::types::ErrorCategory;
use aura_app::ui::workflows::user_errors::{classify, UserFacingError};
use aura_core::{AuraError, TimeoutBudgetError};
use serde::{Deserialize, Serialize};

/// Stable failure classes shared by CLI exit codes and RPC error payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The operation ran and failed.
    Failed,
    /// Arguments or request parameters were invalid, or a required
    /// confirmation was not given.
    InvalidInput,
    /// The account, channel, contact or invitation does not exist.
    NotFound,
    /// The caller lacks the capability for the operation.
    PermissionDenied,
    /// The operation did not finish within `--timeout`.
    Timeout,
    /// A peer, the network or a feature is unavailable.
    Unavailable,
}

impl ErrorCode {
    /// Process exit code for this failure class (0 is success).
    #[must_use]
    pub fn exit_code(self) -> i32 {
        match self {
            Self::Failed => 1,
            Self::InvalidInput => 2,
            Self::NotFound => 3,
            Self::PermissionDenied => 4,
            Self::Timeout => 5,
            Self::Unavailable => 6,
        }
    }

    fn from_category(category: ErrorCategory) -> Self {
        match category {
            ErrorCategory::Input | ErrorCategory::Config => Self::InvalidInput,
            ErrorCategory::Capability => Self::PermissionDenied,
            ErrorCategory::NotFound => Self::NotFound,
            ErrorCategory::Network | ErrorCategory::NotImplemented => Self::Unavailable,
            ErrorCategory::Operation => Self::Failed,
        }
    }

    fn from_aura(error: &AuraError) -> Self {
        match error {
            AuraError::Invalid { .. } | AuraError::Serialization { .. } => Self::InvalidInput,
            AuraError::NotFound { .. } => Self::NotFound,
            AuraError::PermissionDenied { .. } => Self::PermissionDenied,
            AuraError::Network { .. } => Self::Unavailable,
            AuraError::Crypto { .. }
            | AuraError::Storage { .. }
            | AuraError::Internal { .. }
            | AuraError::Terminal(_) => Self::Failed,
        }
    }
}

/// A typed command failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandError {
    /// Failure class; fixes the CLI exit code.
    pub code: ErrorCode,
    /// User-facing sentence.
    pub message: String,
    /// The raw error chain, when it says more than `message`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// The typed semantic failure the workflow published, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<SemanticOperationError>,
}

impl CommandError {
    /// A failure with an explicit class and message.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            detail: None,
            failure: None,
        }
    }

    /// Invalid arguments or request parameters.
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidInput, message)
    }

    /// Missing account, channel, contact or invitation.
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, message)
    }

    /// The operation exceeded its deadline.
    pub fn timeout(seconds: u64) -> Self {
        Self::new(
            ErrorCode::Timeout,
            format!("The operation did not finish within {seconds}s"),
        )
    }

    /// Build from a raw error chain: the class comes from `code`, the
    /// message from `user_errors::classify`.
    fn classified(code: ErrorCode, raw: String) -> Self {
        let (message, detail) = match classify(&raw) {
            UserFacingError::Unchanged => (raw, None),
            UserFacingError::Sentence(sentence) => (sentence, Some(raw)),
            UserFacingError::SeeDetails(Some(label)) => (label, Some(raw)),
            UserFacingError::SeeDetails(None) => ("The operation failed".to_string(), Some(raw)),
        };
        Self {
            code,
            message,
            detail,
            failure: None,
        }
    }

    /// Process exit code for this failure.
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        self.code.exit_code()
    }
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CommandError {}

/// The class of a native error chain: a timeout anywhere in the chain wins,
/// then the first typed `AuraError`.
fn chain_code(error: &(dyn std::error::Error + 'static)) -> Option<ErrorCode> {
    let mut aura = None;
    let mut current = Some(error);
    while let Some(cause) = current {
        if cause.downcast_ref::<TimeoutBudgetError>().is_some() {
            return Some(ErrorCode::Timeout);
        }
        if aura.is_none() {
            aura = cause.downcast_ref::<AuraError>().map(ErrorCode::from_aura);
        }
        current = cause.source();
    }
    aura
}

/// Render an error with its source chain.
fn with_causes(error: &(dyn std::error::Error + 'static)) -> String {
    let mut rendered = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        let text = cause.to_string();
        if !rendered.contains(&text) {
            rendered.push_str(": ");
            rendered.push_str(&text);
        }
        source = cause.source();
    }
    rendered
}

impl From<TerminalError> for CommandError {
    fn from(error: TerminalError) -> Self {
        let code = match &error {
            TerminalError::NativeOperation { .. } => {
                chain_code(&error).unwrap_or(ErrorCode::Failed)
            }
            other => ErrorCode::from_category(other.category()),
        };
        Self::classified(code, with_causes(&error))
    }
}

impl From<AuraError> for CommandError {
    fn from(error: AuraError) -> Self {
        let code = chain_code(&error).unwrap_or_else(|| ErrorCode::from_aura(&error));
        Self::classified(code, with_causes(&error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_are_distinct_and_nonzero() {
        let codes = [
            ErrorCode::Failed,
            ErrorCode::InvalidInput,
            ErrorCode::NotFound,
            ErrorCode::PermissionDenied,
            ErrorCode::Timeout,
            ErrorCode::Unavailable,
        ];
        let mut seen = std::collections::BTreeSet::new();
        for code in codes {
            assert_ne!(code.exit_code(), 0);
            assert!(
                seen.insert(code.exit_code()),
                "{code:?} reuses an exit code"
            );
        }
    }

    #[test]
    fn terminal_categories_map_to_codes() {
        assert_eq!(
            CommandError::from(TerminalError::Input("bad".into())).code,
            ErrorCode::InvalidInput
        );
        assert_eq!(
            CommandError::from(TerminalError::NotFound("x".into())).code,
            ErrorCode::NotFound
        );
        assert_eq!(
            CommandError::from(TerminalError::Capability("x".into())).code,
            ErrorCode::PermissionDenied
        );
    }

    #[test]
    fn native_aura_errors_keep_their_class_through_terminal_errors() {
        let error = TerminalError::from(AuraError::not_found("channel general"));
        assert_eq!(CommandError::from(error).code, ErrorCode::NotFound);
    }

    #[test]
    fn messages_are_worded_by_user_errors_classify() {
        let error = CommandError::from(AuraError::internal(
            "import invitation: Invalid invite code: JSON parsing failed",
        ));
        assert_eq!(error.message, "That invitation code isn't valid");
        assert!(error.detail.is_some());
    }
}
