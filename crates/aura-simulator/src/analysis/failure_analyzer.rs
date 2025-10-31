//! Advanced Failure Analysis
//!
//! This module provides sophisticated failure analysis capabilities that can
//! identify causal chains, critical time windows, and key events leading to
//! property violations in distributed protocol executions.

use crate::metrics::{MetricsCollector, MetricsProvider};
use crate::{
    ExecutionTrace, PropertyViolation, Result, SimulationExecutionResult, SimulationState,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

/// Advanced failure analyzer for property violations
///
/// This analyzer performs sophisticated analysis of property violations,
/// identifying causal chains, critical windows, and key events that
/// contribute to failures in distributed protocol executions.
pub struct FailureAnalyzer {
    /// Configuration for failure analysis
    config: AnalysisConfig,
    /// Causal analyzer for backwards dependency tracking
    causal_analyzer: CausalAnalyzer,
    /// Event significance scoring system
    event_scorer: EventSignificanceScorer,
    /// Metrics collector for analysis statistics
    metrics: MetricsCollector,
}

/// Configuration for failure analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisConfig {
    /// Maximum lookback window for causal analysis (ticks)
    pub max_lookback_ticks: u64,
    /// Minimum significance score for events to be considered
    pub min_significance_score: f64,
    /// Maximum number of causal chains to analyze
    pub max_causal_chains: usize,
    /// Enable deep event pattern analysis
    pub enable_pattern_analysis: bool,
    /// Critical window size around violations (ticks)
    pub critical_window_size: u64,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            max_lookback_ticks: 1000,
            min_significance_score: 0.3,
            max_causal_chains: 10,
            enable_pattern_analysis: true,
            critical_window_size: 50,
        }
    }
}

/// Causal analyzer for backwards dependency tracking
#[derive(Debug, Clone)]
pub struct CausalAnalyzer {
    /// Dependency graph between events
    dependency_graph: HashMap<String, Vec<CausalDependency>>,
    /// Event timeline for temporal analysis
    _event_timeline: VecDeque<AnalyzedEvent>,
    /// Configuration for causal analysis
    config: CausalAnalysisConfig,
}

/// Configuration for causal analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalAnalysisConfig {
    /// Maximum causal chain depth
    pub max_chain_depth: usize,
    /// Weight for temporal proximity in causality
    pub temporal_weight: f64,
    /// Weight for logical dependencies
    pub logical_weight: f64,
    /// Weight for participant relationships
    pub participant_weight: f64,
}

impl Default for CausalAnalysisConfig {
    fn default() -> Self {
        Self {
            max_chain_depth: 20,
            temporal_weight: 0.4,
            logical_weight: 0.4,
            participant_weight: 0.2,
        }
    }
}

/// Causal dependency between events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalDependency {
    /// Source event that influences the target
    pub source_event: String,
    /// Type of causal relationship
    pub dependency_type: DependencyType,
    /// Strength of the causal relationship (0.0 to 1.0)
    pub strength: f64,
    /// Evidence supporting this dependency
    pub evidence: Vec<String>,
}

/// Types of causal dependencies
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DependencyType {
    /// Temporal precedence
    Temporal,
    /// Logical dependency (message flow, state changes)
    Logical,
    /// Participant-based dependency
    Participant,
    /// Protocol-level dependency
    Protocol,
    /// Network-level dependency
    Network,
    /// Byzantine behavior dependency
    Byzantine,
}

/// Event in the analysis timeline
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyzedEvent {
    /// Unique event identifier
    pub event_id: String,
    /// Event type
    pub event_type: EventType,
    /// When the event occurred
    pub timestamp: u64,
    /// Simulation tick when event occurred
    pub tick: u64,
    /// Participants involved
    pub participants: Vec<String>,
    /// Event parameters and data
    pub parameters: HashMap<String, String>,
    /// Significance score for this event
    pub significance_score: f64,
    /// Associated state changes
    pub state_changes: Vec<StateChange>,
}

/// Types of events that can be analyzed
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum EventType {
    /// Protocol message sent
    MessageSent,
    /// Protocol message received
    MessageReceived,
    /// Protocol state transition
    StateTransition,
    /// Network partition event
    NetworkPartition,
    /// Byzantine behavior detected
    ByzantineAction,
    /// Property violation detected
    PropertyViolation,
    /// Timeout occurred
    Timeout,
    /// Error condition
    Error,
    /// Custom event type
    Custom(String),
}

/// State change associated with an event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateChange {
    /// Variable that changed
    pub variable: String,
    /// Previous value
    pub previous_value: Option<String>,
    /// New value
    pub new_value: String,
    /// Participant affected
    pub participant: Option<String>,
}

/// Event significance scorer
#[derive(Debug, Clone)]
pub struct EventSignificanceScorer {
    /// Scoring weights for different event types
    type_weights: HashMap<EventType, f64>,
    /// Scoring rules for pattern detection
    _pattern_rules: Vec<SignificanceRule>,
    /// Historical scoring data
    _historical_scores: HashMap<String, f64>,
}

/// Rule for calculating event significance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignificanceRule {
    /// Rule name
    pub name: String,
    /// Pattern to match
    pub pattern: EventPattern,
    /// Score adjustment when pattern matches
    pub score_adjustment: f64,
    /// Description of why this pattern is significant
    pub description: String,
}

