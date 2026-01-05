//! # Enrollment Context
//!
//! Type-safe enrollment context for device addition ceremonies.

use aura_core::identifiers::{AuthorityId, DeviceId};
use std::fmt;

/// Error when constructing an enrollment context
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnrollmentError {
    /// No authority is configured
    NoAuthority,
    /// Parent device lacks enrollment capability
    ParentDeviceLacksCapability,
    /// The specified parent device was not found
    ParentDeviceNotFound,
}

impl fmt::Display for EnrollmentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EnrollmentError::NoAuthority => {
                write!(f, "No authority configured - create an account first")
            }
            EnrollmentError::ParentDeviceLacksCapability => {
                write!(f, "This device cannot enroll new devices")
            }
            EnrollmentError::ParentDeviceNotFound => {
                write!(f, "Parent device not found in account")
            }
        }
    }
}

impl std::error::Error for EnrollmentError {}

/// A validated enrollment context for adding new devices
///
/// Invariants:
/// - Authority exists and is accessible
/// - Parent device has enrollment capability
///
/// # Example
///
/// ```rust,ignore
/// let ctx = EnrollmentContext::new(authority_id, parent_device_id, true)?;
///
/// // Can now safely start device enrollment
/// start_enrollment(ctx);
/// ```
#[derive(Debug, Clone)]
pub struct EnrollmentContext {
    authority: AuthorityId,
    parent_device: DeviceId,
}

impl EnrollmentContext {
    /// Create an enrollment context
    ///
    /// # Arguments
    ///
    /// * `authority` - The authority to enroll the new device under
    /// * `parent_device` - The device performing the enrollment
    /// * `has_enrollment_capability` - Whether the parent device can enroll new devices
    ///
    /// Returns an error if the parent device lacks enrollment capability.
    pub fn new(
        authority: AuthorityId,
        parent_device: DeviceId,
        has_enrollment_capability: bool,
    ) -> Result<Self, EnrollmentError> {
        if !has_enrollment_capability {
            return Err(EnrollmentError::ParentDeviceLacksCapability);
        }
        Ok(Self {
            authority,
            parent_device,
        })
    }

    /// Get the authority ID
    pub fn authority(&self) -> &AuthorityId {
        &self.authority
    }

    /// Get the parent device ID
    pub fn parent_device(&self) -> &DeviceId {
        &self.parent_device
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU8, Ordering};
    use uuid::Uuid;

    static AUTH_COUNTER: AtomicU8 = AtomicU8::new(1);
    static DEV_COUNTER: AtomicU8 = AtomicU8::new(1);

    fn make_authority() -> AuthorityId {
        let n = AUTH_COUNTER.fetch_add(1, Ordering::Relaxed);
        AuthorityId::from_uuid(Uuid::from_bytes([n; 16]))
    }

    fn make_device() -> DeviceId {
        let n = DEV_COUNTER.fetch_add(1, Ordering::Relaxed);
        DeviceId::from_uuid(Uuid::from_bytes([n; 16]))
    }

    #[test]
    fn test_valid_enrollment() {
        let ctx = EnrollmentContext::new(make_authority(), make_device(), true);
        assert!(ctx.is_ok());
    }

    #[test]
    fn test_lacks_capability() {
        let ctx = EnrollmentContext::new(make_authority(), make_device(), false);
        assert_eq!(
            ctx.unwrap_err(),
            EnrollmentError::ParentDeviceLacksCapability
        );
    }
}
