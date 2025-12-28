//! Unified storage error handling using macro-generated error types.
//!
//! This module uses aura-macros to standardize error categories and messages
//! without relying on ad-hoc type aliases.

use aura_macros::aura_error_types;

aura_error_types! {
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub enum StorageError {
        #[category = "storage"]
        NotFound { key: String } => "Storage key not found: {key}",

        #[category = "storage"]
        ReadFailed { details: String } => "Storage read failed: {details}",

        #[category = "storage"]
        WriteFailed { details: String } => "Storage write failed: {details}",

        #[category = "storage"]
        DeleteFailed { details: String } => "Storage delete failed: {details}",

        #[category = "storage"]
        ListFailed { details: String } => "Storage list failed: {details}",

        #[category = "storage"]
        InvalidInput { details: String } => "Storage invalid input: {details}",
    }
}

/// Storage result type alias using macro-generated error
pub type StorageResult<T> = Result<T, StorageError>;