/// Pattern for event matching
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventPattern {
    /// Event type pattern
    EventType(EventType),
    /// Participant count pattern
    ParticipantCount(usize),
    /// Temporal proximity to violation
    TemporalProximity(u64),
    /// State change pattern
    StateChangePattern(String),
    /// Custom pattern
    Custom(String),
}

/// Complete failure analysis result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureAnalysisResult {
    /// Unique analysis identifier
    pub analysis_id: String,
    /// Violation that was analyzed
    pub analyzed_violation: PropertyViolation,
    /// Critical time window around the violation
    pub critical_window: CriticalWindow,
    /// Identified causal chains
    pub causal_chains: Vec<CausalChain>,
    /// Key events that contributed to the failure
    pub key_events: Vec<KeyEvent>,
    /// Event patterns detected
    pub detected_patterns: Vec<DetectedPattern>,
    /// Analysis summary and insights
    pub analysis_summary: AnalysisSummary,
    /// Performance metrics for the analysis
    pub analysis_metrics: AnalysisMetrics,
}

/// Critical time window around a violation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CriticalWindow {
    /// Start of the critical window
    pub start_tick: u64,
    /// End of the critical window (violation occurrence)
    pub end_tick: u64,
    /// Events within the critical window
    pub events_in_window: Vec<AnalyzedEvent>,
    /// State snapshots at key points
    pub state_snapshots: Vec<StateSnapshot>,
    /// Window significance score
    pub significance_score: f64,
}

/// State snapshot at a specific point
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSnapshot {
    /// Tick when snapshot was taken
    pub tick: u64,
    /// Participant states at this point
    pub participant_states: HashMap<String, HashMap<String, String>>,
    /// Network state at this point
    pub network_state: NetworkStateInfo,
    /// Protocol state at this point
    pub protocol_state: ProtocolStateInfo,
}

/// Network state information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkStateInfo {
    /// Active partitions
    pub partitions: Vec<Vec<String>>,
    /// Message drop rate
    pub drop_rate: f64,
    /// Average latency
    pub average_latency_ms: f64,
}

/// Protocol state information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolStateInfo {
    /// Active protocol sessions
    pub active_sessions: Vec<String>,
    /// Current protocol phase
    pub current_phase: String,
    /// Protocol variables
    pub variables: HashMap<String, String>,
}

/// Causal chain leading to a failure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalChain {
    /// Chain identifier
    pub chain_id: String,
    /// Events in the causal chain (ordered)
    pub events: Vec<AnalyzedEvent>,
    /// Dependencies between events
    pub dependencies: Vec<CausalDependency>,
    /// Overall chain strength
    pub chain_strength: f64,
    /// Chain significance score
    pub significance_score: f64,
    /// Description of the causal relationship
    pub description: String,
}

/// Key event that contributed to failure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyEvent {
    /// The event itself
    pub event: AnalyzedEvent,
    /// Why this event is considered key
    pub significance_reason: String,
    /// Impact score on the violation
    pub impact_score: f64,
    /// Related events
    pub related_events: Vec<String>,
}

/// Detected event pattern
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedPattern {
    /// Pattern name
    pub pattern_name: String,
    /// Pattern type
    pub pattern_type: PatternType,
    /// Events that match this pattern
    pub matching_events: Vec<String>,
    /// Pattern confidence score
    pub confidence: f64,
    /// Pattern description
    pub description: String,
}

/// Types of detectable patterns
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PatternType {
    /// Cascading failure pattern
    CascadingFailure,
    /// Byzantine coordination pattern
    ByzantineCoordination,
    /// Network partition pattern
    NetworkPartition,
    /// Timeout cascade pattern
    TimeoutCascade,
    /// Resource exhaustion pattern
    ResourceExhaustion,
    /// Custom pattern
    Custom(String),
}

/// Summary of the failure analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisSummary {
    /// Primary cause category
    pub primary_cause: CauseCategory,
    /// Contributing factors
    pub contributing_factors: Vec<ContributingFactor>,
    /// Likelihood of reproduction
    pub reproduction_likelihood: f64,
    /// Recommended mitigation strategies
    pub mitigation_strategies: Vec<String>,
    /// Complexity of the failure scenario
    pub failure_complexity: FailureComplexity,
}

/// Categories of failure causes
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CauseCategory {
    /// Byzantine participant behavior
    ByzantineBehavior,
    /// Network conditions
    NetworkConditions,
    /// Protocol implementation issue
    ProtocolIssue,
    /// Timing and synchronization
    TimingIssue,
    /// Resource constraints
    ResourceConstraints,
    /// External factors
    ExternalFactors,
    /// Complex interaction
    ComplexInteraction,
}

/// Contributing factor to failure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributingFactor {
    /// Factor name
    pub factor: String,
    /// Impact weight (0.0 to 1.0)
    pub impact_weight: f64,
    /// Evidence supporting this factor
    pub evidence: Vec<String>,
}

/// Complexity levels for failure scenarios
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum FailureComplexity {
    /// Simple single-cause failure
    Simple,
    /// Moderate complexity with multiple factors
    Moderate,
    /// Complex multi-factor failure
    Complex,
    /// Very complex emergent behavior
    VeryComplex,
}

