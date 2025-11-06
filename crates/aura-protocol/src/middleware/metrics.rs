//! Metrics collection middleware

use super::{MiddlewareContext, AuraMiddleware};
use std::future::Future;
use std::pin::Pin;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::marker::PhantomData;

/// Metrics collection middleware
pub struct MetricsMiddleware<Req, Resp, Err>
where
    Req: Send + Sync,
    Resp: Send + Sync,
    Err: std::error::Error + Send + Sync + 'static,
{
    /// Metrics collector
    collector: Arc<dyn MetricsCollector>,
    
    /// Metrics configuration
    config: MetricsConfig,
    
    /// Phantom data to use type parameters
    _phantom: PhantomData<(Req, Resp, Err)>,
}

impl<Req, Resp, Err> MetricsMiddleware<Req, Resp, Err>
where
    Req: Send + Sync,
    Resp: Send + Sync,
    Err: std::error::Error + Send + Sync + 'static,
{
    /// Create new metrics middleware
    pub fn new(collector: Arc<dyn MetricsCollector>) -> Self {
        Self {
            collector,
            config: MetricsConfig::default(),
            _phantom: PhantomData,
        }
    }

    /// Create metrics middleware with custom configuration
    pub fn with_config(collector: Arc<dyn MetricsCollector>, config: MetricsConfig) -> Self {
        Self {
            collector,
            config,
            _phantom: PhantomData,
        }
    }
}

impl<Req, Resp, Err> AuraMiddleware for MetricsMiddleware<Req, Resp, Err>
where
    Req: Send + Sync + 'static,
    Resp: Send + Sync + 'static,
    Err: std::error::Error + Send + Sync + 'static,
{
    type Request = Req;
    type Response = Resp;
    type Error = Err;

    fn process<'a>(
        &'a self,
        request: Self::Request,
        context: &'a MiddlewareContext,
        effects: &'a dyn super::super::effects::Effects,
        next: Box<dyn super::traits::MiddlewareHandler<Self::Request, Self::Response, Self::Error>>,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'a>> {
        let collector = self.collector.clone();
        let config = self.config.clone();
        let context = context.clone();
        let start_time = std::time::Instant::now();

        // Record request start metrics before async block
        let request_labels: Vec<(String, String)> = vec![
            ("component".to_string(), context.component.clone()),
            ("protocol".to_string(), context.protocol.clone().unwrap_or_else(|| "unknown".to_string())),
        ];

        if config.collect_request_metrics {
            let request_label_refs: Vec<(&str, &str)> = request_labels.iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            collector.record_counter(
                "aura_requests_total",
                1,
                &request_label_refs
            );
        }

        Box::pin(async move {
            // Execute the request
            let result = next.handle(request, &context, effects).await;
            
            let duration = start_time.elapsed();
            
            // Record response metrics after async block
            match &result {
                Ok(_) => {
                    if config.collect_response_metrics {
                        let mut success_labels = request_labels.clone();
                        success_labels.push(("status".to_string(), "success".to_string()));
                        
                        let success_label_refs: Vec<(&str, &str)> = success_labels.iter()
                            .map(|(k, v)| (k.as_str(), v.as_str()))
                            .collect();
                        collector.record_counter(
                            "aura_responses_total",
                            1,
                            &success_label_refs
                        );
                    }
                }
                Err(error) => {
                    if config.collect_error_metrics {
                        let error_string = error.to_string();
                        let mut error_labels = request_labels.clone();
                        error_labels.push(("status".to_string(), "error".to_string()));
                        error_labels.push(("error_type".to_string(), error_string));
                        
                        let error_label_refs: Vec<(&str, &str)> = error_labels.iter()
                            .map(|(k, v)| (k.as_str(), v.as_str()))
                            .collect();
                        collector.record_counter(
                            "aura_errors_total",
                            1,
                            &error_label_refs
                        );
                    }
                }
            }
            
            // Record timing metrics
            if config.collect_timing_metrics {
                let timing_label_refs: Vec<(&str, &str)> = request_labels.iter()
                    .map(|(k, v)| (k.as_str(), v.as_str()))
                    .collect();
                collector.record_histogram(
                    "aura_request_duration_seconds",
                    duration.as_secs_f64(),
                    &timing_label_refs
                );
            }

            result
        })
    }
}

/// Metrics collection configuration
#[derive(Debug, Clone)]
pub struct MetricsConfig {
    /// Whether to collect request metrics
    pub collect_request_metrics: bool,
    
    /// Whether to collect response metrics
    pub collect_response_metrics: bool,
    
    /// Whether to collect error metrics
    pub collect_error_metrics: bool,
    
    /// Whether to collect timing metrics
    pub collect_timing_metrics: bool,
    
    /// Whether to collect resource usage metrics
    pub collect_resource_metrics: bool,
    
