//! # iOS Platform Helpers
//!
//! Helpers for iOS integration via UniFFI.

use crate::errors::AppError;

/// Initialize the iOS platform
///
/// Call this from `application(_:didFinishLaunchingWithOptions:)`
pub fn initialize() -> Result<(), AppError> {
    Err(AppError::internal(
        "platform::ios",
        "iOS platform initialization is not wired in this target",
    ))
}

/// Configure background refresh
///
/// Call this to enable background data sync
pub fn configure_background_refresh() -> Result<(), AppError> {
    Err(AppError::internal(
        "platform::ios",
        "iOS background refresh is not wired in this target",
    ))
}

/// Get the documents directory path
///
/// Returns the path to the app's Documents directory
pub fn documents_directory() -> Result<String, AppError> {
    Err(AppError::internal(
        "platform::ios",
        "iOS documents directory lookup is not wired in this target",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ios_platform_hooks_fail_explicitly() {
        assert!(initialize().is_err());
        assert!(configure_background_refresh().is_err());
        assert!(documents_directory().is_err());
    }
}
