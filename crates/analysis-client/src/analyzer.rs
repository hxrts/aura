//! Main analysis engine implementation

use aura_console_types::{SimulationTrace, TraceEvent};
use serde_wasm_bindgen::{from_value, to_value};
use std::collections::HashMap;
use wasm_bindgen::prelude::*;

use crate::{
    causality::CausalityGraph,
    console_error, console_log,
    query::{EventFilter, TraceIndex},
};

/// High-performance analysis engine for trace data
#[wasm_bindgen]
pub struct AnalysisEngine {
    trace_index: TraceIndex,
    causality_graph: CausalityGraph,
    total_events: usize,
}

#[wasm_bindgen]
impl AnalysisEngine {
    /// Create a new analysis engine from trace bytes (postcard format)
    #[wasm_bindgen(constructor)]
    pub fn new(trace_bytes: &[u8]) -> Result<AnalysisEngine, JsValue> {
        console_log!("Creating AnalysisEngine from {} bytes", trace_bytes.len());

        // Parse trace from postcard format
        let trace: SimulationTrace = postcard::from_bytes(trace_bytes)
            .map_err(|e| JsValue::from_str(&format!("Failed to parse trace: {}", e)))?;

        console_log!("Parsed trace with {} events", trace.timeline.len());

        // Build trace index for efficient querying
        let trace_index = TraceIndex::build(&trace.timeline);

        // Build causality graph for dependency analysis
        let causality_graph = CausalityGraph::build(&trace.timeline);

        let total_events = trace.timeline.len();

        console_log!("AnalysisEngine ready: {} events indexed", total_events);

        Ok(AnalysisEngine {
            trace_index,
            causality_graph,
            total_events,
        })
    }

    /// Create analysis engine from JSON trace data
    #[wasm_bindgen]
    pub fn from_json(trace_json: &str) -> Result<AnalysisEngine, JsValue> {
        console_log!(
            "Creating AnalysisEngine from JSON ({} chars)",
            trace_json.len()
        );

        let trace: SimulationTrace = serde_json::from_str(trace_json)
            .map_err(|e| JsValue::from_str(&format!("Failed to parse JSON trace: {}", e)))?;

        let trace_index = TraceIndex::build(&trace.timeline);
        let causality_graph = CausalityGraph::build(&trace.timeline);
        let total_events = trace.timeline.len();

        console_log!(
            "AnalysisEngine ready from JSON: {} events indexed",
            total_events
        );

        Ok(AnalysisEngine {
            trace_index,
            causality_graph,
            total_events,
        })
    }

    /// Create analysis engine from individual events
    #[wasm_bindgen]
    pub fn from_events(events_js: JsValue) -> Result<AnalysisEngine, JsValue> {
        let events: Vec<TraceEvent> = from_value(events_js)
            .map_err(|e| JsValue::from_str(&format!("Failed to parse events: {}", e)))?;

        console_log!("Creating AnalysisEngine from {} events", events.len());

        let trace_index = TraceIndex::build(&events);
        let causality_graph = CausalityGraph::build(&events);
        let total_events = events.len();

        Ok(AnalysisEngine {
            trace_index,
            causality_graph,
            total_events,
        })
    }

    /// Get events in a specific tick range
    #[wasm_bindgen]
    pub fn get_events_in_range(&self, start: u64, end: u64) -> JsValue {
        let events = self.trace_index.query_range(start, end);
        to_value(&events).unwrap_or(JsValue::NULL)
    }

    /// Get events for a specific participant
    #[wasm_bindgen]
    pub fn get_events_by_participant(&self, participant: &str) -> JsValue {
        let events = self.trace_index.query_by_participant(participant);
        to_value(&events).unwrap_or(JsValue::NULL)
    }

    /// Get events matching a specific type pattern
    #[wasm_bindgen]
    pub fn get_events_by_type(&self, pattern: &str) -> JsValue {
        let events = self.trace_index.query_by_event_type(pattern);
        to_value(&events).unwrap_or(JsValue::NULL)
    }

    /// Execute a complex query with multiple filters
    #[wasm_bindgen]
    pub fn query(&self, filter_js: JsValue) -> JsValue {
        let filter: EventFilter = match from_value(filter_js) {
            Ok(f) => f,
            Err(e) => {
                crate::console_error!("Failed to parse filter: {}", e);
                return JsValue::NULL;
            }
        };

        let (events, stats) = self.trace_index.query(&filter);

        let result = serde_json::json!({
            "events": events,
            "stats": stats
        });

        to_value(&result).unwrap_or(JsValue::NULL)
    }

