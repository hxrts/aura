//! Runtime Service Traits
//!
//! Unified lifecycle management for runtime services. All service managers
//! implement `RuntimeService` for consistent startup, shutdown, and health
//! monitoring.
//!
//! ## Design Principles
//!
//! 1. **Uniform Lifecycle**: All services follow the same start/stop pattern
//! 2. **Dependency Ordering**: Services declare dependencies for ordered startup
//! 3. **Health Monitoring**: Consistent health check interface
//! 4. **Graceful Shutdown**: Services can clean up resources properly

use async_trait::async_trait;
use std::fmt;
use std::sync::Arc;

use super::RuntimeTaskRegistry;

/// Health status of a runtime service
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceHealth {
    /// Service is operating normally
    Healthy,
    /// Service is operational but experiencing issues
    Degraded {
        /// Reason for degraded state
        reason: String,
    },
    /// Service is not operational
    Unhealthy {
        /// Reason for unhealthy state
        reason: String,
    },
    /// Service has not been started
    NotStarted,
    /// Service is starting up
    Starting,
    /// Service is shutting down
    Stopping,
    /// Service has been stopped
    Stopped,
}

impl ServiceHealth {
    /// Returns true if the service is healthy
    pub fn is_healthy(&self) -> bool {
        matches!(self, ServiceHealth::Healthy)
    }

    /// Returns true if the service is operational (healthy or degraded)
    pub fn is_operational(&self) -> bool {
        matches!(self, ServiceHealth::Healthy | ServiceHealth::Degraded { .. })
    }
}

impl fmt::Display for ServiceHealth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServiceHealth::Healthy => write!(f, "healthy"),
            ServiceHealth::Degraded { reason } => write!(f, "degraded: {}", reason),
            ServiceHealth::Unhealthy { reason } => write!(f, "unhealthy: {}", reason),
            ServiceHealth::NotStarted => write!(f, "not started"),
            ServiceHealth::Starting => write!(f, "starting"),
            ServiceHealth::Stopping => write!(f, "stopping"),
            ServiceHealth::Stopped => write!(f, "stopped"),
        }
    }
}

/// Error kinds for service operations
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceErrorKind {
    /// Service failed to start
    StartupFailed,
    /// Service failed to stop gracefully
    ShutdownFailed,
    /// Service configuration is invalid
    InvalidConfiguration,
    /// A required dependency is not available
    DependencyUnavailable,
    /// Service is unavailable or disabled
    Unavailable,
    /// Service encountered an internal error
    Internal,
    /// Service operation timed out
    Timeout,
}

/// Error from a service operation
#[allow(dead_code)]
#[derive(Debug)]
pub struct ServiceError {
    /// Name of the service that encountered the error
    pub service: &'static str,
    /// Kind of error
    pub kind: ServiceErrorKind,
    /// Human-readable error message
    pub message: String,
    /// Optional underlying cause
    pub cause: Option<Box<dyn std::error::Error + Send + Sync>>,
}

#[allow(dead_code)]
impl ServiceError {
    /// Create a new service error
    pub fn new(service: &'static str, kind: ServiceErrorKind, message: impl Into<String>) -> Self {
        Self {
            service,
            kind,
            message: message.into(),
            cause: None,
        }
    }

    /// Create a startup failure error
    pub fn startup_failed(service: &'static str, message: impl Into<String>) -> Self {
        Self::new(service, ServiceErrorKind::StartupFailed, message)
    }

    /// Create a shutdown failure error
    pub fn shutdown_failed(service: &'static str, message: impl Into<String>) -> Self {
        Self::new(service, ServiceErrorKind::ShutdownFailed, message)
    }

    /// Create an internal error
    pub fn internal(service: &'static str, message: impl Into<String>) -> Self {
        Self::new(service, ServiceErrorKind::Internal, message)
    }

    /// Create an unavailable service error
    pub fn unavailable(service: &'static str, message: impl Into<String>) -> Self {
        Self::new(service, ServiceErrorKind::Unavailable, message)
    }

    /// Add a cause to this error
    pub fn with_cause(mut self, cause: impl std::error::Error + Send + Sync + 'static) -> Self {
        self.cause = Some(Box::new(cause));
        self
    }
}

impl fmt::Display for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {:?}: {}", self.service, self.kind, self.message)?;
        if let Some(cause) = &self.cause {
            write!(f, " (caused by: {})", cause)?;
        }
        Ok(())
    }
}

impl std::error::Error for ServiceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.cause
            .as_ref()
            .map(|c| c.as_ref() as &(dyn std::error::Error + 'static))
    }
}

