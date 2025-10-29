//! Simulation logging for deterministic testing
//!
//! Provides structured logging that captures all events during simulation
//! for replay, debugging, and Byzantine behavior analysis.

use aura_journal::{ByzantineEvidence, ProtocolType};
use aura_protocol::tracing::{AuraSpan, LogLevel, LogSink, LogValue, SpanOutcome};
use aura_types::DeviceId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// A logged event in the simulation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationLogEvent {
    /// Simulation tick when event occurred
    pub tick: u64,
    /// Device that generated the event
    pub device_id: DeviceId,
    /// Log level
    pub level: LogLevel,
    /// Span ID for correlation
    pub span_id: Uuid,
    /// Parent span ID if any
    pub parent_span_id: Option<Uuid>,
    /// Human-readable message
    pub message: String,
    /// Structured fields
    pub fields: BTreeMap<String, LogValue>,
    /// Protocol context if applicable
    pub protocol_context: Option<ProtocolType>,
    /// Session ID if in a session
    pub session_id: Option<Uuid>,
    /// Operation being performed
    pub operation: String,
}

/// A span lifecycle event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationSpanEvent {
    /// Simulation tick
    pub tick: u64,
    /// Span information
    pub span: AuraSpan,
    /// Event type
    pub event_type: SpanEventType,
}

/// Types of span events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SpanEventType {
    /// Span entered
    Enter,
    /// Span exited
    Exit(SpanOutcome),
}

/// Pattern of Byzantine behavior detected in logs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ByzantinePattern {
    /// Accused device
    pub device_id: DeviceId,
    /// Evidence found in logs
    pub evidence: ByzantineEvidence,
    /// Events that constitute the evidence
    pub supporting_events: Vec<SimulationLogEvent>,
    /// Confidence level
    pub confidence: f64,
}

/// Export format for traces
#[derive(Debug, Clone)]
pub enum TraceFormat {
    /// JSON format for easy parsing
    Json,
    /// Chrome DevTools tracing format
    ChromeDevTools,
    /// OpenTelemetry format
    OpenTelemetry,
    /// Custom Aura format with full context
    AuraTrace,
}

/// Simulation log sink that captures all events deterministically
pub struct SimulationLogSink {
    /// All logged events
    events: Arc<Mutex<Vec<SimulationLogEvent>>>,
    /// All span events
    spans: Arc<Mutex<Vec<SimulationSpanEvent>>>,
    /// Current simulation tick
    current_tick: Arc<AtomicU64>,
    /// Active spans
    active_spans: Arc<Mutex<BTreeMap<Uuid, AuraSpan>>>,
}

impl SimulationLogSink {
    /// Create a new simulation log sink
    pub fn new() -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
            spans: Arc::new(Mutex::new(Vec::new())),
            current_tick: Arc::new(AtomicU64::new(0)),
            active_spans: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    /// Set the current simulation tick
    pub fn set_tick(&self, tick: u64) {
        self.current_tick.store(tick, Ordering::SeqCst);
    }

    /// Get all events for a specific device
    pub fn get_events_for_device(&self, device_id: DeviceId) -> Vec<SimulationLogEvent> {
        let events = self.events.lock().unwrap();
        events
            .iter()
            .filter(|e| e.device_id == device_id)
            .cloned()
            .collect()
    }

    /// Get protocol timeline across all devices
    pub fn get_protocol_timeline(&self, protocol: ProtocolType) -> Vec<SimulationLogEvent> {
        let events = self.events.lock().unwrap();
        events
            .iter()
            .filter(|e| e.protocol_context == Some(protocol))
            .cloned()
            .collect()
    }

    /// Get session timeline
    pub fn get_session_timeline(&self, session_id: Uuid) -> Vec<SimulationLogEvent> {
        let events = self.events.lock().unwrap();
        events
            .iter()
            .filter(|e| e.session_id == Some(session_id))
            .cloned()
            .collect()
    }

    /// Get all events within a tick range
    pub fn get_events_in_range(&self, start_tick: u64, end_tick: u64) -> Vec<SimulationLogEvent> {
        let events = self.events.lock().unwrap();
        events
            .iter()
            .filter(|e| e.tick >= start_tick && e.tick <= end_tick)
            .cloned()
            .collect()
    }

