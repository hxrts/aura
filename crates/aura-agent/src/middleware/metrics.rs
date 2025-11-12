//! Metrics Collection Middleware
//!
//! Provides telemetry and metrics collection for agent operations, enabling
//! monitoring, performance analysis, and operational insights.

use aura_core::{identifiers::DeviceId, AuraError, AuraResult as Result};
use uuid;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

/// Metrics for a specific operation
#[derive(Debug, Clone)]
pub struct OperationMetrics {
    /// Operation name
    pub operation: String,
    /// Number of successful executions
    pub success_count: u64,
    /// Number of failed executions
    pub error_count: u64,
    /// Total execution time
    pub total_duration: Duration,
    /// Average execution time
    pub avg_duration: Duration,
    /// Minimum execution time
    pub min_duration: Duration,
    /// Maximum execution time
    pub max_duration: Duration,
    /// Last execution timestamp
    pub last_execution: Option<SystemTime>,
}

impl OperationMetrics {
    /// Create new operation metrics
    pub fn new(operation: String) -> Self {
        Self {
            operation,
            success_count: 0,
            error_count: 0,
            total_duration: Duration::ZERO,
            avg_duration: Duration::ZERO,
            min_duration: Duration::MAX,
            max_duration: Duration::ZERO,
            last_execution: None,
        }
    }

    /// Record a successful operation
    pub fn record_success(&mut self, duration: Duration) {
        self.success_count += 1;
        self.update_duration_stats(duration);
    }

    /// Record a failed operation
    pub fn record_error(&mut self, duration: Duration) {
        self.error_count += 1;
        self.update_duration_stats(duration);
    }

    /// Update duration statistics
    fn update_duration_stats(&mut self, duration: Duration) {
        self.total_duration += duration;

        let total_count = self.success_count + self.error_count;
        if total_count > 0 {
            self.avg_duration = self.total_duration / total_count as u32;
        }

        self.min_duration = self.min_duration.min(duration);
        self.max_duration = self.max_duration.max(duration);
        self.last_execution = Some(SystemTime::now());
    }

    /// Get success rate as a percentage
    pub fn success_rate(&self) -> f64 {
        let total = self.success_count + self.error_count;
        if total == 0 {
            return 0.0;
        }
        (self.success_count as f64 / total as f64) * 100.0
    }

    /// Get operations per second based on recent activity
    pub fn ops_per_second(&self, window: Duration) -> f64 {
        // TODO fix - Simplified calculation - would need more sophisticated tracking in production
        let total_ops = self.success_count + self.error_count;
        if total_ops == 0 || window.is_zero() {
            return 0.0;
        }

        total_ops as f64 / window.as_secs_f64()
    }
}

/// System-wide agent metrics
#[derive(Debug, Clone)]
pub struct AgentMetrics {
    /// Device ID for this agent
    pub device_id: DeviceId,
    /// Metrics by operation name
    pub operations: HashMap<String, OperationMetrics>,
    /// System start time
    pub start_time: SystemTime,
    /// Total operations across all types
    pub total_operations: u64,
    /// Memory usage statistics (TODO fix - Simplified)
    pub memory_usage_mb: u64,
    /// Active connections count
    pub active_connections: u32,
}

impl AgentMetrics {
    /// Create new agent metrics
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            device_id,
            operations: HashMap::new(),
            start_time: SystemTime::now(),
            total_operations: 0,
            memory_usage_mb: 0, // Would be populated by actual memory tracking
            active_connections: 0,
        }
    }

    /// Record an operation execution
    pub fn record_operation(&mut self, operation: &str, duration: Duration, success: bool) {
        let metrics = self
            .operations
            .entry(operation.to_string())
            .or_insert_with(|| OperationMetrics::new(operation.to_string()));

        if success {
            metrics.record_success(duration);
        } else {
            metrics.record_error(duration);
        }

        self.total_operations += 1;
    }

    /// Get metrics for a specific operation
    pub fn get_operation_metrics(&self, operation: &str) -> Option<&OperationMetrics> {
        self.operations.get(operation)
    }

    /// Get all operation names
    pub fn operation_names(&self) -> Vec<String> {
        self.operations.keys().cloned().collect()
    }

    /// Get system uptime
    pub fn uptime(&self) -> Duration {
        self.start_time.elapsed().unwrap_or_default()
    }

    /// Generate a summary report
    pub fn summary(&self) -> MetricsSummary {
        let total_success = self.operations.values().map(|m| m.success_count).sum();
        let total_errors = self.operations.values().map(|m| m.error_count).sum();
        let avg_success_rate = if self.operations.is_empty() {
            0.0
        } else {
            self.operations
                .values()
                .map(|m| m.success_rate())
                .sum::<f64>()
                / self.operations.len() as f64
        };

        MetricsSummary {
            device_id: self.device_id,
            uptime: self.uptime(),
            total_operations: self.total_operations,
            total_success,
            total_errors,
            avg_success_rate,
            unique_operations: self.operations.len(),
            memory_usage_mb: self.memory_usage_mb,
            active_connections: self.active_connections,
        }
    }
}

