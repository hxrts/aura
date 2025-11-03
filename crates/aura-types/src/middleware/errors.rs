//! Error handling middleware and error types

use super::{MiddlewareContext, AuraMiddleware};
use crate::effects::AuraEffects;
use crate::errors::AuraError;
use std::future::Future;
use std::pin::Pin;
use std::collections::HashMap;
use std::marker::PhantomData;

/// Error handling middleware
pub struct ErrorMiddleware<Req, Resp, Err>
where
    Req: Send + Sync,
    Resp: Send + Sync,
    Err: std::error::Error + Send + Sync + 'static,
{
    /// Error handler
    handler: Box<dyn ErrorHandler<Err>>,
    
    /// Error handling configuration
    config: ErrorConfig,
    
    /// Phantom data to use type parameters
    _phantom: PhantomData<(Req, Resp)>,
}

impl<Req, Resp, Err> ErrorMiddleware<Req, Resp, Err>
where
    Req: Send + Sync,
    Resp: Send + Sync,
    Err: std::error::Error + Send + Sync + 'static,
{
    /// Create new error handling middleware
    pub fn new(handler: Box<dyn ErrorHandler<Err>>) -> Self {
        Self {
            handler,
            config: ErrorConfig::default(),
            _phantom: PhantomData,
        }
    }

    /// Create error middleware with custom configuration
    pub fn with_config(handler: Box<dyn ErrorHandler<Err>>, config: ErrorConfig) -> Self {
        Self {
            handler,
            config,
            _phantom: PhantomData,
        }
    }
}

impl<Req, Resp, Err> AuraMiddleware for ErrorMiddleware<Req, Resp, Err>
where
    Req: Send + Sync + 'static,
    Resp: Send + Sync + 'static,
    Err: std::error::Error + Send + Sync + 'static,
{
    type Request = Req;
    type Response = Resp;
    type Error = Err;

    fn process(
        &self,
        request: Self::Request,
        context: &MiddlewareContext,
        effects: &dyn AuraEffects,
        next: Box<dyn super::traits::MiddlewareHandler<Self::Request, Self::Response, Self::Error>>,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + '_>> {
        let handler = &self.handler;
        let config = self.config.clone();
        let context = context.clone();

        Box::pin(async move {
            let start_time = std::time::Instant::now();
            
            // Execute the request
            let result = next.handle(request, &context, effects).await;
            
            match result {
                Ok(response) => Ok(response),
                Err(error) => {
                    let duration = start_time.elapsed();
                    let error_context = ErrorContext::new(&context, duration);
                    
                    // Handle the error
                    match handler.handle_error(error, &error_context, &config, effects).await {
                        Ok(_never) => unreachable!("Never type can never be constructed"),
                        Err(handled_error) => Err(handled_error),
                    }
                }
            }
        })
    }
}

/// Error handling configuration
#[derive(Debug, Clone)]
pub struct ErrorConfig {
    /// Whether to log errors
    pub log_errors: bool,
    
    /// Whether to include stack traces in error responses
    pub include_stack_traces: bool,
    
    /// Whether to include internal error details
    pub include_internal_details: bool,
    
    /// Maximum error message length
    pub max_error_message_length: Option<usize>,
    
    /// Whether to sanitize error messages
    pub sanitize_error_messages: bool,
    
    /// Error retry configuration
    pub retry_config: Option<RetryConfig>,
    
    /// Circuit breaker configuration
    pub circuit_breaker_config: Option<CircuitBreakerConfig>,
    
    /// Custom error mappings
    pub error_mappings: HashMap<String, ErrorMapping>,
}

impl Default for ErrorConfig {
    fn default() -> Self {
        Self {
            log_errors: true,
            include_stack_traces: false, // Don't expose stack traces by default
            include_internal_details: false, // Don't expose internal details by default
            max_error_message_length: Some(500),
            sanitize_error_messages: true,
            retry_config: Some(RetryConfig::default()),
            circuit_breaker_config: Some(CircuitBreakerConfig::default()),
            error_mappings: HashMap::new(),
        }
    }
}

