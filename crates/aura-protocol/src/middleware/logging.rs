//! Logging middleware for structured request/response logging

use super::{MiddlewareContext, AuraMiddleware};
use crate::effects::Effects;
use std::future::Future;
use std::pin::Pin;
use std::collections::HashMap;
use std::marker::PhantomData;

/// Logging middleware for structured logging
pub struct LoggingMiddleware<Req, Resp, Err>
where
    Req: Send + Sync,
    Resp: Send + Sync,
    Err: std::error::Error + Send + Sync + 'static,
{
    /// Structured logger
    logger: Box<dyn StructuredLogger>,
    
    /// Logging configuration
    config: LoggingConfig,
    
    /// Phantom data to use type parameters
    _phantom: PhantomData<(Req, Resp, Err)>,
}

impl<Req, Resp, Err> LoggingMiddleware<Req, Resp, Err>
where
    Req: Send + Sync,
    Resp: Send + Sync,
    Err: std::error::Error + Send + Sync + 'static,
{
    /// Create new logging middleware
    pub fn new(logger: Box<dyn StructuredLogger>) -> Self {
        Self {
            logger,
            config: LoggingConfig::default(),
            _phantom: PhantomData,
        }
    }

    /// Create logging middleware with custom configuration
    pub fn with_config(logger: Box<dyn StructuredLogger>, config: LoggingConfig) -> Self {
        Self {
            logger,
            config,
            _phantom: PhantomData,
        }
    }
}

impl<Req, Resp, Err> AuraMiddleware for LoggingMiddleware<Req, Resp, Err>
where
    Req: Send + Sync + std::fmt::Debug + 'static,
    Resp: Send + Sync + std::fmt::Debug + 'static,
    Err: std::error::Error + Send + Sync + 'static,
{
    type Request = Req;
    type Response = Resp;
    type Error = Err;

    fn process<'a>(
        &'a self,
        request: Self::Request,
        context: &'a MiddlewareContext,
        effects: &'a dyn Effects,
        next: Box<dyn super::traits::MiddlewareHandler<Self::Request, Self::Response, Self::Error>>,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'a>> {
        let logger = &self.logger;
        let config = self.config.clone();
        let context = context.clone();
        let start_time = std::time::Instant::now();

        Box::pin(async move {
            // Create log context
            let log_context = LogContext::new(&context);

            // Log request start
            if config.log_requests {
                let request_debug = format!("{:?}", request);
                logger.log_request(&request_debug, &log_context, effects);
            }

            // Execute the request
            let result = next.handle(request, &context, effects).await;
            
            let duration = start_time.elapsed();
            
            // Log response or error
            match &result {
                Ok(response) => {
                    if config.log_responses {
                        let response_debug = format!("{:?}", response);
                        logger.log_response(&response_debug, &log_context, duration, effects);
                    }
                    
                    if config.log_request_summary {
                        logger.log_info(
                            "Request completed successfully",
                            &[
                                ("duration_ms", &duration.as_millis().to_string()),
                                ("component", &context.component),
                                ("execution_id", &context.execution_id),
                            ],
                            effects
                        );
                    }
                }
                Err(error) => {
                    if config.log_errors {
                        let error_debug = format!("{:?}", error);
                        logger.log_error(&error_debug, &log_context, duration, effects);
                    }
                    
                    if config.log_request_summary {
                        logger.log_error_msg(
                            &format!("Request failed: {}", error),
                            &[
                                ("duration_ms", &duration.as_millis().to_string()),
                                ("component", &context.component),
                                ("execution_id", &context.execution_id),
                                ("error_type", &error.to_string()),
                            ],
                            effects
                        );
                    }
                }
            }

            result
        })
    }
}

/// Logging configuration
#[derive(Debug, Clone)]
pub struct LoggingConfig {
    /// Whether to log incoming requests
    pub log_requests: bool,
    
    /// Whether to log outgoing responses
    pub log_responses: bool,
    
    /// Whether to log errors
    pub log_errors: bool,
    
    /// Whether to log request summaries
    pub log_request_summary: bool,
    
    /// Minimum log level
    pub min_level: LogLevel,
    
    /// Whether to include request/response bodies
    pub include_bodies: bool,
    
    /// Whether to include request headers
    pub include_headers: bool,
    
    /// Whether to sanitize sensitive data
    pub sanitize_sensitive: bool,
    
    /// Fields to sanitize (will be replaced with [REDACTED])
    pub sensitive_fields: Vec<String>,
    
    /// Maximum body length to log
    pub max_body_length: Option<usize>,
    
    /// Additional context fields to include
    pub additional_fields: HashMap<String, String>,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            log_requests: true,
            log_responses: true,
            log_errors: true,
            log_request_summary: true,
            min_level: LogLevel::Info,
            include_bodies: false,  // Don't log bodies by default for privacy
            include_headers: false, // Don't log headers by default for privacy
            sanitize_sensitive: true,
            sensitive_fields: vec![
                "password".to_string(),
                "token".to_string(),
                "secret".to_string(),
                "key".to_string(),
                "auth".to_string(),
                "authorization".to_string(),
                "api_key".to_string(),
                "private_key".to_string(),
            ],
            max_body_length: Some(1024), // 1KB max
            additional_fields: HashMap::new(),
        }
    }
}