/// Summary metrics for reporting
#[derive(Debug, Clone)]
pub struct MetricsSummary {
    pub device_id: DeviceId,
    pub uptime: Duration,
    pub total_operations: u64,
    pub total_success: u64,
    pub total_errors: u64,
    pub avg_success_rate: f64,
    pub unique_operations: usize,
    pub memory_usage_mb: u64,
    pub active_connections: u32,
}

impl MetricsSummary {
    /// Generate a human-readable report
    pub fn report(&self) -> String {
        format!(
            "Agent Metrics Summary
Device ID: {}
Uptime: {:?}
Total Operations: {}
Success: {} ({:.1}%)
Errors: {} ({:.1}%)
Unique Operation Types: {}
Memory Usage: {} MB
Active Connections: {}",
            self.device_id,
            self.uptime,
            self.total_operations,
            self.total_success,
            if self.total_operations > 0 {
                (self.total_success as f64 / self.total_operations as f64) * 100.0
            } else {
                0.0
            },
            self.total_errors,
            if self.total_operations > 0 {
                (self.total_errors as f64 / self.total_operations as f64) * 100.0
            } else {
                0.0
            },
            self.unique_operations,
            self.memory_usage_mb,
            self.active_connections
        )
    }
}

/// Metrics collection middleware
pub struct MetricsMiddleware {
    /// Agent metrics data
    metrics: Arc<RwLock<AgentMetrics>>,
    /// Device ID
    device_id: DeviceId,
}

impl MetricsMiddleware {
    /// Create new metrics middleware
    pub async fn new(device_id: DeviceId) -> Result<Self> {
        let metrics = AgentMetrics::new(device_id);

        Ok(Self {
            metrics: Arc::new(RwLock::new(metrics)),
            device_id,
        })
    }

    /// Record an operation execution
    pub async fn record_operation(
        &self,
        operation: &str,
        duration: Duration,
        success: bool,
    ) -> Result<()> {
        let mut metrics = self.metrics.write().await;
        metrics.record_operation(operation, duration, success);
        Ok(())
    }

    /// Get current metrics snapshot
    pub async fn get_metrics(&self) -> AgentMetrics {
        let metrics = self.metrics.read().await;
        metrics.clone()
    }

    /// Get metrics summary
    pub async fn get_summary(&self) -> MetricsSummary {
        let metrics = self.metrics.read().await;
        metrics.summary()
    }

    /// Get operation metrics for a specific operation
    pub async fn get_operation_metrics(&self, operation: &str) -> Option<OperationMetrics> {
        let metrics = self.metrics.read().await;
        metrics.get_operation_metrics(operation).cloned()
    }

    /// Update connection count
    pub async fn update_connection_count(&self, count: u32) -> Result<()> {
        let mut metrics = self.metrics.write().await;
        metrics.active_connections = count;
        Ok(())
    }

    /// Update memory usage
    pub async fn update_memory_usage(&self, usage_mb: u64) -> Result<()> {
        let mut metrics = self.metrics.write().await;
        metrics.memory_usage_mb = usage_mb;
        Ok(())
    }

    /// Reset all metrics
    pub async fn reset(&self) -> Result<()> {
        let mut metrics = self.metrics.write().await;
        *metrics = AgentMetrics::new(self.device_id);
        Ok(())
    }

    /// Export metrics for external monitoring systems
    pub async fn export_metrics(&self) -> Result<String> {
        let metrics = self.metrics.read().await;
        let summary = metrics.summary();

        // Export in a simple format - could be enhanced for Prometheus, etc.
        let mut export = String::new();

        export.push_str(&format!("# Agent Metrics Export\n"));
        export.push_str(&format!("# Device: {}\n", summary.device_id));
        export.push_str(&format!(
            "# Generated: {}\n",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        ));
        export.push_str(&format!("\n"));

        export.push_str(&format!(
            "aura_agent_uptime_seconds {}\n",
            summary.uptime.as_secs()
        ));
        export.push_str(&format!(
            "aura_agent_total_operations {}\n",
            summary.total_operations
        ));
        export.push_str(&format!(
            "aura_agent_success_operations {}\n",
            summary.total_success
        ));
        export.push_str(&format!(
            "aura_agent_error_operations {}\n",
            summary.total_errors
        ));
        export.push_str(&format!(
            "aura_agent_success_rate {:.4}\n",
            summary.avg_success_rate / 100.0
        ));
        export.push_str(&format!(
            "aura_agent_memory_usage_mb {}\n",
            summary.memory_usage_mb
        ));
        export.push_str(&format!(
            "aura_agent_active_connections {}\n",
            summary.active_connections
        ));

        // Per-operation metrics
        for (op_name, op_metrics) in &metrics.operations {
            let clean_name = op_name.replace(' ', "_").replace('-', "_");
            export.push_str(&format!(
                "aura_agent_operation_success_total{{operation=\"{}\"}} {}\n",
                clean_name, op_metrics.success_count
            ));
            export.push_str(&format!(
                "aura_agent_operation_error_total{{operation=\"{}\"}} {}\n",
                clean_name, op_metrics.error_count
            ));
            export.push_str(&format!(
                "aura_agent_operation_duration_avg_seconds{{operation=\"{}\"}} {:.6}\n",
                clean_name,
                op_metrics.avg_duration.as_secs_f64()
            ));
            export.push_str(&format!(
                "aura_agent_operation_duration_min_seconds{{operation=\"{}\"}} {:.6}\n",
                clean_name,
                op_metrics.min_duration.as_secs_f64()
            ));
            export.push_str(&format!(
                "aura_agent_operation_duration_max_seconds{{operation=\"{}\"}} {:.6}\n",
                clean_name,
                op_metrics.max_duration.as_secs_f64()
            ));
        }

        Ok(export)
    }
}

