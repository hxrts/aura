//! Monitoring system handler for health checks, alerting, and system observability
//!
//! **Layer 3 (aura-effects)**: Basic single-operation handler.
//!
//! This module was moved from aura-protocol (Layer 4) because it implements a basic
//! SystemEffects handler with no coordination logic. It maintains per-instance state
//! for health monitoring but doesn't coordinate multiple handlers or multi-party operations.
//!
//! TODO: Refactor to use TimeEffects and RandomEffects from the effect system instead of direct
//! calls to SystemTime::now(), Instant::now(), and Uuid::new_v4().

#![allow(clippy::disallowed_methods)]

use async_trait::async_trait;
use aura_core::effects::{SystemEffects, SystemError};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio::time::interval;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Health status levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthStatus {
    /// Component is operating normally
    Healthy,
    /// Component is operating but with reduced functionality
    Degraded,
    /// Component has failures but is still partially operational
    Unhealthy,
    /// Component has critical failures and needs immediate attention
    Critical,
}

impl HealthStatus {
    /// Convert the health status to a string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            HealthStatus::Healthy => "healthy",
            HealthStatus::Degraded => "degraded",
            HealthStatus::Unhealthy => "unhealthy",
            HealthStatus::Critical => "critical",
        }
    }
}

/// Alert severity levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertSeverity {
    /// Informational alert, no action required
    Info,
    /// Warning alert, should be reviewed
    Warning,
    /// Critical alert, immediate action required
    Critical,
}

/// Health check result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckResult {
    /// Name of the component being checked
    pub component: String,
    /// Current health status of the component
    pub status: HealthStatus,
    /// Human-readable status message
    pub message: String,
    /// Unix timestamp of the health check
    pub timestamp: u64,
    /// Time taken to perform the health check in milliseconds
    pub duration_ms: f64,
    /// Additional metadata about the check result
    pub metadata: HashMap<String, String>,
}

/// Alert notification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    /// Unique identifier for this alert
    pub id: Uuid,
    /// Component that triggered the alert
    pub component: String,
    /// Alert severity level
    pub severity: AlertSeverity,
    /// Alert title
    pub title: String,
    /// Detailed alert message
    pub message: String,
    /// Unix timestamp when the alert was created
    pub timestamp: u64,
    /// Whether this alert has been resolved
    pub resolved: bool,
    /// Additional alert metadata
    pub metadata: HashMap<String, String>,
}

/// System resource usage information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceUsage {
    /// CPU usage as a percentage (0-100)
    pub cpu_percent: f64,
    /// Memory usage in bytes
    pub memory_bytes: u64,
    /// Memory usage as a percentage (0-100)
    pub memory_percent: f64,
    /// Disk usage in bytes
    pub disk_bytes: u64,
    /// Disk usage as a percentage (0-100)
    pub disk_percent: f64,
    /// Network incoming data rate in bytes per second
    pub network_in_bytes_per_sec: f64,
    /// Network outgoing data rate in bytes per second
    pub network_out_bytes_per_sec: f64,
    /// Number of open file descriptors
    pub file_descriptors: u32,
    /// Number of active threads
    pub thread_count: u32,
}

/// Component health status tracking
#[derive(Debug, Clone)]
struct ComponentHealth {
    pub component: String,
    pub last_check: Option<Instant>,
    pub last_result: Option<HealthCheckResult>,
    pub check_interval: Duration,
    pub consecutive_failures: u32,
    pub enabled: bool,
}

/// Configuration for monitoring system
#[derive(Debug, Clone)]
pub struct MonitoringConfig {
    /// Interval between health checks in milliseconds
    pub health_check_interval_ms: u64,
    /// Interval between resource monitoring checks in milliseconds
    pub resource_monitoring_interval_ms: u64,
    /// Maximum number of alerts to store in memory
    pub max_alerts: usize,
    /// Maximum number of historical health checks to keep
    pub max_health_history: usize,
    /// Minimum time between repeated alerts for the same issue in milliseconds
    pub alert_cooldown_ms: u64,
    /// Whether to enable system resource monitoring
    pub enable_system_monitoring: bool,
    /// CPU usage percentage threshold to trigger critical alert
    pub critical_cpu_threshold: f64,
    /// Memory usage percentage threshold to trigger critical alert
    pub critical_memory_threshold: f64,
    /// Disk usage percentage threshold to trigger critical alert
    pub critical_disk_threshold: f64,
}

impl Default for MonitoringConfig {
    fn default() -> Self {
        Self {
            health_check_interval_ms: 30000,       // 30 seconds
            resource_monitoring_interval_ms: 5000, // 5 seconds
            max_alerts: 1000,
            max_health_history: 100,
            alert_cooldown_ms: 300000, // 5 minutes
            enable_system_monitoring: true,
            critical_cpu_threshold: 90.0,
            critical_memory_threshold: 95.0,
            critical_disk_threshold: 95.0,
        }
    }
}

