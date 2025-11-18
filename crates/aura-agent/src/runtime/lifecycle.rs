//! Lifecycle management for AuraEffectSystem
//!
//! This module provides explicit lifecycle state management for effect systems,
//! ensuring proper initialization, health monitoring, and graceful shutdown.

use std::sync::{
    atomic::{AtomicU8, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, error, info, warn};

use aura_core::{AuraError, AuraResult, DeviceId};

/// Lifecycle states for the effect system
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum EffectSystemState {
    /// System has been created but not initialized
    Uninitialized = 0,
    /// System is in the process of initializing
    Initializing = 1,
    /// System is ready for use
    Ready = 2,
    /// System is in the process of shutting down
    ShuttingDown = 3,
    /// System has been shut down
    Shutdown = 4,
}

impl EffectSystemState {
    /// Convert from u8 representation
    fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Uninitialized,
            1 => Self::Initializing,
            2 => Self::Ready,
            3 => Self::ShuttingDown,
            4 => Self::Shutdown,
            _ => Self::Uninitialized, // Default fallback
        }
    }

    /// Check if the system is operational
    pub fn is_operational(&self) -> bool {
        matches!(self, Self::Ready)
    }

    /// Check if the system can transition to a new state
    pub fn can_transition_to(&self, target: Self) -> bool {
        match (*self, target) {
            // Uninitialized can only go to Initializing
            (Self::Uninitialized, Self::Initializing) => true,

            // Initializing can go to Ready or ShuttingDown (on init failure)
            (Self::Initializing, Self::Ready) => true,
            (Self::Initializing, Self::ShuttingDown) => true,

            // Ready can only go to ShuttingDown
            (Self::Ready, Self::ShuttingDown) => true,

            // ShuttingDown can only go to Shutdown
            (Self::ShuttingDown, Self::Shutdown) => true,

            // All other transitions are invalid
            _ => false,
        }
    }
}

/// Lifecycle-aware trait for handlers and services
#[async_trait]
pub trait LifecycleAware: Send + Sync {
    /// Called during system initialization
    async fn on_initialize(&self) -> AuraResult<()> {
        Ok(())
    }

    /// Called during system shutdown
    async fn on_shutdown(&self) -> AuraResult<()> {
        Ok(())
    }

    /// Health check for this component
    async fn health_check(&self, now: Instant) -> HealthStatus {
        HealthStatus::healthy(now)
    }
}

/// Health status for system components
#[derive(Debug, Clone)]
pub struct HealthStatus {
    /// Whether the component is healthy
    pub is_healthy: bool,
    /// Optional message describing the health state
    pub message: Option<String>,
    /// Optional metadata about the health check
    pub metadata: Option<serde_json::Value>,
    /// Timestamp of the health check
    pub checked_at: Instant,
}

impl HealthStatus {
    /// Create a healthy status
    pub fn healthy(now: Instant) -> Self {
        Self {
            is_healthy: true,
            message: None,
            metadata: None,
            checked_at: now,
        }
    }

    /// Create an unhealthy status with a message
    pub fn unhealthy(message: impl Into<String>, now: Instant) -> Self {
        Self {
            is_healthy: false,
            message: Some(message.into()),
            metadata: None,
            checked_at: now,
        }
    }

    /// Add metadata to the health status
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

/// System-wide health report
#[derive(Debug)]
pub struct SystemHealthReport {
    /// Overall system health
    pub is_healthy: bool,
    /// Individual component health statuses
    pub component_health: Vec<(String, HealthStatus)>,
    /// System uptime
    pub uptime: Duration,
    /// Current lifecycle state
    pub state: EffectSystemState,
    /// Report generation time
    pub generated_at: Instant,
}

/// Lifecycle manager for the effect system
pub struct LifecycleManager {
    /// Current state (atomic for lock-free reads)
    state: Arc<AtomicU8>,
    /// Lifecycle-aware components
    components: Arc<RwLock<Vec<(String, Box<dyn LifecycleAware>)>>>,
    /// System start time
    start_time: Instant,
    /// Device ID for logging
    device_id: DeviceId,
    /// State transition lock to prevent concurrent transitions
    transition_lock: Arc<Mutex<()>>,
}

impl LifecycleManager {
    /// Create a new lifecycle manager
    pub fn new(device_id: DeviceId, start_time: Instant) -> Self {
        Self {
            state: Arc::new(AtomicU8::new(EffectSystemState::Uninitialized as u8)),
            components: Arc::new(RwLock::new(Vec::new())),
            start_time,
            device_id,
            transition_lock: Arc::new(Mutex::new(())),
        }
    }

    /// Get the current state
    pub fn current_state(&self) -> EffectSystemState {
        EffectSystemState::from_u8(self.state.load(Ordering::Acquire))
    }

    /// Register a lifecycle-aware component
    pub async fn register_component(
        &self,
        name: impl Into<String>,
        component: Box<dyn LifecycleAware>,
    ) {
        let mut components = self.components.write().await;
        components.push((name.into(), component));
    }

