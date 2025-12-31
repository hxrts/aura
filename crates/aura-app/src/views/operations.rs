//! Portable operation result types
//!
//! This module provides structured response types for operations that
//! return complex data (invitations, ceremonies, etc.). These types
//! can be used across frontends for consistent handling.
//!
//! Note: Simple ok/error results don't need dedicated types - they use
//! standard Result<(), Error>. This module is for operations that return
//! rich, structured data.

use aura_core::types::Epoch;

// ============================================================================
// Invitation Operations
// ============================================================================

/// Result of exporting an invitation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportedInvitation {
    /// The invitation ID
    pub id: String,
    /// The exportable invitation code
    pub code: String,
}

impl ExportedInvitation {
    /// Create a new exported invitation.
    #[must_use]
    pub fn new(id: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            code: code.into(),
        }
    }
}

/// Result of importing an invitation code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedInvitation {
    /// The parsed invitation ID
    pub invitation_id: String,
    /// Sender authority ID
    pub sender_id: String,
    /// Invitation type (e.g., "channel", "guardian", "contact")
    pub invitation_type: String,
    /// Optional expiration timestamp (ms since epoch)
    pub expires_at: Option<u64>,
    /// Optional message from sender
    pub message: Option<String>,
}

impl ImportedInvitation {
    /// Create a new imported invitation result.
    pub fn new(
        invitation_id: impl Into<String>,
        sender_id: impl Into<String>,
        invitation_type: impl Into<String>,
    ) -> Self {
        Self {
            invitation_id: invitation_id.into(),
            sender_id: sender_id.into(),
            invitation_type: invitation_type.into(),
            expires_at: None,
            message: None,
        }
    }

    /// Set the expiration time.
    #[must_use]
    pub fn with_expires_at(mut self, expires_at: u64) -> Self {
        self.expires_at = Some(expires_at);
        self
    }

    /// Set the message.
    #[must_use]
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    /// Check if the invitation has expired.
    #[must_use]
    pub fn is_expired(&self, now_ms: u64) -> bool {
        self.expires_at.map_or(false, |exp| now_ms >= exp)
    }
}

// ============================================================================
// Device Enrollment Operations
// ============================================================================

/// Result of starting a device enrollment ceremony.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceEnrollmentStarted {
    /// Ceremony identifier for polling/cancel
    pub ceremony_id: String,
    /// Shareable enrollment code to import on the new device
    pub enrollment_code: String,
    /// Pending epoch created during prepare
    pub pending_epoch: Epoch,
    /// Device id being enrolled
    pub device_id: String,
}

impl DeviceEnrollmentStarted {
    /// Create a new device enrollment result.
    pub fn new(
        ceremony_id: impl Into<String>,
        enrollment_code: impl Into<String>,
        pending_epoch: Epoch,
        device_id: impl Into<String>,
    ) -> Self {
        Self {
            ceremony_id: ceremony_id.into(),
            enrollment_code: enrollment_code.into(),
            pending_epoch,
            device_id: device_id.into(),
        }
    }
}

/// Result of starting a device removal ceremony.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceRemovalStarted {
    /// Ceremony identifier for polling/cancel
    pub ceremony_id: String,
}

impl DeviceRemovalStarted {
    /// Create a new device removal result.
    #[must_use]
    pub fn new(ceremony_id: impl Into<String>) -> Self {
        Self {
            ceremony_id: ceremony_id.into(),
        }
    }
}

// ============================================================================
// Settings Operations
// ============================================================================

/// Result of updating the display name/nickname.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NicknameUpdated {
    /// The new display name
    pub name: String,
}

impl NicknameUpdated {
    /// Create a new nickname update result.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

/// Result of updating MFA policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MfaPolicyUpdated {
    /// Whether MFA is now required
    pub require_mfa: bool,
}

impl MfaPolicyUpdated {
    /// Create a new MFA policy update result.
    #[must_use]
    pub const fn new(require_mfa: bool) -> Self {
        Self { require_mfa }
    }
}

/// Result of updating channel mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelModeUpdated {
    /// Channel ID that was updated
    pub channel_id: String,
    /// Mode flags that were applied
    pub flags: String,
}

impl ChannelModeUpdated {
    /// Create a new channel mode update result.
    pub fn new(channel_id: impl Into<String>, flags: impl Into<String>) -> Self {
        Self {
            channel_id: channel_id.into(),
            flags: flags.into(),
        }
    }
}

/// Result of changing context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextChanged {
    /// The new context ID (None to clear)
    pub context_id: Option<String>,
}

impl ContextChanged {
    /// Create a context change to a specific context.
    #[must_use]
    pub fn to_context(context_id: impl Into<String>) -> Self {
        Self {
            context_id: Some(context_id.into()),
        }
    }

    /// Create a context change clearing the context.
    #[must_use]
    pub const fn cleared() -> Self {
        Self { context_id: None }
    }

