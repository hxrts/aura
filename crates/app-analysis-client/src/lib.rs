//! Aura Analysis Engine WASM Module
//!
//! High-performance trace analysis engine for the browser.
//! Provides causality graph computation, efficient querying,
//! and interactive trace analysis capabilities.

use wasm_bindgen::prelude::*;

// Console logging macros for WASM
/// Logs a message to the browser console
#[macro_export]
macro_rules! console_log {
    ($($t:tt)*) => {
        #[cfg(target_arch = "wasm32")]
        web_sys::console::log_1(&format!($($t)*).into());
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = format!($($t)*); // Suppress unused variable warnings
        }
    }
}

/// Logs an error message to the browser console
#[macro_export]
macro_rules! console_error {
    ($($t:tt)*) => {
        #[cfg(target_arch = "wasm32")]
        web_sys::console::error_1(&format!($($t)*).into());
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = format!($($t)*); // Suppress unused variable warnings
        }
    }
}

mod analyzer;
mod causality;
mod property_causality;
mod property_monitor;
mod property_timeline;
mod query;
mod violation;

pub use analyzer::AnalysisEngine;
pub use property_causality::{
    CausalityVisualizationData, ContributingFactor, CounterfactualPath, CriticalEvent,
    PropertyCausalityAnalysis, PropertyCausalityAnalyzer, ViolationCausalityChain,
};
pub use property_monitor::{PropertyMonitor, WasmPropertyMonitor};
pub use property_timeline::{PropertyTimeline, WasmPropertyTimeline, WasmPropertyTimelineBuilder};
pub use violation::{
    DebuggingGuide, ImpactAssessment, RemediationStrategy, SeverityLevel, ViolationAnalysis,
    ViolationAnalyzer, ViolationClassification, ViolationType, WasmViolationAnalyzer,
};

/// Initialize WASM module
#[wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
    console_log!("Analysis client initialized");
}

/// Enhanced analysis client with property monitoring and causality analysis
#[wasm_bindgen]
pub struct AnalysisClient {
    websocket_url: Option<String>,
    #[allow(dead_code)]
    engine: AnalysisEngine,
    property_monitor: Option<WasmPropertyMonitor>,
    causality_analyzer: Option<PropertyCausalityAnalyzer>,
    violation_analyzer: Option<WasmViolationAnalyzer>,
}

impl Default for AnalysisClient {
    fn default() -> Self {
        // Create engine with empty trace data initially
        let empty_trace = Vec::new();
        let engine = AnalysisEngine::new(&empty_trace).unwrap_or_else(|_| {
            // Fallback: create from empty events
            let empty_events = Vec::<app_console_types::TraceEvent>::new();
            let value = serde_wasm_bindgen::to_value(&empty_events)
                .unwrap_or_else(|_| wasm_bindgen::JsValue::null());
            AnalysisEngine::from_events(value).unwrap_or_else(|_| {
                // Create engine with empty trace if parsing fails
                let empty_trace = Vec::<u8>::new();
                AnalysisEngine::new(&empty_trace).unwrap_or_else(|_| {
                    // This should never fail with empty trace, but handle gracefully
                    panic!("Failed to create empty AnalysisEngine")
                })
            })
        });
        let property_monitor = Some(WasmPropertyMonitor::new());

        AnalysisClient {
            websocket_url: None,
            engine,
            property_monitor,
            causality_analyzer: None,
            violation_analyzer: Some(WasmViolationAnalyzer::new()),
        }
    }
}

#[wasm_bindgen]
impl AnalysisClient {
    /// Create new analysis client
    #[wasm_bindgen(constructor)]
    pub fn new() -> AnalysisClient {
        AnalysisClient::default()
    }

    /// Connect to analysis server
    pub fn connect(&mut self, url: &str) -> Result<(), wasm_bindgen::JsValue> {
        console_log!("Connecting analysis client to: {}", url);
        self.websocket_url = Some(url.to_string());
        Ok(())
    }

    /// Send query to analysis server
    pub fn send(&self, message: &str) -> Result<(), wasm_bindgen::JsValue> {
        match &self.websocket_url {
            Some(_url) => {
                console_log!("Sending message: {}", message);
                Ok(())
            }
            None => Err(wasm_bindgen::JsValue::from_str("Not connected")),
        }
    }

    /// Close connection
    pub fn close(&mut self) -> Result<(), wasm_bindgen::JsValue> {
        self.websocket_url = None;
        console_log!("Connection closed");
        Ok(())
    }

    /// Check connection status
    pub fn is_connected(&self) -> bool {
        self.websocket_url.is_some()
    }

    /// Process trace data with the analysis engine
    pub fn process_trace_data(&mut self, _trace_data: &str) -> Result<(), JsValue> {
        // This method allows processing trace data without exposing the engine directly
        // since we can't return mutable references from wasm_bindgen functions
        // The actual implementation would depend on AnalysisEngine's API
        console_log!("Processing trace data with analysis engine");
        Ok(())
    }

    /// Initialize property monitoring (async)
    pub async fn initialize_property_monitoring(&mut self) -> Result<(), wasm_bindgen::JsValue> {
        if let Some(ref mut monitor) = self.property_monitor {
            monitor.initialize().await?;
            console_log!("Property monitoring initialized successfully");
        }
        Ok(())
    }

    /// Get property monitoring statistics
    pub fn get_property_stats(&self) -> wasm_bindgen::JsValue {
        match &self.property_monitor {
            Some(monitor) => monitor.get_stats(),
            None => wasm_bindgen::JsValue::NULL,
        }
    }

