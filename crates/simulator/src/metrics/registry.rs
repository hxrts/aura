//! Global metrics registry for centralized collection

use super::*;
use std::sync::{Arc, Mutex, OnceLock};

/// Global metrics registry
static GLOBAL_REGISTRY: OnceLock<Arc<Mutex<MetricRegistry>>> = OnceLock::new();

/// Centralized metric registry
#[derive(Debug)]
pub struct MetricRegistry {
    /// Named collectors
    collectors: HashMap<String, Arc<MetricsCollector>>,
    /// Default collector
    default_collector: Arc<MetricsCollector>,
    /// Registry metadata
    metadata: HashMap<String, String>,
}

/// Global metrics access helper
pub struct GlobalMetrics;

impl MetricRegistry {
    /// Create a new metric registry
    pub fn new() -> Self {
        Self {
            collectors: HashMap::new(),
            default_collector: Arc::new(MetricsCollector::new()),
            metadata: HashMap::new(),
        }
    }

    /// Register a named collector
    pub fn register_collector<S: Into<String>>(&mut self, name: S, collector: MetricsCollector) {
        self.collectors.insert(name.into(), Arc::new(collector));
    }

    /// Get a named collector
    pub fn get_collector<S: AsRef<str>>(&self, name: S) -> Option<Arc<MetricsCollector>> {
        self.collectors.get(name.as_ref()).cloned()
    }

    /// Get the default collector
    pub fn default_collector(&self) -> Arc<MetricsCollector> {
        self.default_collector.clone()
    }

    /// Get all registered collector names
    pub fn collector_names(&self) -> Vec<String> {
        self.collectors.keys().cloned().collect()
    }

    /// Get combined snapshot from all collectors
    pub fn global_snapshot(&self) -> GlobalMetricsSnapshot {
        let mut snapshots = HashMap::new();

        // Add default collector
        snapshots.insert("default".to_string(), self.default_collector.snapshot());

        // Add named collectors
        for (name, collector) in &self.collectors {
            snapshots.insert(name.clone(), collector.snapshot());
        }

        GlobalMetricsSnapshot {
            timestamp: crate::utils::time::current_unix_timestamp_secs(),
            snapshots,
            metadata: self.metadata.clone(),
        }
    }

    /// Reset all collectors
    pub fn reset_all(&self) {
        self.default_collector.reset();
        for collector in self.collectors.values() {
            collector.reset();
        }
    }

    /// Add registry metadata
    pub fn add_metadata<K, V>(&mut self, key: K, value: V)
    where
        K: Into<String>,
        V: Into<String>,
    {
        self.metadata.insert(key.into(), value.into());
    }

    /// Enable/disable all collectors
    pub fn set_enabled(&mut self, enabled: bool) {
        // Note: MetricsCollector doesn't have a mutable set_enabled method,
        // so we'd need to modify that or handle this differently
        // For now, we'll just store the setting in metadata
        self.metadata
            .insert("enabled".to_string(), enabled.to_string());
    }
}

/// Combined snapshot from all collectors
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalMetricsSnapshot {
    /// Timestamp when snapshot was taken
    pub timestamp: u64,
    /// Snapshots from all collectors
    pub snapshots: HashMap<String, MetricsSnapshot>,
    /// Registry metadata
    pub metadata: HashMap<String, String>,
}

impl GlobalMetrics {
    /// Initialize the global registry
    pub fn init() -> Arc<Mutex<MetricRegistry>> {
        GLOBAL_REGISTRY
            .get_or_init(|| Arc::new(Mutex::new(MetricRegistry::new())))
            .clone()
    }

    /// Get the global registry
    pub fn registry() -> Arc<Mutex<MetricRegistry>> {
        Self::init()
    }

    /// Get the default global collector
    pub fn default_collector() -> Arc<MetricsCollector> {
        let registry = Self::registry();
        let registry_lock = registry.lock().unwrap();
        registry_lock.default_collector()
    }

    /// Register a named collector globally
    pub fn register<S: Into<String>>(name: S, collector: MetricsCollector) {
        let registry = Self::registry();
        let mut registry_lock = registry.lock().unwrap();
        registry_lock.register_collector(name, collector);
    }

    /// Get a named collector from global registry
    pub fn get<S: AsRef<str>>(name: S) -> Option<Arc<MetricsCollector>> {
        let registry = Self::registry();
        let registry_lock = registry.lock().unwrap();
        registry_lock.get_collector(name)
    }

    /// Get global snapshot
    pub fn snapshot() -> GlobalMetricsSnapshot {
        let registry = Self::registry();
        let registry_lock = registry.lock().unwrap();
        registry_lock.global_snapshot()
    }

    /// Reset all global metrics
    pub fn reset_all() {
        let registry = Self::registry();
        let registry_lock = registry.lock().unwrap();
        registry_lock.reset_all();
    }

    /// Convenience methods for default collector
    pub fn counter<S: Into<String>>(name: S, value: u64) {
        Self::default_collector().counter(&name.into(), value);
    }

    pub fn gauge<S: Into<String>>(name: S, value: f64) {
        Self::default_collector().gauge(&name.into(), value);
    }

    pub fn histogram<S: Into<String>>(name: S, value: f64) {
        Self::default_collector().histogram(&name.into(), value);
    }

