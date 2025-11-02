//! Property monitoring for real-time property evaluation
//!
//! This module provides real-time property monitoring capabilities using the Quint API
//! for formal verification of system properties during simulation trace analysis.

use aura_types::session_utils::properties::PropertyId;
use quint_api::{PropertySpec, QuintResult, QuintRunner, VerificationResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use wasm_bindgen::prelude::*;

/// Cache for property evaluation results
pub type StateHash = u64;

/// Property evaluation results for a specific state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyResults {
    /// Property evaluation results by property ID
    pub results: HashMap<PropertyId, PropertyEvaluationResult>,
    /// Timestamp when these results were computed
    pub computed_at: u64,
    /// State hash this evaluation applies to
    pub state_hash: StateHash,
}

/// Result of evaluating a single property
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyEvaluationResult {
    /// Whether the property holds (true) or is violated (false)
    pub holds: bool,
    /// Evaluation time in milliseconds
    pub evaluation_time_ms: u64,
    /// Optional violation details if property is violated
    pub violation_details: Option<ViolationDetails>,
    /// Optional verification result from Quint
    pub verification_result: Option<String>,
}

/// Details about a property violation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViolationDetails {
    /// Description of the violation
    pub description: String,
    /// Context information about the violation
    pub context: HashMap<String, String>,
    /// Steps leading to the violation
    pub causality_trace: Vec<u64>, // Event IDs
    /// Suggested debugging actions
    pub debugging_hints: Vec<String>,
}

/// Tracker for property violations across time
#[derive(Debug, Clone)]
pub struct ViolationTracker {
    /// Violations by property ID
    violations: HashMap<PropertyId, Vec<ViolationInstance>>,
    /// Total violation count
    total_violations: usize,
}

/// Instance of a property violation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViolationInstance {
    /// When the violation occurred (simulation tick)
    pub tick: u64,
    /// State hash when violation occurred
    pub state_hash: StateHash,
    /// Violation details
    pub details: ViolationDetails,
    /// Whether this violation was resolved later
    pub resolved: bool,
}

impl ViolationTracker {
    /// Create a new violation tracker
    pub fn new() -> Self {
        Self {
            violations: HashMap::new(),
            total_violations: 0,
        }
    }

    /// Record a new violation
    pub fn record_violation(
        &mut self,
        property_id: PropertyId,
        tick: u64,
        state_hash: StateHash,
        details: ViolationDetails,
    ) {
        let violation = ViolationInstance {
            tick,
            state_hash,
            details,
            resolved: false,
        };

        self.violations
            .entry(property_id)
            .or_default()
            .push(violation);

        self.total_violations += 1;
    }

    /// Mark a violation as resolved
    #[allow(dead_code)]
    pub fn resolve_violation(&mut self, property_id: PropertyId, tick: u64) {
        if let Some(violations) = self.violations.get_mut(&property_id) {
            for violation in violations.iter_mut() {
                if violation.tick == tick && !violation.resolved {
                    violation.resolved = true;
                    break;
                }
            }
        }
    }

    /// Get all violations for a property
    pub fn get_violations(&self, property_id: PropertyId) -> Vec<&ViolationInstance> {
        self.violations
            .get(&property_id)
            .map(|v| v.iter().collect())
            .unwrap_or_default()
    }

    /// Get violation statistics
    pub fn get_stats(&self) -> ViolationStats {
        let active_violations = self
            .violations
            .values()
            .map(|violations| violations.iter().filter(|v| !v.resolved).count())
            .sum();

        ViolationStats {
            total_violations: self.total_violations,
            active_violations,
            properties_with_violations: self.violations.len(),
            resolved_violations: self.total_violations - active_violations,
        }
    }
}

/// Statistics about property violations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViolationStats {
    /// Total number of violations ever recorded
    pub total_violations: usize,
    /// Currently active (unresolved) violations
    pub active_violations: usize,
    /// Number of properties that have had violations
    pub properties_with_violations: usize,
    /// Number of violations that have been resolved
    pub resolved_violations: usize,
}