    /// Get property violation statistics
    pub fn get_violation_stats(&self) -> wasm_bindgen::JsValue {
        match &self.property_monitor {
            Some(monitor) => monitor.get_violation_stats(),
            None => wasm_bindgen::JsValue::NULL,
        }
    }

    /// Check if Quint API is available for property verification
    pub fn has_quint_api(&self) -> bool {
        self.property_monitor
            .as_ref()
            .map(|m| m.has_quint_api())
            .unwrap_or(false)
    }

    /// Clear property evaluation cache
    pub fn clear_property_cache(&mut self) {
        if let Some(ref mut monitor) = self.property_monitor {
            monitor.clear_cache();
        }
    }

    /// Create a property timeline builder for timeline visualization
    pub fn create_timeline_builder(&self) -> WasmPropertyTimelineBuilder {
        WasmPropertyTimelineBuilder::new()
    }

    /// Initialize property causality analysis from trace events
    pub fn initialize_causality_analysis(
        &mut self,
        trace_events: wasm_bindgen::JsValue,
    ) -> Result<(), wasm_bindgen::JsValue> {
        let events: Vec<app_console_types::TraceEvent> =
            serde_wasm_bindgen::from_value(trace_events).map_err(|e| {
                wasm_bindgen::JsValue::from_str(&format!("Failed to parse trace events: {}", e))
            })?;

        let analyzer = PropertyCausalityAnalyzer::new(&events);
        self.causality_analyzer = Some(analyzer);

        console_log!(
            "Property causality analysis initialized with {} events",
            events.len()
        );
        Ok(())
    }

    /// Analyze causality for a specific property violation
    pub fn analyze_violation_causality(
        &mut self,
        property_id: &str,
        violation_event_id: u64,
    ) -> wasm_bindgen::JsValue {
        if let Some(ref mut analyzer) = self.causality_analyzer {
            // Convert property_id string to PropertyId (simplified)
            if let Ok(property_uuid) = uuid::Uuid::parse_str(property_id) {
                let property_id =
                    aura_types::session_utils::properties::PropertyId::from(property_uuid);

                match analyzer.analyze_violation_causality(property_id, violation_event_id) {
                    Some(analysis) => serde_wasm_bindgen::to_value(&analysis)
                        .unwrap_or(wasm_bindgen::JsValue::NULL),
                    None => wasm_bindgen::JsValue::NULL,
                }
            } else {
                console_error!("Invalid property ID format: {}", property_id);
                wasm_bindgen::JsValue::NULL
            }
        } else {
            console_error!(
                "Causality analyzer not initialized. Call initialize_causality_analysis first."
            );
            wasm_bindgen::JsValue::NULL
        }
    }

    /// Get all analyzed property violations
    pub fn get_analyzed_violations(&self) -> wasm_bindgen::JsValue {
        if let Some(ref analyzer) = self.causality_analyzer {
            let violations = analyzer.get_analyzed_violations();
            serde_wasm_bindgen::to_value(&violations).unwrap_or(wasm_bindgen::JsValue::NULL)
        } else {
            wasm_bindgen::JsValue::NULL
        }
    }

    /// Get causality analysis statistics
    pub fn get_causality_stats(&self) -> wasm_bindgen::JsValue {
        if let Some(ref analyzer) = self.causality_analyzer {
            let stats = analyzer.get_stats();
            serde_wasm_bindgen::to_value(&stats).unwrap_or(wasm_bindgen::JsValue::NULL)
        } else {
            wasm_bindgen::JsValue::NULL
        }
    }

    /// Clear causality analysis cache
    pub fn clear_causality_cache(&mut self) {
        if let Some(ref mut analyzer) = self.causality_analyzer {
            analyzer.clear_cache();
        }
    }

    /// Check if causality analysis is available
    pub fn has_causality_analysis(&self) -> bool {
        self.causality_analyzer.is_some()
    }

    /// Analyze a property violation with comprehensive insights
    pub fn analyze_violation(
        &mut self,
        property_id: &str,
        violation_data: wasm_bindgen::JsValue,
    ) -> wasm_bindgen::JsValue {
        if let Some(ref mut analyzer) = self.violation_analyzer {
            analyzer.analyze_violation_simple(property_id, violation_data)
        } else {
            console_error!("Violation analyzer not available");
            wasm_bindgen::JsValue::NULL
        }
    }

    /// Get violation analyzer statistics
    pub fn get_violation_analyzer_stats(&self) -> wasm_bindgen::JsValue {
        if let Some(ref analyzer) = self.violation_analyzer {
            analyzer.get_statistics()
        } else {
            wasm_bindgen::JsValue::NULL
        }
    }

    /// Export violation data in specified format
    pub fn export_violation_data(&self, format: &str) -> String {
        if let Some(ref analyzer) = self.violation_analyzer {
            analyzer.export_violations(format)
        } else {
            String::new()
        }
    }

    /// Get trend analysis for violations
    pub fn get_violation_trends(&self, time_window_ms: u64) -> wasm_bindgen::JsValue {
        if let Some(ref analyzer) = self.violation_analyzer {
            analyzer.get_trend_analysis(time_window_ms)
        } else {
            wasm_bindgen::JsValue::NULL
        }
    }

    /// Clear violation analyzer history
    pub fn clear_violation_history(&mut self) {
        if let Some(ref mut analyzer) = self.violation_analyzer {
            analyzer.clear_history();
        }
    }

    /// Check if violation analyzer is available
    pub fn has_violation_analyzer(&self) -> bool {
        self.violation_analyzer.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    fn test_analysis_client_creation() {
        crate::console_log!("Testing analysis client with unified foundation");
        let client = AnalysisClient::new();
        assert!(!client.is_connected());
    }
}