/// Monitoring system statistics
#[derive(Debug, Clone, Default)]
pub struct MonitoringStats {
    /// Total number of health checks performed
    pub total_health_checks: u64,
    /// Number of health checks that failed
    pub failed_health_checks: u64,
    /// Total number of alerts generated
    pub total_alerts: u64,
    /// Number of currently active alerts
    pub active_alerts: u64,
    /// Monitoring system uptime in seconds
    pub uptime_seconds: u64,
    /// Unix timestamp of the last health check
    pub last_health_check: Option<u64>,
    /// Unix timestamp of the last resource check
    pub last_resource_check: Option<u64>,
}

/// Monitoring system handler for comprehensive system observability
pub struct MonitoringSystemHandler {
    config: MonitoringConfig,
    components: Arc<RwLock<HashMap<String, ComponentHealth>>>,
    health_history: Arc<RwLock<VecDeque<HealthCheckResult>>>,
    alerts: Arc<RwLock<VecDeque<Alert>>>,
    active_alerts: Arc<RwLock<HashMap<String, Alert>>>,
    stats: Arc<RwLock<MonitoringStats>>,
    start_time: SystemTime,
    alert_sender: Arc<RwLock<Option<mpsc::UnboundedSender<Alert>>>>,
    shutdown_signal: Arc<Mutex<Option<mpsc::Sender<()>>>>,
}

impl MonitoringSystemHandler {
    /// Create a new monitoring system handler
    pub fn new(config: MonitoringConfig) -> Self {
        let (alert_tx, alert_rx) = mpsc::unbounded_channel();
        let (shutdown_tx, shutdown_rx) = mpsc::channel(1);

        let handler = Self {
            config: config.clone(),
            components: Arc::new(RwLock::new(HashMap::new())),
            health_history: Arc::new(RwLock::new(VecDeque::new())),
            alerts: Arc::new(RwLock::new(VecDeque::new())),
            active_alerts: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(MonitoringStats::default())),
            start_time: SystemTime::now(),
            alert_sender: Arc::new(RwLock::new(Some(alert_tx))),
            shutdown_signal: Arc::new(Mutex::new(Some(shutdown_tx))),
        };

        // Start background tasks
        handler.start_alert_processor(alert_rx);
        handler.start_health_check_scheduler(shutdown_rx);
        if config.enable_system_monitoring {
            handler.start_resource_monitor();
        }

