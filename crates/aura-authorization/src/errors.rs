//! Web of Trust error handling using unified error macros.
//!
//! **DESIGN**: Uses aura-macros error generator for consistent error shape and categories.

use aura_macros::aura_error_types;

aura_error_types! {
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub enum WotError {
        #[category = "authorization"]
        InvalidToken { details: String } =>
            "Invalid authorization token: {details}",

        #[category = "authorization"]
        InvalidCapabilities { details: String } =>
            "Invalid capabilities: {details}",

        #[category = "authorization"]
        PermissionDenied { details: String } =>
            "Permission denied: {details}",

        #[category = "system"]
        SystemError { details: String } =>
            "System error: {details}",
    }
}

/// WoT result type alias using macro-generated error
pub type WotResult<T> = Result<T, WotError>;
