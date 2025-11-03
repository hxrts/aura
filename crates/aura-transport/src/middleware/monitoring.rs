//! Monitoring Middleware

use super::stack::TransportMiddleware;
use super::handler::{TransportHandler, TransportOperation, TransportResult};
use aura_types::effects::AuraEffects;
use aura_types::{MiddlewareContext, MiddlewareResult, AuraError};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct MonitoringConfig {
    pub enable_metrics: bool,
    pub enable_tracing: bool,
    pub sample_rate: f64, // 0.0 to 1.0
    pub metrics_interval_ms: u64,
    pub max_operation_history: usize,
}

impl Default for MonitoringConfig {
    fn default() -> Self {
        Self {
            enable_metrics: true,
            enable_tracing: true,
            sample_rate: 1.0, // Sample everything by default
            metrics_interval_ms: 60000, // 1 minute
            max_operation_history: 1000,
        }
    }
}

#[derive(Debug, Clone)]
struct OperationMetrics {
    operation_type: String,
    start_time: u64,
    end_time: u64,
    duration_ms: u64,
    bytes_transferred: usize,
    success: bool,
    error_message: Option<String>,
    destination: Option<String>,
}

#[derive(Debug, Default)]
struct AggregatedMetrics {
    total_operations: u64,
    successful_operations: u64,
    failed_operations: u64,
    total_bytes_sent: u64,
    total_bytes_received: u64,
    total_duration_ms: u64,
    avg_latency_ms: f64,
    operations_per_second: f64,
    last_metrics_reset: u64,
}

impl AggregatedMetrics {
    fn update(&mut self, operation: &OperationMetrics, current_time: u64) {
        self.total_operations += 1;
        if operation.success {
            self.successful_operations += 1;
        } else {
            self.failed_operations += 1;
        }
        
        self.total_duration_ms += operation.duration_ms;
        self.avg_latency_ms = self.total_duration_ms as f64 / self.total_operations as f64;
        
        // Update operations per second
        let time_window_ms = current_time.saturating_sub(self.last_metrics_reset);
        if time_window_ms > 0 {
            self.operations_per_second = (self.total_operations as f64 * 1000.0) / time_window_ms as f64;
        }
        
        // Track bytes transferred
        match operation.operation_type.as_str() {
            "Send" => self.total_bytes_sent += operation.bytes_transferred as u64,
            "Receive" => self.total_bytes_received += operation.bytes_transferred as u64,
            _ => {}
        }
    }
    
    fn reset(&mut self, current_time: u64) {
        *self = AggregatedMetrics::default();
        self.last_metrics_reset = current_time;
    }
}

pub struct MonitoringMiddleware {
    config: MonitoringConfig,
    metrics: AggregatedMetrics,
    operation_history: Vec<OperationMetrics>,
    per_destination_metrics: HashMap<String, AggregatedMetrics>,
    last_metrics_log: u64,
}

impl MonitoringMiddleware {
    pub fn new() -> Self {
        Self {
            config: MonitoringConfig::default(),
            metrics: AggregatedMetrics::default(),
            operation_history: Vec::new(),
            per_destination_metrics: HashMap::new(),
            last_metrics_log: 0,
        }
    }
    
    pub fn with_config(config: MonitoringConfig) -> Self {
        Self {
            config,
            metrics: AggregatedMetrics::default(),
            operation_history: Vec::new(),
            per_destination_metrics: HashMap::new(),
            last_metrics_log: 0,
        }
    }
    
    fn should_sample(&self, effects: &dyn AuraEffects) -> bool {
        if self.config.sample_rate >= 1.0 {
            return true;
        }
        
        // Use timestamp as pseudo-random source for sampling
        let timestamp = effects.current_timestamp();
        let sample_threshold = (self.config.sample_rate * 1000.0) as u64;
        (timestamp % 1000) < sample_threshold
    }
    
