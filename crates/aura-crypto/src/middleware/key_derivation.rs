//! TODO fix - Simplified Key Derivation Validation
//!
//! **CLEANUP**: TODO fix - Simplified from complex middleware with caching and rate limiting
//! to basic validation-only approach. Caching and rate limiting moved to higher layers.

use super::{CryptoContext, CryptoHandler, CryptoMiddleware, CryptoOperation};
use crate::{CryptoError, Result};

/// Basic key derivation configuration
#[derive(Debug, Clone)]
pub struct KeyDerivationConfig {
    /// Maximum app ID length for validation
    pub max_app_id_length: usize,
    /// Maximum context length for validation
    pub max_context_length: usize,
    /// Maximum derivation path depth
    pub max_derivation_depth: usize,
}

impl Default for KeyDerivationConfig {
    fn default() -> Self {
        Self {
            max_app_id_length: 128,
            max_context_length: 256,
            max_derivation_depth: 10,
        }
    }
}

/// TODO fix - Simplified key derivation middleware for basic validation
pub struct KeyDerivationMiddleware {
    config: KeyDerivationConfig,
}

impl KeyDerivationMiddleware {
    /// Create new key derivation middleware
    pub fn new(config: KeyDerivationConfig) -> Self {
        Self { config }
    }

    /// Validate key derivation parameters
    fn validate_derivation_params(
        &self,
        app_id: &str,
        context: &str,
        derivation_path: &[u32],
    ) -> Result<()> {
        // Validate app ID
        if app_id.is_empty() {
            return Err(CryptoError::invalid("App ID cannot be empty"));
        }
        if app_id.len() > self.config.max_app_id_length {
            return Err(CryptoError::invalid("App ID exceeds maximum length"));
        }
        if !app_id
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return Err(CryptoError::invalid("App ID contains invalid characters"));
        }

        // Validate context
        if context.len() > self.config.max_context_length {
            return Err(CryptoError::invalid("Context exceeds maximum length"));
        }

        // Validate derivation path
        if derivation_path.len() > self.config.max_derivation_depth {
            return Err(CryptoError::invalid(
                "Derivation path exceeds maximum depth",
            ));
        }

        Ok(())
    }
}

impl CryptoMiddleware for KeyDerivationMiddleware {
    fn process(
        &self,
        operation: CryptoOperation,
        context: &CryptoContext,
        next: &dyn CryptoHandler,
    ) -> Result<serde_json::Value> {
        match operation {
            CryptoOperation::DeriveKey {
                app_id,
                context: derivation_context,
                derivation_path,
            } => {
                // Validate parameters
                self.validate_derivation_params(&app_id, &derivation_context, &derivation_path)?;

                // Pass to next handler
                next.handle(
                    CryptoOperation::DeriveKey {
                        app_id,
                        context: derivation_context,
                        derivation_path,
                    },
                    context,
                )
            }
            other => next.handle(other, context),
        }
    }

    fn name(&self) -> &str {
        "KeyDerivationMiddleware"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_parameters() {
        let middleware = KeyDerivationMiddleware::new(KeyDerivationConfig::default());

        assert!(middleware
            .validate_derivation_params("test_app", "test_context", &[1, 2, 3])
            .is_ok());
    }

    #[test]
    fn test_invalid_app_id() {
        let middleware = KeyDerivationMiddleware::new(KeyDerivationConfig::default());

        // Empty app ID
        assert!(middleware
            .validate_derivation_params("", "context", &[1])
            .is_err());

        // Too long app ID
        let long_app_id = "a".repeat(200);
        assert!(middleware
            .validate_derivation_params(&long_app_id, "context", &[1])
            .is_err());

        // Invalid characters
        assert!(middleware
            .validate_derivation_params("app with spaces", "context", &[1])
            .is_err());
    }

    #[test]
    fn test_invalid_context() {
        let middleware = KeyDerivationMiddleware::new(KeyDerivationConfig::default());

        // Too long context
        let long_context = "a".repeat(300);
        assert!(middleware
            .validate_derivation_params("app", &long_context, &[1])
            .is_err());
    }

    #[test]
    fn test_invalid_derivation_path() {
        let middleware = KeyDerivationMiddleware::new(KeyDerivationConfig::default());

        // Too deep path
        let deep_path: Vec<u32> = (0..20).collect();
        assert!(middleware
            .validate_derivation_params("app", "context", &deep_path)
            .is_err());
    }
}