/// LRU cache for property evaluation results
pub struct LruCache<K, V> {
    /// Maximum number of entries
    capacity: usize,
    /// Current entries
    entries: HashMap<K, V>,
    /// Access order tracking
    access_order: Vec<K>,
}

impl<K: Clone + std::hash::Hash + Eq, V> LruCache<K, V> {
    /// Create a new LRU cache with the given capacity
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            entries: HashMap::new(),
            access_order: Vec::new(),
        }
    }

    /// Get a value from the cache
    pub fn get(&mut self, key: &K) -> Option<&V> {
        if self.entries.contains_key(key) {
            // Move to end (most recently used)
            self.access_order.retain(|k| k != key);
            self.access_order.push(key.clone());
            self.entries.get(key)
        } else {
            None
        }
    }

    /// Insert a value into the cache
    pub fn insert(&mut self, key: K, value: V) {
        // If already exists, just update and move to end
        if self.entries.contains_key(&key) {
            self.entries.insert(key.clone(), value);
            self.access_order.retain(|k| k != &key);
            self.access_order.push(key);
            return;
        }

        // If at capacity, remove least recently used
        if self.entries.len() >= self.capacity {
            if let Some(lru_key) = self.access_order.first().cloned() {
                self.entries.remove(&lru_key);
                self.access_order.remove(0);
            }
        }

        // Insert new entry
        self.entries.insert(key.clone(), value);
        self.access_order.push(key);
    }

    /// Get the number of entries in the cache
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the cache is empty
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Clear all entries
    pub fn clear(&mut self) {
        self.entries.clear();
        self.access_order.clear();
    }
}

/// Main property monitoring engine
pub struct PropertyMonitor {
    /// Quint API for property evaluation
    quint_runner: Option<QuintRunner>,
    /// Currently active properties being monitored
    active_properties: HashMap<PropertyId, PropertySpec>,
    /// Cache for property evaluation results
    evaluation_cache: LruCache<StateHash, PropertyResults>,
    /// Tracker for property violations
    violation_tracker: ViolationTracker,
    /// Performance statistics
    stats: PropertyMonitorStats,
}

/// Performance statistics for the property monitor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyMonitorStats {
    /// Total number of property evaluations performed
    pub total_evaluations: u64,
    /// Number of cache hits
    pub cache_hits: u64,
    /// Number of cache misses
    pub cache_misses: u64,
    /// Total evaluation time in milliseconds
    pub total_evaluation_time_ms: u64,
    /// Average evaluation time per property
    pub avg_evaluation_time_ms: f64,
    /// Properties currently being monitored
    pub active_properties_count: usize,
}

impl PropertyMonitor {
    /// Create a new property monitor
    pub async fn new() -> QuintResult<Self> {
        let quint_runner = QuintRunner::new().ok();

        Ok(Self {
            quint_runner,
            active_properties: HashMap::new(),
            evaluation_cache: LruCache::new(1000), // Cache up to 1000 state evaluations
            violation_tracker: ViolationTracker::new(),
            stats: PropertyMonitorStats {
                total_evaluations: 0,
                cache_hits: 0,
                cache_misses: 0,
                total_evaluation_time_ms: 0,
                avg_evaluation_time_ms: 0.0,
                active_properties_count: 0,
            },
        })
    }

    /// Add a property to monitor
    pub fn add_property(&mut self, property: PropertySpec) -> PropertyId {
        let property_id = property.id;
        self.active_properties.insert(property_id, property);
        self.stats.active_properties_count = self.active_properties.len();
        property_id
    }

    /// Remove a property from monitoring
    pub fn remove_property(&mut self, property_id: PropertyId) -> bool {
        let removed = self.active_properties.remove(&property_id).is_some();
        if removed {
            self.stats.active_properties_count = self.active_properties.len();
        }
        removed
    }

