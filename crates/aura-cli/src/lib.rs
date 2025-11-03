//! Aura CLI Library
//!
//! This crate implements CLI functionality through composable middleware layers that handle:
//! - Input validation and sanitization
//! - Output formatting and presentation
//! - Progress reporting for long-running operations
//! - Comprehensive error handling and recovery
//! - Configuration loading and management
//! - Authentication and authorization
//!
//! All CLI functionality is now implemented as middleware components
//! following Aura's foundation pattern for algebraic effect composition.

#![allow(missing_docs)]

pub mod middleware;

// Re-export core middleware types for external usage
pub use middleware::{
    CliMiddlewareStack,
    CliStackBuilder,
    CliHandler,
    CliOperation,
    CliContext,
    CliConfig,
    InputValidationMiddleware,
    OutputFormattingMiddleware,
    ProgressReportingMiddleware,
    ErrorHandlingMiddleware,
    ConfigurationMiddleware,
    AuthenticationMiddleware,
};

/// CLI error types
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("Command not found: {0}")]
    CommandNotFound(String),
    
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    
    #[error("Configuration error: {0}")]
    Configuration(String),
    
    #[error("File system error: {0}")]
    FileSystem(String),
    
    #[error("Serialization error: {0}")]
    Serialization(String),
    
    #[error("Not implemented: {0}")]
    NotImplemented(String),
    
    #[error("Authentication error: {0}")]
    Authentication(String),
    
    #[error("Network error: {0}")]
    Network(String),
    
    #[error("Operation failed: {0}")]
    OperationFailed(String),
}

pub type Result<T> = std::result::Result<T, CliError>;