/// Helper for collecting system metrics
pub struct SystemMetricsCollector;

impl SystemMetricsCollector {
    /// Get current memory usage (TODO fix - Simplified)
    pub fn get_memory_usage_mb() -> u64 {
        // TODO fix - In a real implementation, this would use platform-specific APIs
        // TODO fix - For now, return a placeholder
        #[cfg(target_os = "linux")]
        {
            // Could read /proc/self/status on Linux
        }
        #[cfg(target_os = "macos")]
        {
            // Could use mach APIs on macOS
        }
        #[cfg(target_os = "windows")]
        {
            // Could use Windows APIs
        }

        // Placeholder: return 0 TODO fix - For now
        0
    }

    /// Get current CPU usage percentage
    pub fn get_cpu_usage() -> f64 {
        // Placeholder implementation
        0.0
    }

    /// Get current disk usage
    pub fn get_disk_usage() -> (u64, u64) {
        // Returns (used_bytes, total_bytes)
        (0, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_operation_metrics() {
        let mut metrics = OperationMetrics::new("test_op".to_string());

        assert_eq!(metrics.success_count, 0);
        assert_eq!(metrics.error_count, 0);

        metrics.record_success(Duration::from_millis(100));
        assert_eq!(metrics.success_count, 1);
        assert_eq!(metrics.avg_duration, Duration::from_millis(100));

        metrics.record_error(Duration::from_millis(200));
        assert_eq!(metrics.error_count, 1);
        assert_eq!(metrics.avg_duration, Duration::from_millis(150));
        assert_eq!(metrics.success_rate(), 50.0);
    }

    #[tokio::test]
    async fn test_agent_metrics() {
        let device_id = DeviceId(uuid::Uuid::new_v4());
        let mut metrics = AgentMetrics::new(device_id);

        metrics.record_operation("test_op", Duration::from_millis(100), true);
        metrics.record_operation("test_op", Duration::from_millis(200), false);

        assert_eq!(metrics.total_operations, 2);

        let op_metrics = metrics.get_operation_metrics("test_op").unwrap();
        assert_eq!(op_metrics.success_count, 1);
        assert_eq!(op_metrics.error_count, 1);

        let summary = metrics.summary();
        assert_eq!(summary.total_operations, 2);
        assert_eq!(summary.total_success, 1);
        assert_eq!(summary.total_errors, 1);
    }

    #[tokio::test]
    async fn test_metrics_middleware() {
        let device_id = DeviceId(uuid::Uuid::new_v4());
        let middleware = MetricsMiddleware::new(device_id).await.unwrap();

        middleware
            .record_operation("test", Duration::from_millis(100), true)
            .await
            .unwrap();

        let metrics = middleware.get_metrics().await;
        assert_eq!(metrics.total_operations, 1);

        let summary = middleware.get_summary().await;
        assert_eq!(summary.total_success, 1);

        // Test export
        let export = middleware.export_metrics().await.unwrap();
        assert!(export.contains("aura_agent_total_operations"));
        assert!(export.contains("aura_agent_success_operations"));
    }

    #[test]
    fn test_metrics_summary_report() {
        let device_id = DeviceId(uuid::Uuid::new_v4());
        let summary = MetricsSummary {
            device_id,
            uptime: Duration::from_secs(3600), // 1 hour
            total_operations: 100,
            total_success: 95,
            total_errors: 5,
            avg_success_rate: 95.0,
            unique_operations: 5,
            memory_usage_mb: 128,
            active_connections: 3,
        };

        let report = summary.report();
        assert!(report.contains("95 (95.0%)"));
        assert!(report.contains("5 (5.0%)"));
        assert!(report.contains("128 MB"));
    }
}