    /// Get causality path to a specific event
    #[wasm_bindgen]
    pub fn get_causality_path(&self, event_id: u64) -> JsValue {
        match self.causality_graph.path_to(event_id) {
            Some(path) => to_value(&path).unwrap_or(JsValue::NULL),
            None => JsValue::NULL,
        }
    }

    /// Get all events that the given event depends on
    #[wasm_bindgen]
    pub fn get_dependencies(&self, event_id: u64) -> JsValue {
        let deps = self.causality_graph.get_dependencies(event_id);
        to_value(&deps).unwrap_or(JsValue::NULL)
    }

    /// Get all events that depend on the given event
    #[wasm_bindgen]
    pub fn get_dependents(&self, event_id: u64) -> JsValue {
        let dependents = self.causality_graph.get_dependents(event_id);
        to_value(&dependents).unwrap_or(JsValue::NULL)
    }

    /// Get events concurrent with the given event
    #[wasm_bindgen]
    pub fn get_concurrent_events(&self, event_id: u64) -> JsValue {
        let concurrent = self.causality_graph.get_concurrent_events(event_id);
        to_value(&concurrent).unwrap_or(JsValue::NULL)
    }

    /// Analyze the causality graph for patterns and anomalies
    #[wasm_bindgen]
    pub fn analyze_causality(&self) -> JsValue {
        let analysis = self.causality_graph.analyze();
        to_value(&analysis).unwrap_or(JsValue::NULL)
    }

    /// Get summary statistics about the trace
    #[wasm_bindgen]
    pub fn get_summary(&self) -> JsValue {
        let summary = self.trace_index.get_summary();
        to_value(&summary).unwrap_or(JsValue::NULL)
    }

    /// Get causality graph statistics
    #[wasm_bindgen]
    pub fn get_causality_stats(&self) -> JsValue {
        let stats = self.causality_graph.get_stats();
        to_value(&stats).unwrap_or(JsValue::NULL)
    }

    /// Get list of all participants in the trace
    #[wasm_bindgen]
    pub fn get_participants(&self) -> JsValue {
        let participants = self.trace_index.get_participants();
        to_value(&participants).unwrap_or(JsValue::NULL)
    }

    /// Get list of all event types in the trace
    #[wasm_bindgen]
    pub fn get_event_types(&self) -> JsValue {
        let event_types = self.trace_index.get_event_types();
        to_value(&event_types).unwrap_or(JsValue::NULL)
    }

    /// Find events that violate causality constraints
    #[wasm_bindgen]
    pub fn find_causality_violations(&self) -> JsValue {
        // Look for events that claim to happen before events with earlier ticks
        let mut violations = Vec::new();

        for participant in self.trace_index.get_participants() {
            let events = self.trace_index.query_by_participant(&participant);

            for event in &events {
                // Check if any happens-before events have later ticks
                for &before_id in &event.causality.happens_before {
                    if let Some(before_event) = self.trace_index.get_event(before_id) {
                        if before_event.tick > event.tick {
                            violations.push(serde_json::json!({
                                "type": "tick_order_violation",
                                "event_id": event.event_id,
                                "event_tick": event.tick,
                                "before_event_id": before_id,
                                "before_event_tick": before_event.tick,
                                "description": format!(
                                    "Event {} (tick {}) claims to happen before event {} (tick {})",
                                    event.event_id, event.tick, before_id, before_event.tick
                                )
                            }));
                        }
                    }
                }

                // Check parent events
                for &parent_id in &event.causality.parent_events {
                    if let Some(parent_event) = self.trace_index.get_event(parent_id) {
                        if parent_event.tick > event.tick {
                            violations.push(serde_json::json!({
                                "type": "parent_tick_violation",
                                "event_id": event.event_id,
                                "event_tick": event.tick,
                                "parent_event_id": parent_id,
                                "parent_event_tick": parent_event.tick,
                                "description": format!(
                                    "Event {} (tick {}) has parent event {} (tick {})",
                                    event.event_id, event.tick, parent_id, parent_event.tick
                                )
                            }));
                        }
                    }
                }
            }
        }

        to_value(&violations).unwrap_or(JsValue::NULL)
    }