/// Log levels
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Trace = 0,
    Debug = 1,
    Info = 2,
    Warn = 3,
    Error = 4,
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Trace => write!(f, "TRACE"),
            LogLevel::Debug => write!(f, "DEBUG"),
            LogLevel::Info => write!(f, "INFO"),
            LogLevel::Warn => write!(f, "WARN"),
            LogLevel::Error => write!(f, "ERROR"),
        }
    }
}

/// Log context for structured logging
#[derive(Debug, Clone)]
pub struct LogContext {
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
    
    /// Request timestamp
    pub timestamp: std::time::Instant,
    
    /// Additional context fields
    pub fields: HashMap<String, String>,
}

impl LogContext {
    /// Create log context from middleware context
    pub fn new(context: &MiddlewareContext) -> Self {
        Self {
            execution_id: context.execution_id.clone(),
            component: context.component.clone(),
            protocol: context.protocol.clone(),
            session_id: context.session_id.clone(),
            device_id: context.device_id.clone(),
            timestamp: context.timestamp,
            fields: context.metadata.clone(),
        }
    }

    /// Add a field to the log context
    pub fn with_field(mut self, key: &str, value: &str) -> Self {
        self.fields.insert(key.to_string(), value.to_string());
        self
    }

    /// Get elapsed time since context creation
    pub fn elapsed(&self) -> std::time::Duration {
        self.timestamp.elapsed()
    }