/// Retry configuration
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retries
    pub max_retries: u32,
    
    /// Base delay between retries
    pub base_delay: std::time::Duration,
    
    /// Maximum delay between retries
    pub max_delay: std::time::Duration,
    
    /// Backoff multiplier
    pub backoff_multiplier: f64,
    
    /// Jitter to add to delays
    pub jitter: bool,
    
    /// Error types that should trigger retries
    pub retryable_errors: Vec<String>,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay: std::time::Duration::from_millis(100),
            max_delay: std::time::Duration::from_secs(10),
            backoff_multiplier: 2.0,
            jitter: true,
            retryable_errors: vec![
                "network_timeout".to_string(),
                "connection_refused".to_string(),
                "temporary_failure".to_string(),
            ],
        }
    }
}

/// Circuit breaker configuration
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Failure threshold to open circuit
    pub failure_threshold: u32,
    
    /// Time window for counting failures
    pub failure_window: std::time::Duration,
    
    /// Timeout before trying to close circuit
    pub timeout: std::time::Duration,
    
    /// Success threshold to close circuit
    pub success_threshold: u32,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            failure_window: std::time::Duration::from_secs(60),
            timeout: std::time::Duration::from_secs(30),
            success_threshold: 3,
        }
    }
}

/// Error mapping for custom error handling
#[derive(Debug, Clone)]
pub struct ErrorMapping {
    /// HTTP status code to map to
    pub status_code: Option<u16>,
    
    /// Custom error message
    pub message: Option<String>,
    
    /// Whether this error should be retried
    pub retryable: bool,
    
    /// Additional context to include
    pub additional_context: HashMap<String, String>,
}

/// Error context for error handling
#[derive(Debug, Clone)]
pub struct ErrorContext {
    /// Execution ID from middleware context
    pub execution_id: String,
    
    /// Component name
    pub component: String,
    
    /// Protocol being executed
    pub protocol: Option<String>,
    
    /// Session ID
    pub session_id: Option<String>,
    
    /// Device ID
    pub device_id: Option<String>,
    
    /// Error timestamp
    pub timestamp: std::time::Instant,
    
    /// Request duration when error occurred
    pub request_duration: std::time::Duration,
    
    /// Additional context fields
    pub fields: HashMap<String, String>,
}

impl ErrorContext {
    /// Create error context from middleware context
    pub fn new(context: &MiddlewareContext, request_duration: std::time::Duration) -> Self {
        Self {
            execution_id: context.execution_id.clone(),
            component: context.component.clone(),
            protocol: context.protocol.clone(),
            session_id: context.session_id.clone(),
            device_id: context.device_id.clone(),
            timestamp: std::time::Instant::now(),
            request_duration,
            fields: context.metadata.clone(),
        }
    }

    /// Add a field to the error context
    pub fn with_field(mut self, key: &str, value: &str) -> Self {
        self.fields.insert(key.to_string(), value.to_string());
        self
    }

    /// Convert to key-value pairs for logging
    pub fn to_fields(&self) -> Vec<(&str, String)> {
        let mut fields = Vec::new();
        
        fields.push(("execution_id", self.execution_id.clone()));
        fields.push(("component", self.component.clone()));
        fields.push(("request_duration_ms", self.request_duration.as_millis().to_string()));
        
        if let Some(protocol) = &self.protocol {
            fields.push(("protocol", protocol.clone()));
        }
        
        if let Some(session_id) = &self.session_id {
            fields.push(("session_id", session_id.clone()));
        }
        
        if let Some(device_id) = &self.device_id {
            fields.push(("device_id", device_id.clone()));
        }
        
        for (key, value) in &self.fields {
            fields.push((key, value.clone()));
        }
        
        fields
    }
}

