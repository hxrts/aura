//! Property timeline generation for visualization
//!
//! This module generates timeline data showing property status at each simulation step,
//! enabling visualization of property violations and their temporal relationships.

use crate::property_monitor::{PropertyEvaluationResult, PropertyResults, StateHash};
use aura_types::session_utils::properties::PropertyId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use wasm_bindgen::prelude::*;

/// Timeline showing property status over simulation steps
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyTimeline {
    /// Timeline entries by simulation tick
    pub timeline: HashMap<u64, PropertyTimelineEntry>,
    /// Property IDs included in this timeline
    pub properties: Vec<PropertyId>,
    /// Total span of the timeline
    pub span: TimelineSpan,
    /// Summary statistics
    pub summary: TimelineSummary,
}

/// Timeline span information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineSpan {
    /// First tick in the timeline
    pub start_tick: u64,
    /// Last tick in the timeline
    pub end_tick: u64,
    /// Total duration in ticks
    pub duration: u64,
}

/// Entry for a specific tick in the property timeline
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyTimelineEntry {
    /// Simulation tick
    pub tick: u64,
    /// Property evaluation results at this tick
    pub property_states: HashMap<PropertyId, PropertyStatus>,
    /// State hash for this simulation state
    pub state_hash: StateHash,
    /// Whether any properties were violated at this tick
    pub has_violations: bool,
    /// Number of properties holding at this tick
    pub properties_holding: usize,
    /// Number of properties violated at this tick
    pub properties_violated: usize,
}

/// Status of a single property at a specific point in time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyStatus {
    /// Whether the property holds (true) or is violated (false)
    pub holds: bool,
    /// Evaluation time for this property
    pub evaluation_time_ms: u64,
    /// Whether this is a new violation (not present in previous tick)
    pub is_new_violation: bool,
    /// Whether this violation was resolved from previous tick
    pub is_resolved: bool,
    /// Violation severity (if violated)
    pub severity: ViolationSeverity,
    /// Brief description of violation (if any)
    pub violation_summary: Option<String>,
}

/// Severity level of a property violation
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[derive(Default)]
pub enum ViolationSeverity {
    /// Low severity - minor protocol deviation
    Low,
    /// Medium severity - notable protocol violation
    #[default]
    Medium,
    /// High severity - critical safety violation
    High,
    /// Critical severity - system integrity at risk
    Critical,
}


/// Summary statistics for the property timeline
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineSummary {
    /// Total number of ticks in timeline
    pub total_ticks: u64,
    /// Total number of property evaluations
    pub total_evaluations: usize,
    /// Number of ticks with violations
    pub ticks_with_violations: u64,
    /// Number of ticks with all properties holding
    pub ticks_all_holding: u64,
    /// Violation rate (percentage of ticks with violations)
    pub violation_rate: f64,
    /// Properties that never violated
    pub never_violated: Vec<PropertyId>,
    /// Properties that always violated
    pub always_violated: Vec<PropertyId>,
    /// Most frequently violated properties
    pub most_violated: Vec<(PropertyId, u64)>, // (property_id, violation_count)
}

/// Builder for creating property timelines from evaluation results
pub struct PropertyTimelineBuilder {
    /// Timeline entries being built
    entries: HashMap<u64, PropertyTimelineEntry>,
    /// Properties being tracked
    properties: Vec<PropertyId>,
    /// Previous tick state for comparison
    previous_tick: Option<u64>,
    /// Previous property states for diff calculation
    previous_states: HashMap<PropertyId, PropertyStatus>,
}