    /// Initialize the system
    pub async fn initialize(&self) -> AuraResult<()> {
        // Acquire transition lock
        let _lock = self.transition_lock.lock().await;

        let current = self.current_state();
        if !current.can_transition_to(EffectSystemState::Initializing) {
            return Err(AuraError::invalid(format!(
                "Cannot initialize from state {:?}",
                current
            )));
        }

        // Transition to Initializing
        self.state
            .store(EffectSystemState::Initializing as u8, Ordering::Release);

        info!(
            device_id = %self.device_id,
            "Starting effect system initialization"
        );

        // Initialize all components
        let components = self.components.read().await;
        let mut init_errors = Vec::new();

        for (name, component) in components.iter() {
            debug!(component = %name, "Initializing component");

            match component.on_initialize().await {
                Ok(()) => {
                    debug!(component = %name, "Component initialized successfully");
                }
                Err(e) => {
                    error!(component = %name, error = %e, "Component initialization failed");
                    init_errors.push((name.clone(), e));
                }
            }
        }

        // Check if initialization succeeded
        if init_errors.is_empty() {
            // Transition to Ready
            self.state
                .store(EffectSystemState::Ready as u8, Ordering::Release);

            info!(
                device_id = %self.device_id,
                "Effect system initialization completed successfully"
            );

            Ok(())
        } else {
            // Initialization failed, transition to ShuttingDown
            self.state
                .store(EffectSystemState::ShuttingDown as u8, Ordering::Release);

            // Try to clean up initialized components
            let _ = self.shutdown_internal().await;

            Err(AuraError::invalid(format!(
                "Effect system initialization failed: {} components failed",
                init_errors.len()
            )))
        }
    }

    /// Shutdown the system
    pub async fn shutdown(&self) -> AuraResult<()> {
        // Acquire transition lock
        let _lock = self.transition_lock.lock().await;

        let current = self.current_state();
        if !current.can_transition_to(EffectSystemState::ShuttingDown) {
            return Err(AuraError::invalid(format!(
                "Cannot shutdown from state {:?}",
                current
            )));
        }

        // Transition to ShuttingDown
        self.state
            .store(EffectSystemState::ShuttingDown as u8, Ordering::Release);

        info!(
            device_id = %self.device_id,
            "Starting effect system shutdown"
        );

        self.shutdown_internal().await
    }

    /// Internal shutdown logic
    async fn shutdown_internal(&self) -> AuraResult<()> {
        // Shutdown all components in reverse order
        let components = self.components.read().await;
        let mut shutdown_errors = Vec::new();

        for (name, component) in components.iter().rev() {
            debug!(component = %name, "Shutting down component");

            match component.on_shutdown().await {
                Ok(()) => {
                    debug!(component = %name, "Component shut down successfully");
                }
                Err(e) => {
                    error!(component = %name, error = %e, "Component shutdown failed");
                    shutdown_errors.push((name.clone(), e));
                }
            }
        }

        // Transition to Shutdown regardless of errors
        self.state
            .store(EffectSystemState::Shutdown as u8, Ordering::Release);

        if shutdown_errors.is_empty() {
            info!(
                device_id = %self.device_id,
                "Effect system shutdown completed successfully"
            );
            Ok(())
        } else {
            warn!(
                device_id = %self.device_id,
                errors = shutdown_errors.len(),
                "Effect system shutdown completed with errors"
            );
            Err(AuraError::invalid(format!(
                "Effect system shutdown had {} errors",
                shutdown_errors.len()
            )))
        }
    }

    /// Perform a system health check
    pub async fn health_check(&self, now: Instant) -> SystemHealthReport {
        let current_state = self.current_state();
        let uptime = self.start_time.elapsed();

        let components = self.components.read().await;
        let mut component_health = Vec::new();
        let mut all_healthy = true;

        for (name, component) in components.iter() {
            let health = component.health_check(now).await;
            if !health.is_healthy {
                all_healthy = false;
            }
            component_health.push((name.clone(), health));
        }

        // System is healthy if state is Ready and all components are healthy
        let is_healthy = current_state == EffectSystemState::Ready && all_healthy;

        SystemHealthReport {
            is_healthy,
            component_health,
            uptime,
            state: current_state,
            generated_at: now,
        }
    }