/// Error handler trait
pub trait ErrorHandler<Err>: Send + Sync 
where
    Err: std::error::Error + Send + Sync + 'static,
{
    /// Handle an error
    fn handle_error(
        &self,
        error: Err,
        context: &ErrorContext,
        config: &ErrorConfig,
        effects: &dyn AuraEffects,
    ) -> Pin<Box<dyn Future<Output = Result<never::Never, Err>> + Send + '_>>;

    /// Check if an error is retryable
    fn is_retryable(&self, error: &Err, config: &ErrorConfig) -> bool {
        if let Some(retry_config) = &config.retry_config {
            let error_type = self.classify_error(error);
            retry_config.retryable_errors.contains(&error_type)
        } else {
            false
        }
    }

    /// Classify an error for handling
    fn classify_error(&self, error: &Err) -> String {
        // Default classification based on error message
        let error_str = error.to_string().to_lowercase();
        
        if error_str.contains("timeout") {
            "network_timeout".to_string()
        } else if error_str.contains("connection") {
            "connection_refused".to_string()
        } else if error_str.contains("temporary") {
            "temporary_failure".to_string()
        } else {
            "unknown_error".to_string()
        }
    }

    /// Sanitize error message for external consumption
    fn sanitize_error_message(&self, error: &Err, config: &ErrorConfig) -> String {
        let message = error.to_string();
        
        if !config.sanitize_error_messages {
            return message;
        }
        
        // Remove potentially sensitive information
        let sanitized = message
            .replace("password", "[REDACTED]")
            .replace("token", "[REDACTED]")
            .replace("secret", "[REDACTED]")
            .replace("key", "[REDACTED]");
        
        // Truncate if needed
        if let Some(max_len) = config.max_error_message_length {
            if sanitized.len() > max_len {
                format!("{}... [truncated]", &sanitized[..max_len])
            } else {
                sanitized
            }
        } else {
            sanitized
        }
    }
}

/// Default error handler implementation
pub struct DefaultErrorHandler;

impl<Err> ErrorHandler<Err> for DefaultErrorHandler
where
    Err: std::error::Error + Send + Sync + 'static,
{
    fn handle_error(
        &self,
        error: Err,
        context: &ErrorContext,
        config: &ErrorConfig,
        effects: &dyn AuraEffects,
    ) -> Pin<Box<dyn Future<Output = Result<never::Never, Err>> + Send + '_>> {
        Box::pin(async move {
            // Log the error if configured
            if config.log_errors {
                let fields = context.to_fields();
                let field_refs: Vec<(&str, &str)> = fields.iter()
                    .map(|(k, v)| (*k, v.as_str()))
                    .collect();
                
                effects.log_error(
                    &format!("Error in middleware: {}", error),
                    &field_refs
                );
            }

            // Check if error should be mapped
            let error_type = self.classify_error(&error);
            if let Some(mapping) = config.error_mappings.get(&error_type) {
                // Apply error mapping if configured
                effects.log_info(
                    &format!("Applied error mapping for {}", error_type),
                    &[("mapping", &format!("{:?}", mapping))]
                );
            }

            // Return the original error (no transformation in default handler)
            Err(error)
        })
    }
}

/// Middleware-specific error types
#[derive(Debug, thiserror::Error)]
pub enum MiddlewareError {
    #[error("Handler error: {message}")]
    HandlerError { message: String },
    
    #[error("Configuration error: {message}")]
    ConfigError { message: String },
    
    #[error("Timeout error: operation timed out after {duration:?}")]
    TimeoutError { duration: std::time::Duration },
    
    #[error("Circuit breaker is open")]
    CircuitBreakerOpen,
    
    #[error("Rate limit exceeded")]
    RateLimitExceeded,
    
    #[error("Authentication failed: {reason}")]
    AuthenticationFailed { reason: String },
    
    #[error("Authorization failed: {reason}")]
    AuthorizationFailed { reason: String },
    