impl PropertyTimelineBuilder {
    /// Create a new timeline builder
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            properties: Vec::new(),
            previous_tick: None,
            previous_states: HashMap::new(),
        }
    }

    /// Add property evaluation results for a specific tick
    pub fn add_tick_results(&mut self, tick: u64, results: &PropertyResults) {
        let mut property_states = HashMap::new();
        let mut has_violations = false;
        let mut properties_holding = 0;
        let mut properties_violated = 0;

        for (property_id, result) in &results.results {
            let is_new_violation = self.is_new_violation(*property_id, result);
            let is_resolved = self.is_resolved_violation(*property_id, result);

            if !result.holds {
                has_violations = true;
                properties_violated += 1;
            } else {
                properties_holding += 1;
            }

            let severity = self.determine_violation_severity(result);
            let violation_summary = result
                .violation_details
                .as_ref()
                .map(|details| details.description.clone());

            let status = PropertyStatus {
                holds: result.holds,
                evaluation_time_ms: result.evaluation_time_ms,
                is_new_violation,
                is_resolved,
                severity,
                violation_summary,
            };

            property_states.insert(*property_id, status.clone());

            // Track property if not already tracked
            if !self.properties.contains(property_id) {
                self.properties.push(*property_id);
            }
        }

        let entry = PropertyTimelineEntry {
            tick,
            property_states: property_states.clone(),
            state_hash: results.state_hash,
            has_violations,
            properties_holding,
            properties_violated,
        };

        self.entries.insert(tick, entry);
        self.previous_tick = Some(tick);
        self.previous_states = property_states;
    }

    /// Check if this is a new violation (not present in previous tick)
    fn is_new_violation(&self, property_id: PropertyId, result: &PropertyEvaluationResult) -> bool {
        if result.holds {
            return false;
        }

        match self.previous_states.get(&property_id) {
            Some(prev_status) => prev_status.holds, // Was holding before, now violated
            None => true,                           // First evaluation, and it's violated
        }
    }

    /// Check if this violation was resolved from previous tick
    fn is_resolved_violation(
        &self,
        property_id: PropertyId,
        result: &PropertyEvaluationResult,
    ) -> bool {
        if !result.holds {
            return false;
        }

        match self.previous_states.get(&property_id) {
            Some(prev_status) => !prev_status.holds, // Was violated before, now holding
            None => false,                           // First evaluation, not a resolution
        }
    }

    /// Determine violation severity based on property evaluation result
    fn determine_violation_severity(&self, result: &PropertyEvaluationResult) -> ViolationSeverity {
        if result.holds {
            return ViolationSeverity::Low; // Not actually violated
        }

        // Basic heuristics for severity - could be enhanced with property metadata
        if let Some(ref details) = result.violation_details {
            if details.description.contains("critical") || details.description.contains("safety") {
                ViolationSeverity::Critical
            } else if details.description.contains("invariant")
                || details.description.contains("consistency")
            {
                ViolationSeverity::High
            } else if details.description.contains("performance")
                || details.description.contains("liveness")
            {
                ViolationSeverity::Medium
            } else {
                ViolationSeverity::Low
            }
        } else {
            ViolationSeverity::Medium
        }
    }

    /// Build the final property timeline
    pub fn build(self) -> PropertyTimeline {
        let summary = self.calculate_summary();
        let span = self.calculate_span();

        PropertyTimeline {
            timeline: self.entries,
            properties: self.properties,
            span,
            summary,
        }
    }

    /// Calculate timeline span
    fn calculate_span(&self) -> TimelineSpan {
        let ticks: Vec<u64> = self.entries.keys().cloned().collect();

        if ticks.is_empty() {
            return TimelineSpan {
                start_tick: 0,
                end_tick: 0,
                duration: 0,
            };
        }

        let start_tick = ticks.iter().min().copied().unwrap_or(0);
        let end_tick = ticks.iter().max().copied().unwrap_or(0);
        let duration = end_tick.saturating_sub(start_tick) + 1;

        TimelineSpan {
            start_tick,
            end_tick,
            duration,
        }
    }

    /// Calculate summary statistics
    fn calculate_summary(&self) -> TimelineSummary {
        let total_ticks = self.entries.len() as u64;
        let total_evaluations = self
            .entries
            .values()
            .map(|entry| entry.property_states.len())
            .sum();

        let ticks_with_violations = self
            .entries
            .values()
            .filter(|entry| entry.has_violations)
            .count() as u64;

        let ticks_all_holding = total_ticks - ticks_with_violations;
        let violation_rate = if total_ticks > 0 {
            (ticks_with_violations as f64 / total_ticks as f64) * 100.0
        } else {
            0.0
        };

        // Analyze property violation patterns
        let mut property_violation_counts: HashMap<PropertyId, u64> = HashMap::new();
        let mut property_total_counts: HashMap<PropertyId, u64> = HashMap::new();

        for entry in self.entries.values() {
            for (property_id, status) in &entry.property_states {
                *property_total_counts.entry(*property_id).or_insert(0) += 1;
                if !status.holds {
                    *property_violation_counts.entry(*property_id).or_insert(0) += 1;
                }
            }
        }

        let never_violated: Vec<PropertyId> = self
            .properties
            .iter()
            .filter(|&property_id| property_violation_counts.get(property_id).unwrap_or(&0) == &0)
            .cloned()
            .collect();

        let always_violated: Vec<PropertyId> = self
            .properties
            .iter()
            .filter(|&property_id| {
                let violations = property_violation_counts.get(property_id).unwrap_or(&0);
                let total = property_total_counts.get(property_id).unwrap_or(&0);
                violations == total && *total > 0
            })
            .cloned()
            .collect();

        let mut most_violated: Vec<(PropertyId, u64)> =
            property_violation_counts.into_iter().collect();
        most_violated.sort_by(|a, b| b.1.cmp(&a.1));
        most_violated.truncate(5); // Top 5 most violated

        TimelineSummary {
            total_ticks,
            total_evaluations,
            ticks_with_violations,
            ticks_all_holding,
            violation_rate,
            never_violated,
            always_violated,
            most_violated,
        }
    }
}

impl Default for PropertyTimelineBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// WASM-bindgen wrapper for property timeline functionality
#[wasm_bindgen]
pub struct WasmPropertyTimeline {
    inner: PropertyTimeline,
}

#[wasm_bindgen]
impl WasmPropertyTimeline {
    /// Get the timeline as JSON
    pub fn get_timeline(&self) -> JsValue {
        serde_wasm_bindgen::to_value(&self.inner).unwrap_or(JsValue::NULL)
    }