    /// Check if context was cleared.
    #[must_use]
    pub fn is_cleared(&self) -> bool {
        self.context_id.is_none()
    }
}

// ============================================================================
// Operation Errors
// ============================================================================

/// Portable operation error type.
///
/// This corresponds to the terminal's `OpError` but is portable across frontends.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OperationError {
    /// Feature not implemented
    #[error("Not implemented: {0}")]
    NotImplemented(String),
    /// Invalid argument provided
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),
    /// Operation failed
    #[error("Operation failed: {0}")]
    Failed(String),
}

impl OperationError {
    /// Create a not implemented error.
    pub fn not_implemented(what: impl Into<String>) -> Self {
        Self::NotImplemented(what.into())
    }

    /// Create an invalid argument error.
    pub fn invalid_argument(why: impl Into<String>) -> Self {
        Self::InvalidArgument(why.into())
    }

    /// Create a failed error.
    pub fn failed(why: impl Into<String>) -> Self {
        Self::Failed(why.into())
    }

    /// Get the error category for this error.
    #[must_use]
    pub fn category(&self) -> crate::errors::ErrorCategory {
        match self {
            Self::NotImplemented(_) => crate::errors::ErrorCategory::NotImplemented,
            Self::InvalidArgument(_) => crate::errors::ErrorCategory::Input,
            Self::Failed(_) => crate::errors::ErrorCategory::Operation,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exported_invitation() {
        let inv = ExportedInvitation::new("inv-123", "CODE123");
        assert_eq!(inv.id, "inv-123");
        assert_eq!(inv.code, "CODE123");
    }

    #[test]
    fn test_imported_invitation() {
        let inv = ImportedInvitation::new("inv-123", "sender-456", "channel")
            .with_expires_at(1000)
            .with_message("Join my channel!");

        assert_eq!(inv.invitation_id, "inv-123");
        assert_eq!(inv.sender_id, "sender-456");
        assert_eq!(inv.invitation_type, "channel");
        assert_eq!(inv.expires_at, Some(1000));
        assert_eq!(inv.message.as_deref(), Some("Join my channel!"));
    }

    #[test]
    fn test_imported_invitation_expiry() {
        let inv = ImportedInvitation::new("inv-123", "sender", "guardian").with_expires_at(1000);

        assert!(!inv.is_expired(500)); // Before expiry
        assert!(inv.is_expired(1000)); // At expiry
        assert!(inv.is_expired(1500)); // After expiry

        // No expiration set
        let inv2 = ImportedInvitation::new("inv-456", "sender", "contact");
        assert!(!inv2.is_expired(u64::MAX)); // Never expires
    }

    #[test]
    fn test_device_enrollment_started() {
        let enroll = DeviceEnrollmentStarted::new("cer-123", "ENROLL-CODE", Epoch::new(5), "dev-1");

        assert_eq!(enroll.ceremony_id, "cer-123");
        assert_eq!(enroll.enrollment_code, "ENROLL-CODE");
        assert_eq!(enroll.pending_epoch, Epoch::new(5));
        assert_eq!(enroll.device_id, "dev-1");
    }

    #[test]
    fn test_device_removal_started() {
        let removal = DeviceRemovalStarted::new("cer-456");
        assert_eq!(removal.ceremony_id, "cer-456");
    }

    #[test]
    fn test_context_changed() {
        let change = ContextChanged::to_context("ctx-123");
        assert_eq!(change.context_id.as_deref(), Some("ctx-123"));
        assert!(!change.is_cleared());

        let cleared = ContextChanged::cleared();
        assert!(cleared.context_id.is_none());
        assert!(cleared.is_cleared());
    }

    #[test]
    fn test_nickname_updated() {
        let update = NicknameUpdated::new("New Name");
        assert_eq!(update.name, "New Name");
    }

    #[test]
    fn test_mfa_policy_updated() {
        let enabled = MfaPolicyUpdated::new(true);
        assert!(enabled.require_mfa);

        let disabled = MfaPolicyUpdated::new(false);
        assert!(!disabled.require_mfa);
    }

    #[test]
    fn test_channel_mode_updated() {
        let update = ChannelModeUpdated::new("chan-123", "+m+s");
        assert_eq!(update.channel_id, "chan-123");
        assert_eq!(update.flags, "+m+s");
    }

    #[test]
    fn test_operation_error() {
        let e1 = OperationError::not_implemented("feature X");
        assert!(matches!(e1, OperationError::NotImplemented(_)));
        assert_eq!(
            e1.category(),
            crate::errors::ErrorCategory::NotImplemented
        );

        let e2 = OperationError::invalid_argument("bad value");
        assert!(matches!(e2, OperationError::InvalidArgument(_)));
        assert_eq!(e2.category(), crate::errors::ErrorCategory::Input);

        let e3 = OperationError::failed("something went wrong");
        assert!(matches!(e3, OperationError::Failed(_)));
        assert_eq!(e3.category(), crate::errors::ErrorCategory::Operation);
    }
}