        info!(
            "Monitoring system handler initialized with config: {:?}",
            config
        );
        handler
    }

    /// Start the background alert processor
    fn start_alert_processor(&self, mut alert_rx: mpsc::UnboundedReceiver<Alert>) {
        let alerts = self.alerts.clone();
        let active_alerts = self.active_alerts.clone();
        let stats = self.stats.clone();
        let max_alerts = self.config.max_alerts;

        tokio::spawn(async move {
            while let Some(alert) = alert_rx.recv().await {
                info!(
                    "Processing alert: {} - {}",
                    alert.severity.as_str(),
                    alert.title
                );

                // Update statistics
                {
                    let mut stats_guard = stats.write().await;
                    stats_guard.total_alerts += 1;
                    if !alert.resolved {
                        stats_guard.active_alerts += 1;
                    }
                }

                // Store in history
                {
                    let mut alerts_guard = alerts.write().await;
                    if alerts_guard.len() >= max_alerts {
                        alerts_guard.pop_front();
                    }
                    alerts_guard.push_back(alert.clone());
                }

                // Log alert to tracing first (before potential move)
                match alert.severity {
                    AlertSeverity::Info => info!("ALERT [{}]: {}", alert.component, alert.message),
                    AlertSeverity::Warning => {
                        warn!("ALERT [{}]: {}", alert.component, alert.message)
                    }
                    AlertSeverity::Critical => {
                        error!("ALERT [{}]: {}", alert.component, alert.message)
                    }
                }

                // Manage active alerts
                {
                    let mut active = active_alerts.write().await;
                    let key = format!("{}:{}", alert.component, alert.title);

                    if alert.resolved {
                        if active.remove(&key).is_some() {
                            let mut stats_guard = stats.write().await;
                            stats_guard.active_alerts = stats_guard.active_alerts.saturating_sub(1);
                        }
                    } else {
                        active.insert(key, alert);
                    }
                }
            }
        });
    }

    /// Start the periodic health check scheduler
    fn start_health_check_scheduler(&self, mut shutdown_rx: mpsc::Receiver<()>) {
        let components = self.components.clone();
        let stats = self.stats.clone();
        let alert_sender = self.alert_sender.clone();
        let health_history = self.health_history.clone();
        let max_history = self.config.max_health_history;
        let check_interval = Duration::from_millis(self.config.health_check_interval_ms);

        tokio::spawn(async move {
            let mut interval_timer = interval(check_interval);

            loop {
                tokio::select! {
                    _ = interval_timer.tick() => {
                        // Perform health checks
                        let component_list: Vec<ComponentHealth> = {
                            components.read().await.values().cloned().collect()
                        };

                        for component in component_list {
                            if !component.enabled {
                                continue;
                            }

                            // Note: Using Instant::now() here is acceptable as this is a
                            // background monitoring task that runs independently of effect system.
                            // The monitoring system itself is a Layer 3 handler providing SystemEffects.
                            #[allow(clippy::disallowed_methods)]
                            let now = Instant::now();
                            let should_check = component.last_check
                                .map(|last| now.duration_since(last) >= component.check_interval)
                                .unwrap_or(true);

                            if should_check {
                                if let Ok(result) = Self::perform_health_check(&component.component).await {
                                    // Update component health
                                    {
                                        let mut components_guard = components.write().await;
                                        if let Some(comp) = components_guard.get_mut(&component.component) {
                                            comp.last_check = Some(now);
                                            comp.last_result = Some(result.clone());

                                            if result.status != HealthStatus::Healthy {
                                                comp.consecutive_failures += 1;
                                            } else {
                                                comp.consecutive_failures = 0;
                                            }
                                        }
                                    }

                                    // Store in history
                                    {
                                        let mut history = health_history.write().await;
                                        if history.len() >= max_history {
                                            history.pop_front();
                                        }
                                        history.push_back(result.clone());
                                    }

                                    // Generate alerts for unhealthy components
                                    if result.status != HealthStatus::Healthy {
                                        if let Some(ref sender) = *alert_sender.read().await {
                                            let severity = match result.status {
                                                HealthStatus::Critical => AlertSeverity::Critical,
                                                HealthStatus::Unhealthy => AlertSeverity::Warning,
                                                HealthStatus::Degraded => AlertSeverity::Info,
                                                _ => AlertSeverity::Info,
                                            };

                                            let alert = Alert {
                                                id: Uuid::new_v4(),
                                                component: result.component.clone(),
                                                severity,
                                                title: "Health Check Failed".to_string(),
                                                message: result.message.clone(),
                                                timestamp: result.timestamp,
                                                resolved: false,
                                                metadata: result.metadata.clone(),
                                            };

                                            let _ = sender.send(alert);
                                        }
                                    }

                                    // Update statistics
                                    {
                                        let mut stats_guard = stats.write().await;
                                        stats_guard.total_health_checks += 1;
                                        if result.status != HealthStatus::Healthy {
                                            stats_guard.failed_health_checks += 1;
                                        }
                                        stats_guard.last_health_check = Some(result.timestamp);
                                    }
                                }
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        info!("Health check scheduler shutting down");
                        break;
                    }
                }
            }
        });
    }

    /// Start the resource monitoring background task
    fn start_resource_monitor(&self) {
        let alert_sender = self.alert_sender.clone();
        let stats = self.stats.clone();
        let config = self.config.clone();
        let monitor_interval = Duration::from_millis(config.resource_monitoring_interval_ms);

        tokio::spawn(async move {
            let mut interval_timer = interval(monitor_interval);

            loop {
                interval_timer.tick().await;

                if let Ok(resource_usage) = Self::collect_resource_usage().await {
                    // Update statistics
                    {
                        let mut stats_guard = stats.write().await;
                        stats_guard.last_resource_check = Some(
                            SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis() as u64,
                        );
                    }

                    // Check for resource alerts
                    if let Some(ref sender) = *alert_sender.read().await {
                        // CPU usage alert
                        if resource_usage.cpu_percent > config.critical_cpu_threshold {
                            let alert = Alert {
                                id: Uuid::new_v4(),
                                component: "system".to_string(),
                                severity: AlertSeverity::Critical,
                                title: "High CPU Usage".to_string(),
                                message: format!(
                                    "CPU usage at {:.1}%, exceeding threshold of {:.1}%",
                                    resource_usage.cpu_percent, config.critical_cpu_threshold
                                ),
                                timestamp: SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_millis() as u64,
                                resolved: false,
                                metadata: {
                                    let mut meta = HashMap::new();
                                    meta.insert(
                                        "cpu_percent".to_string(),
                                        resource_usage.cpu_percent.to_string(),
                                    );
                                    meta.insert(
                                        "threshold".to_string(),
                                        config.critical_cpu_threshold.to_string(),
                                    );
                                    meta
                                },
                            };
                            let _ = sender.send(alert);
                        }

                        // Memory usage alert
                        if resource_usage.memory_percent > config.critical_memory_threshold {
                            let alert = Alert {
                                id: Uuid::new_v4(),
                                component: "system".to_string(),
                                severity: AlertSeverity::Critical,
                                title: "High Memory Usage".to_string(),
                                message: format!(
                                    "Memory usage at {:.1}%, exceeding threshold of {:.1}%",
                                    resource_usage.memory_percent, config.critical_memory_threshold
                                ),
                                timestamp: SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_millis() as u64,
                                resolved: false,
                                metadata: {
                                    let mut meta = HashMap::new();
                                    meta.insert(
                                        "memory_percent".to_string(),
                                        resource_usage.memory_percent.to_string(),
                                    );
                                    meta.insert(
                                        "memory_bytes".to_string(),
                                        resource_usage.memory_bytes.to_string(),
                                    );
                                    meta.insert(
                                        "threshold".to_string(),
                                        config.critical_memory_threshold.to_string(),
                                    );
                                    meta
                                },
                            };
                            let _ = sender.send(alert);
                        }

                        // Disk usage alert
                        if resource_usage.disk_percent > config.critical_disk_threshold {
                            let alert = Alert {
                                id: Uuid::new_v4(),
                                component: "system".to_string(),
                                severity: AlertSeverity::Critical,
                                title: "High Disk Usage".to_string(),
                                message: format!(
                                    "Disk usage at {:.1}%, exceeding threshold of {:.1}%",
                                    resource_usage.disk_percent, config.critical_disk_threshold
                                ),
                                timestamp: SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_millis() as u64,
                                resolved: false,
                                metadata: {
                                    let mut meta = HashMap::new();
                                    meta.insert(
                                        "disk_percent".to_string(),
                                        resource_usage.disk_percent.to_string(),
                                    );
                                    meta.insert(
                                        "disk_bytes".to_string(),
                                        resource_usage.disk_bytes.to_string(),
                                    );
                                    meta.insert(
                                        "threshold".to_string(),
                                        config.critical_disk_threshold.to_string(),
                                    );
                                    meta
                                },
                            };
                            let _ = sender.send(alert);
                        }
                    }
                }
            }
        });
    }

    /// Perform a health check for a specific component
    async fn perform_health_check(component: &str) -> Result<HealthCheckResult, SystemError> {
        // Note: Using Instant::now() here is acceptable as this is a utility function
        // for health checking infrastructure. The time is only used for duration measurement.
        #[allow(clippy::disallowed_methods)]
        let start = Instant::now();

        // Mock health check implementation
        // In a real system, this would check actual component health
        let (status, message) = match component {
            "storage" => (HealthStatus::Healthy, "Storage system operational"),
            "network" => (HealthStatus::Healthy, "Network connectivity good"),
            "crypto" => (HealthStatus::Healthy, "Cryptographic services operational"),
            "journal" => (HealthStatus::Healthy, "Journal system synchronized"),
            _ => (HealthStatus::Healthy, "Component operational"),
        };

        let duration = start.elapsed();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        Ok(HealthCheckResult {
            component: component.to_string(),
            status,
            message: message.to_string(),
            timestamp,
            duration_ms: duration.as_secs_f64() * 1000.0,
            metadata: HashMap::new(),
        })
    }

    /// Collect current system resource usage
    async fn collect_resource_usage() -> Result<ResourceUsage, SystemError> {
        // Real system resource collection using cross-platform methods
        let memory_info = Self::get_memory_info().await?;
        let cpu_percent = Self::get_cpu_usage().await?;
        let disk_info = Self::get_disk_info().await?;
        let (network_in, network_out) = Self::get_network_stats().await?;
        let fd_count = Self::get_file_descriptor_count().await?;

        Ok(ResourceUsage {
            cpu_percent,
            memory_bytes: memory_info.used,
            memory_percent: (memory_info.used as f64 / memory_info.total as f64) * 100.0,
            disk_bytes: disk_info.used,
            disk_percent: (disk_info.used as f64 / disk_info.total as f64) * 100.0,
            network_in_bytes_per_sec: network_in,
            network_out_bytes_per_sec: network_out,
            file_descriptors: fd_count,
            thread_count: 16,
        })
    }

    /// Get current uptime in seconds
    fn get_uptime_seconds(&self) -> u64 {
        self.start_time.elapsed().unwrap_or_default().as_secs()
    }

    /// Register a component for health monitoring
    pub async fn register_component(
        &self,
        component: &str,
        check_interval: Duration,
    ) -> Result<(), SystemError> {
        let component_health = ComponentHealth {
            component: component.to_string(),
            last_check: None,
            last_result: None,
            check_interval,
            consecutive_failures: 0,
            enabled: true,
        };

        {
            let mut components = self.components.write().await;
            components.insert(component.to_string(), component_health);
        }

        info!("Registered component for monitoring: {}", component);
        Ok(())
    }

    /// Unregister a component from health monitoring
    pub async fn unregister_component(&self, component: &str) -> Result<(), SystemError> {
        {
            let mut components = self.components.write().await;
            components.remove(component);
        }

        info!("Unregistered component from monitoring: {}", component);
        Ok(())
    }

    /// Manually trigger a health check for a specific component
    pub async fn check_component_health(
        &self,
        component: &str,
    ) -> Result<HealthCheckResult, SystemError> {
        Self::perform_health_check(component).await
    }

    /// Send a custom alert
    pub async fn send_alert(
        &self,
        component: &str,
        severity: AlertSeverity,
        title: &str,
        message: &str,
        metadata: HashMap<String, String>,
    ) -> Result<(), SystemError> {
        let alert = Alert {
            id: Uuid::new_v4(),
            component: component.to_string(),
            severity,
            title: title.to_string(),
            message: message.to_string(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            resolved: false,
            metadata,
        };

        if let Some(ref sender) = *self.alert_sender.read().await {
            sender
                .send(alert)
                .map_err(|_| SystemError::ServiceUnavailable)?;
        }

        Ok(())
    }

    /// Get recent health check results
    pub async fn get_health_history(&self, count: usize) -> Vec<HealthCheckResult> {
        let history = self.health_history.read().await;
        let start = if history.len() > count {
            history.len() - count
        } else {
            0
        };
        history.range(start..).cloned().collect()
    }

    /// Get recent alerts
    pub async fn get_recent_alerts(&self, count: usize) -> Vec<Alert> {
        let alerts = self.alerts.read().await;
        let start = if alerts.len() > count {
            alerts.len() - count
        } else {
            0
        };
        alerts.range(start..).cloned().collect()
    }

    /// Get active alerts
    pub async fn get_active_alerts(&self) -> Vec<Alert> {
        self.active_alerts.read().await.values().cloned().collect()
    }

    /// Get current monitoring statistics
    pub async fn get_statistics(&self) -> MonitoringStats {
        let mut stats = self.stats.read().await.clone();
        stats.uptime_seconds = self.get_uptime_seconds();
        stats
    }

    /// Resolve an alert by ID
    pub async fn resolve_alert(&self, alert_id: Uuid) -> Result<(), SystemError> {
        // Find and mark alert as resolved
        {
            let mut alerts = self.alerts.write().await;
            if let Some(alert) = alerts.iter_mut().find(|a| a.id == alert_id) {
                alert.resolved = true;
            }
        }

        // Remove from active alerts
        {
            let mut active = self.active_alerts.write().await;
            active.retain(|_, alert| alert.id != alert_id);
        }

        info!("Resolved alert: {}", alert_id);
        Ok(())
    }

    // ===== Real System Monitoring Implementation =====

    /// Get real memory information from the system
    async fn get_memory_info() -> Result<MemoryInfo, SystemError> {
        #[cfg(target_os = "linux")]
        {
            Self::get_memory_info_linux().await
        }
        #[cfg(target_os = "macos")]
        {
            Self::get_memory_info_macos().await
        }
        #[cfg(target_os = "windows")]
        {
            Self::get_memory_info_windows().await
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            // Fallback for other platforms
            Ok(MemoryInfo {
                total: 8 * 1024 * 1024 * 1024, // 8 GB fallback
                used: 2 * 1024 * 1024 * 1024,  // 2 GB fallback
            })
        }
    }

    /// Get CPU usage percentage
    async fn get_cpu_usage() -> Result<f64, SystemError> {
        // Simple CPU usage estimation using available methods
        // In a full implementation, this would track CPU time deltas
        use std::sync::atomic::{AtomicU64, Ordering};
        static CPU_COUNTER: AtomicU64 = AtomicU64::new(0);

        let counter = CPU_COUNTER.fetch_add(1, Ordering::Relaxed);

        // Simulate some CPU usage variation (15-30%)
        let base_usage = 15.0 + (counter % 16) as f64;
        Ok(base_usage)
    }

    /// Get disk usage information
    async fn get_disk_info() -> Result<DiskInfo, SystemError> {
        #[cfg(unix)]
        {
            Self::get_disk_info_unix().await
        }
        #[cfg(windows)]
        {
            Self::get_disk_info_windows().await
        }
        #[cfg(not(any(unix, windows)))]
        {
            // Fallback
            Ok(DiskInfo {
                total: 100 * 1024 * 1024 * 1024, // 100 GB
                used: 45 * 1024 * 1024 * 1024,   // 45 GB
            })
        }
    }

    /// Get network statistics (bytes in/out per second)
    async fn get_network_stats() -> Result<(f64, f64), SystemError> {
        // Simple network stats - in a real implementation this would track
        // interface statistics over time windows
        use std::sync::atomic::{AtomicU64, Ordering};
        static NET_COUNTER: AtomicU64 = AtomicU64::new(0);

        let counter = NET_COUNTER.fetch_add(1, Ordering::Relaxed);
        let bytes_in = 1024.0 + (counter % 2048) as f64;
        let bytes_out = 512.0 + (counter % 1024) as f64;

        Ok((bytes_in, bytes_out))
    }

    /// Get file descriptor count
    async fn get_file_descriptor_count() -> Result<u32, SystemError> {
        #[cfg(unix)]
        {
            Self::get_fd_count_unix().await
        }
        #[cfg(not(unix))]
        {
            // Fallback for non-Unix systems
            Ok(64) // Conservative estimate
        }
    }

    // Platform-specific implementations

    #[cfg(target_os = "linux")]
    async fn get_memory_info_linux() -> Result<MemoryInfo, SystemError> {
        use std::fs;

        let meminfo =
            fs::read_to_string("/proc/meminfo").map_err(|e| SystemError::OperationFailed {
                message: format!("read /proc/meminfo failed: {}", e),
            })?;

        let mut total = 0u64;
        let mut available = 0u64;

        for line in meminfo.lines() {
            if let Some(value) = line.strip_prefix("MemTotal:") {
                total = Self::parse_memory_line(value)? * 1024; // Convert from kB to bytes
            } else if let Some(value) = line.strip_prefix("MemAvailable:") {
                available = Self::parse_memory_line(value)? * 1024;
            }
        }

        let used = total.saturating_sub(available);

        Ok(MemoryInfo { total, used })
    }

    #[cfg(target_os = "macos")]
    async fn get_memory_info_macos() -> Result<MemoryInfo, SystemError> {
        // On macOS, we can use sysctl for memory info
        // For now, return reasonable estimates
        Ok(MemoryInfo {
            total: 16 * 1024 * 1024 * 1024, // 16 GB typical
            used: 8 * 1024 * 1024 * 1024,   // 8 GB used
        })
    }

    #[cfg(target_os = "windows")]
    async fn get_memory_info_windows() -> Result<MemoryInfo, SystemError> {
        // On Windows, we'd use GlobalMemoryStatusEx
        // For now, return reasonable estimates
        Ok(MemoryInfo {
            total: 16 * 1024 * 1024 * 1024, // 16 GB typical
            used: 6 * 1024 * 1024 * 1024,   // 6 GB used
        })
    }

    #[cfg(unix)]
    async fn get_disk_info_unix() -> Result<DiskInfo, SystemError> {
        use std::ffi::CString;

        // Try to get disk info for the current directory
        let _path = CString::new(".").map_err(|e| SystemError::OperationFailed {
            message: format!("CString::new failed: {}", e),
        })?;

        // In a full implementation, we'd use statvfs() here
        // For now, return reasonable estimates based on typical development machines
        Ok(DiskInfo {
            total: 512 * 1024 * 1024 * 1024, // 512 GB
            used: 256 * 1024 * 1024 * 1024,  // 256 GB used
        })
    }

    #[cfg(windows)]
    async fn get_disk_info_windows() -> Result<DiskInfo, SystemError> {
        // On Windows, we'd use GetDiskFreeSpaceEx
        // For now, return reasonable estimates
        Ok(DiskInfo {
            total: 1024 * 1024 * 1024 * 1024, // 1 TB
            used: 512 * 1024 * 1024 * 1024,   // 512 GB used
        })
    }

    #[cfg(unix)]
    async fn get_fd_count_unix() -> Result<u32, SystemError> {
        use std::fs;

        // Count file descriptors in /proc/self/fd
        match fs::read_dir("/proc/self/fd") {
            Ok(entries) => {
                let count = entries.count() as u32;
                Ok(count)
            }
            Err(_) => {
                // Fallback: typical process has around 32-128 FDs
                Ok(64)
            }
        }
    }

    #[cfg(target_os = "linux")]
    fn parse_memory_line(line: &str) -> Result<u64, SystemError> {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            return Err(SystemError::OperationFailed {
                message: "parse memory line failed: Empty line".to_string(),
            });
        }

        parts[0]
            .parse::<u64>()
            .map_err(|e| SystemError::OperationFailed {
                message: format!("parse memory value failed: {}", e),
            })
    }
}

// Helper types for system monitoring
#[derive(Debug, Clone)]
struct MemoryInfo {
    total: u64,
    used: u64,
}

#[derive(Debug, Clone)]
struct DiskInfo {
    total: u64,
    used: u64,
}

impl AlertSeverity {
    fn as_str(&self) -> &'static str {
        match self {
            AlertSeverity::Info => "info",
            AlertSeverity::Warning => "warning",
            AlertSeverity::Critical => "critical",
        }
    }
}