/// Performance metrics for analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisMetrics {
    /// Total analysis time (milliseconds)
    pub analysis_time_ms: u64,
    /// Number of events analyzed
    pub events_analyzed: usize,
    /// Number of causal chains explored
    pub causal_chains_explored: usize,
    /// Memory usage during analysis
    pub memory_usage_mb: f64,
}

impl FailureAnalyzer {
    /// Create a new failure analyzer
    pub fn new() -> Self {
        Self {
            config: AnalysisConfig::default(),
            causal_analyzer: CausalAnalyzer::new(),
            event_scorer: EventSignificanceScorer::new(),
            metrics: MetricsCollector::new(),
        }
    }

    /// Create analyzer with custom configuration
    pub fn with_config(config: AnalysisConfig) -> Self {
        Self {
            config,
            causal_analyzer: CausalAnalyzer::new(),
            event_scorer: EventSignificanceScorer::new(),
            metrics: MetricsCollector::new(),
        }
    }

    /// Analyze a property violation to identify causes and patterns
    pub fn analyze_violation(
        &mut self,
        violation: &PropertyViolation,
        execution_trace: &ExecutionTrace,
        _simulation_result: &SimulationExecutionResult,
    ) -> Result<FailureAnalysisResult> {
        let start_time = crate::utils::time::current_unix_timestamp_millis();
        let analysis_id = format!(
            "analysis_{}_{}",
            violation.property_name, violation.detected_at
        );

        // Extract events from execution trace
        let analyzed_events = self.extract_and_analyze_events(execution_trace, violation)?;

        // Identify critical window around the violation
        let critical_window = self.identify_critical_window(violation, &analyzed_events)?;

        // Perform causal analysis
        let causal_chains = self.causal_analyzer.identify_causal_chains(
            violation,
            &analyzed_events,
            &self.config,
        )?;

        // Identify key events
        let key_events = self.identify_key_events(&analyzed_events, violation, &causal_chains)?;

        // Detect patterns if enabled
        let detected_patterns = if self.config.enable_pattern_analysis {
            self.detect_event_patterns(&analyzed_events, violation)?
        } else {
            Vec::new()
        };

        // Generate analysis summary
        let analysis_summary = self.generate_analysis_summary(
            violation,
            &causal_chains,
            &key_events,
            &detected_patterns,
        )?;

        let end_time = crate::utils::time::current_unix_timestamp_millis();

        // Update metrics
        self.metrics.counter("total_analyses", 1);
        let analysis_time = end_time - start_time;
        self.metrics.counter("analysis_time_ms", analysis_time);
        self.metrics
            .gauge("analysis_duration_ms", analysis_time as f64);

        Ok(FailureAnalysisResult {
            analysis_id,
            analyzed_violation: violation.clone(),
            critical_window,
            causal_chains,
            key_events,
            detected_patterns,
            analysis_summary,
            analysis_metrics: AnalysisMetrics {
                analysis_time_ms: analysis_time,
                events_analyzed: analyzed_events.len(),
                causal_chains_explored: 0, // Would be tracked in real implementation
                memory_usage_mb: 0.0,      // Would be measured in real implementation
            },
        })
    }

    /// Identify key events that contributed to property violations
    pub fn identify_key_events(
        &self,
        events: &[AnalyzedEvent],
        violation: &PropertyViolation,
        causal_chains: &[CausalChain],
    ) -> Result<Vec<KeyEvent>> {
        let mut key_events = Vec::new();

        // Events with high significance scores
        for event in events {
            if event.significance_score >= self.config.min_significance_score {
                let impact_score = self.calculate_impact_score(event, violation, causal_chains);

                if impact_score > 0.5 {
                    key_events.push(KeyEvent {
                        event: event.clone(),
                        significance_reason: format!(
                            "High significance score ({:.2}) and impact score ({:.2})",
                            event.significance_score, impact_score
                        ),
                        impact_score,
                        related_events: self.find_related_events(event, events),
                    });
                }
            }
        }

        // Events that appear in multiple causal chains
        let mut chain_event_counts: HashMap<String, usize> = HashMap::new();
        for chain in causal_chains {
            for event in &chain.events {
                *chain_event_counts
                    .entry(event.event_id.clone())
                    .or_insert(0) += 1;
            }
        }

        for (event_id, count) in chain_event_counts {
            if count >= 2 {
                if let Some(event) = events.iter().find(|e| e.event_id == event_id) {
                    if !key_events.iter().any(|ke| ke.event.event_id == event_id) {
                        key_events.push(KeyEvent {
                            event: event.clone(),
                            significance_reason: format!("Appears in {} causal chains", count),
                            impact_score: 0.7 + (count as f64 * 0.1),
                            related_events: self.find_related_events(event, events),
                        });
                    }
                }
            }
        }

        // Sort by impact score
        key_events.sort_by(|a, b| b.impact_score.partial_cmp(&a.impact_score).unwrap());

        // Limit to reasonable number
        key_events.truncate(10);

        Ok(key_events)
    }

    /// Get analysis metrics
    pub fn get_metrics_snapshot(&self) -> crate::metrics::MetricsSnapshot {
        self.metrics.snapshot()
    }

    // Private implementation methods