    fn get_operation_type(operation: &TransportOperation) -> String {
        match operation {
            TransportOperation::Send { .. } => "Send".to_string(),
            TransportOperation::Receive { .. } => "Receive".to_string(),
            TransportOperation::Connect { .. } => "Connect".to_string(),
            TransportOperation::Disconnect { .. } => "Disconnect".to_string(),
            TransportOperation::Listen { .. } => "Listen".to_string(),
            TransportOperation::Discover { .. } => "Discover".to_string(),
            TransportOperation::Status { .. } => "Status".to_string(),
        }
    }
    
    fn get_destination(operation: &TransportOperation) -> Option<String> {
        match operation {
            TransportOperation::Send { destination, .. } => Some(destination.as_string()),
            TransportOperation::Connect { address, .. } => Some(address.as_string()),
            TransportOperation::Disconnect { address } => Some(address.as_string()),
            TransportOperation::Listen { address, .. } => Some(address.as_string()),
            _ => None,
        }
    }
    
    fn get_bytes_transferred(operation: &TransportOperation, result: &Result<TransportResult, AuraError>) -> usize {
        match (operation, result) {
            (TransportOperation::Send { data, .. }, _) => data.len(),
            (_, Ok(TransportResult::Received { data, .. })) => data.len(),
            (_, Ok(TransportResult::Sent { bytes_sent, .. })) => *bytes_sent,
            _ => 0,
        }
    }
    
    fn record_operation(&mut self, operation_metrics: OperationMetrics, current_time: u64) {
        // Update global metrics
        self.metrics.update(&operation_metrics, current_time);
        
        // Update per-destination metrics
        if let Some(ref destination) = operation_metrics.destination {
            let dest_metrics = self.per_destination_metrics
                .entry(destination.clone())
                .or_insert_with(AggregatedMetrics::default);
            dest_metrics.update(&operation_metrics, current_time);
        }
        
        // Add to operation history
        if self.operation_history.len() >= self.config.max_operation_history {
            self.operation_history.remove(0);
        }
        self.operation_history.push(operation_metrics);
    }
    
    fn log_metrics(&mut self, effects: &dyn AuraEffects, current_time: u64) {
        if current_time.saturating_sub(self.last_metrics_log) >= self.config.metrics_interval_ms {
            effects.log_info(
                &format!(
                    "Transport Metrics - Operations: {} (success: {}, failed: {}), Avg Latency: {:.1}ms, Ops/sec: {:.1}, Bytes Sent: {}, Bytes Received: {}",
                    self.metrics.total_operations,
                    self.metrics.successful_operations,
                    self.metrics.failed_operations,
                    self.metrics.avg_latency_ms,
                    self.metrics.operations_per_second,
                    self.metrics.total_bytes_sent,
                    self.metrics.total_bytes_received
                ),
                &[]
            );
            
            // Log top destinations
            let mut dest_ops: Vec<_> = self.per_destination_metrics
                .iter()
                .map(|(dest, metrics)| (dest.clone(), metrics.total_operations))
                .collect();
            dest_ops.sort_by(|a, b| b.1.cmp(&a.1));
            
            if !dest_ops.is_empty() {
                let top_destinations: Vec<String> = dest_ops
                    .into_iter()
                    .take(5)
                    .map(|(dest, ops)| format!("{}({})", dest, ops))
                    .collect();
                
                effects.log_info(
                    &format!("Top Destinations: {}", top_destinations.join(", ")),
                    &[]
                );
            }
            
            self.last_metrics_log = current_time;
        }
    }
    
    fn add_monitoring_metadata(&self, metadata: &mut HashMap<String, String>, start_time: u64) {
        if self.config.enable_tracing {
            metadata.insert("trace_id".to_string(), format!("trace_{}", start_time));
            metadata.insert("monitored".to_string(), "true".to_string());
        }
    }
}