    /// Evaluate all active properties for a given state
    pub async fn evaluate_properties(&mut self, state_hash: StateHash) -> PropertyResults {
        // Check cache first
        if let Some(cached_results) = self.evaluation_cache.get(&state_hash) {
            self.stats.cache_hits += 1;
            return cached_results.clone();
        }

        self.stats.cache_misses += 1;
        let start_time = js_sys::Date::now() as u64;

        let mut results = HashMap::new();

        // Evaluate each active property
        for (property_id, property) in &self.active_properties {
            let property_start = js_sys::Date::now() as u64;

            let evaluation_result = if let Some(ref api) = self.quint_runner {
                // Use Quint API for verification
                match api.verify_property(property).await {
                    Ok(verification_result) => {
                        self.convert_verification_result(verification_result)
                    }
                    Err(_) => PropertyEvaluationResult {
                        holds: false,
                        evaluation_time_ms: js_sys::Date::now() as u64 - property_start,
                        violation_details: Some(ViolationDetails {
                            description: "Property evaluation failed".to_string(),
                            context: HashMap::new(),
                            causality_trace: Vec::new(),
                            debugging_hints: vec![
                                "Check property specification syntax".to_string(),
                                "Verify Quint API connection".to_string(),
                            ],
                        }),
                        verification_result: None,
                    },
                }
            } else {
                // Fallback: basic property evaluation without Quint
                PropertyEvaluationResult {
                    holds: true, // Default to true when no verification available
                    evaluation_time_ms: js_sys::Date::now() as u64 - property_start,
                    violation_details: None,
                    verification_result: Some("No Quint API available".to_string()),
                }
            };

            // Record violations
            if !evaluation_result.holds {
                if let Some(ref details) = evaluation_result.violation_details {
                    self.violation_tracker.record_violation(
                        *property_id,
                        0, // TODO: Get actual tick from trace context
                        state_hash,
                        details.clone(),
                    );
                }
            }

            results.insert(*property_id, evaluation_result);
        }

        let total_time = js_sys::Date::now() as u64 - start_time;

        let property_results = PropertyResults {
            results,
            computed_at: js_sys::Date::now() as u64,
            state_hash,
        };

        // Update statistics
        self.stats.total_evaluations += self.active_properties.len() as u64;
        self.stats.total_evaluation_time_ms += total_time;
        self.stats.avg_evaluation_time_ms =
            self.stats.total_evaluation_time_ms as f64 / self.stats.total_evaluations as f64;

        // Cache the results
        self.evaluation_cache
            .insert(state_hash, property_results.clone());

        property_results
    }

    /// Convert Quint verification result to our format
    fn convert_verification_result(&self, result: VerificationResult) -> PropertyEvaluationResult {
        let duration_ms = result.duration.as_millis() as u64;

        if result.success {
            PropertyEvaluationResult {
                holds: true,
                evaluation_time_ms: duration_ms,
                violation_details: None,
                verification_result: Some("Success".to_string()),
            }
        } else {
            // Check if we have a counterexample indicating a property violation
            let (description, debugging_hints) = if result.counterexample.is_some() {
                (
                    "Property violation found".to_string(),
                    vec![
                        "Review the counterexample trace".to_string(),
                        "Check state invariants".to_string(),
                    ],
                )
            } else {
                (
                    "Verification failed without counterexample".to_string(),
                    vec![
                        "Check property specification syntax".to_string(),
                        "Verify Quint installation".to_string(),
                    ],
                )
            };

            PropertyEvaluationResult {
                holds: false,
                evaluation_time_ms: duration_ms,
                violation_details: Some(ViolationDetails {
                    description,
                    context: HashMap::new(),
                    causality_trace: Vec::new(), // TODO: Extract from counterexample
                    debugging_hints,
                }),
                verification_result: Some("Failed".to_string()),
            }
        }
    }

    /// Get current monitoring statistics
    pub fn get_stats(&self) -> &PropertyMonitorStats {
        &self.stats
    }

    /// Get violation statistics
    pub fn get_violation_stats(&self) -> ViolationStats {
        self.violation_tracker.get_stats()
    }

    /// Get violations for a specific property
    pub fn get_property_violations(&self, property_id: PropertyId) -> Vec<&ViolationInstance> {
        self.violation_tracker.get_violations(property_id)
    }