    #[error("Validation error: {field} - {message}")]
    ValidationError { field: String, message: String },
    
    #[error("Resource limit exceeded: {resource} - {message}")]
    ResourceLimitExceeded { resource: String, message: String },
    
    #[error("Internal middleware error: {message}")]
    InternalError { message: String },
}

impl MiddlewareError {
    /// Create a handler error
    pub fn handler_error(message: &str) -> Self {
        Self::HandlerError { message: message.to_string() }
    }

    /// Create a configuration error
    pub fn config_error(message: &str) -> Self {
        Self::ConfigError { message: message.to_string() }
    }

    /// Create a timeout error
    pub fn timeout_error(duration: std::time::Duration) -> Self {
        Self::TimeoutError { duration }
    }

    /// Create an authentication error
    pub fn auth_failed(reason: &str) -> Self {
        Self::AuthenticationFailed { reason: reason.to_string() }
    }

    /// Create an authorization error
    pub fn authorization_failed(reason: &str) -> Self {
        Self::AuthorizationFailed { reason: reason.to_string() }
    }

    /// Create a validation error
    pub fn validation_error(field: &str, message: &str) -> Self {
        Self::ValidationError { 
            field: field.to_string(), 
            message: message.to_string() 
        }
    }

    /// Create a resource limit error
    pub fn resource_limit_exceeded(resource: &str, message: &str) -> Self {
        Self::ResourceLimitExceeded { 
            resource: resource.to_string(), 
            message: message.to_string() 
        }
    }

    /// Create an internal error
    pub fn internal_error(message: &str) -> Self {
        Self::InternalError { message: message.to_string() }
    }

    /// Check if this error is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(self, 
            MiddlewareError::TimeoutError { .. } |
            MiddlewareError::RateLimitExceeded |
            MiddlewareError::ResourceLimitExceeded { .. }
        )
    }

    /// Get error category for metrics and logging
    pub fn category(&self) -> &str {
        match self {
            MiddlewareError::HandlerError { .. } => "handler_error",
            MiddlewareError::ConfigError { .. } => "config_error",
            MiddlewareError::TimeoutError { .. } => "timeout_error",
            MiddlewareError::CircuitBreakerOpen => "circuit_breaker_error",
            MiddlewareError::RateLimitExceeded => "rate_limit_error",
            MiddlewareError::AuthenticationFailed { .. } => "auth_error",
            MiddlewareError::AuthorizationFailed { .. } => "authz_error",
            MiddlewareError::ValidationError { .. } => "validation_error",
            MiddlewareError::ResourceLimitExceeded { .. } => "resource_error",
            MiddlewareError::InternalError { .. } => "internal_error",
        }
    }
}

/// Handler error alias for common use
pub type HandlerError = MiddlewareError;

/// Convenience functions for creating error middleware
impl<Req, Resp, Err> ErrorMiddleware<Req, Resp, Err>
where
    Req: Send + Sync,
    Resp: Send + Sync,
    Err: std::error::Error + Send + Sync + 'static,
{
    /// Create error middleware with default handler and configuration
    pub fn default_config() -> Self {
        Self::new(Box::new(DefaultErrorHandler))
    }

    /// Create error middleware with custom retry configuration
    pub fn with_retries(max_retries: u32) -> Self {
        let mut config = ErrorConfig::default();
        if let Some(retry_config) = &mut config.retry_config {
            retry_config.max_retries = max_retries;
        }
        Self::with_config(Box::new(DefaultErrorHandler), config)
    }

    /// Create error middleware with circuit breaker disabled
    pub fn no_circuit_breaker() -> Self {
        let mut config = ErrorConfig::default();
        config.circuit_breaker_config = None;
        Self::with_config(Box::new(DefaultErrorHandler), config)
    }
}

/// Never type for type-level guarantees (errors always propagate)
mod never {
    #[derive(Debug)]
    pub enum Never {}
}