    /// Get system uptime
    pub fn uptime(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Check if the system is ready for operations
    pub fn is_ready(&self) -> bool {
        self.current_state() == EffectSystemState::Ready
    }

    /// Ensure the system is in a ready state
    pub fn ensure_ready(&self) -> AuraResult<()> {
        if !self.is_ready() {
            Err(AuraError::invalid(format!(
                "Effect system is not ready (current state: {:?})",
                self.current_state()
            )))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_macros::aura_test;
    use aura_testkit::{ TestFixture};

    #[test]
    fn test_state_transitions() {
        // Valid transitions
        assert!(EffectSystemState::Uninitialized.can_transition_to(EffectSystemState::Initializing));
        assert!(EffectSystemState::Initializing.can_transition_to(EffectSystemState::Ready));
        assert!(EffectSystemState::Initializing.can_transition_to(EffectSystemState::ShuttingDown));
        assert!(EffectSystemState::Ready.can_transition_to(EffectSystemState::ShuttingDown));
        assert!(EffectSystemState::ShuttingDown.can_transition_to(EffectSystemState::Shutdown));

        // Invalid transitions
        assert!(!EffectSystemState::Uninitialized.can_transition_to(EffectSystemState::Ready));
        assert!(!EffectSystemState::Ready.can_transition_to(EffectSystemState::Initializing));
        assert!(!EffectSystemState::Shutdown.can_transition_to(EffectSystemState::Ready));
    }

    #[aura_test]
    async fn test_lifecycle_manager_basic() -> AuraResult<()> {
        let fixture = TestFixture::new().await?;
        let now = Instant::now();
        let manager = LifecycleManager::new(fixture.device_id(), now);

        // Initial state
        assert_eq!(manager.current_state(), EffectSystemState::Uninitialized);
        assert!(!manager.is_ready());

        // Initialize
        manager.initialize().await?;
        assert_eq!(manager.current_state(), EffectSystemState::Ready);
        assert!(manager.is_ready());

        // Shutdown
        manager.shutdown().await?;
        assert_eq!(manager.current_state(), EffectSystemState::Shutdown);
        assert!(!manager.is_ready());
        Ok(())
    }

    #[aura_test]
    async fn test_invalid_transitions() -> AuraResult<()> {
        let fixture = TestFixture::new().await?;
        let now = Instant::now();
        let manager = LifecycleManager::new(fixture.device_id(), now);

        // Cannot shutdown from Uninitialized
        assert!(manager.shutdown().await.is_err());

        // Initialize first
        manager.initialize().await?;

        // Cannot initialize again
        assert!(manager.initialize().await.is_err());

        // Shutdown
        manager.shutdown().await?;

        // Cannot shutdown again
        assert!(manager.shutdown().await.is_err());
        Ok(())
    }

    /// Mock component for testing
    struct MockComponent {
        name: String,
        init_result: AuraResult<()>,
        shutdown_result: AuraResult<()>,
        is_healthy: bool,
    }

    #[async_trait]
    impl LifecycleAware for MockComponent {
        async fn on_initialize(&self) -> AuraResult<()> {
            self.init_result.clone()
        }

        async fn on_shutdown(&self) -> AuraResult<()> {
            self.shutdown_result.clone()
        }

        async fn health_check(&self, now: Instant) -> HealthStatus {
            #[allow(clippy::disallowed_methods)]
            // Test code - Instant::now() acceptable for testing context
            let now = now;
            if self.is_healthy {
                HealthStatus::healthy(now)
            } else {
                HealthStatus::unhealthy(format!("{} is unhealthy", self.name), now)
            }
        }
    }

    #[aura_test]
    async fn test_component_lifecycle() -> AuraResult<()> {
        let fixture = TestFixture::new().await?;
        let now = Instant::now();
        let manager = LifecycleManager::new(fixture.device_id(), now);

        // Register a healthy component
        let component = MockComponent {
            name: "test_component".to_string(),
            init_result: Ok(()),
            shutdown_result: Ok(()),
            is_healthy: true,
        };
        manager
            .register_component("test_component", Box::new(component))
            .await;

        // Initialize should succeed
        manager.initialize().await?;
        assert_eq!(manager.current_state(), EffectSystemState::Ready);

        // Health check should be healthy
        #[allow(clippy::disallowed_methods)]
        // Test code - Instant::now() acceptable for testing
        let now = Instant::now();
        let health_report = manager.health_check(now).await;
        assert!(health_report.is_healthy);
        assert_eq!(health_report.component_health.len(), 1);
        assert!(health_report.component_health[0].1.is_healthy);

        // Shutdown should succeed
        manager.shutdown().await?;
        assert_eq!(manager.current_state(), EffectSystemState::Shutdown);
        Ok(())
    }

    #[aura_test]
    async fn test_component_init_failure() -> AuraResult<()> {
        let fixture = TestFixture::new().await?;
        let now = Instant::now();
        let manager = LifecycleManager::new(fixture.device_id(), now);

        // Register a component that fails to initialize
        let component = MockComponent {
            name: "failing_component".to_string(),
            init_result: Err(AuraError::invalid("Init failed")),
            shutdown_result: Ok(()),
            is_healthy: false,
        };
        manager
            .register_component("failing_component", Box::new(component))
            .await;

        // Initialize should fail and transition to Shutdown
        assert!(manager.initialize().await.is_err());
        assert_eq!(manager.current_state(), EffectSystemState::Shutdown);
        Ok(())
    }
}