    /// Get span hierarchy for debugging
    pub fn get_span_hierarchy(&self, root_span_id: Uuid) -> Vec<SimulationSpanEvent> {
        let spans = self.spans.lock().unwrap();
        let mut hierarchy = Vec::new();

        // Find root span
        if let Some(root) = spans.iter().find(|s| s.span.span_id == root_span_id) {
            hierarchy.push(root.clone());

            // Find child spans
            self.collect_child_spans(&spans, root_span_id, &mut hierarchy);
        }

        hierarchy
    }

    /// Detect Byzantine behavior patterns in the logs
    pub fn detect_byzantine_patterns(&self) -> Vec<ByzantinePattern> {
        let events = self.events.lock().unwrap();
        let mut patterns = Vec::new();

        // Group events by device
        let mut device_events: BTreeMap<DeviceId, Vec<&SimulationLogEvent>> = BTreeMap::new();
        for event in events.iter() {
            device_events
                .entry(event.device_id)
                .or_default()
                .push(event);
        }

        // Analyze each device's events for suspicious patterns
        for (device_id, device_events) in &device_events {
            patterns.extend(self.analyze_device_patterns(*device_id, device_events));
        }

        patterns
    }

    /// Export trace in the specified format
    pub fn export_trace(&self, format: TraceFormat) -> String {
        match format {
            TraceFormat::Json => self.export_json(),
            TraceFormat::ChromeDevTools => self.export_chrome_devtools(),
            TraceFormat::OpenTelemetry => self.export_opentelemetry(),
            TraceFormat::AuraTrace => self.export_aura_trace(),
        }
    }

    /// Get summary statistics
    pub fn get_statistics(&self) -> SimulationStatistics {
        let events = self.events.lock().unwrap();
        let spans = self.spans.lock().unwrap();

        let mut stats = SimulationStatistics {
            total_events: events.len(),
            total_spans: spans.len(),
            events_by_level: BTreeMap::new(),
            events_by_device: BTreeMap::new(),
            events_by_protocol: HashMap::new(),
            byzantine_detections: 0,
            error_count: 0,
        };

        for event in events.iter() {
            // Count by level
            *stats.events_by_level.entry(event.level).or_insert(0) += 1;

            // Count by device
            *stats.events_by_device.entry(event.device_id).or_insert(0) += 1;

            // Count by protocol
            if let Some(protocol) = event.protocol_context {
                *stats.events_by_protocol.entry(protocol).or_insert(0) += 1;
            }

            // Count errors and Byzantine detections
            if event.level == LogLevel::Error {
                stats.error_count += 1;

                if event.message.contains("Byzantine") {
                    stats.byzantine_detections += 1;
                }
            }
        }

        stats
    }

    /// Clear all logged events (for memory management in long simulations)
    pub fn clear(&self) {
        self.events.lock().unwrap().clear();
        self.spans.lock().unwrap().clear();
        self.active_spans.lock().unwrap().clear();
    }

    // Private helper methods

    fn collect_child_spans(
        &self,
        spans: &[SimulationSpanEvent],
        parent_id: Uuid,
        hierarchy: &mut Vec<SimulationSpanEvent>,
    ) {
        for span in spans {
            if span.span.parent_id == Some(parent_id) {
                hierarchy.push(span.clone());
                self.collect_child_spans(spans, span.span.span_id, hierarchy);
            }
        }
    }

    fn analyze_device_patterns(
        &self,
        device_id: DeviceId,
        events: &[&SimulationLogEvent],
    ) -> Vec<ByzantinePattern> {
        let mut patterns = Vec::new();

        // Pattern 1: Excessive error rates
        let error_count = events.iter().filter(|e| e.level == LogLevel::Error).count();
        let error_rate = error_count as f64 / events.len() as f64;

        if error_rate > 0.1 {
            // More than 10% errors
            if let Some(evidence) = self.build_resource_exhaustion_evidence(events) {
                patterns.push(ByzantinePattern {
                    device_id,
                    evidence,
                    supporting_events: events
                        .iter()
                        .filter(|e| e.level == LogLevel::Error)
                        .cloned()
                        .cloned()
                        .collect(),
                    confidence: error_rate.min(1.0),
                });
            }
        }

        // Pattern 2: Inconsistent state transitions
        patterns.extend(self.detect_state_inconsistencies(device_id, events));

        // Pattern 3: Unusual message patterns
        patterns.extend(self.detect_message_anomalies(device_id, events));

        patterns
    }