    pub fn timer<S: Into<String>>(name: S, duration_ms: u64) {
        Self::default_collector().timer(&name.into(), duration_ms);
    }
}

impl Default for MetricRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience macros for global metrics
#[macro_export]
macro_rules! global_counter {
    ($name:expr, $value:expr) => {
        $crate::metrics::GlobalMetrics::counter($name, $value);
    };
}

#[macro_export]
macro_rules! global_gauge {
    ($name:expr, $value:expr) => {
        $crate::metrics::GlobalMetrics::gauge($name, $value);
    };
}

#[macro_export]
macro_rules! global_histogram {
    ($name:expr, $value:expr) => {
        $crate::metrics::GlobalMetrics::histogram($name, $value);
    };
}

#[macro_export]
macro_rules! global_timer {
    ($name:expr, $duration:expr) => {
        $crate::metrics::GlobalMetrics::timer($name, $duration);
    };
}

#[macro_export]
macro_rules! global_time_block {
    ($name:expr, $block:block) => {{
        let _timer = $crate::metrics::GlobalMetrics::timer_start($name);
        $block
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metric_registry_basic() {
        let mut registry = MetricRegistry::new();

        let custom_collector = MetricsCollector::new();
        custom_collector.counter("test_metric", 42);

        registry.register_collector("custom", custom_collector);

        assert!(registry.get_collector("custom").is_some());
        assert!(registry.get_collector("nonexistent").is_none());
        assert_eq!(registry.collector_names(), vec!["custom"]);
    }

    #[test]
    fn test_metric_registry_global_snapshot() {
        let mut registry = MetricRegistry::new();

        // Add metrics to default collector
        registry.default_collector().counter("default_metric", 100);

        // Add custom collector
        let custom_collector = MetricsCollector::new();
        custom_collector.gauge("custom_metric", 3.14);
        registry.register_collector("custom", custom_collector);

        let snapshot = registry.global_snapshot();
        assert_eq!(snapshot.snapshots.len(), 2); // default + custom
        assert!(snapshot.snapshots.contains_key("default"));
        assert!(snapshot.snapshots.contains_key("custom"));
    }

    #[test]
    fn test_metric_registry_metadata() {
        let mut registry = MetricRegistry::new();
        registry.add_metadata("version", "1.0.0");
        registry.add_metadata("environment", "test");

        let snapshot = registry.global_snapshot();
        assert_eq!(snapshot.metadata.get("version"), Some(&"1.0.0".to_string()));
        assert_eq!(
            snapshot.metadata.get("environment"),
            Some(&"test".to_string())
        );
    }

    #[test]
    fn test_global_metrics_basic() {
        // Reset to clean state
        GlobalMetrics::reset_all();

        GlobalMetrics::counter("global_counter", 42);
        GlobalMetrics::gauge("global_gauge", 2.71);

        let snapshot = GlobalMetrics::snapshot();
        let default_snapshot = snapshot.snapshots.get("default").unwrap();

        assert!(default_snapshot
            .metrics
            .custom
            .contains_key("global_counter"));
        assert!(default_snapshot.metrics.custom.contains_key("global_gauge"));
    }

    #[test]
    fn test_global_metrics_named_collectors() {
        GlobalMetrics::reset_all();

        let test_collector = MetricsCollector::new();
        test_collector.timer("test_timer", 1000);

        GlobalMetrics::register("test_collector", test_collector);

        let retrieved = GlobalMetrics::get("test_collector");
        assert!(retrieved.is_some());

        let snapshot = GlobalMetrics::snapshot();
        assert!(snapshot.snapshots.contains_key("test_collector"));
    }

    #[test]
    fn test_global_metric_macros() {
        GlobalMetrics::reset_all();

        global_counter!("macro_counter", 123);
        global_gauge!("macro_gauge", 4.56);
        global_timer!("macro_timer", 789);

        let result = global_time_block!("macro_block", {
            std::thread::sleep(std::time::Duration::from_millis(1));
            "test_result"
        });

        assert_eq!(result, "test_result");

        let snapshot = GlobalMetrics::snapshot();
        let default_snapshot = snapshot.snapshots.get("default").unwrap();

        assert!(default_snapshot
            .metrics
            .custom
            .contains_key("macro_counter"));
        assert!(default_snapshot.metrics.custom.contains_key("macro_gauge"));
        assert!(default_snapshot.metrics.custom.contains_key("macro_timer"));
        assert!(default_snapshot.metrics.custom.contains_key("macro_block"));
    }

    #[test]
    fn test_registry_reset_all() {
        let mut registry = MetricRegistry::new();

        registry.default_collector().counter("test1", 42);

        let custom_collector = MetricsCollector::new();
        custom_collector.gauge("test2", 3.14);
        registry.register_collector("custom", custom_collector);

        // Verify metrics exist
        let snapshot_before = registry.global_snapshot();
        assert!(!snapshot_before
            .snapshots
            .get("default")
            .unwrap()
            .metrics
            .custom
            .is_empty());

        // Reset and verify
        registry.reset_all();
        let snapshot_after = registry.global_snapshot();
        assert!(snapshot_after
            .snapshots
            .get("default")
            .unwrap()
            .metrics
            .custom
            .is_empty());
    }
}