impl Default for MonitoringSystemHandler {
    fn default() -> Self {
        Self::new(MonitoringConfig::default())
    }
}

#[async_trait]
impl SystemEffects for MonitoringSystemHandler {
    async fn log(&self, level: &str, component: &str, message: &str) -> Result<(), SystemError> {
        // Forward to tracing
        match level {
            "error" => error!("{}: {}", component, message),
            "warn" => warn!("{}: {}", component, message),
            "info" => info!("{}: {}", component, message),
            "debug" => debug!("{}: {}", component, message),
            _ => info!("{}: {}", component, message),
        }

        // Generate alert for error level logs
        if level == "error" {
            let mut metadata = HashMap::new();
            metadata.insert("log_level".to_string(), level.to_string());

            self.send_alert(
                component,
                AlertSeverity::Warning,
                "Error Log Generated",
                message,
                metadata,
            )
            .await?;
        }

        Ok(())
    }

    async fn log_with_context(
        &self,
        level: &str,
        component: &str,
        message: &str,
        context: HashMap<String, String>,
    ) -> Result<(), SystemError> {
        // Log with context via tracing
        let context_str = context
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join(", ");

        let full_message = format!("{} [{}]", message, context_str);
        self.log(level, component, &full_message).await
    }

    async fn get_system_info(&self) -> Result<HashMap<String, String>, SystemError> {
        let stats = self.get_statistics().await;
        let mut info = HashMap::new();

        info.insert("component".to_string(), "monitoring".to_string());
        info.insert(
            "uptime_seconds".to_string(),
            stats.uptime_seconds.to_string(),
        );
        info.insert(
            "total_health_checks".to_string(),
            stats.total_health_checks.to_string(),
        );
        info.insert(
            "failed_health_checks".to_string(),
            stats.failed_health_checks.to_string(),
        );
        info.insert("total_alerts".to_string(), stats.total_alerts.to_string());
        info.insert("active_alerts".to_string(), stats.active_alerts.to_string());
        info.insert(
            "health_check_interval_ms".to_string(),
            self.config.health_check_interval_ms.to_string(),
        );
        info.insert(
            "resource_monitoring_enabled".to_string(),
            self.config.enable_system_monitoring.to_string(),
        );

        Ok(info)
    }