    /// Extract and analyze events from execution trace
    fn extract_and_analyze_events(
        &mut self,
        execution_trace: &ExecutionTrace,
        violation: &PropertyViolation,
    ) -> Result<Vec<AnalyzedEvent>> {
        let mut events = Vec::new();
        let trace_states = execution_trace.get_all_states();

        for (i, state) in trace_states.iter().enumerate() {
            // Extract events from state transitions
            if i > 0 {
                let prev_state = &trace_states[i - 1];
                let transition_events =
                    self.extract_state_transition_events(prev_state, state, i as u64)?;
                events.extend(transition_events);
            }

            // Extract protocol events
            let protocol_events = self.extract_protocol_events(state, i as u64)?;
            events.extend(protocol_events);

            // Extract network events
            let network_events = self.extract_network_events(state, i as u64)?;
            events.extend(network_events);
        }

        // Score events for significance
        for event in &mut events {
            event.significance_score = self.event_scorer.calculate_significance(event, violation);
        }

        // Sort by timestamp
        events.sort_by_key(|e| e.timestamp);

        Ok(events)
    }

    /// Identify critical time window around violation
    fn identify_critical_window(
        &self,
        violation: &PropertyViolation,
        events: &[AnalyzedEvent],
    ) -> Result<CriticalWindow> {
        let violation_tick = violation.violation_state.tick;
        let window_start = violation_tick.saturating_sub(self.config.critical_window_size);
        let window_end = violation_tick;

        let events_in_window: Vec<AnalyzedEvent> = events
            .iter()
            .filter(|e| e.tick >= window_start && e.tick <= window_end)
            .cloned()
            .collect();

        // Calculate window significance based on event density and scores
        let significance_score = if events_in_window.is_empty() {
            0.0
        } else {
            let avg_significance: f64 = events_in_window
                .iter()
                .map(|e| e.significance_score)
                .sum::<f64>()
                / events_in_window.len() as f64;
            let density_factor =
                events_in_window.len() as f64 / self.config.critical_window_size as f64;
            (avg_significance + density_factor).min(1.0)
        };

        Ok(CriticalWindow {
            start_tick: window_start,
            end_tick: window_end,
            events_in_window,
            state_snapshots: Vec::new(), // Would be populated in full implementation
            significance_score,
        })
    }

    /// Extract state transition events
    fn extract_state_transition_events(
        &self,
        prev_state: &SimulationState,
        current_state: &SimulationState,
        tick: u64,
    ) -> Result<Vec<AnalyzedEvent>> {
        let mut events = Vec::new();

        // Compare participant states
        for (i, current_participant) in current_state.participants.iter().enumerate() {
            if let Some(prev_participant) = prev_state.participants.get(i) {
                if prev_participant.status != current_participant.status {
                    events.push(AnalyzedEvent {
                        event_id: format!("state_transition_{}_{}", current_participant.id, tick),
                        event_type: EventType::StateTransition,
                        timestamp: current_state.time,
                        tick,
                        participants: vec![current_participant.id.clone()],
                        parameters: {
                            let mut params = HashMap::new();
                            params
                                .insert("participant".to_string(), current_participant.id.clone());
                            params.insert(
                                "old_status".to_string(),
                                format!("{:?}", prev_participant.status),
                            );
                            params.insert(
                                "new_status".to_string(),
                                format!("{:?}", current_participant.status),
                            );
                            params
                        },
                        significance_score: 0.0, // Will be calculated later
                        state_changes: vec![StateChange {
                            variable: "status".to_string(),
                            previous_value: Some(format!("{:?}", prev_participant.status)),
                            new_value: format!("{:?}", current_participant.status),
                            participant: Some(current_participant.id.clone()),
                        }],
                    });
                }
            }
        }

        Ok(events)
    }

    /// Extract protocol-related events
    fn extract_protocol_events(
        &self,
        state: &SimulationState,
        tick: u64,
    ) -> Result<Vec<AnalyzedEvent>> {
        let mut events = Vec::new();

        // Extract events from active protocol sessions
        for session in &state.protocol_state.active_sessions {
            events.push(AnalyzedEvent {
                event_id: format!("protocol_session_{}_{}", session.session_id, tick),
                event_type: EventType::StateTransition,
                timestamp: state.time,
                tick,
                participants: session.participants.clone(),
                parameters: {
                    let mut params = HashMap::new();
                    params.insert("session_id".to_string(), session.session_id.clone());
                    params.insert("protocol_type".to_string(), session.protocol_type.clone());
                    params.insert("phase".to_string(), session.current_phase.clone());
                    params
                },
                significance_score: 0.0,
                state_changes: Vec::new(),
            });
        }

        Ok(events)
    }

    /// Extract network-related events
    fn extract_network_events(
        &self,
        state: &SimulationState,
        tick: u64,
    ) -> Result<Vec<AnalyzedEvent>> {
        let mut events = Vec::new();

        // Extract partition events
        if !state.network_state.partitions.is_empty() {
            events.push(AnalyzedEvent {
                event_id: format!("network_partition_{}", tick),
                event_type: EventType::NetworkPartition,
                timestamp: state.time,
                tick,
                participants: state.network_state.partitions.clone(),
                parameters: {
                    let mut params = HashMap::new();
                    params.insert(
                        "partition_count".to_string(),
                        state.network_state.partitions.len().to_string(),
                    );
                    params.insert(
                        "drop_rate".to_string(),
                        state.network_state.failure_conditions.drop_rate.to_string(),
                    );
                    params
                },
                significance_score: 0.0,
                state_changes: Vec::new(),
            });
        }

        Ok(events)
    }