    /// Get timeline summary statistics
    pub fn get_summary(&self) -> JsValue {
        serde_wasm_bindgen::to_value(&self.inner.summary).unwrap_or(JsValue::NULL)
    }

    /// Get timeline span information
    pub fn get_span(&self) -> JsValue {
        serde_wasm_bindgen::to_value(&self.inner.span).unwrap_or(JsValue::NULL)
    }

    /// Get property states for a specific tick
    pub fn get_tick_states(&self, tick: u64) -> JsValue {
        match self.inner.timeline.get(&tick) {
            Some(entry) => {
                serde_wasm_bindgen::to_value(&entry.property_states).unwrap_or(JsValue::NULL)
            }
            None => JsValue::NULL,
        }
    }

    /// Get all ticks with violations
    pub fn get_violation_ticks(&self) -> JsValue {
        let violation_ticks: Vec<u64> = self
            .inner
            .timeline
            .iter()
            .filter(|(_, entry)| entry.has_violations)
            .map(|(tick, _)| *tick)
            .collect();

        serde_wasm_bindgen::to_value(&violation_ticks).unwrap_or(JsValue::NULL)
    }

    /// Get violation rate as a percentage
    pub fn get_violation_rate(&self) -> f64 {
        self.inner.summary.violation_rate
    }

    /// Check if a specific tick has violations
    pub fn tick_has_violations(&self, tick: u64) -> bool {
        self.inner
            .timeline
            .get(&tick)
            .map(|entry| entry.has_violations)
            .unwrap_or(false)
    }

    /// Get the number of properties being tracked
    pub fn get_property_count(&self) -> usize {
        self.inner.properties.len()
    }

    /// Get the duration of the timeline in ticks
    pub fn get_duration(&self) -> u64 {
        self.inner.span.duration
    }
}

/// Builder for WASM usage
#[wasm_bindgen]
#[derive(Default)]
pub struct WasmPropertyTimelineBuilder {
    inner: PropertyTimelineBuilder,
}


#[wasm_bindgen]
impl WasmPropertyTimelineBuilder {
    /// Create a new timeline builder
    #[wasm_bindgen(constructor)]
    pub fn new() -> WasmPropertyTimelineBuilder {
        WasmPropertyTimelineBuilder::default()
    }

    /// Add property results for a tick (results should be JSON)
    pub fn add_tick_results_json(&mut self, tick: u64, results_json: &str) -> Result<(), JsValue> {
        let results: PropertyResults = serde_json::from_str(results_json)
            .map_err(|e| JsValue::from_str(&format!("Failed to parse results: {}", e)))?;

        self.inner.add_tick_results(tick, &results);
        Ok(())
    }

    /// Build the timeline
    pub fn build(self) -> WasmPropertyTimeline {
        WasmPropertyTimeline {
            inner: self.inner.build(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::property_monitor::{PropertyEvaluationResult, ViolationDetails};
    use std::collections::HashMap;
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    fn create_test_results(holds: bool, description: Option<String>) -> PropertyResults {
        let property_id = PropertyId::new_v4();
        let mut results = HashMap::new();

        results.insert(
            property_id,
            PropertyEvaluationResult {
                holds,
                evaluation_time_ms: 10,
                violation_details: description.map(|desc| ViolationDetails {
                    description: desc,
                    context: HashMap::new(),
                    causality_trace: Vec::new(),
                    debugging_hints: Vec::new(),
                }),
                verification_result: None,
            },
        );

        PropertyResults {
            results,
            computed_at: 0,
            state_hash: 12345,
        }
    }

    #[test]
    fn test_timeline_builder() {
        let mut builder = PropertyTimelineBuilder::new();

        // Add some test results
        let results1 = create_test_results(true, None);
        let results2 = create_test_results(false, Some("Test violation".to_string()));

        builder.add_tick_results(0, &results1);
        builder.add_tick_results(1, &results2);

        let timeline = builder.build();

        assert_eq!(timeline.timeline.len(), 2);
        assert_eq!(timeline.span.duration, 2);
        assert_eq!(timeline.summary.ticks_with_violations, 1);
    }

    #[test]
    fn test_violation_severity() {
        let builder = PropertyTimelineBuilder::new();

        let critical_result = PropertyEvaluationResult {
            holds: false,
            evaluation_time_ms: 10,
            violation_details: Some(ViolationDetails {
                description: "Critical safety violation".to_string(),
                context: HashMap::new(),
                causality_trace: Vec::new(),
                debugging_hints: Vec::new(),
            }),
            verification_result: None,
        };

        let severity = builder.determine_violation_severity(&critical_result);
        assert_eq!(severity, ViolationSeverity::Critical);
    }

    #[wasm_bindgen_test]
    fn test_wasm_timeline_builder() {
        let mut builder = WasmPropertyTimelineBuilder::new();

        // This would normally come from property monitor results
        let results_json = r#"{
            "results": {},
            "computed_at": 0,
            "state_hash": 12345
        }"#;

        builder.add_tick_results_json(0, results_json).unwrap();
        let timeline = builder.build();

        assert_eq!(timeline.get_duration(), 1);
    }
}
