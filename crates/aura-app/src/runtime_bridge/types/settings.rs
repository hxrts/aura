//! Settings and authority-summary bridge types.

use crate::views::naming::{truncate_id_for_display, EffectiveName};
use aura_core::types::identifiers::AuthorityId;
use aura_core::DeviceId;

/// Bridge-level settings state returned from `RuntimeBridge`.
#[derive(Debug, Clone, Default)]
pub struct SettingsBridgeState {
    /// User's nickname suggestion (what they want to be called).
    pub nickname_suggestion: String,
    /// MFA policy setting.
    pub mfa_policy: String,
    /// Threshold signing configuration (k of n).
    pub threshold_k: u16,
    /// Total guardians in threshold scheme.
    pub threshold_n: u16,
    /// Number of registered devices.
    pub device_count: usize,
    /// Number of contacts.
    pub contact_count: usize,
}

impl SettingsBridgeState {
    /// Returns `true` if this state was populated from a real runtime.
    pub fn has_valid_threshold(&self) -> bool {
        self.threshold_k >= 2 && self.threshold_n >= self.threshold_k
    }
}

/// Bridge-level device summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeDeviceInfo {
    /// Stable device identifier.
    pub id: DeviceId,
    /// Human-friendly label (best effort, computed for display).
    pub name: String,
    /// Local nickname override (user-assigned name for this device).
    pub nickname: Option<String>,
    /// Nickname suggestion (what the device wants to be called, from enrollment).
    pub nickname_suggestion: Option<String>,
    /// Whether this is the current device.
    pub is_current: bool,
    /// Last-seen timestamp (ms since epoch), if known.
    pub last_seen: Option<u64>,
}

impl EffectiveName for BridgeDeviceInfo {
    fn nickname(&self) -> Option<&str> {
        self.nickname.as_deref().filter(|s| !s.is_empty())
    }

    fn nickname_suggestion(&self) -> Option<&str> {
        self.nickname_suggestion
            .as_deref()
            .filter(|s| !s.is_empty())
    }

    fn fallback_id(&self) -> String {
        truncate_id_for_display(&self.id.to_string())
    }
}

/// Bridge-level authority summary for settings and authority switching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeAuthorityInfo {
    /// Stable authority identifier.
    pub id: AuthorityId,
    /// Best-effort display label or nickname suggestion.
    pub nickname_suggestion: Option<String>,
    /// Whether this is the currently active authority for the runtime.
    pub is_current: bool,
}