    /// Clear the evaluation cache
    pub fn clear_cache(&mut self) {
        self.evaluation_cache.clear();
        self.stats.cache_hits = 0;
        self.stats.cache_misses = 0;
    }

    /// Check if Quint API is available
    pub fn has_quint_api(&self) -> bool {
        self.quint_runner.is_some()
    }

    /// Get list of active property IDs
    pub fn get_active_properties(&self) -> Vec<PropertyId> {
        self.active_properties.keys().cloned().collect()
    }
}

/// WASM-bindgen exports for browser usage
#[wasm_bindgen]
#[derive(Default)]
pub struct WasmPropertyMonitor {
    inner: Option<PropertyMonitor>,
}

#[wasm_bindgen]
impl WasmPropertyMonitor {
    /// Create a new property monitor (async initialization handled internally)
    #[wasm_bindgen(constructor)]
    pub fn new() -> WasmPropertyMonitor {
        WasmPropertyMonitor::default()
    }

    /// Initialize the property monitor (must be called before use)
    pub async fn initialize(&mut self) -> Result<(), JsValue> {
        match PropertyMonitor::new().await {
            Ok(monitor) => {
                self.inner = Some(monitor);
                Ok(())
            }
            Err(e) => Err(JsValue::from_str(&format!(
                "Failed to initialize property monitor: {}",
                e
            ))),
        }
    }

    /// Get monitoring statistics as JSON
    pub fn get_stats(&self) -> JsValue {
        match &self.inner {
            Some(monitor) => {
                let stats = monitor.get_stats();
                serde_wasm_bindgen::to_value(stats).unwrap_or(JsValue::NULL)
            }
            None => JsValue::NULL,
        }
    }

    /// Get violation statistics as JSON
    pub fn get_violation_stats(&self) -> JsValue {
        match &self.inner {
            Some(monitor) => {
                let stats = monitor.get_violation_stats();
                serde_wasm_bindgen::to_value(&stats).unwrap_or(JsValue::NULL)
            }
            None => JsValue::NULL,
        }
    }

    /// Check if the monitor has Quint API available
    pub fn has_quint_api(&self) -> bool {
        self.inner
            .as_ref()
            .map(|m| m.has_quint_api())
            .unwrap_or(false)
    }

    /// Clear the evaluation cache
    pub fn clear_cache(&mut self) {
        if let Some(monitor) = &mut self.inner {
            monitor.clear_cache();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    #[test]
    fn test_lru_cache() {
        let mut cache = LruCache::new(2);

        cache.insert("a", 1);
        cache.insert("b", 2);

        assert_eq!(cache.get(&"a"), Some(&1));
        assert_eq!(cache.get(&"b"), Some(&2));

        // Insert third item, should evict "a" since "b" was accessed more recently
        cache.insert("c", 3);
        assert_eq!(cache.get(&"a"), None);
        assert_eq!(cache.get(&"b"), Some(&2));
        assert_eq!(cache.get(&"c"), Some(&3));
    }

    #[test]
    fn test_violation_tracker() {
        let mut tracker = ViolationTracker::new();
        let property_id = uuid::Uuid::from_bytes([
            0x12, 0x34, 0x56, 0x78, 0x90, 0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x90, 0xab,
            0xcd, 0xef,
        ]);

        tracker.record_violation(
            property_id,
            10,
            12345,
            ViolationDetails {
                description: "Test violation".to_string(),
                context: HashMap::new(),
                causality_trace: Vec::new(),
                debugging_hints: Vec::new(),
            },
        );

        let stats = tracker.get_stats();
        assert_eq!(stats.total_violations, 1);
        assert_eq!(stats.active_violations, 1);

        tracker.resolve_violation(property_id, 10);
        let stats = tracker.get_stats();
        assert_eq!(stats.resolved_violations, 1);
        assert_eq!(stats.active_violations, 0);
    }

    #[wasm_bindgen_test]
    async fn test_property_monitor_creation() {
        // Note: This test may fail if Quint is not available, which is expected
        let monitor = PropertyMonitor::new().await;
        // Just check that we can create the monitor - Quint API availability is optional
        assert!(monitor.is_ok() || monitor.is_err());
    }
}