/// Trait for runtime services with unified lifecycle management
///
/// Note: This trait is infrastructure for future lifecycle management integration.
/// Services will implement this trait incrementally to enable unified startup/shutdown.
///
/// All service managers in the runtime should implement this trait to enable:
/// - Consistent startup/shutdown ordering
/// - Health monitoring and reporting
/// - Dependency management
///
/// ## Example
///
/// ```ignore
/// use aura_agent::runtime::services::{RuntimeService, ServiceHealth, ServiceError};
///
/// struct MyService { /* ... */ }
///
/// #[async_trait]
/// impl RuntimeService for MyService {
///     fn name(&self) -> &'static str {
///         "my_service"
///     }
///
///     fn dependencies(&self) -> &[&'static str] {
///         &["indexed_journal", "transport"]
///     }
///
///     async fn start(&self, tasks: Arc<RuntimeTaskRegistry>) -> Result<(), ServiceError> {
///         // Initialize and start background tasks
///         Ok(())
///     }
///
///     async fn stop(&self) -> Result<(), ServiceError> {
///         // Graceful shutdown
///         Ok(())
///     }
///
///     fn health(&self) -> ServiceHealth {
///         ServiceHealth::Healthy
///     }
/// }
/// ```
#[allow(dead_code)]
#[async_trait]
pub trait RuntimeService: Send + Sync {
    /// Returns the unique name of this service
    ///
    /// Used for logging, metrics, and dependency resolution.
    fn name(&self) -> &'static str;

    /// Returns the names of services this service depends on
    ///
    /// Dependencies will be started before this service and stopped after.
    /// Return an empty slice if there are no dependencies.
    fn dependencies(&self) -> &[&'static str] {
        &[]
    }

    /// Start the service
    ///
    /// Called during runtime startup. The service should initialize any
    /// required state and spawn background tasks using the provided
    /// task registry.
    ///
    /// # Arguments
    /// * `tasks` - Task registry for spawning background tasks
    ///
    /// # Errors
    /// Returns `ServiceError` if startup fails
    async fn start(&self, tasks: Arc<RuntimeTaskRegistry>) -> Result<(), ServiceError>;

    /// Stop the service gracefully
    ///
    /// Called during runtime shutdown. The service should:
    /// 1. Stop accepting new work
    /// 2. Complete or cancel in-progress operations
    /// 3. Release resources
    ///
    /// Background tasks spawned via `RuntimeTaskRegistry` are automatically
    /// cancelled, but the service may need to perform additional cleanup.
    ///
    /// # Errors
    /// Returns `ServiceError` if shutdown fails
    async fn stop(&self) -> Result<(), ServiceError>;

    /// Returns the current health status of the service
    ///
    /// Called periodically for health monitoring. Should be fast and
    /// non-blocking.
    fn health(&self) -> ServiceHealth;
}

/// Extension trait for collections of runtime services
#[allow(dead_code)]
pub trait RuntimeServiceCollection {
    /// Get a service by name
    fn get_service(&self, name: &str) -> Option<&dyn RuntimeService>;

    /// Get all services sorted by dependency order (dependencies first)
    fn services_in_start_order(&self) -> Vec<&dyn RuntimeService>;

    /// Get all services sorted by reverse dependency order (dependents first)
    fn services_in_stop_order(&self) -> Vec<&dyn RuntimeService>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_health_display() {
        assert_eq!(format!("{}", ServiceHealth::Healthy), "healthy");
        assert_eq!(
            format!(
                "{}",
                ServiceHealth::Degraded {
                    reason: "high load".to_string()
                }
            ),
            "degraded: high load"
        );
    }

    #[test]
    fn test_service_health_checks() {
        assert!(ServiceHealth::Healthy.is_healthy());
        assert!(ServiceHealth::Healthy.is_operational());

        let degraded = ServiceHealth::Degraded {
            reason: "test".to_string(),
        };
        assert!(!degraded.is_healthy());
        assert!(degraded.is_operational());

        let unhealthy = ServiceHealth::Unhealthy {
            reason: "test".to_string(),
        };
        assert!(!unhealthy.is_healthy());
        assert!(!unhealthy.is_operational());
    }

    #[test]
    fn test_service_error_display() {
        let err = ServiceError::startup_failed("test_service", "failed to connect");
        assert!(err.to_string().contains("test_service"));
        assert!(err.to_string().contains("StartupFailed"));
        assert!(err.to_string().contains("failed to connect"));
    }
}