    /// Convert to key-value pairs for logging  
    pub fn to_fields(&self) -> Vec<(&str, String)> {
        let mut fields = Vec::new();
        
        fields.push(("execution_id", self.execution_id.clone()));
        fields.push(("component", self.component.clone()));
        
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

/// Structured logger trait
pub trait StructuredLogger: Send + Sync {
    /// Log a request (takes formatted string to avoid generics)
    fn log_request(
        &self,
        request_debug: &str,
        context: &LogContext,
        effects: &dyn Effects,
    );

    /// Log a response (takes formatted string to avoid generics)
    fn log_response(
        &self,
        response_debug: &str,
        context: &LogContext,
        duration: std::time::Duration,
        effects: &dyn Effects,
    );

    /// Log an error (takes formatted string to avoid generics)
    fn log_error(
        &self,
        error_debug: &str,
        context: &LogContext,
        duration: std::time::Duration,
        effects: &dyn Effects,
    );

    /// Log at info level
    fn log_info(
        &self,
        message: &str,
        fields: &[(&str, &str)],
        effects: &dyn Effects,
    );

    /// Log at warn level
    fn log_warn(
        &self,
        message: &str,
        fields: &[(&str, &str)],
        effects: &dyn Effects,
    );

    /// Log at error level
    fn log_error_msg(
        &self,
        message: &str,
        fields: &[(&str, &str)],
        effects: &dyn Effects,
    );

    /// Log at debug level
    fn log_debug(
        &self,
        message: &str,
        fields: &[(&str, &str)],
        effects: &dyn Effects,
    );

    /// Log at trace level
    fn log_trace(
        &self,
        message: &str,
        fields: &[(&str, &str)],
        effects: &dyn Effects,
    );

    /// Set minimum log level
    fn set_min_level(&mut self, level: LogLevel);

    /// Check if a log level is enabled
    fn is_enabled(&self, level: LogLevel) -> bool;
}

/// Default structured logger implementation
pub struct DefaultStructuredLogger {
    /// Minimum log level
    min_level: LogLevel,
    
    /// Logging configuration
    config: LoggingConfig,
}

impl DefaultStructuredLogger {
    /// Create a new default structured logger
    pub fn new(config: LoggingConfig) -> Self {
        let min_level = config.min_level.clone();
        Self {
            min_level,
            config,
        }
    }

    /// Sanitize sensitive data from a string
    fn sanitize_data(&self, data: &str) -> String {
        if !self.config.sanitize_sensitive {
            return data.to_string();
        }

        let mut sanitized = data.to_string();
        for field in &self.config.sensitive_fields {
            // Simple pattern matching - in real implementation would use regex
            if sanitized.to_lowercase().contains(&field.to_lowercase()) {
                sanitized = sanitized.replace(field, "[REDACTED]");
            }
        }
        sanitized
    }

    /// Truncate data if it exceeds max length
    fn truncate_data(&self, data: &str) -> String {
        if let Some(max_len) = self.config.max_body_length {
            if data.len() > max_len {
                format!("{}... [truncated]", &data[..max_len])
            } else {
                data.to_string()
            }
        } else {
            data.to_string()
        }
    }
}

impl StructuredLogger for DefaultStructuredLogger {
    fn log_request(
        &self,
        request_debug: &str,
        context: &LogContext,
        effects: &dyn Effects,
    ) {
        if !self.is_enabled(LogLevel::Info) {
            return;
        }

        let mut fields = context.to_fields();
        fields.push(("event_type", "request".to_string()));
        
        if self.config.include_bodies {
            let sanitized = self.sanitize_data(request_debug);
            let truncated = self.truncate_data(&sanitized);
            fields.push(("request_body", truncated));
        }

        // Use console effects to log  
        let field_refs: Vec<(&str, &str)> = fields.iter()
            .map(|(k, v)| (*k, v.as_str()))
            .collect();
        effects.log_info(&format!("Processing request"), &field_refs);
    }

    fn log_response(
        &self,
        response_debug: &str,
        context: &LogContext,
        duration: std::time::Duration,
        effects: &dyn Effects,
    ) {
        if !self.is_enabled(LogLevel::Info) {
            return;
        }

        let mut fields = context.to_fields();
        fields.push(("event_type", "response".to_string()));
        fields.push(("duration_ms", duration.as_millis().to_string()));
        
        if self.config.include_bodies {
            let sanitized = self.sanitize_data(response_debug);
            let truncated = self.truncate_data(&sanitized);
            fields.push(("response_body", truncated));
        }

        let field_refs: Vec<(&str, &str)> = fields.iter()
            .map(|(k, v)| (*k, v.as_str()))
            .collect();
        effects.log_info(&format!("Request completed"), &field_refs);
    }

    fn log_error(
        &self,
        error_debug: &str,
        context: &LogContext,
        duration: std::time::Duration,
        effects: &dyn Effects,
    ) {
        if !self.is_enabled(LogLevel::Error) {
            return;
        }

        let mut fields = context.to_fields();
        fields.push(("event_type", "error".to_string()));
        fields.push(("duration_ms", duration.as_millis().to_string()));
        fields.push(("error_message", error_debug.to_string()));

        let field_refs: Vec<(&str, &str)> = fields.iter()
            .map(|(k, v)| (*k, v.as_str()))
            .collect();
        effects.log_error(&format!("Request failed: {}", error_debug), &field_refs);
    }

    fn log_info(&self, message: &str, fields: &[(&str, &str)], effects: &dyn Effects) {
        if self.is_enabled(LogLevel::Info) {
            effects.log_info(message, fields);
        }
    }

    fn log_warn(&self, message: &str, fields: &[(&str, &str)], effects: &dyn Effects) {
        if self.is_enabled(LogLevel::Warn) {
            effects.log_warn(message, fields);
        }
    }

    fn log_error_msg(&self, message: &str, fields: &[(&str, &str)], effects: &dyn Effects) {
        if self.is_enabled(LogLevel::Error) {
            effects.log_error(message, fields);
        }
    }

    fn log_debug(&self, message: &str, fields: &[(&str, &str)], effects: &dyn Effects) {
        if self.is_enabled(LogLevel::Debug) {
            effects.log_debug(message, fields);
        }
    }

    fn log_trace(&self, message: &str, fields: &[(&str, &str)], effects: &dyn Effects) {
        if self.is_enabled(LogLevel::Trace) {
            effects.log_trace(message, fields);
        }
    }

    fn set_min_level(&mut self, level: LogLevel) {
        self.min_level = level;
    }

    fn is_enabled(&self, level: LogLevel) -> bool {
        level >= self.min_level
    }
}

/// Convenience functions for creating logging middleware
impl<Req, Resp, Err> LoggingMiddleware<Req, Resp, Err>
where
    Req: Send + Sync + std::fmt::Debug,
    Resp: Send + Sync + std::fmt::Debug,
    Err: std::error::Error + Send + Sync + 'static,
{
    /// Create logging middleware with default configuration
    pub fn default_config() -> Self {
        let config = LoggingConfig::default();
        let logger = Box::new(DefaultStructuredLogger::new(config.clone()));
        Self::new(logger)
    }

    /// Create logging middleware for specific component
    pub fn for_component(component: &str) -> Self {
        let mut config = LoggingConfig::default();
        config.additional_fields.insert("component".to_string(), component.to_string());
        let logger = Box::new(DefaultStructuredLogger::new(config.clone()));
        Self::with_config(logger, config)
    }

    /// Create logging middleware with minimal logging (errors only)
    pub fn minimal() -> Self {
        let mut config = LoggingConfig::default();
        config.log_requests = false;
        config.log_responses = false;
        config.log_request_summary = false;
        config.min_level = LogLevel::Error;
        let logger = Box::new(DefaultStructuredLogger::new(config.clone()));
        Self::with_config(logger, config)
    }

    /// Create logging middleware with verbose logging
    pub fn verbose() -> Self {
        let mut config = LoggingConfig::default();
        config.include_bodies = true;
        config.include_headers = true;
        config.min_level = LogLevel::Debug;
        let logger = Box::new(DefaultStructuredLogger::new(config.clone()));
        Self::with_config(logger, config)
    }
}