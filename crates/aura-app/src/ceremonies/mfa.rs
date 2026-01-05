//! # MFA Device Set
//!
//! Type-safe MFA device set ensuring minimum device count for threshold signing.

use aura_core::identifiers::DeviceId;
use std::fmt;

/// Minimum number of devices required for MFA threshold signing
pub const MIN_MFA_DEVICES: usize = 2;

/// Error when constructing an MFA device set
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MfaSetupError {
    /// Not enough devices for MFA threshold signing
    InsufficientDevices {
        /// Number of devices required (always 2)
        required: usize,
        /// Number of devices available
        available: usize,
    },
}

impl fmt::Display for MfaSetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MfaSetupError::InsufficientDevices {
                required,
                available,
            } => {
                write!(
                    f,
                    "MFA requires at least {required} devices, but only {available} available"
                )
            }
        }
    }
}

impl std::error::Error for MfaSetupError {}

/// A device set with at least 2 devices for MFA threshold signing
///
/// Invariants:
/// - At least 2 devices must be present
///
/// # Example
///
/// ```rust,ignore
/// let devices = vec![device1, device2];
/// let mfa_set = MfaDeviceSet::from_devices(devices)?;
///
/// // Can now safely configure MFA
/// setup_mfa(mfa_set);
/// ```
#[derive(Debug, Clone)]
pub struct MfaDeviceSet {
    devices: Vec<DeviceId>,
}

impl MfaDeviceSet {
    /// Create an MFA device set from a list of device IDs
    ///
    /// Returns an error if fewer than 2 devices are provided.
    pub fn from_devices(devices: Vec<DeviceId>) -> Result<Self, MfaSetupError> {
        if devices.len() < MIN_MFA_DEVICES {
            return Err(MfaSetupError::InsufficientDevices {
                required: MIN_MFA_DEVICES,
                available: devices.len(),
            });
        }
        Ok(Self { devices })
    }

    /// Get the number of devices
    pub fn count(&self) -> usize {
        self.devices.len()
    }

    /// Get the device IDs
    pub fn devices(&self) -> &[DeviceId] {
        &self.devices
    }

    /// Consume and return the inner device list
    pub fn into_devices(self) -> Vec<DeviceId> {
        self.devices
    }

    /// Get maximum possible threshold k value
    ///
    /// For MFA, the maximum k equals the number of devices.
    pub fn max_threshold_k(&self) -> u8 {
        self.devices.len().min(255) as u8
    }

    /// Get recommended threshold for this device set
    ///
    /// Returns ceil((n+1)/2) for majority threshold.
    pub fn recommended_threshold(&self) -> u8 {
        let n = self.devices.len() as u8;
        (n / 2) + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU8, Ordering};
    use uuid::Uuid;

    static COUNTER: AtomicU8 = AtomicU8::new(1);

    fn make_device() -> DeviceId {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        DeviceId::from_uuid(Uuid::from_bytes([n; 16]))
    }

    #[test]
    fn test_insufficient_devices() {
        // 0 devices
        let result = MfaDeviceSet::from_devices(vec![]);
        assert_eq!(
            result.unwrap_err(),
            MfaSetupError::InsufficientDevices {
                required: 2,
                available: 0
            }
        );

        // 1 device
        let result = MfaDeviceSet::from_devices(vec![make_device()]);
        assert_eq!(
            result.unwrap_err(),
            MfaSetupError::InsufficientDevices {
                required: 2,
                available: 1
            }
        );
    }

    #[test]
    fn test_valid_device_set() {
        let devices = vec![make_device(), make_device()];
        let mfa_set = MfaDeviceSet::from_devices(devices).unwrap();
        assert_eq!(mfa_set.count(), 2);
    }

    #[test]
    fn test_recommended_threshold() {
        // 2 devices -> 2-of-2 (majority)
        let set2 = MfaDeviceSet::from_devices(vec![make_device(), make_device()]).unwrap();
        assert_eq!(set2.recommended_threshold(), 2);

        // 3 devices -> 2-of-3 (majority)
        let set3 =
            MfaDeviceSet::from_devices(vec![make_device(), make_device(), make_device()]).unwrap();
        assert_eq!(set3.recommended_threshold(), 2);
    }
}
