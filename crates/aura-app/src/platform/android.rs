//! # Android Platform Helpers
//!
//! Helpers for Android integration via UniFFI.

use crate::errors::AppError;

/// Initialize the Android platform
///
/// Call this from `Application.onCreate()`
pub fn initialize() -> Result<(), AppError> {
    Err(AppError::internal(
        "platform::android",
        "Android platform initialization is not wired in this target",
    ))
}

/// Configure work manager for background sync
pub fn configure_background_sync() -> Result<(), AppError> {
    Err(AppError::internal(
        "platform::android",
        "Android background sync is not wired in this target",
    ))
}

/// Get the files directory path
///
/// Returns the path to the app's internal files directory
pub fn files_directory() -> Result<String, AppError> {
    Err(AppError::internal(
        "platform::android",
        "Android files directory lookup is not wired in this target",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn android_platform_hooks_fail_explicitly() {
        assert!(initialize().is_err());
        assert!(configure_background_sync().is_err());
        assert!(files_directory().is_err());
    }
}