    /// Calculate impact score of an event on a violation
    fn calculate_impact_score(
        &self,
        event: &AnalyzedEvent,
        violation: &PropertyViolation,
        causal_chains: &[CausalChain],
    ) -> f64 {
        let mut impact = 0.0;

        // Base impact from significance score
        impact += event.significance_score * 0.3;

        // Impact from temporal proximity to violation
        let time_diff = violation
            .violation_state
            .time
            .saturating_sub(event.timestamp);
        let temporal_impact = 1.0 / (1.0 + (time_diff as f64 / 10000.0)); // Decay over 10 seconds
        impact += temporal_impact * 0.3;

        // Impact from causal chain participation
        let chain_participation = causal_chains
            .iter()
            .filter(|chain| chain.events.iter().any(|e| e.event_id == event.event_id))
            .count() as f64;
        impact += (chain_participation / causal_chains.len().max(1) as f64) * 0.4;

        impact.min(1.0)
    }

    /// Find events related to a given event
    fn find_related_events(
        &self,
        event: &AnalyzedEvent,
        all_events: &[AnalyzedEvent],
    ) -> Vec<String> {
        all_events
            .iter()
            .filter(|e| e.event_id != event.event_id)
            .filter(|e| {
                // Related if same participants or close in time
                !event.participants.is_empty()
                    && !e.participants.is_empty()
                    && event
                        .participants
                        .iter()
                        .any(|p| e.participants.contains(p))
                    || (e.timestamp >= event.timestamp.saturating_sub(5000)
                        && e.timestamp <= event.timestamp + 5000)
            })
            .map(|e| e.event_id.clone())
            .take(5)
            .collect()
    }

    /// Detect patterns in events
    fn detect_event_patterns(
        &self,
        events: &[AnalyzedEvent],
        _violation: &PropertyViolation,
    ) -> Result<Vec<DetectedPattern>> {
        let mut patterns = Vec::new();

        // Detect cascading failure pattern
        if let Some(cascade) = self.detect_cascading_failure(events) {
            patterns.push(cascade);
        }

        // Detect timeout cascade pattern
        if let Some(timeout_cascade) = self.detect_timeout_cascade(events) {
            patterns.push(timeout_cascade);
        }

        // Detect byzantine coordination pattern
        if let Some(byzantine_pattern) = self.detect_byzantine_coordination(events) {
            patterns.push(byzantine_pattern);
        }

        Ok(patterns)
    }

    /// Detect cascading failure pattern
    fn detect_cascading_failure(&self, events: &[AnalyzedEvent]) -> Option<DetectedPattern> {
        let error_events: Vec<_> = events
            .iter()
            .filter(|e| matches!(e.event_type, EventType::Error | EventType::Timeout))
            .collect();

        if error_events.len() >= 3 {
            // Check if errors are cascading (affecting different participants over time)
            let mut participants_affected = std::collections::HashSet::new();
            let mut is_cascading = true;

            for window in error_events.windows(2) {
                if window[1].timestamp <= window[0].timestamp + 1000 {
                    // Within 1 second
                    participants_affected.extend(window[0].participants.iter().cloned());
                    participants_affected.extend(window[1].participants.iter().cloned());
                } else {
                    is_cascading = false;
                    break;
                }
            }

            if is_cascading && participants_affected.len() >= 2 {
                return Some(DetectedPattern {
                    pattern_name: "Cascading Failure".to_string(),
                    pattern_type: PatternType::CascadingFailure,
                    matching_events: error_events.iter().map(|e| e.event_id.clone()).collect(),
                    confidence: 0.8,
                    description: format!(
                        "Detected cascading failure affecting {} participants",
                        participants_affected.len()
                    ),
                });
            }
        }

        None
    }

    /// Detect timeout cascade pattern
    fn detect_timeout_cascade(&self, events: &[AnalyzedEvent]) -> Option<DetectedPattern> {
        let timeout_events: Vec<_> = events
            .iter()
            .filter(|e| e.event_type == EventType::Timeout)
            .collect();

        if timeout_events.len() >= 2 {
            return Some(DetectedPattern {
                pattern_name: "Timeout Cascade".to_string(),
                pattern_type: PatternType::TimeoutCascade,
                matching_events: timeout_events.iter().map(|e| e.event_id.clone()).collect(),
                confidence: 0.7,
                description: format!("Detected {} sequential timeouts", timeout_events.len()),
            });
        }

        None
    }

