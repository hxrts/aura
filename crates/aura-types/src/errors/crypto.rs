//! Cryptographic operation errors

use super::{ErrorCode, ErrorContext, ErrorSeverity};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Cryptographic operation failures
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CryptoError {
    /// Key generation failed
    KeyGenerationFailed {
        reason: String,
        key_type: Option<String>,
        context: ErrorContext,
    },
    
    /// Key derivation failed
    KeyDerivationFailed {
        reason: String,
        path: Option<String>,
        context: ErrorContext,
    },
    
    /// Signature operation failed
    SignatureFailed {
        reason: String,
        operation: Option<String>,
        context: ErrorContext,
    },
    
    /// Encryption operation failed
    EncryptionFailed {
        reason: String,
        algorithm: Option<String>,
        context: ErrorContext,
    },
    
    /// Decryption operation failed
    DecryptionFailed {
        reason: String,
        algorithm: Option<String>,
        context: ErrorContext,
    },
    
    /// Key storage operation failed
    KeyStorageFailed {
        reason: String,
        operation: Option<String>,
        context: ErrorContext,
    },
    
    /// Threshold cryptography operation failed
    ThresholdOperationFailed {
        reason: String,
        threshold: Option<u16>,
        shares: Option<u16>,
        context: ErrorContext,
    },
    
    /// Invalid cryptographic material
    InvalidCryptoMaterial {
        material_type: String,
        reason: String,
        context: ErrorContext,
    },
}

impl fmt::Display for CryptoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KeyGenerationFailed { reason, key_type, .. } => {
                write!(f, "Key generation failed: {}", reason)?;
                if let Some(kt) = key_type {
                    write!(f, " (type: {})", kt)?;
                }
                Ok(())
            }
            Self::KeyDerivationFailed { reason, path, .. } => {
                write!(f, "Key derivation failed: {}", reason)?;
                if let Some(p) = path {
                    write!(f, " (path: {})", p)?;
                }
                Ok(())
            }
            Self::SignatureFailed { reason, operation, .. } => {
                write!(f, "Signature operation failed: {}", reason)?;
                if let Some(op) = operation {
                    write!(f, " (operation: {})", op)?;
                }
                Ok(())
            }
            Self::EncryptionFailed { reason, algorithm, .. } => {
                write!(f, "Encryption failed: {}", reason)?;
                if let Some(alg) = algorithm {
                    write!(f, " (algorithm: {})", alg)?;
                }
                Ok(())
            }
            Self::DecryptionFailed { reason, algorithm, .. } => {
                write!(f, "Decryption failed: {}", reason)?;
                if let Some(alg) = algorithm {
                    write!(f, " (algorithm: {})", alg)?;
                }
                Ok(())
            }
            Self::KeyStorageFailed { reason, operation, .. } => {
                write!(f, "Key storage failed: {}", reason)?;
                if let Some(op) = operation {
                    write!(f, " (operation: {})", op)?;
                }
                Ok(())
            }
            Self::ThresholdOperationFailed { reason, threshold, shares, .. } => {
                write!(f, "Threshold operation failed: {}", reason)?;
                if let (Some(t), Some(s)) = (threshold, shares) {
                    write!(f, " (threshold: {}/{} shares)", t, s)?;
                }
                Ok(())
            }
            Self::InvalidCryptoMaterial { material_type, reason, .. } => {
                write!(f, "Invalid {} material: {}", material_type, reason)
            }
        }
    }
}

impl CryptoError {
    /// Get the error code for this error
    pub fn error_code(&self) -> ErrorCode {
        match self {
            Self::KeyGenerationFailed { .. } => ErrorCode::KeyGenerationFailed,
            Self::KeyDerivationFailed { .. } => ErrorCode::KeyDerivationFailed,
            Self::SignatureFailed { .. } => ErrorCode::SignatureFailed,
            Self::EncryptionFailed { .. } => ErrorCode::EncryptionFailed,
            Self::DecryptionFailed { .. } => ErrorCode::DecryptionFailed,
            Self::KeyStorageFailed { .. } => ErrorCode::KeyStorageFailed,
            Self::ThresholdOperationFailed { .. } => ErrorCode::ThresholdOperationFailed,
            Self::InvalidCryptoMaterial { .. } => ErrorCode::InvalidCryptoMaterial,
        }
    }

    /// Get the severity of this error
    pub fn severity(&self) -> ErrorSeverity {
        match self {
            Self::KeyGenerationFailed { .. } => ErrorSeverity::Critical,
            Self::KeyDerivationFailed { .. } => ErrorSeverity::High,
            Self::SignatureFailed { .. } => ErrorSeverity::High,
            Self::EncryptionFailed { .. } => ErrorSeverity::High,
            Self::DecryptionFailed { .. } => ErrorSeverity::High,
            Self::KeyStorageFailed { .. } => ErrorSeverity::Critical,
            Self::ThresholdOperationFailed { .. } => ErrorSeverity::High,
            Self::InvalidCryptoMaterial { .. } => ErrorSeverity::Medium,
        }
    }
}