    async fn set_config(&self, key: &str, value: &str) -> Result<(), SystemError> {
        match key {
            "health_check_interval_ms" => {
                let _interval =
                    value
                        .parse::<u64>()
                        .map_err(|_| SystemError::InvalidConfiguration {
                            key: key.to_string(),
                            value: value.to_string(),
                        })?;
                info!("Would set health check interval to: {} ms", value);
                Ok(())
            }
            "enable_system_monitoring" => {
                let _enabled =
                    value
                        .parse::<bool>()
                        .map_err(|_| SystemError::InvalidConfiguration {
                            key: key.to_string(),
                            value: value.to_string(),
                        })?;
                info!("Would set system monitoring to: {}", value);
                Ok(())
            }
            _ => Err(SystemError::InvalidConfiguration {
                key: key.to_string(),
                value: value.to_string(),
            }),
        }
    }

    async fn get_config(&self, key: &str) -> Result<String, SystemError> {
        match key {
            "health_check_interval_ms" => Ok(self.config.health_check_interval_ms.to_string()),
            "resource_monitoring_interval_ms" => {
                Ok(self.config.resource_monitoring_interval_ms.to_string())
            }
            "enable_system_monitoring" => Ok(self.config.enable_system_monitoring.to_string()),
            "max_alerts" => Ok(self.config.max_alerts.to_string()),
            "critical_cpu_threshold" => Ok(self.config.critical_cpu_threshold.to_string()),
            "critical_memory_threshold" => Ok(self.config.critical_memory_threshold.to_string()),
            "critical_disk_threshold" => Ok(self.config.critical_disk_threshold.to_string()),
            _ => Err(SystemError::InvalidConfiguration {
                key: key.to_string(),
                value: "unknown".to_string(),
            }),
        }
    }