    /// Detect byzantine coordination pattern
    fn detect_byzantine_coordination(&self, events: &[AnalyzedEvent]) -> Option<DetectedPattern> {
        let byzantine_events: Vec<_> = events
            .iter()
            .filter(|e| e.event_type == EventType::ByzantineAction)
            .collect();

        if byzantine_events.len() >= 2 {
            // Check if byzantine actions are coordinated (same timeframe)
            let time_window = 2000; // 2 seconds
            let mut coordinated_events = Vec::new();

            for event in &byzantine_events {
                let concurrent = byzantine_events
                    .iter()
                    .filter(|e| e.event_id != event.event_id)
                    .filter(|e| (e.timestamp as i64 - event.timestamp as i64).abs() <= time_window)
                    .count();

                if concurrent >= 1 {
                    coordinated_events.push(event.event_id.clone());
                }
            }

            if !coordinated_events.is_empty() {
                return Some(DetectedPattern {
                    pattern_name: "Byzantine Coordination".to_string(),
                    pattern_type: PatternType::ByzantineCoordination,
                    matching_events: coordinated_events,
                    confidence: 0.9,
                    description: "Detected coordinated byzantine behavior".to_string(),
                });
            }
        }

        None
    }

    /// Generate analysis summary
    fn generate_analysis_summary(
        &self,
        _violation: &PropertyViolation,
        causal_chains: &[CausalChain],
        key_events: &[KeyEvent],
        patterns: &[DetectedPattern],
    ) -> Result<AnalysisSummary> {
        // Determine primary cause
        let primary_cause = if patterns
            .iter()
            .any(|p| p.pattern_type == PatternType::ByzantineCoordination)
        {
            CauseCategory::ByzantineBehavior
        } else if patterns
            .iter()
            .any(|p| p.pattern_type == PatternType::NetworkPartition)
        {
            CauseCategory::NetworkConditions
        } else if patterns
            .iter()
            .any(|p| p.pattern_type == PatternType::TimeoutCascade)
        {
            CauseCategory::TimingIssue
        } else {
            CauseCategory::ComplexInteraction
        };

        // Identify contributing factors
        let mut contributing_factors = Vec::new();

        for event in key_events {
            contributing_factors.push(ContributingFactor {
                factor: format!("{:?} event", event.event.event_type),
                impact_weight: event.impact_score,
                evidence: vec![event.significance_reason.clone()],
            });
        }

        // Assess reproduction likelihood
        let reproduction_likelihood = if patterns.len() >= 2 {
            0.8 // High likelihood if multiple patterns detected
        } else if !causal_chains.is_empty() {
            0.6 // Moderate likelihood if causal chains identified
        } else {
            0.3 // Low likelihood for complex failures
        };

        // Generate mitigation strategies
        let mitigation_strategies = self.generate_mitigation_strategies(&primary_cause, patterns);

        // Assess complexity
        let failure_complexity = match (causal_chains.len(), key_events.len(), patterns.len()) {
            (0..=1, 0..=2, 0..=1) => FailureComplexity::Simple,
            (2..=3, 3..=5, 1..=2) => FailureComplexity::Moderate,
            (4..=6, 6..=10, 2..=4) => FailureComplexity::Complex,
            _ => FailureComplexity::VeryComplex,
        };

        Ok(AnalysisSummary {
            primary_cause,
            contributing_factors,
            reproduction_likelihood,
            mitigation_strategies,
            failure_complexity,
        })
    }

    /// Generate mitigation strategies based on analysis
    fn generate_mitigation_strategies(
        &self,
        primary_cause: &CauseCategory,
        patterns: &[DetectedPattern],
    ) -> Vec<String> {
        let mut strategies = Vec::new();

        match primary_cause {
            CauseCategory::ByzantineBehavior => {
                strategies.push(
                    "Increase threshold parameters to tolerate more byzantine participants"
                        .to_string(),
                );
                strategies.push(
                    "Implement additional verification checks for participant messages".to_string(),
                );
                strategies.push("Add byzantine detection and isolation mechanisms".to_string());
            }
            CauseCategory::NetworkConditions => {
                strategies.push("Increase timeout values to handle network latency".to_string());
                strategies.push("Implement adaptive timeout mechanisms".to_string());
                strategies
                    .push("Add network partition detection and recovery protocols".to_string());
            }
            CauseCategory::TimingIssue => {
                strategies.push("Review and adjust timeout configurations".to_string());
                strategies.push("Implement back-off and retry mechanisms".to_string());
                strategies.push("Add clock synchronization checks".to_string());
            }
            _ => {
                strategies.push("Implement comprehensive monitoring and alerting".to_string());
                strategies.push("Add graceful degradation mechanisms".to_string());
                strategies.push("Increase logging and observability".to_string());
            }
        }

        // Add pattern-specific strategies
        for pattern in patterns {
            match pattern.pattern_type {
                PatternType::CascadingFailure => {
                    strategies.push(
                        "Implement circuit breaker patterns to prevent cascade failures"
                            .to_string(),
                    );
                }
                PatternType::TimeoutCascade => {
                    strategies
                        .push("Use jittered timeouts to prevent synchronized failures".to_string());
                }
                _ => {}
            }
        }

        strategies
    }
}

impl Default for CausalAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl CausalAnalyzer {
    /// Create a new causal analyzer
    pub fn new() -> Self {
        Self {
            dependency_graph: HashMap::new(),
            _event_timeline: VecDeque::new(),
            config: CausalAnalysisConfig::default(),
        }
    }