    fn build_resource_exhaustion_evidence(
        &self,
        events: &[&SimulationLogEvent],
    ) -> Option<ByzantineEvidence> {
        // Count events per tick to detect flooding
        let mut events_per_tick: BTreeMap<u64, usize> = BTreeMap::new();
        for event in events {
            *events_per_tick.entry(event.tick).or_insert(0) += 1;
        }

        // Find peak rate
        let max_rate = events_per_tick.values().max().copied().unwrap_or(0);

        if max_rate > 100 {
            // More than 100 events per tick is suspicious
            Some(ByzantineEvidence::ResourceExhaustion {
                request_count: max_rate as u64,
                window_ms: 1000, // 1 second window
            })
        } else {
            None
        }
    }

    fn detect_state_inconsistencies(
        &self,
        _device_id: DeviceId,
        _events: &[&SimulationLogEvent],
    ) -> Vec<ByzantinePattern> {
        // Look for invalid state transitions in the logs
        // This is a simplified analysis - real implementation would be more sophisticated
        Vec::new()
    }

    fn detect_message_anomalies(
        &self,
        _device_id: DeviceId,
        _events: &[&SimulationLogEvent],
    ) -> Vec<ByzantinePattern> {
        // Look for unusual message sending patterns
        // This is a simplified analysis - real implementation would be more sophisticated
        Vec::new()
    }

    fn export_json(&self) -> String {
        let events = self.events.lock().unwrap();
        serde_json::to_string_pretty(&*events).unwrap_or_default()
    }

    fn export_chrome_devtools(&self) -> String {
        // Chrome DevTools tracing format implementation
        "Chrome DevTools format not yet implemented".to_string()
    }

    fn export_opentelemetry(&self) -> String {
        // OpenTelemetry format implementation
        "OpenTelemetry format not yet implemented".to_string()
    }

    fn export_aura_trace(&self) -> String {
        // Custom Aura trace format with full context
        let events = self.events.lock().unwrap();
        let spans = self.spans.lock().unwrap();

        format!(
            "Aura Trace:\nEvents: {}\nSpans: {}\n",
            events.len(),
            spans.len()
        )
    }
}

impl LogSink for SimulationLogSink {
    fn log_event(
        &self,
        level: LogLevel,
        span: &AuraSpan,
        message: String,
        fields: BTreeMap<String, LogValue>,
    ) {
        let event = SimulationLogEvent {
            tick: self.current_tick.load(Ordering::SeqCst),
            device_id: span.device_id,
            level,
            span_id: span.span_id,
            parent_span_id: span.parent_id,
            message,
            fields,
            protocol_context: span.protocol,
            session_id: span.session_id,
            operation: span.operation.clone(),
        };

        self.events.lock().unwrap().push(event);
    }

    fn enter_span(&self, span: AuraSpan) {
        let span_event = SimulationSpanEvent {
            tick: self.current_tick.load(Ordering::SeqCst),
            span: span.clone(),
            event_type: SpanEventType::Enter,
        };

        self.active_spans.lock().unwrap().insert(span.span_id, span);
        self.spans.lock().unwrap().push(span_event);
    }

    fn exit_span(&self, span_id: Uuid, outcome: SpanOutcome) {
        if let Some(span) = self.active_spans.lock().unwrap().remove(&span_id) {
            let span_event = SimulationSpanEvent {
                tick: self.current_tick.load(Ordering::SeqCst),
                span,
                event_type: SpanEventType::Exit(outcome),
            };

            self.spans.lock().unwrap().push(span_event);
        }
    }

    fn is_enabled(&self, _level: LogLevel) -> bool {
        true // Simulation captures everything
    }
}

/// Summary statistics for simulation logs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationStatistics {
    /// Total number of events logged
    pub total_events: usize,
    /// Total number of spans
    pub total_spans: usize,
    /// Events by log level
    pub events_by_level: BTreeMap<LogLevel, usize>,
    /// Events by device
    pub events_by_device: BTreeMap<DeviceId, usize>,
    /// Events by protocol
    pub events_by_protocol: HashMap<ProtocolType, usize>,
    /// Number of Byzantine behavior detections
    pub byzantine_detections: usize,
    /// Total error count
    pub error_count: usize,
}

impl Default for SimulationLogSink {
    fn default() -> Self {
        Self::new()
    }
}