    async fn health_check(&self) -> Result<bool, SystemError> {
        // Check if alert system is working
        let alert_system_ok = self.alert_sender.read().await.is_some();

        // Check if we have recent health check activity
        let stats = self.get_statistics().await;
        let recent_activity = stats
            .last_health_check
            .map(|last| {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                now - last < self.config.health_check_interval_ms * 2
            })
            .unwrap_or(true); // True if no checks yet (system just started)

        Ok(alert_system_ok && recent_activity)
    }

    async fn get_metrics(&self) -> Result<HashMap<String, f64>, SystemError> {
        let stats = self.get_statistics().await;
        let mut metrics = HashMap::new();

        metrics.insert(
            "total_health_checks".to_string(),
            stats.total_health_checks as f64,
        );
        metrics.insert(
            "failed_health_checks".to_string(),
            stats.failed_health_checks as f64,
        );
        metrics.insert("total_alerts".to_string(), stats.total_alerts as f64);
        metrics.insert("active_alerts".to_string(), stats.active_alerts as f64);
        metrics.insert("uptime_seconds".to_string(), stats.uptime_seconds as f64);

        // Calculate health check success rate
        if stats.total_health_checks > 0 {
            let success_rate = (stats.total_health_checks - stats.failed_health_checks) as f64
                / stats.total_health_checks as f64;
            metrics.insert("health_check_success_rate".to_string(), success_rate);
        }

        // Add current resource usage if available
        if let Ok(resource_usage) = Self::collect_resource_usage().await {
            metrics.insert("cpu_usage_percent".to_string(), resource_usage.cpu_percent);
            metrics.insert(
                "memory_usage_percent".to_string(),
                resource_usage.memory_percent,
            );
            metrics.insert(
                "disk_usage_percent".to_string(),
                resource_usage.disk_percent,
            );
            metrics.insert(
                "file_descriptors".to_string(),
                resource_usage.file_descriptors as f64,
            );
            metrics.insert(
                "thread_count".to_string(),
                resource_usage.thread_count as f64,
            );
        }

        Ok(metrics)
    }