    /// Identify causal chains leading to a violation
    pub fn identify_causal_chains(
        &mut self,
        violation: &PropertyViolation,
        events: &[AnalyzedEvent],
        config: &AnalysisConfig,
    ) -> Result<Vec<CausalChain>> {
        // Build dependency graph
        self.build_dependency_graph(events)?;

        // Find chains leading to violation
        let mut causal_chains = Vec::new();
        let violation_tick = violation.violation_state.tick;

        // Start from events close to the violation and work backwards
        let relevant_events: Vec<_> = events
            .iter()
            .filter(|e| {
                e.tick <= violation_tick
                    && e.tick >= violation_tick.saturating_sub(config.max_lookback_ticks)
            })
            .collect();

        for start_event in relevant_events.iter().rev().take(5) {
            if let Some(chain) = self.trace_causal_chain(start_event, events, violation_tick)? {
                causal_chains.push(chain);

                if causal_chains.len() >= config.max_causal_chains {
                    break;
                }
            }
        }

        // Sort chains by strength
        causal_chains.sort_by(|a, b| b.chain_strength.partial_cmp(&a.chain_strength).unwrap());

        Ok(causal_chains)
    }

    // Private methods for causal analysis

    fn build_dependency_graph(&mut self, events: &[AnalyzedEvent]) -> Result<()> {
        self.dependency_graph.clear();

        for (i, event) in events.iter().enumerate() {
            let mut dependencies = Vec::new();

            // Look for temporal dependencies
            if i > 0 {
                let prev_event = &events[i - 1];
                if event.timestamp > prev_event.timestamp {
                    dependencies.push(CausalDependency {
                        source_event: prev_event.event_id.clone(),
                        dependency_type: DependencyType::Temporal,
                        strength: self.calculate_temporal_strength(prev_event, event),
                        evidence: vec!["Temporal sequence".to_string()],
                    });
                }
            }

            // Look for logical dependencies
            for other_event in events.iter().take(i) {
                if let Some(logical_dep) = self.find_logical_dependency(other_event, event) {
                    dependencies.push(logical_dep);
                }
            }

            self.dependency_graph
                .insert(event.event_id.clone(), dependencies);
        }

        Ok(())
    }

