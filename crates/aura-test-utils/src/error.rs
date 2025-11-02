//! Test utilities error types
//!
//! Defines specific error types for test utilities to replace generic Box<dyn Error> usage

use thiserror::Error;

/// Errors that can occur in test utilities
#[derive(Error, Debug)]
pub enum TestError {
    /// Error from journal operations
    #[error("Journal error: {0}")]
    Journal(#[from] aura_journal::AuraError),
    
    /// Error from protocol operations  
    #[error("Protocol error: {0}")]
    Protocol(#[from] aura_protocol::AuraError),
    
    /// Error from crypto operations
    #[error("Crypto error: {0}")]
    Crypto(#[from] aura_crypto::CryptoError),
    
    /// Error from types operations
    #[error("Types error: {0}")]
    Types(#[from] aura_types::AuraError),
    
    /// Configuration error
    #[error("Configuration error: {message}")]
    Configuration { message: String },
    
    /// Invalid test setup
    #[error("Invalid test setup: {message}")]
    InvalidSetup { message: String },
    
    /// Test fixture creation failed
    #[error("Test fixture creation failed: {message}")]
    FixtureCreation { message: String },
    
    /// IO error during test operations
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    /// Generic test error for cases where specific typing isn't worth it
    #[error("Test error: {message}")]
    Generic { message: String },
}

impl TestError {
    /// Create a configuration error
    pub fn configuration(message: impl Into<String>) -> Self {
        Self::Configuration {
            message: message.into(),
        }
    }
    
    /// Create an invalid setup error
    pub fn invalid_setup(message: impl Into<String>) -> Self {
        Self::InvalidSetup {
            message: message.into(),
        }
    }
    
    /// Create a fixture creation error
    pub fn fixture_creation(message: impl Into<String>) -> Self {
        Self::FixtureCreation {
            message: message.into(),
        }
    }
    
    /// Create a generic test error
    pub fn generic(message: impl Into<String>) -> Self {
        Self::Generic {
            message: message.into(),
        }
    }
}

/// Result type for test utilities
pub type TestResult<T> = Result<T, TestError>;