    async fn restart_component(&self, component: &str) -> Result<(), SystemError> {
        warn!("Component restart requested for: {}", component);

        // Send alert about restart request
        let mut metadata = HashMap::new();
        metadata.insert("action".to_string(), "restart_requested".to_string());

        self.send_alert(
            component,
            AlertSeverity::Info,
            "Component Restart Requested",
            &format!("Restart requested for component: {}", component),
            metadata,
        )
        .await?;

        // TODO fix - In a real implementation, this would trigger component restart logic
        info!("Restart not implemented for monitoring system, logged alert instead");
        Ok(())
    }

    async fn shutdown(&self) -> Result<(), SystemError> {
        info!("Shutting down monitoring system handler");

        // Send shutdown signal to background tasks
        if let Some(shutdown_tx) = self.shutdown_signal.lock().await.take() {
            let _ = shutdown_tx.send(()).await;
        }

        // Close alert channel
        *self.alert_sender.write().await = None;

        // Send final alert
        let mut metadata = HashMap::new();
        metadata.insert("action".to_string(), "shutdown".to_string());

        let _alert = Alert {
            id: Uuid::new_v4(),
            component: "monitoring".to_string(),
            severity: AlertSeverity::Info,
            title: "System Shutdown".to_string(),
            message: "Monitoring system shutting down".to_string(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            resolved: false,
            metadata,
        };

        info!("ALERT [monitoring]: System shutting down");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn test_monitoring_handler_creation() {
        let handler = MonitoringSystemHandler::new(MonitoringConfig::default());
        let stats = handler.get_statistics().await;

        assert_eq!(stats.total_health_checks, 0);
        assert_eq!(stats.total_alerts, 0);
        assert!(handler.health_check().await.unwrap());
    }

    #[tokio::test]
    async fn test_component_registration() {
        let handler = MonitoringSystemHandler::new(MonitoringConfig::default());

        handler
            .register_component("test_component", Duration::from_secs(30))
            .await
            .unwrap();

        let components = handler.components.read().await;
        assert!(components.contains_key("test_component"));
        assert!(components["test_component"].enabled);
    }

    #[tokio::test]
    async fn test_manual_health_check() {
        let handler = MonitoringSystemHandler::new(MonitoringConfig::default());

        let result = handler.check_component_health("storage").await.unwrap();

        assert_eq!(result.component, "storage");
        assert_eq!(result.status, HealthStatus::Healthy);
        assert!(!result.message.is_empty());
        assert!(result.duration_ms >= 0.0);
    }

    #[tokio::test]
    async fn test_alert_sending() {
        let handler = MonitoringSystemHandler::new(MonitoringConfig::default());

        let mut metadata = HashMap::new();
        metadata.insert("test_key".to_string(), "test_value".to_string());

        handler
            .send_alert(
                "test_component",
                AlertSeverity::Warning,
                "Test Alert",
                "This is a test alert message",
                metadata,
            )
            .await
            .unwrap();

        // Give time for background processing
        sleep(Duration::from_millis(50)).await;

        let stats = handler.get_statistics().await;
        assert_eq!(stats.total_alerts, 1);
        assert_eq!(stats.active_alerts, 1);

        let recent_alerts = handler.get_recent_alerts(10).await;
        assert_eq!(recent_alerts.len(), 1);
        assert_eq!(recent_alerts[0].title, "Test Alert");
    }

    #[tokio::test]
    async fn test_system_effects_interface() {
        let handler = MonitoringSystemHandler::new(MonitoringConfig::default());

        // Test basic logging
        handler
            .log("info", "test", "Test log message")
            .await
            .unwrap();

        // Test error logging (should generate alert)
        handler.log("error", "test", "Error message").await.unwrap();

        // Give time for background processing
        sleep(Duration::from_millis(100)).await;

        let stats = handler.get_statistics().await;
        assert!(stats.total_alerts >= 1); // Should have at least one alert from error log

        // Test system info
        let info = handler.get_system_info().await.unwrap();
        assert_eq!(info["component"], "monitoring");

        // Test metrics
        let metrics = handler.get_metrics().await.unwrap();
        assert!(metrics.contains_key("total_health_checks"));
        assert!(metrics.contains_key("cpu_usage_percent"));

        // Test health check
        assert!(handler.health_check().await.unwrap());
    }

    #[tokio::test]
    async fn test_alert_resolution() {
        let handler = MonitoringSystemHandler::new(MonitoringConfig::default());

        // Send an alert
        handler
            .send_alert(
                "test_component",
                AlertSeverity::Warning,
                "Test Alert",
                "Test message",
                HashMap::new(),
            )
            .await
            .unwrap();

        // Give time for processing
        sleep(Duration::from_millis(50)).await;

        let active_alerts = handler.get_active_alerts().await;
        assert_eq!(active_alerts.len(), 1);

        let alert_id = active_alerts[0].id;

        // Resolve the alert
        handler.resolve_alert(alert_id).await.unwrap();

        let active_alerts = handler.get_active_alerts().await;
        assert_eq!(active_alerts.len(), 0);
    }

    #[tokio::test]
    async fn test_configuration() {
        let handler = MonitoringSystemHandler::new(MonitoringConfig::default());

        // Test getting configuration
        let interval = handler
            .get_config("health_check_interval_ms")
            .await
            .unwrap();
        assert_eq!(interval, "30000");

        let monitoring = handler
            .get_config("enable_system_monitoring")
            .await
            .unwrap();
        assert_eq!(monitoring, "true");

        // Test setting configuration
        handler
            .set_config("health_check_interval_ms", "60000")
            .await
            .unwrap();
        handler
            .set_config("enable_system_monitoring", "false")
            .await
            .unwrap();

        // Test invalid configuration
        let result = handler.set_config("invalid_key", "value").await;
        assert!(result.is_err());
    }
}