    fn trace_causal_chain(
        &self,
        start_event: &AnalyzedEvent,
        all_events: &[AnalyzedEvent],
        violation_tick: u64,
    ) -> Result<Option<CausalChain>> {
        let mut chain_events = vec![start_event.clone()];
        let mut chain_dependencies = Vec::new();
        let mut current_event = start_event;

        // Trace backwards through dependencies
        for _ in 0..self.config.max_chain_depth {
            if let Some(dependencies) = self.dependency_graph.get(&current_event.event_id) {
                // Find the strongest dependency
                if let Some(strongest_dep) = dependencies
                    .iter()
                    .max_by(|a, b| a.strength.partial_cmp(&b.strength).unwrap())
                {
                    if let Some(source_event) = all_events
                        .iter()
                        .find(|e| e.event_id == strongest_dep.source_event)
                    {
                        chain_events.insert(0, source_event.clone());
                        chain_dependencies.push(strongest_dep.clone());
                        current_event = source_event;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        if chain_events.len() > 1 {
            let chain_strength = chain_dependencies.iter().map(|d| d.strength).sum::<f64>()
                / chain_dependencies.len() as f64;
            let significance_score = chain_events
                .iter()
                .map(|e| e.significance_score)
                .sum::<f64>()
                / chain_events.len() as f64;

            Ok(Some(CausalChain {
                chain_id: format!("chain_{}_{}", start_event.event_id, violation_tick),
                events: chain_events,
                dependencies: chain_dependencies,
                chain_strength,
                significance_score,
                description: "Causal chain leading to violation".to_string(),
            }))
        } else {
            Ok(None)
        }
    }

    fn calculate_temporal_strength(
        &self,
        prev_event: &AnalyzedEvent,
        current_event: &AnalyzedEvent,
    ) -> f64 {
        let time_diff = current_event.timestamp - prev_event.timestamp;
        let strength = 1.0 / (1.0 + (time_diff as f64 / 1000.0)); // Decay over 1 second
        strength * self.config.temporal_weight
    }

    fn find_logical_dependency(
        &self,
        source: &AnalyzedEvent,
        target: &AnalyzedEvent,
    ) -> Option<CausalDependency> {
        // Check for participant overlap
        let participant_overlap = source
            .participants
            .iter()
            .any(|p| target.participants.contains(p));

        if participant_overlap {
            return Some(CausalDependency {
                source_event: source.event_id.clone(),
                dependency_type: DependencyType::Participant,
                strength: self.config.participant_weight,
                evidence: vec!["Shared participants".to_string()],
            });
        }

        // Check for protocol-level dependencies
        if source.event_type == EventType::StateTransition
            && target.event_type == EventType::MessageSent
        {
            return Some(CausalDependency {
                source_event: source.event_id.clone(),
                dependency_type: DependencyType::Protocol,
                strength: self.config.logical_weight,
                evidence: vec!["Protocol state change before message".to_string()],
            });
        }

        None
    }
}

impl Default for EventSignificanceScorer {
    fn default() -> Self {
        Self::new()
    }
}

impl EventSignificanceScorer {
    /// Create a new event significance scorer
    pub fn new() -> Self {
        let mut type_weights = HashMap::new();
        type_weights.insert(EventType::PropertyViolation, 1.0);
        type_weights.insert(EventType::Error, 0.9);
        type_weights.insert(EventType::ByzantineAction, 0.8);
        type_weights.insert(EventType::Timeout, 0.7);
        type_weights.insert(EventType::NetworkPartition, 0.7);
        type_weights.insert(EventType::StateTransition, 0.5);
        type_weights.insert(EventType::MessageSent, 0.3);
        type_weights.insert(EventType::MessageReceived, 0.3);

        Self {
            type_weights,
            _pattern_rules: Vec::new(),
            _historical_scores: HashMap::new(),
        }
    }

    /// Calculate significance score for an event
    pub fn calculate_significance(
        &self,
        event: &AnalyzedEvent,
        violation: &PropertyViolation,
    ) -> f64 {
        let mut score = 0.0;

        // Base score from event type
        score += self
            .type_weights
            .get(&event.event_type)
            .copied()
            .unwrap_or(0.1);

        // Temporal proximity to violation
        let time_diff = violation
            .violation_state
            .time
            .saturating_sub(event.timestamp);
        let temporal_score = 1.0 / (1.0 + (time_diff as f64 / 5000.0)); // Decay over 5 seconds
        score += temporal_score * 0.3;

        // Participant involvement
        if event.participants.len() > 1 {
            score += 0.2; // Multi-participant events are more significant
        }

        // State changes
        if !event.state_changes.is_empty() {
            score += event.state_changes.len() as f64 * 0.1;
        }

        score.min(1.0)
    }
}

impl Default for FailureAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{PropertyViolationType, ViolationDetails, ViolationSeverity};

    #[test]
    fn test_failure_analyzer_creation() {
        let analyzer = FailureAnalyzer::new();
        let metrics = analyzer.get_metrics_snapshot();
        assert_eq!(metrics.get_counter("total_analyses").unwrap_or(0), 0);
    }

    #[test]
    fn test_event_significance_scoring() {
        let scorer = EventSignificanceScorer::new();

        let violation = create_test_violation();

        let high_significance_event = AnalyzedEvent {
            event_id: "test_error".to_string(),
            event_type: EventType::Error,
            timestamp: violation.violation_state.time - 1000, // 1 second before violation
            tick: violation.violation_state.tick - 10,
            participants: vec!["participant_1".to_string()],
            parameters: HashMap::new(),
            significance_score: 0.0,
            state_changes: vec![StateChange {
                variable: "status".to_string(),
                previous_value: Some("active".to_string()),
                new_value: "failed".to_string(),
                participant: Some("participant_1".to_string()),
            }],
        };

        let score = scorer.calculate_significance(&high_significance_event, &violation);
        assert!(score > 0.5); // Should be high significance
    }

    #[test]
    fn test_critical_window_identification() {
        let analyzer = FailureAnalyzer::new();
        let violation = create_test_violation();

        let events = vec![
            create_test_event(
                "event_1",
                EventType::StateTransition,
                violation.violation_state.tick - 20,
            ),
            create_test_event(
                "event_2",
                EventType::Error,
                violation.violation_state.tick - 10,
            ),
            create_test_event(
                "event_3",
                EventType::Timeout,
                violation.violation_state.tick - 5,
            ),
        ];

        let window = analyzer
            .identify_critical_window(&violation, &events)
            .unwrap();

        assert_eq!(
            window.start_tick,
            violation
                .violation_state
                .tick
                .saturating_sub(analyzer.config.critical_window_size)
        );
        assert_eq!(window.end_tick, violation.violation_state.tick);
        assert_eq!(window.events_in_window.len(), 3); // All events should be in window
    }

    fn create_test_violation() -> PropertyViolation {
        PropertyViolation {
            property_name: "test_property".to_string(),
            property_type: PropertyViolationType::Invariant,
            violation_state: crate::testing::SimulationState {
                tick: 100,
                time: 10000,
                variables: HashMap::new(),
                protocol_state: crate::testing::ProtocolExecutionState {
                    active_sessions: Vec::new(),
                    completed_sessions: Vec::new(),
                    queued_protocols: Vec::new(),
                },
                participants: Vec::new(),
                network_state: crate::testing::NetworkStateSnapshot {
                    partitions: Vec::new(),
                    message_stats: crate::testing::MessageDeliveryStats {
                        messages_sent: 0,
                        messages_delivered: 0,
                        messages_dropped: 0,
                        average_latency_ms: 0.0,
                    },
                    failure_conditions: crate::testing::NetworkFailureConditions {
                        drop_rate: 0.0,
                        latency_range_ms: (0, 100),
                        partitions_active: false,
                    },
                },
            },
            violation_details: ViolationDetails {
                description: "Test violation".to_string(),
                evidence: Vec::new(),
                potential_causes: Vec::new(),
                severity: ViolationSeverity::High,
                remediation_suggestions: Vec::new(),
            },
            confidence: 0.9,
            detected_at: 10000,
        }
    }

    fn create_test_event(id: &str, event_type: EventType, tick: u64) -> AnalyzedEvent {
        AnalyzedEvent {
            event_id: id.to_string(),
            event_type,
            timestamp: tick * 100, // Convert tick to timestamp
            tick,
            participants: vec!["test_participant".to_string()],
            parameters: HashMap::new(),
            significance_score: 0.5,
            state_changes: Vec::new(),
        }
    }
}