    /// Sampling rate (0.0 to 1.0)
    pub sampling_rate: f64,
    
    /// Custom metric prefixes
    pub metric_prefix: String,
    
    /// Additional labels to add to all metrics
    pub default_labels: HashMap<String, String>,
    
    /// Histogram buckets for timing metrics
    pub histogram_buckets: Vec<f64>,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            collect_request_metrics: true,
            collect_response_metrics: true,
            collect_error_metrics: true,
            collect_timing_metrics: true,
            collect_resource_metrics: false,
            sampling_rate: 1.0,
            metric_prefix: "aura".to_string(),
            default_labels: HashMap::new(),
            histogram_buckets: vec![
                0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0
            ],
        }
    }
}

/// Metrics collector trait
pub trait MetricsCollector: Send + Sync {
    /// Record a counter metric
    fn record_counter(
        &self,
        name: &str,
        value: u64,
        labels: &[(&str, &str)],
    );

    /// Record a gauge metric
    fn record_gauge(
        &self,
        name: &str,
        value: f64,
        labels: &[(&str, &str)],
    );

    /// Record a histogram metric
    fn record_histogram(
        &self,
        name: &str,
        value: f64,
        labels: &[(&str, &str)],
    );

    /// Record a timing metric
    fn record_timing(
        &self,
        name: &str,
        duration: std::time::Duration,
        labels: &[(&str, &str)],
    ) {
        self.record_histogram(name, duration.as_secs_f64(), labels);
    }

    /// Get current metric values
    fn get_metrics(&self) -> HashMap<String, MetricValue>;

    /// Reset all metrics
    fn reset_metrics(&self);
}

/// Types of metrics that can be recorded
#[derive(Debug, Clone)]
pub enum MetricType {
    /// Counter - monotonically increasing value
    Counter,
    
    /// Gauge - arbitrary value that can go up or down
    Gauge,
    
    /// Histogram - distribution of values
    Histogram,
    
    /// Summary - quantiles of observed values
    Summary,
}

/// Metric event for recording metrics
#[derive(Debug, Clone)]
pub struct MetricEvent {
    /// Metric name
    pub name: String,
    
    /// Metric type
    pub metric_type: MetricType,
    
    /// Metric value
    pub value: f64,
    
    /// Metric labels
    pub labels: HashMap<String, String>,
    
    /// Event timestamp
    pub timestamp: std::time::Instant,
    
    /// Component that generated this metric
    pub component: String,
}

impl MetricEvent {
    /// Create a new metric event
    pub fn new(name: &str, metric_type: MetricType, value: f64) -> Self {
        Self {
            name: name.to_string(),
            metric_type,
            value,
            labels: HashMap::new(),
            timestamp: std::time::Instant::now(),
            component: "unknown".to_string(),
        }
    }

    /// Add a label to the metric
    pub fn with_label(mut self, key: &str, value: &str) -> Self {
        self.labels.insert(key.to_string(), value.to_string());
        self
    }

    /// Set the component name
    pub fn with_component(mut self, component: &str) -> Self {
        self.component = component.to_string();
        self
    }

    /// Get the age of this metric event
    pub fn age(&self) -> std::time::Duration {
        self.timestamp.elapsed()
    }
}

/// Metric value representation
#[derive(Debug, Clone)]
pub enum MetricValue {
    /// Counter value
    Counter(u64),
    
    /// Gauge value
    Gauge(f64),
    
    /// Histogram with buckets
    Histogram {
        count: u64,
        sum: f64,
        buckets: HashMap<String, u64>,
    },
    
    /// Summary with quantiles
    Summary {
        count: u64,
        sum: f64,
        quantiles: HashMap<String, f64>,
    },
}

/// In-memory metrics collector implementation
pub struct InMemoryMetricsCollector {
    /// Counter metrics
    counters: Arc<std::sync::RwLock<HashMap<String, AtomicU64>>>,
    
    /// Gauge metrics
    gauges: Arc<std::sync::RwLock<HashMap<String, Arc<std::sync::RwLock<f64>>>>>,
    
    /// Histogram metrics
    histograms: Arc<std::sync::RwLock<HashMap<String, HistogramData>>>,
}

#[derive(Debug)]
struct HistogramData {
    count: AtomicU64,
    sum: Arc<std::sync::RwLock<f64>>,
    buckets: HashMap<String, AtomicU64>,
}

impl Default for InMemoryMetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryMetricsCollector {
    /// Create a new in-memory metrics collector
    pub fn new() -> Self {
        Self {
            counters: Arc::new(std::sync::RwLock::new(HashMap::new())),
            gauges: Arc::new(std::sync::RwLock::new(HashMap::new())),
            histograms: Arc::new(std::sync::RwLock::new(HashMap::new())),
        }
    }
    