impl Default for MonitoringMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl TransportMiddleware for MonitoringMiddleware {
    fn process(
        &mut self,
        operation: TransportOperation,
        context: &MiddlewareContext,
        effects: &dyn AuraEffects,
        next: &mut dyn TransportHandler,
    ) -> MiddlewareResult<TransportResult> {
        let current_time = effects.current_timestamp();
        let start_time = current_time;
        
        // Check if we should sample this operation
        if !self.should_sample(effects) {
            return next.execute(operation, effects);
        }
        
        let operation_type = Self::get_operation_type(&operation);
        let destination = Self::get_destination(&operation);
        
        // Add monitoring metadata if tracing is enabled
        let mut modified_operation = operation.clone();
        if self.config.enable_tracing {
            match &mut modified_operation {
                TransportOperation::Send { metadata, .. } => {
                    self.add_monitoring_metadata(metadata, start_time);
                }
                _ => {}
            }
        }
        
        // Log operation start if tracing is enabled
        if self.config.enable_tracing {
            effects.log_info(
                &format!("Starting {} operation to {:?}", operation_type, destination),
                &[
                    ("operation_type", &operation_type),
                    ("operation_name", &context.operation_name),
                    ("start_time", &start_time.to_string()),
                ]
            );
        }
        
        // Execute the operation
        let result = next.execute(modified_operation, effects);
        let end_time = effects.current_timestamp();
        let duration_ms = end_time.saturating_sub(start_time);
        
        // Record metrics
        if self.config.enable_metrics {
            let bytes_transferred = Self::get_bytes_transferred(&operation, &result);
            let success = result.is_ok();
            let error_message = if let Err(ref e) = result {
                Some(format!("{}", e))
            } else {
                None
            };
            
            let operation_metrics = OperationMetrics {
                operation_type: operation_type.clone(),
                start_time,
                end_time,
                duration_ms,
                bytes_transferred,
                success,
                error_message: error_message.clone(),
                destination: destination.clone(),
            };
            
            self.record_operation(operation_metrics, current_time);
        }
        
        // Log operation completion if tracing is enabled
        if self.config.enable_tracing {
            let status = if result.is_ok() { "SUCCESS" } else { "FAILED" };
            effects.log_info(
                &format!("{} {} operation in {}ms", status, operation_type, duration_ms),
                &[
                    ("operation_type", &operation_type),
                    ("duration_ms", &duration_ms.to_string()),
                    ("status", status),
                ]
            );
            
            if let Err(ref e) = result {
                effects.log_error(
                    &format!("Operation failed: {}", e),
                    &[
                        ("operation_type", &operation_type),
                        ("error", &format!("{}", e)),
                    ]
                );
            }
        }
        
        // Periodic metrics logging
        self.log_metrics(effects, current_time);
        
        result
    }
    
    fn middleware_name(&self) -> &'static str {
        "MonitoringMiddleware"
    }
    
    fn middleware_info(&self) -> HashMap<String, String> {
        let mut info = HashMap::new();
        info.insert("enable_metrics".to_string(), self.config.enable_metrics.to_string());
        info.insert("enable_tracing".to_string(), self.config.enable_tracing.to_string());
        info.insert("sample_rate".to_string(), self.config.sample_rate.to_string());
        info.insert("total_operations".to_string(), self.metrics.total_operations.to_string());
        info.insert("successful_operations".to_string(), self.metrics.successful_operations.to_string());
        info.insert("failed_operations".to_string(), self.metrics.failed_operations.to_string());
        info.insert("avg_latency_ms".to_string(), format!("{:.2}", self.metrics.avg_latency_ms));
        info.insert("operations_per_second".to_string(), format!("{:.2}", self.metrics.operations_per_second));
        info.insert("total_bytes_sent".to_string(), self.metrics.total_bytes_sent.to_string());
        info.insert("total_bytes_received".to_string(), self.metrics.total_bytes_received.to_string());
        info.insert("operation_history_size".to_string(), self.operation_history.len().to_string());
        info.insert("tracked_destinations".to_string(), self.per_destination_metrics.len().to_string());
        info
    }
}