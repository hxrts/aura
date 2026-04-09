#![allow(missing_docs)]

use serde::{Deserialize, Serialize};

/// Message delivery status for tracking message lifecycle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum MessageDeliveryStatus {
    Sending,
    #[default]
    Sent,
    Delivered,
    Read,
    Failed,
}

impl MessageDeliveryStatus {
    /// Get the status indicator character for display.
    #[must_use]
    pub fn indicator(&self) -> &'static str {
        match self {
            Self::Sending => "◐",
            Self::Sent => "✓",
            Self::Delivered => "✓✓",
            Self::Read => "✓✓",
            Self::Failed => "✗",
        }
    }

    /// Get a short description for the status.
    #[must_use]
    pub fn description(&self) -> &'static str {
        match self {
            Self::Sending => "Sending...",
            Self::Sent => "Sent",
            Self::Delivered => "Delivered",
            Self::Read => "Read",
            Self::Failed => "Failed",
        }
    }

    /// Get a lowercase label for logging/serialization.
    #[must_use]
    pub fn label_lowercase(&self) -> &'static str {
        match self {
            Self::Sending => "sending",
            Self::Sent => "sent",
            Self::Delivered => "delivered",
            Self::Read => "read",
            Self::Failed => "failed",
        }
    }

    /// Whether the message has reached the recipient's device.
    #[must_use]
    pub fn is_delivered(&self) -> bool {
        matches!(self, Self::Delivered | Self::Read)
    }

    /// Whether the message has been read by the recipient.
    #[must_use]
    pub fn is_read(&self) -> bool {
        matches!(self, Self::Read)
    }

    /// Whether the message is still pending.
    #[must_use]
    pub fn is_pending(&self) -> bool {
        matches!(self, Self::Sending | Self::Sent)
    }

    /// Whether the message failed to send.
    #[must_use]
    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed)
    }

    /// Whether the message can be retried.
    #[must_use]
    pub fn can_retry(&self) -> bool {
        matches!(self, Self::Failed)
    }

    /// Whether the message has been successfully sent.
    #[must_use]
    pub fn is_sent(&self) -> bool {
        matches!(self, Self::Sent | Self::Delivered | Self::Read)
    }
}