    /// Get the metric key from name and labels
    fn metric_key(name: &str, labels: &[(&str, &str)]) -> String {
        if labels.is_empty() {
            name.to_string()
        } else {
            let label_str = labels
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join(",");
            format!("{}{{{}}}", name, label_str)
        }
    }
}

impl MetricsCollector for InMemoryMetricsCollector {
    fn record_counter(
        &self,
        name: &str,
        value: u64,
        labels: &[(&str, &str)],
    ) {
        let key = Self::metric_key(name, labels);
        let counters = self.counters.read().unwrap();
        
        if let Some(counter) = counters.get(&key) {
            counter.fetch_add(value, Ordering::Relaxed);
        } else {
            drop(counters);
            let mut counters = self.counters.write().unwrap();
            counters.entry(key).or_insert_with(|| AtomicU64::new(0))
                .fetch_add(value, Ordering::Relaxed);
        }
    }

    fn record_gauge(
        &self,
        name: &str,
        value: f64,
        labels: &[(&str, &str)],
    ) {
        let key = Self::metric_key(name, labels);
        let gauges = self.gauges.read().unwrap();
        
        if let Some(gauge) = gauges.get(&key) {
            *gauge.write().unwrap() = value;
        } else {
            drop(gauges);
            let mut gauges = self.gauges.write().unwrap();
            gauges.insert(key, Arc::new(std::sync::RwLock::new(value)));
        }
    }

    fn record_histogram(
        &self,
        name: &str,
        value: f64,
        labels: &[(&str, &str)],
    ) {
        let key = Self::metric_key(name, labels);
        let histograms = self.histograms.read().unwrap();
        
        if let Some(histogram) = histograms.get(&key) {
            histogram.count.fetch_add(1, Ordering::Relaxed);
            *histogram.sum.write().unwrap() += value;
            
            // Update buckets based on value
            for (bucket_name, bucket_counter) in &histogram.buckets {
                if let Ok(bucket_value) = bucket_name.parse::<f64>() {
                    if value <= bucket_value {
                        bucket_counter.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        } else {
            drop(histograms);
            let mut histograms = self.histograms.write().unwrap();
            
            // Create histogram with default buckets
            let mut buckets = HashMap::new();
            for &bucket in &[0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0] {
                buckets.insert(bucket.to_string(), AtomicU64::new(if value <= bucket { 1 } else { 0 }));
            }
            
            let histogram_data = HistogramData {
                count: AtomicU64::new(1),
                sum: Arc::new(std::sync::RwLock::new(value)),
                buckets,
            };
            
            histograms.insert(key, histogram_data);
        }
    }

    fn get_metrics(&self) -> HashMap<String, MetricValue> {
        let mut metrics = HashMap::new();
        
        // Add counters
        for (key, counter) in self.counters.read().unwrap().iter() {
            metrics.insert(
                key.clone(),
                MetricValue::Counter(counter.load(Ordering::Relaxed)),
            );
        }
        
        // Add gauges
        for (key, gauge) in self.gauges.read().unwrap().iter() {
            metrics.insert(
                key.clone(),
                MetricValue::Gauge(*gauge.read().unwrap()),
            );
        }
        
        // Add histograms
        for (key, histogram) in self.histograms.read().unwrap().iter() {
            let mut buckets = HashMap::new();
            for (bucket_name, bucket_counter) in &histogram.buckets {
                buckets.insert(bucket_name.clone(), bucket_counter.load(Ordering::Relaxed));
            }
            
            metrics.insert(
                key.clone(),
                MetricValue::Histogram {
                    count: histogram.count.load(Ordering::Relaxed),
                    sum: *histogram.sum.read().unwrap(),
                    buckets,
                },
            );
        }
        
        metrics
    }

    fn reset_metrics(&self) {
        self.counters.write().unwrap().clear();
        self.gauges.write().unwrap().clear();
        self.histograms.write().unwrap().clear();
    }
}

/// Convenience functions for creating common metrics middleware
impl<Req, Resp, Err> MetricsMiddleware<Req, Resp, Err>
where
    Req: Send + Sync,
    Resp: Send + Sync,
    Err: std::error::Error + Send + Sync + 'static,
{
    /// Create metrics middleware with in-memory collector
    pub fn in_memory() -> Self {
        Self::new(Arc::new(InMemoryMetricsCollector::new()))
    }

    /// Create metrics middleware with default configuration
    pub fn default_config() -> Self {
        Self::new(Arc::new(InMemoryMetricsCollector::new()))
    }

    /// Create metrics middleware for specific component
    pub fn for_component(component: &str) -> Self {
        let mut config = MetricsConfig::default();
        config.default_labels.insert("component".to_string(), component.to_string());
        Self::with_config(Arc::new(InMemoryMetricsCollector::new()), config)
    }
}