    /// Get performance metrics about the analysis engine
    #[wasm_bindgen]
    pub fn get_performance_metrics(&self) -> JsValue {
        let metrics = serde_json::json!({
            "total_events": self.total_events,
            "memory_usage_estimate": {
                "trace_index_kb": (self.total_events * 2) / 1024, // Rough estimate
                "causality_graph_kb": (self.total_events * 3) / 1024, // Rough estimate
                "total_kb": (self.total_events * 5) / 1024
            },
            "index_stats": {
                "participants": self.trace_index.get_participants().len(),
                "event_types": self.trace_index.get_event_types().len(),
            },
            "causality_stats": self.causality_graph.get_stats()
        });

        to_value(&metrics).unwrap_or(JsValue::NULL)
    }

    /// Test serialization performance
    #[wasm_bindgen]
    pub fn benchmark_query(&self, iterations: u32) -> JsValue {
        let start_time = js_sys::Date::now();

        for _ in 0..iterations {
            // Perform a representative query
            let filter = EventFilter {
                tick_range: Some((0, 100)),
                participants: None,
                event_types: None,
                event_ids: None,
                limit: Some(1000),
            };

            let _ = self.trace_index.query(&filter);
        }

        let end_time = js_sys::Date::now();
        let total_time = end_time - start_time;
        let avg_time = total_time / iterations as f64;

        let result = serde_json::json!({
            "iterations": iterations,
            "total_time_ms": total_time,
            "avg_time_ms": avg_time,
            "queries_per_second": 1000.0 / avg_time
        });

        to_value(&result).unwrap_or(JsValue::NULL)
    }

    /// Get a single event by ID
    #[wasm_bindgen]
    pub fn get_event(&self, event_id: u64) -> JsValue {
        match self.trace_index.get_event(event_id) {
            Some(event) => to_value(event).unwrap_or(JsValue::NULL),
            None => JsValue::NULL,
        }
    }

    /// Get the total number of events
    #[wasm_bindgen]
    pub fn event_count(&self) -> usize {
        self.total_events
    }

    /// Clear internal caches (for memory management)
    #[wasm_bindgen]
    pub fn clear_caches(&mut self) {
        // Currently no caches to clear, but this could be used
        // for future optimizations that maintain computed results
        console_log!("Caches cleared (no-op for now)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_console_types::{CausalityInfo, EventType, TraceMetadata};
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    fn create_test_trace() -> SimulationTrace {
        let events = vec![
            TraceEvent {
                tick: 0,
                event_id: 1,
                event_type: EventType::EffectExecuted {
                    effect_type: "test1".to_string(),
                    effect_data: vec![],
                },
                participant: "alice".to_string(),
                causality: CausalityInfo {
                    parent_events: vec![],
                    happens_before: vec![],
                    concurrent_with: vec![],
                },
            },
            TraceEvent {
                tick: 1,
                event_id: 2,
                event_type: EventType::EffectExecuted {
                    effect_type: "test2".to_string(),
                    effect_data: vec![],
                },
                participant: "bob".to_string(),
                causality: CausalityInfo {
                    parent_events: vec![1],
                    happens_before: vec![],
                    concurrent_with: vec![],
                },
            },
        ];

        SimulationTrace {
            metadata: TraceMetadata {
                scenario_name: "test".to_string(),
                seed: 42,
                total_ticks: 2,
                properties_checked: vec![],
                violations: vec![],
            },
            timeline: events,
            checkpoints: vec![],
            participants: std::collections::HashMap::new(),
            network_topology: aura_console_types::NetworkTopology {
                nodes: HashMap::new(),
                edges: vec![],
                partitions: vec![],
            },
        }
    }

    #[wasm_bindgen_test]
    fn test_analysis_engine_creation() {
        let trace = create_test_trace();
        let trace_bytes = postcard::to_allocvec(&trace).unwrap();

        let engine = AnalysisEngine::new(&trace_bytes).unwrap();
        assert_eq!(engine.event_count(), 2);
    }

    #[wasm_bindgen_test]
    fn test_query_range() {
        let trace = create_test_trace();
        let events_js = to_value(&trace.timeline).unwrap();

        let engine = AnalysisEngine::from_events(events_js).unwrap();
        let events = engine.get_events_in_range(0, 1);

        assert!(!events.is_null());
    }

    #[wasm_bindgen_test]
    fn test_causality_path() {
        let trace = create_test_trace();
        let events_js = to_value(&trace.timeline).unwrap();

        let engine = AnalysisEngine::from_events(events_js).unwrap();
        let path = engine.get_causality_path(2);

        assert!(!path.is_null());
    }
}
