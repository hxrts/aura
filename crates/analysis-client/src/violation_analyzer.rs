//! Property violation analysis engine for detailed investigation and remediation guidance
//!
//! This module provides comprehensive analysis of property violations, offering detailed
//! context extraction, debugging insights, and actionable remediation strategies. It builds
//! on the property monitor and causality analysis to deliver investigation-ready reports.

use crate::property_causality::{ContributingFactorType, PropertyCausalityAnalysis};
use crate::property_monitor::{ViolationDetails, ViolationInstance};
use aura_types::session_utils::properties::PropertyId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use wasm_bindgen::prelude::*;

/// Comprehensive violation analysis with actionable insights
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViolationAnalysis {
    /// Unique identifier for this analysis
    pub analysis_id: String,
    /// Property that was violated
    pub property_id: PropertyId,
    /// Violation instance being analyzed
    pub violation: ViolationInstance,
    /// Violation classification and severity
    pub classification: ViolationClassification,
    /// Extracted context and environmental factors
    pub context: ViolationContext,
    /// Root cause analysis
    pub root_cause_analysis: RootCauseAnalysis,
    /// Step-by-step debugging guide
    pub debugging_guide: DebuggingGuide,
    /// Remediation strategies
    pub remediation_strategies: Vec<RemediationStrategy>,
    /// Impact assessment
    pub impact_assessment: ImpactAssessment,
    /// Comparison with similar violations
    pub similarity_analysis: SimilarityAnalysis,
    /// Export data for external tools
    pub export_data: ExportData,
    /// Analysis metadata
    pub metadata: AnalysisMetadata,
}

/// Classification and severity assessment of a violation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViolationClassification {
    /// Primary violation type
    pub violation_type: ViolationType,
    /// Severity level
    pub severity: SeverityLevel,
    /// Confidence in the classification (0.0 to 1.0)
    pub confidence: f64,
    /// Additional tags for categorization
    pub tags: Vec<String>,
    /// Whether this appears to be a recurring violation
    pub is_recurring: bool,
    /// Pattern family this violation belongs to
    pub pattern_family: PatternFamily,
}

/// Types of property violations
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ViolationType {
    /// Safety property violation (something bad happened)
    SafetyViolation,
    /// Liveness property violation (something good didn't happen)
    LivenessViolation,
    /// Consistency violation across replicas
    ConsistencyViolation,
    /// Threshold requirement not met
    ThresholdViolation,
    /// Session type protocol violation
    ProtocolViolation,
    /// Temporal ordering constraint violation
    TemporalViolation,
    /// Resource constraint violation
    ResourceViolation,
    /// Authentication or authorization failure
    SecurityViolation,
    /// Network partition tolerance failure
    PartitionViolation,
    /// Byzantine fault tolerance failure
    ByzantineViolation,
}

/// Severity levels for violations
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum SeverityLevel {
    /// Critical system failure requiring immediate attention
    Critical,
    /// High impact requiring prompt resolution
    High,
    /// Medium impact that should be addressed
    Medium,
    /// Low impact for awareness
    Low,
    /// Informational only
    Info,
}

/// Pattern families for grouping similar violations
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PatternFamily {
    /// Race condition patterns
    RaceConditions,
    /// Network partition patterns
    PartitionPatterns,
    /// Consensus failure patterns
    ConsensusFailures,
    /// State synchronization patterns
    StateSyncPatterns,
    /// Threshold signature patterns
    ThresholdSignaturePatterns,
    /// Message ordering patterns
    MessageOrderingPatterns,
    /// Resource exhaustion patterns
    ResourceExhaustionPatterns,
    /// Session type patterns
    SessionTypePatterns,
    /// Byzantine behavior patterns
    ByzantinePatterns,
    /// Unknown or unique patterns
    Unknown,
}

/// Environmental context when violation occurred
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViolationContext {
    /// System state at time of violation
    pub system_state: SystemState,
    /// Network conditions
    pub network_conditions: NetworkConditions,
    /// Participant status
    pub participant_status: ParticipantStatus,
    /// Resource utilization
    pub resource_utilization: ResourceUtilization,
    /// Recent events that may have contributed
    pub recent_events: Vec<ContextualEvent>,
    /// Environmental factors
    pub environmental_factors: Vec<EnvironmentalFactor>,
}

/// System state information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemState {
    /// Current epoch or round
    pub current_epoch: u64,
    /// Active participants
    pub active_participants: HashSet<String>,
    /// Pending operations
    pub pending_operations: u32,
    /// CRDT state hash
    pub state_hash: String,
    /// Session types in various states
    pub session_states: HashMap<String, String>,
    /// Threshold requirements
    pub threshold_requirements: HashMap<String, ThresholdRequirement>,
}

/// Network condition assessment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConditions {
    /// Detected partitions
    pub partitions: Vec<NetworkPartition>,
    /// Message delivery rates by participant
    pub delivery_rates: HashMap<String, f64>,
    /// Average message latency
    pub avg_latency_ms: f64,
    /// Network stability score (0.0 to 1.0)
    pub stability_score: f64,
    /// Recent network events
    pub recent_network_events: Vec<NetworkEvent>,
}

/// Participant status at time of violation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipantStatus {
    /// Online participants
    pub online: HashSet<String>,
    /// Offline participants
    pub offline: HashSet<String>,
    /// Participants showing byzantine behavior
    pub byzantine: HashSet<String>,
    /// Participant response times
    pub response_times: HashMap<String, f64>,
    /// Trust scores
    pub trust_scores: HashMap<String, f64>,
}

/// Resource utilization metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceUtilization {
    /// Memory usage percentage
    pub memory_usage_percent: f64,
    /// CPU usage percentage
    pub cpu_usage_percent: f64,
    /// Network bandwidth utilization
    pub network_bandwidth_percent: f64,
    /// Storage usage
    pub storage_usage_bytes: u64,
    /// Open connections
    pub open_connections: u32,
    /// Queue depths
    pub queue_depths: HashMap<String, u32>,
}

/// Event with contextual information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextualEvent {
    /// Event identifier
    pub event_id: u64,
    /// Event description
    pub description: String,
    /// Time relative to violation (negative = before, 0 = at violation)
    pub relative_time_ticks: i64,
    /// Participants involved
    pub participants: Vec<String>,
    /// Event impact score
    pub impact_score: f64,
    /// Event category
    pub category: EventCategory,
}

/// Categories for contextual events
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventCategory {
    /// State transition
    StateTransition,
    /// Message communication
    MessagePassing,
    /// Network topology change
    NetworkChange,
    /// Participant behavior change
    ParticipantChange,
    /// Resource utilization change
    ResourceChange,
    /// Protocol event
    ProtocolEvent,
    /// External factor
    ExternalFactor,
}

/// Environmental factors that may have contributed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentalFactor {
    /// Factor type
    pub factor_type: EnvironmentalFactorType,
    /// Factor description
    pub description: String,
    /// Contribution likelihood (0.0 to 1.0)
    pub contribution_likelihood: f64,
    /// Supporting evidence
    pub evidence: Vec<String>,
}

/// Types of environmental factors
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentalFactorType {
    /// High system load
    HighLoad,
    /// Network instability
    NetworkInstability,
    /// Participant unavailability
    ParticipantUnavailability,
    /// Resource contention
    ResourceContention,
    /// External interference
    ExternalInterference,
    /// Configuration issue
    ConfigurationIssue,
    /// Timing issue
    TimingIssue,
}

/// Threshold requirement information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdRequirement {
    /// Required threshold (M in M-of-N)
    pub required: u32,
    /// Total participants (N in M-of-N)
    pub total: u32,
    /// Currently available
    pub available: u32,
    /// Whether requirement is currently met
    pub is_met: bool,
}

/// Network partition information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkPartition {
    /// Participants in this partition
    pub participants: HashSet<String>,
    /// When partition was detected
    pub detected_at_tick: u64,
    /// Whether this is the majority partition
    pub is_majority: bool,
}

/// Network event information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkEvent {
    /// Event description
    pub description: String,
    /// When it occurred
    pub tick: u64,
    /// Affected participants
    pub affected_participants: Vec<String>,
    /// Event severity
    pub severity: SeverityLevel,
}

/// Root cause analysis results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootCauseAnalysis {
    /// Primary root cause
    pub primary_cause: RootCause,
    /// Contributing causes
    pub contributing_causes: Vec<RootCause>,
    /// Causal chain summary
    pub causal_chain_summary: String,
    /// Key decision points that led to violation
    pub key_decision_points: Vec<DecisionPoint>,
    /// Alternative paths that could have been taken
    pub alternative_paths: Vec<AlternativePath>,
}

/// Root cause identification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootCause {
    /// Cause type
    pub cause_type: RootCauseType,
    /// Detailed description
    pub description: String,
    /// Confidence in this being the root cause (0.0 to 1.0)
    pub confidence: f64,
    /// Events that evidence this cause
    pub evidence_events: Vec<u64>,
    /// Participants involved
    pub involved_participants: Vec<String>,
    /// Remediation complexity
    pub remediation_complexity: ComplexityLevel,
}

/// Types of root causes
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RootCauseType {
    /// Insufficient participants for threshold
    InsufficientParticipants,
    /// Network partition preventing communication
    NetworkPartition,
    /// Byzantine participant behavior
    ByzantineBehavior,
    /// Race condition in concurrent operations
    RaceCondition,
    /// State inconsistency between replicas
    StateInconsistency,
    /// Message ordering violation
    MessageOrderingViolation,
    /// Resource exhaustion
    ResourceExhaustion,
    /// Configuration error
    ConfigurationError,
    /// Session type protocol violation
    ProtocolViolation,
    /// Temporal constraint violation
    TemporalConstraintViolation,
    /// External system failure
    ExternalSystemFailure,
}

/// Decision points in the causal chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionPoint {
    /// Event where decision was made
    pub event_id: u64,
    /// Description of the decision
    pub decision_description: String,
    /// Participant who made the decision
    pub decision_maker: String,
    /// Available alternatives at the time
    pub available_alternatives: Vec<String>,
    /// Why this decision was made
    pub decision_rationale: String,
    /// Impact of this decision
    pub decision_impact: f64,
}

/// Alternative path that could have been taken
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlternativePath {
    /// Description of the alternative
    pub description: String,
    /// Required changes to take this path
    pub required_changes: Vec<String>,
    /// Likelihood of success (0.0 to 1.0)
    pub success_likelihood: f64,
    /// Cost/complexity of this alternative
    pub complexity: ComplexityLevel,
}

/// Complexity levels
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ComplexityLevel {
    /// Very simple change
    VeryLow,
    /// Simple change
    Low,
    /// Moderate complexity
    Medium,
    /// Complex change
    High,
    /// Very complex change
    VeryHigh,
}

/// Step-by-step debugging guide
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebuggingGuide {
    /// Overview of the debugging approach
    pub overview: String,
    /// Debugging steps in order
    pub steps: Vec<DebuggingStep>,
    /// Tools and techniques to use
    pub recommended_tools: Vec<RecommendedTool>,
    /// Common pitfalls to avoid
    pub pitfalls_to_avoid: Vec<String>,
    /// Expected time to complete investigation
    pub estimated_investigation_time: String,
}

/// Individual debugging step
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebuggingStep {
    /// Step number
    pub step_number: u32,
    /// Step title
    pub title: String,
    /// Detailed instructions
    pub instructions: String,
    /// Expected outcomes
    pub expected_outcomes: Vec<String>,
    /// If this step fails, what to try next
    pub fallback_actions: Vec<String>,
    /// Related events to examine
    pub related_events: Vec<u64>,
    /// Commands or queries to run
    pub suggested_commands: Vec<String>,
}

/// Recommended debugging tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecommendedTool {
    /// Tool name
    pub name: String,
    /// Tool description
    pub description: String,
    /// How to use it for this violation
    pub usage_guidance: String,
    /// Expected insights from this tool
    pub expected_insights: Vec<String>,
}

/// Remediation strategy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemediationStrategy {
    /// Strategy name
    pub name: String,
    /// Strategy type
    pub strategy_type: RemediationStrategyType,
    /// Detailed description
    pub description: String,
    /// Implementation steps
    pub implementation_steps: Vec<ImplementationStep>,
    /// Effectiveness rating (0.0 to 1.0)
    pub effectiveness: f64,
    /// Implementation complexity
    pub complexity: ComplexityLevel,
    /// Time to implement
    pub implementation_time: String,
    /// Potential side effects
    pub side_effects: Vec<String>,
    /// Prerequisites
    pub prerequisites: Vec<String>,
}

/// Types of remediation strategies
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemediationStrategyType {
    /// Immediate fix for current violation
    ImmediateFix,
    /// Prevention of similar violations
    Prevention,
    /// System hardening
    Hardening,
    /// Monitoring improvement
    MonitoringImprovement,
    /// Process improvement
    ProcessImprovement,
    /// Configuration change
    ConfigurationChange,
    /// Code change
    CodeChange,
    /// Infrastructure change
    InfrastructureChange,
}

/// Implementation step for remediation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImplementationStep {
    /// Step number
    pub step_number: u32,
    /// Step description
    pub description: String,
    /// Commands to execute
    pub commands: Vec<String>,
    /// Expected results
    pub expected_results: Vec<String>,
    /// Validation checks
    pub validation_checks: Vec<String>,
    /// Rollback instructions if needed
    pub rollback_instructions: Vec<String>,
}

/// Impact assessment of the violation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactAssessment {
    /// Overall impact score (0.0 to 1.0)
    pub overall_impact: f64,
    /// Affected system components
    pub affected_components: Vec<String>,
    /// Business impact
    pub business_impact: BusinessImpact,
    /// Technical impact
    pub technical_impact: TechnicalImpact,
    /// User impact
    pub user_impact: UserImpact,
    /// Recovery time estimate
    pub recovery_time_estimate: String,
    /// Cascading effects
    pub cascading_effects: Vec<CascadingEffect>,
}

/// Business impact assessment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BusinessImpact {
    /// Service availability impact
    pub availability_impact: f64,
    /// Data integrity impact
    pub data_integrity_impact: f64,
    /// Performance impact
    pub performance_impact: f64,
    /// Security impact
    pub security_impact: f64,
    /// Compliance impact
    pub compliance_impact: f64,
}

/// Technical impact assessment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TechnicalImpact {
    /// System stability impact
    pub stability_impact: f64,
    /// Scalability impact
    pub scalability_impact: f64,
    /// Maintainability impact
    pub maintainability_impact: f64,
    /// Resource utilization impact
    pub resource_impact: f64,
}

/// User impact assessment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserImpact {
    /// Number of affected users
    pub affected_users: u32,
    /// User experience degradation (0.0 to 1.0)
    pub experience_degradation: f64,
    /// Features unavailable
    pub unavailable_features: Vec<String>,
    /// Performance degradation
    pub performance_degradation: f64,
}

/// Cascading effect from the violation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CascadingEffect {
    /// Effect description
    pub description: String,
    /// Likelihood of occurrence (0.0 to 1.0)
    pub likelihood: f64,
    /// Potential impact if it occurs
    pub potential_impact: f64,
    /// Timeline for effect to manifest
    pub timeline: String,
    /// Mitigation strategies
    pub mitigation_strategies: Vec<String>,
}

/// Similarity analysis with other violations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarityAnalysis {
    /// Similar violations found
    pub similar_violations: Vec<SimilarViolation>,
    /// Pattern recognition results
    pub patterns: Vec<ViolationPattern>,
    /// Trend analysis
    pub trends: TrendAnalysis,
    /// Recommendations based on similarities
    pub pattern_based_recommendations: Vec<String>,
}

/// Similar violation reference
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarViolation {
    /// Analysis ID of similar violation
    pub analysis_id: String,
    /// Similarity score (0.0 to 1.0)
    pub similarity_score: f64,
    /// Key similarities
    pub key_similarities: Vec<String>,
    /// Key differences
    pub key_differences: Vec<String>,
    /// When it occurred
    pub occurred_at: u64,
    /// Resolution approach used
    pub resolution_approach: String,
    /// Effectiveness of resolution
    pub resolution_effectiveness: f64,
}

/// Violation pattern identification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViolationPattern {
    /// Pattern name
    pub pattern_name: String,
    /// Pattern description
    pub pattern_description: String,
    /// Confidence in pattern match (0.0 to 1.0)
    pub confidence: f64,
    /// Frequency of this pattern
    pub frequency: u32,
    /// Known solutions for this pattern
    pub known_solutions: Vec<String>,
}

/// Trend analysis results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendAnalysis {
    /// Whether violations are increasing
    pub increasing_trend: bool,
    /// Rate of change
    pub change_rate: f64,
    /// Seasonal patterns detected
    pub seasonal_patterns: Vec<String>,
    /// Correlation with system changes
    pub system_change_correlations: Vec<String>,
    /// Predictive insights
    pub predictive_insights: Vec<String>,
}

/// Export data for external tools
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportData {
    /// JSON export for general consumption
    pub json_export: String,
    /// CSV export for spreadsheet analysis
    pub csv_export: String,
    /// Grafana-compatible metrics
    pub grafana_metrics: Vec<GrafanaMetric>,
    /// SIEM-compatible events
    pub siem_events: Vec<SiemEvent>,
    /// Custom export formats
    pub custom_exports: HashMap<String, String>,
}

/// Grafana metric format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrafanaMetric {
    /// Metric name
    pub name: String,
    /// Metric value
    pub value: f64,
    /// Timestamp
    pub timestamp: u64,
    /// Tags
    pub tags: HashMap<String, String>,
}

/// SIEM event format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiemEvent {
    /// Event timestamp
    pub timestamp: u64,
    /// Event severity
    pub severity: SeverityLevel,
    /// Event category
    pub category: String,
    /// Event description
    pub description: String,
    /// Additional fields
    pub fields: HashMap<String, String>,
}

/// Analysis metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisMetadata {
    /// When analysis was created
    pub created_at: u64,
    /// Analysis version
    pub version: String,
    /// Analyzer configuration used
    pub analyzer_config: HashMap<String, String>,
    /// Computation time in milliseconds
    pub computation_time_ms: u64,
    /// Data sources used
    pub data_sources: Vec<String>,
    /// Analysis quality score (0.0 to 1.0)
    pub quality_score: f64,
}

/// Main violation analyzer engine
pub struct ViolationAnalyzer {
    /// Historical violations for comparison
    violation_history: BTreeMap<u64, ViolationAnalysis>,
    /// Pattern database
    pattern_database: HashMap<PatternFamily, Vec<ViolationPattern>>,
    /// Configuration settings
    config: AnalyzerConfig,
    /// Statistics
    stats: AnalyzerStats,
}

/// Configuration for the violation analyzer
#[derive(Debug, Clone)]
pub struct AnalyzerConfig {
    /// Maximum violations to keep in history
    max_history_size: usize,
    /// Similarity threshold for pattern matching
    similarity_threshold: f64,
    /// Enable detailed causality analysis
    enable_causality_analysis: bool,
    /// Enable trend analysis
    enable_trend_analysis: bool,
    /// Export formats to generate
    export_formats: HashSet<String>,
}

/// Statistics for the analyzer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyzerStats {
    /// Total violations analyzed
    pub total_violations_analyzed: u64,
    /// Patterns identified
    pub patterns_identified: u32,
    /// Average analysis time in milliseconds
    pub avg_analysis_time_ms: f64,
    /// Most common violation types
    pub common_violation_types: HashMap<ViolationType, u32>,
    /// Most effective remediation strategies
    pub effective_strategies: HashMap<String, f64>,
}

impl ViolationAnalyzer {
    /// Create a new violation analyzer
    pub fn new() -> Self {
        Self {
            violation_history: BTreeMap::new(),
            pattern_database: HashMap::new(),
            config: AnalyzerConfig::default(),
            stats: AnalyzerStats::default(),
        }
    }

    /// Create analyzer with custom configuration
    pub fn with_config(config: AnalyzerConfig) -> Self {
        Self {
            violation_history: BTreeMap::new(),
            pattern_database: HashMap::new(),
            config,
            stats: AnalyzerStats::default(),
        }
    }

    /// Analyze a property violation with comprehensive insights
    pub fn analyze_violation(
        &mut self,
        property_id: PropertyId,
        violation: ViolationInstance,
        causality_analysis: Option<PropertyCausalityAnalysis>,
    ) -> ViolationAnalysis {
        let start_time = js_sys::Date::now() as u64;

        web_sys::console::log_1(
            &format!(
                "Starting comprehensive violation analysis for property {:?}",
                property_id
            )
            .into(),
        );

        // Generate unique analysis ID
        let analysis_id = format!("violation_{}_{}", violation.tick, violation.state_hash);

        // Classify the violation
        let classification = self.classify_violation(&violation, causality_analysis.as_ref());

        // Extract context
        let context = self.extract_violation_context(&violation, causality_analysis.as_ref());

        // Perform root cause analysis
        let root_cause_analysis =
            self.perform_root_cause_analysis(&violation, causality_analysis.as_ref(), &context);

        // Generate debugging guide
        let debugging_guide = self.generate_debugging_guide(
            &classification,
            &root_cause_analysis,
            causality_analysis.as_ref(),
        );

        // Create remediation strategies
        let remediation_strategies =
            self.generate_remediation_strategies(&classification, &root_cause_analysis, &context);

        // Assess impact
        let impact_assessment = self.assess_impact(&violation, &classification, &context);

        // Perform similarity analysis
        let similarity_analysis = self.analyze_similarities(&classification, &context);

        // Generate export data
        let export_data =
            self.generate_export_data(&violation, &classification, &impact_assessment);

        let computation_time_ms = js_sys::Date::now() as u64 - start_time;

        let metadata = AnalysisMetadata {
            created_at: js_sys::Date::now() as u64,
            version: "1.0.0".to_string(),
            analyzer_config: HashMap::new(),
            computation_time_ms,
            data_sources: vec![
                "property_monitor".to_string(),
                "causality_analyzer".to_string(),
            ],
            quality_score: self
                .calculate_quality_score(&classification, causality_analysis.is_some()),
        };

        let analysis = ViolationAnalysis {
            analysis_id: analysis_id.clone(),
            property_id,
            violation,
            classification,
            context,
            root_cause_analysis,
            debugging_guide,
            remediation_strategies,
            impact_assessment,
            similarity_analysis,
            export_data,
            metadata,
        };

        // Store in history
        self.violation_history.insert(start_time, analysis.clone());

        // Maintain history size limit
        if self.violation_history.len() > self.config.max_history_size {
            let oldest_key = *self.violation_history.keys().next().unwrap();
            self.violation_history.remove(&oldest_key);
        }

        // Update statistics
        self.update_statistics(&analysis);

        web_sys::console::log_1(
            &format!("Violation analysis completed in {}ms", computation_time_ms).into(),
        );

        analysis
    }

    /// Compare two violations for similarity
    pub fn compare_violations(
        &self,
        analysis1: &ViolationAnalysis,
        analysis2: &ViolationAnalysis,
    ) -> ViolationComparison {
        let similarity_score = self.calculate_similarity_score(analysis1, analysis2);

        let key_similarities = self.identify_similarities(analysis1, analysis2);
        let key_differences = self.identify_differences(analysis1, analysis2);

        ViolationComparison {
            analysis1_id: analysis1.analysis_id.clone(),
            analysis2_id: analysis2.analysis_id.clone(),
            similarity_score,
            key_similarities,
            key_differences,
            recommendations: self.generate_comparison_recommendations(analysis1, analysis2),
        }
    }

    /// Get trend analysis for violations over time
    pub fn get_trend_analysis(&self, time_window: u64) -> TrendAnalysis {
        let current_time = js_sys::Date::now() as u64;
        let window_start = current_time.saturating_sub(time_window);

        let recent_violations: Vec<_> = self
            .violation_history
            .range(window_start..)
            .map(|(_, analysis)| analysis)
            .collect();

        if recent_violations.is_empty() {
            return TrendAnalysis {
                increasing_trend: false,
                change_rate: 0.0,
                seasonal_patterns: Vec::new(),
                system_change_correlations: Vec::new(),
                predictive_insights: Vec::new(),
            };
        }

        let trend_direction = self.calculate_trend_direction(&recent_violations);
        let change_rate = self.calculate_change_rate(&recent_violations);
        let seasonal_patterns = self.detect_seasonal_patterns(&recent_violations);
        let correlations = self.detect_system_correlations(&recent_violations);
        let insights = self.generate_predictive_insights(&recent_violations);

        TrendAnalysis {
            increasing_trend: trend_direction > 0.0,
            change_rate,
            seasonal_patterns,
            system_change_correlations: correlations,
            predictive_insights: insights,
        }
    }

    /// Export violation data in various formats
    pub fn export_violations(
        &self,
        format: &str,
        filter: Option<ViolationFilter>,
    ) -> Result<String, String> {
        let violations: Vec<_> = if let Some(filter) = filter {
            self.violation_history
                .values()
                .filter(|analysis| self.matches_filter(analysis, &filter))
                .collect()
        } else {
            self.violation_history.values().collect()
        };

        match format {
            "json" => self.export_as_json(&violations),
            "csv" => self.export_as_csv(&violations),
            "grafana" => self.export_as_grafana(&violations),
            "siem" => self.export_as_siem(&violations),
            _ => Err(format!("Unsupported export format: {}", format)),
        }
    }

    /// Get analyzer statistics
    pub fn get_statistics(&self) -> &AnalyzerStats {
        &self.stats
    }

    /// Clear violation history
    pub fn clear_history(&mut self) {
        self.violation_history.clear();
        web_sys::console::log_1(&"Violation history cleared".into());
    }

    // Private implementation methods

    fn classify_violation(
        &self,
        violation: &ViolationInstance,
        causality_analysis: Option<&PropertyCausalityAnalysis>,
    ) -> ViolationClassification {
        // Analyze violation details to determine type
        let violation_type = self.determine_violation_type(&violation.details);
        let severity = self.assess_severity(violation, causality_analysis);
        let confidence = self.calculate_classification_confidence(violation, causality_analysis);

        let tags = self.extract_classification_tags(&violation.details);
        let is_recurring = self.check_if_recurring(&violation_type);
        let pattern_family = self.identify_pattern_family(&violation_type, causality_analysis);

        ViolationClassification {
            violation_type,
            severity,
            confidence,
            tags,
            is_recurring,
            pattern_family,
        }
    }

    fn extract_violation_context(
        &self,
        violation: &ViolationInstance,
        causality_analysis: Option<&PropertyCausalityAnalysis>,
    ) -> ViolationContext {
        // Extract system state at time of violation
        let system_state = self.extract_system_state(violation);
        let network_conditions = self.extract_network_conditions(violation, causality_analysis);
        let participant_status = self.extract_participant_status(violation, causality_analysis);
        let resource_utilization = self.extract_resource_utilization(violation);
        let recent_events = self.extract_recent_events(violation, causality_analysis);
        let environmental_factors =
            self.extract_environmental_factors(violation, causality_analysis);

        ViolationContext {
            system_state,
            network_conditions,
            participant_status,
            resource_utilization,
            recent_events,
            environmental_factors,
        }
    }

    fn perform_root_cause_analysis(
        &self,
        violation: &ViolationInstance,
        causality_analysis: Option<&PropertyCausalityAnalysis>,
        context: &ViolationContext,
    ) -> RootCauseAnalysis {
        let primary_cause = self.identify_primary_cause(violation, causality_analysis, context);
        let contributing_causes = self.identify_contributing_causes(causality_analysis, context);
        let causal_chain_summary = self.summarize_causal_chain(causality_analysis);
        let key_decision_points = self.identify_decision_points(causality_analysis);
        let alternative_paths = self.identify_alternative_paths(causality_analysis, context);

        RootCauseAnalysis {
            primary_cause,
            contributing_causes,
            causal_chain_summary,
            key_decision_points,
            alternative_paths,
        }
    }

    fn generate_debugging_guide(
        &self,
        classification: &ViolationClassification,
        root_cause_analysis: &RootCauseAnalysis,
        causality_analysis: Option<&PropertyCausalityAnalysis>,
    ) -> DebuggingGuide {
        let overview = format!(
            "Investigation guide for {} violation with {} severity",
            format!("{:?}", classification.violation_type).to_lowercase(),
            format!("{:?}", classification.severity).to_lowercase()
        );

        let steps = self.generate_debugging_steps(classification, root_cause_analysis);
        let recommended_tools = self.recommend_debugging_tools(classification);
        let pitfalls_to_avoid = self.identify_debugging_pitfalls(classification);
        let estimated_investigation_time = self.estimate_investigation_time(classification);

        DebuggingGuide {
            overview,
            steps,
            recommended_tools,
            pitfalls_to_avoid,
            estimated_investigation_time,
        }
    }

    fn generate_remediation_strategies(
        &self,
        classification: &ViolationClassification,
        root_cause_analysis: &RootCauseAnalysis,
        context: &ViolationContext,
    ) -> Vec<RemediationStrategy> {
        let mut strategies = Vec::new();

        // Immediate fix strategy
        if let Some(immediate_fix) =
            self.generate_immediate_fix(classification, root_cause_analysis)
        {
            strategies.push(immediate_fix);
        }

        // Prevention strategies
        strategies.extend(self.generate_prevention_strategies(classification, root_cause_analysis));

        // System hardening strategies
        strategies.extend(self.generate_hardening_strategies(classification, context));

        // Monitoring improvement strategies
        strategies.extend(self.generate_monitoring_strategies(classification));

        strategies
    }

    fn assess_impact(
        &self,
        violation: &ViolationInstance,
        classification: &ViolationClassification,
        context: &ViolationContext,
    ) -> ImpactAssessment {
        let overall_impact = self.calculate_overall_impact(classification, context);
        let affected_components = self.identify_affected_components(violation, context);
        let business_impact = self.assess_business_impact(classification, context);
        let technical_impact = self.assess_technical_impact(classification, context);
        let user_impact = self.assess_user_impact(classification, context);
        let recovery_time_estimate = self.estimate_recovery_time(classification);
        let cascading_effects = self.identify_cascading_effects(classification, context);

        ImpactAssessment {
            overall_impact,
            affected_components,
            business_impact,
            technical_impact,
            user_impact,
            recovery_time_estimate,
            cascading_effects,
        }
    }

    fn analyze_similarities(
        &self,
        classification: &ViolationClassification,
        context: &ViolationContext,
    ) -> SimilarityAnalysis {
        let similar_violations = self.find_similar_violations(classification, context);
        let patterns = self.identify_violation_patterns(classification);
        let trends = self.get_trend_analysis(7 * 24 * 60 * 60 * 1000); // 7 days
        let pattern_based_recommendations = self.generate_pattern_recommendations(&patterns);

        SimilarityAnalysis {
            similar_violations,
            patterns,
            trends,
            pattern_based_recommendations,
        }
    }

    fn generate_export_data(
        &self,
        violation: &ViolationInstance,
        classification: &ViolationClassification,
        impact_assessment: &ImpactAssessment,
    ) -> ExportData {
        let json_export = self.generate_json_export(violation, classification, impact_assessment);
        let csv_export = self.generate_csv_export(violation, classification, impact_assessment);
        let grafana_metrics =
            self.generate_grafana_metrics(violation, classification, impact_assessment);
        let siem_events = self.generate_siem_events(violation, classification, impact_assessment);
        let custom_exports = HashMap::new();

        ExportData {
            json_export,
            csv_export,
            grafana_metrics,
            siem_events,
            custom_exports,
        }
    }

    // Helper methods (simplified implementations)

    fn determine_violation_type(&self, details: &ViolationDetails) -> ViolationType {
        // Analyze violation description and context to determine type
        if details.description.contains("safety") {
            ViolationType::SafetyViolation
        } else if details.description.contains("liveness") {
            ViolationType::LivenessViolation
        } else if details.description.contains("consistency") {
            ViolationType::ConsistencyViolation
        } else if details.description.contains("threshold") {
            ViolationType::ThresholdViolation
        } else if details.description.contains("protocol") {
            ViolationType::ProtocolViolation
        } else if details.description.contains("byzantine") {
            ViolationType::ByzantineViolation
        } else {
            ViolationType::SafetyViolation // Default
        }
    }

    fn assess_severity(
        &self,
        violation: &ViolationInstance,
        causality_analysis: Option<&PropertyCausalityAnalysis>,
    ) -> SeverityLevel {
        // Assess severity based on various factors
        let has_critical_events = causality_analysis
            .map(|ca| !ca.critical_events.is_empty())
            .unwrap_or(false);

        if has_critical_events || violation.details.description.contains("critical") {
            SeverityLevel::Critical
        } else if violation.details.description.contains("high") {
            SeverityLevel::High
        } else if violation.details.description.contains("medium") {
            SeverityLevel::Medium
        } else if violation.details.description.contains("low") {
            SeverityLevel::Low
        } else {
            SeverityLevel::Medium // Default
        }
    }

    fn calculate_classification_confidence(
        &self,
        _violation: &ViolationInstance,
        causality_analysis: Option<&PropertyCausalityAnalysis>,
    ) -> f64 {
        // Higher confidence if we have detailed causality analysis
        if causality_analysis.is_some() {
            0.9
        } else {
            0.6
        }
    }

    fn extract_classification_tags(&self, details: &ViolationDetails) -> Vec<String> {
        let mut tags = Vec::new();

        if details.description.contains("network") {
            tags.push("network".to_string());
        }
        if details.description.contains("consensus") {
            tags.push("consensus".to_string());
        }
        if details.description.contains("threshold") {
            tags.push("threshold".to_string());
        }

        tags
    }

    fn check_if_recurring(&self, violation_type: &ViolationType) -> bool {
        // Check if this violation type has occurred before
        self.violation_history
            .values()
            .any(|analysis| analysis.classification.violation_type == *violation_type)
    }

    fn identify_pattern_family(
        &self,
        violation_type: &ViolationType,
        causality_analysis: Option<&PropertyCausalityAnalysis>,
    ) -> PatternFamily {
        match violation_type {
            ViolationType::ByzantineViolation => PatternFamily::ByzantinePatterns,
            ViolationType::PartitionViolation => PatternFamily::PartitionPatterns,
            ViolationType::ThresholdViolation => PatternFamily::ThresholdSignaturePatterns,
            ViolationType::ProtocolViolation => PatternFamily::SessionTypePatterns,
            ViolationType::ConsistencyViolation => PatternFamily::StateSyncPatterns,
            ViolationType::ResourceViolation => PatternFamily::ResourceExhaustionPatterns,
            _ => {
                // Use causality analysis to determine pattern family
                if let Some(ca) = causality_analysis {
                    if ca
                        .contributing_factors
                        .iter()
                        .any(|f| f.factor_type == ContributingFactorType::RaceCondition)
                    {
                        PatternFamily::RaceConditions
                    } else if ca
                        .contributing_factors
                        .iter()
                        .any(|f| f.factor_type == ContributingFactorType::NetworkPartition)
                    {
                        PatternFamily::PartitionPatterns
                    } else {
                        PatternFamily::Unknown
                    }
                } else {
                    PatternFamily::Unknown
                }
            }
        }
    }

    // Additional helper methods would be implemented here...
    // For brevity, I'm including placeholder implementations

    fn extract_system_state(&self, _violation: &ViolationInstance) -> SystemState {
        SystemState {
            current_epoch: 0,
            active_participants: HashSet::new(),
            pending_operations: 0,
            state_hash: "placeholder".to_string(),
            session_states: HashMap::new(),
            threshold_requirements: HashMap::new(),
        }
    }

    fn extract_network_conditions(
        &self,
        _violation: &ViolationInstance,
        _causality_analysis: Option<&PropertyCausalityAnalysis>,
    ) -> NetworkConditions {
        NetworkConditions {
            partitions: Vec::new(),
            delivery_rates: HashMap::new(),
            avg_latency_ms: 0.0,
            stability_score: 1.0,
            recent_network_events: Vec::new(),
        }
    }

    fn extract_participant_status(
        &self,
        _violation: &ViolationInstance,
        _causality_analysis: Option<&PropertyCausalityAnalysis>,
    ) -> ParticipantStatus {
        ParticipantStatus {
            online: HashSet::new(),
            offline: HashSet::new(),
            byzantine: HashSet::new(),
            response_times: HashMap::new(),
            trust_scores: HashMap::new(),
        }
    }

    fn extract_resource_utilization(&self, _violation: &ViolationInstance) -> ResourceUtilization {
        ResourceUtilization {
            memory_usage_percent: 0.0,
            cpu_usage_percent: 0.0,
            network_bandwidth_percent: 0.0,
            storage_usage_bytes: 0,
            open_connections: 0,
            queue_depths: HashMap::new(),
        }
    }

    fn extract_recent_events(
        &self,
        _violation: &ViolationInstance,
        _causality_analysis: Option<&PropertyCausalityAnalysis>,
    ) -> Vec<ContextualEvent> {
        Vec::new()
    }

    fn extract_environmental_factors(
        &self,
        _violation: &ViolationInstance,
        _causality_analysis: Option<&PropertyCausalityAnalysis>,
    ) -> Vec<EnvironmentalFactor> {
        Vec::new()
    }

    fn identify_primary_cause(
        &self,
        _violation: &ViolationInstance,
        _causality_analysis: Option<&PropertyCausalityAnalysis>,
        _context: &ViolationContext,
    ) -> RootCause {
        RootCause {
            cause_type: RootCauseType::StateInconsistency,
            description: "Primary cause analysis pending".to_string(),
            confidence: 0.5,
            evidence_events: Vec::new(),
            involved_participants: Vec::new(),
            remediation_complexity: ComplexityLevel::Medium,
        }
    }

    fn identify_contributing_causes(
        &self,
        _causality_analysis: Option<&PropertyCausalityAnalysis>,
        _context: &ViolationContext,
    ) -> Vec<RootCause> {
        Vec::new()
    }

    fn summarize_causal_chain(
        &self,
        causality_analysis: Option<&PropertyCausalityAnalysis>,
    ) -> String {
        if let Some(ca) = causality_analysis {
            format!(
                "Causal chain with {} events leading to violation",
                ca.causality_chain.chain_length
            )
        } else {
            "No detailed causal chain available".to_string()
        }
    }

    fn identify_decision_points(
        &self,
        _causality_analysis: Option<&PropertyCausalityAnalysis>,
    ) -> Vec<DecisionPoint> {
        Vec::new()
    }

    fn identify_alternative_paths(
        &self,
        _causality_analysis: Option<&PropertyCausalityAnalysis>,
        _context: &ViolationContext,
    ) -> Vec<AlternativePath> {
        Vec::new()
    }

    fn generate_debugging_steps(
        &self,
        classification: &ViolationClassification,
        _root_cause_analysis: &RootCauseAnalysis,
    ) -> Vec<DebuggingStep> {
        vec![DebuggingStep {
            step_number: 1,
            title: "Verify violation occurrence".to_string(),
            instructions: format!(
                "Confirm the {} violation actually occurred",
                format!("{:?}", classification.violation_type)
            ),
            expected_outcomes: vec!["Violation confirmed".to_string()],
            fallback_actions: vec!["Check property specification".to_string()],
            related_events: Vec::new(),
            suggested_commands: vec!["Check logs".to_string()],
        }]
    }

    fn recommend_debugging_tools(
        &self,
        _classification: &ViolationClassification,
    ) -> Vec<RecommendedTool> {
        vec![RecommendedTool {
            name: "Causality Graph Analyzer".to_string(),
            description: "Analyze event causality".to_string(),
            usage_guidance: "Examine event dependencies".to_string(),
            expected_insights: vec!["Root cause identification".to_string()],
        }]
    }

    fn identify_debugging_pitfalls(
        &self,
        _classification: &ViolationClassification,
    ) -> Vec<String> {
        vec![
            "Don't assume first suspicious event is the root cause".to_string(),
            "Consider concurrent events that may not be directly related".to_string(),
        ]
    }

    fn estimate_investigation_time(&self, classification: &ViolationClassification) -> String {
        match classification.severity {
            SeverityLevel::Critical => "1-2 hours".to_string(),
            SeverityLevel::High => "2-4 hours".to_string(),
            SeverityLevel::Medium => "4-8 hours".to_string(),
            SeverityLevel::Low => "1-2 days".to_string(),
            SeverityLevel::Info => "As time permits".to_string(),
        }
    }

    fn generate_immediate_fix(
        &self,
        _classification: &ViolationClassification,
        _root_cause_analysis: &RootCauseAnalysis,
    ) -> Option<RemediationStrategy> {
        Some(RemediationStrategy {
            name: "Immediate Stabilization".to_string(),
            strategy_type: RemediationStrategyType::ImmediateFix,
            description: "Immediate steps to stabilize the system".to_string(),
            implementation_steps: Vec::new(),
            effectiveness: 0.8,
            complexity: ComplexityLevel::Low,
            implementation_time: "15-30 minutes".to_string(),
            side_effects: Vec::new(),
            prerequisites: Vec::new(),
        })
    }

    fn generate_prevention_strategies(
        &self,
        _classification: &ViolationClassification,
        _root_cause_analysis: &RootCauseAnalysis,
    ) -> Vec<RemediationStrategy> {
        Vec::new()
    }

    fn generate_hardening_strategies(
        &self,
        _classification: &ViolationClassification,
        _context: &ViolationContext,
    ) -> Vec<RemediationStrategy> {
        Vec::new()
    }

    fn generate_monitoring_strategies(
        &self,
        _classification: &ViolationClassification,
    ) -> Vec<RemediationStrategy> {
        Vec::new()
    }

    fn calculate_overall_impact(
        &self,
        classification: &ViolationClassification,
        _context: &ViolationContext,
    ) -> f64 {
        match classification.severity {
            SeverityLevel::Critical => 1.0,
            SeverityLevel::High => 0.8,
            SeverityLevel::Medium => 0.6,
            SeverityLevel::Low => 0.4,
            SeverityLevel::Info => 0.2,
        }
    }

    fn identify_affected_components(
        &self,
        _violation: &ViolationInstance,
        _context: &ViolationContext,
    ) -> Vec<String> {
        vec!["consensus".to_string(), "state_sync".to_string()]
    }

    fn assess_business_impact(
        &self,
        _classification: &ViolationClassification,
        _context: &ViolationContext,
    ) -> BusinessImpact {
        BusinessImpact {
            availability_impact: 0.5,
            data_integrity_impact: 0.3,
            performance_impact: 0.4,
            security_impact: 0.2,
            compliance_impact: 0.1,
        }
    }

    fn assess_technical_impact(
        &self,
        _classification: &ViolationClassification,
        _context: &ViolationContext,
    ) -> TechnicalImpact {
        TechnicalImpact {
            stability_impact: 0.6,
            scalability_impact: 0.3,
            maintainability_impact: 0.2,
            resource_impact: 0.4,
        }
    }

    fn assess_user_impact(
        &self,
        _classification: &ViolationClassification,
        _context: &ViolationContext,
    ) -> UserImpact {
        UserImpact {
            affected_users: 0,
            experience_degradation: 0.3,
            unavailable_features: Vec::new(),
            performance_degradation: 0.2,
        }
    }

    fn estimate_recovery_time(&self, classification: &ViolationClassification) -> String {
        match classification.severity {
            SeverityLevel::Critical => "1-4 hours".to_string(),
            SeverityLevel::High => "4-12 hours".to_string(),
            SeverityLevel::Medium => "12-24 hours".to_string(),
            SeverityLevel::Low => "1-3 days".to_string(),
            SeverityLevel::Info => "No recovery needed".to_string(),
        }
    }

    fn identify_cascading_effects(
        &self,
        _classification: &ViolationClassification,
        _context: &ViolationContext,
    ) -> Vec<CascadingEffect> {
        Vec::new()
    }

    fn find_similar_violations(
        &self,
        classification: &ViolationClassification,
        _context: &ViolationContext,
    ) -> Vec<SimilarViolation> {
        self.violation_history
            .values()
            .filter(|analysis| {
                analysis.classification.violation_type == classification.violation_type
            })
            .take(5)
            .map(|analysis| SimilarViolation {
                analysis_id: analysis.analysis_id.clone(),
                similarity_score: 0.8,
                key_similarities: vec!["Same violation type".to_string()],
                key_differences: vec!["Different context".to_string()],
                occurred_at: analysis.violation.tick,
                resolution_approach: "Standard remediation".to_string(),
                resolution_effectiveness: 0.7,
            })
            .collect()
    }

    fn identify_violation_patterns(
        &self,
        _classification: &ViolationClassification,
    ) -> Vec<ViolationPattern> {
        Vec::new()
    }

    fn generate_pattern_recommendations(&self, _patterns: &[ViolationPattern]) -> Vec<String> {
        vec!["Monitor for similar patterns".to_string()]
    }

    fn calculate_similarity_score(
        &self,
        analysis1: &ViolationAnalysis,
        analysis2: &ViolationAnalysis,
    ) -> f64 {
        let mut score = 0.0;

        // Same violation type
        if analysis1.classification.violation_type == analysis2.classification.violation_type {
            score += 0.3;
        }

        // Same severity
        if analysis1.classification.severity == analysis2.classification.severity {
            score += 0.2;
        }

        // Same pattern family
        if analysis1.classification.pattern_family == analysis2.classification.pattern_family {
            score += 0.3;
        }

        // Similar contexts (simplified)
        if !analysis1.context.recent_events.is_empty()
            && !analysis2.context.recent_events.is_empty()
        {
            score += 0.2;
        }

        score
    }

    fn identify_similarities(
        &self,
        analysis1: &ViolationAnalysis,
        analysis2: &ViolationAnalysis,
    ) -> Vec<String> {
        let mut similarities = Vec::new();

        if analysis1.classification.violation_type == analysis2.classification.violation_type {
            similarities.push("Same violation type".to_string());
        }

        if analysis1.classification.severity == analysis2.classification.severity {
            similarities.push("Same severity level".to_string());
        }

        similarities
    }

    fn identify_differences(
        &self,
        analysis1: &ViolationAnalysis,
        analysis2: &ViolationAnalysis,
    ) -> Vec<String> {
        let mut differences = Vec::new();

        if analysis1.classification.violation_type != analysis2.classification.violation_type {
            differences.push("Different violation types".to_string());
        }

        if analysis1.classification.severity != analysis2.classification.severity {
            differences.push("Different severity levels".to_string());
        }

        differences
    }

    fn generate_comparison_recommendations(
        &self,
        _analysis1: &ViolationAnalysis,
        _analysis2: &ViolationAnalysis,
    ) -> Vec<String> {
        vec!["Compare remediation effectiveness".to_string()]
    }

    fn calculate_trend_direction(&self, _violations: &[&ViolationAnalysis]) -> f64 {
        0.0 // Simplified
    }

    fn calculate_change_rate(&self, _violations: &[&ViolationAnalysis]) -> f64 {
        0.0 // Simplified
    }

    fn detect_seasonal_patterns(&self, _violations: &[&ViolationAnalysis]) -> Vec<String> {
        Vec::new()
    }

    fn detect_system_correlations(&self, _violations: &[&ViolationAnalysis]) -> Vec<String> {
        Vec::new()
    }

    fn generate_predictive_insights(&self, _violations: &[&ViolationAnalysis]) -> Vec<String> {
        Vec::new()
    }

    fn export_as_json(&self, violations: &[&ViolationAnalysis]) -> Result<String, String> {
        serde_json::to_string_pretty(violations)
            .map_err(|e| format!("JSON serialization error: {}", e))
    }

    fn export_as_csv(&self, violations: &[&ViolationAnalysis]) -> Result<String, String> {
        let mut csv = String::new();
        csv.push_str("analysis_id,property_id,violation_type,severity,tick,description\n");

        for violation in violations {
            csv.push_str(&format!(
                "{},{:?},{:?},{:?},{},{}\n",
                violation.analysis_id,
                violation.property_id,
                violation.classification.violation_type,
                violation.classification.severity,
                violation.violation.tick,
                violation.violation.details.description.replace(',', ";")
            ));
        }

        Ok(csv)
    }

    fn export_as_grafana(&self, violations: &[&ViolationAnalysis]) -> Result<String, String> {
        let metrics: Vec<_> = violations
            .iter()
            .map(|v| {
                format!(
                    "violation_count{{type=\"{:?}\",severity=\"{:?}\"}} 1",
                    v.classification.violation_type, v.classification.severity
                )
            })
            .collect();

        Ok(metrics.join("\n"))
    }

    fn export_as_siem(&self, violations: &[&ViolationAnalysis]) -> Result<String, String> {
        let events: Vec<_> = violations
            .iter()
            .map(|v| {
                format!(
                    "timestamp={} severity={:?} category=property_violation description=\"{}\"",
                    v.violation.tick, v.classification.severity, v.violation.details.description
                )
            })
            .collect();

        Ok(events.join("\n"))
    }

    fn matches_filter(&self, _analysis: &ViolationAnalysis, _filter: &ViolationFilter) -> bool {
        true // Simplified
    }

    fn generate_json_export(
        &self,
        _violation: &ViolationInstance,
        _classification: &ViolationClassification,
        _impact_assessment: &ImpactAssessment,
    ) -> String {
        "{}".to_string() // Simplified
    }

    fn generate_csv_export(
        &self,
        _violation: &ViolationInstance,
        _classification: &ViolationClassification,
        _impact_assessment: &ImpactAssessment,
    ) -> String {
        "".to_string() // Simplified
    }

    fn generate_grafana_metrics(
        &self,
        _violation: &ViolationInstance,
        _classification: &ViolationClassification,
        _impact_assessment: &ImpactAssessment,
    ) -> Vec<GrafanaMetric> {
        Vec::new()
    }

    fn generate_siem_events(
        &self,
        _violation: &ViolationInstance,
        _classification: &ViolationClassification,
        _impact_assessment: &ImpactAssessment,
    ) -> Vec<SiemEvent> {
        Vec::new()
    }

    fn calculate_quality_score(
        &self,
        _classification: &ViolationClassification,
        has_causality: bool,
    ) -> f64 {
        if has_causality {
            0.9
        } else {
            0.6
        }
    }

    fn update_statistics(&mut self, analysis: &ViolationAnalysis) {
        self.stats.total_violations_analyzed += 1;
        self.stats.avg_analysis_time_ms = (self.stats.avg_analysis_time_ms
            * (self.stats.total_violations_analyzed - 1) as f64
            + analysis.metadata.computation_time_ms as f64)
            / self.stats.total_violations_analyzed as f64;

        *self
            .stats
            .common_violation_types
            .entry(analysis.classification.violation_type.clone())
            .or_insert(0) += 1;
    }
}

/// Filter for violation queries
#[derive(Debug, Clone)]
pub struct ViolationFilter {
    pub violation_types: Option<HashSet<ViolationType>>,
    pub severity_levels: Option<HashSet<SeverityLevel>>,
    pub time_range: Option<(u64, u64)>,
    pub pattern_families: Option<HashSet<PatternFamily>>,
}

/// Comparison result between two violations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViolationComparison {
    pub analysis1_id: String,
    pub analysis2_id: String,
    pub similarity_score: f64,
    pub key_similarities: Vec<String>,
    pub key_differences: Vec<String>,
    pub recommendations: Vec<String>,
}

// Default implementations

impl Default for AnalyzerConfig {
    fn default() -> Self {
        Self {
            max_history_size: 1000,
            similarity_threshold: 0.7,
            enable_causality_analysis: true,
            enable_trend_analysis: true,
            export_formats: ["json", "csv"].iter().map(|s| s.to_string()).collect(),
        }
    }
}

impl Default for AnalyzerStats {
    fn default() -> Self {
        Self {
            total_violations_analyzed: 0,
            patterns_identified: 0,
            avg_analysis_time_ms: 0.0,
            common_violation_types: HashMap::new(),
            effective_strategies: HashMap::new(),
        }
    }
}

/// WASM bindings for browser usage
#[wasm_bindgen]
pub struct WasmViolationAnalyzer {
    inner: ViolationAnalyzer,
}

#[wasm_bindgen]
impl WasmViolationAnalyzer {
    /// Create a new violation analyzer
    #[wasm_bindgen(constructor)]
    pub fn new() -> WasmViolationAnalyzer {
        WasmViolationAnalyzer {
            inner: ViolationAnalyzer::new(),
        }
    }

    /// Analyze a violation (simplified interface for WASM)
    pub fn analyze_violation_simple(
        &mut self,
        property_id: &str,
        violation_data: JsValue,
    ) -> JsValue {
        // Parse the violation data from JavaScript
        match serde_wasm_bindgen::from_value::<ViolationInstance>(violation_data) {
            Ok(violation) => {
                // Parse property ID
                if let Ok(property_uuid) = uuid::Uuid::parse_str(property_id) {
                    let property_id = PropertyId::from(property_uuid);
                    let analysis = self.inner.analyze_violation(property_id, violation, None);
                    serde_wasm_bindgen::to_value(&analysis).unwrap_or(JsValue::NULL)
                } else {
                    JsValue::NULL
                }
            }
            Err(_) => JsValue::NULL,
        }
    }

    /// Get analyzer statistics as JSON
    pub fn get_statistics(&self) -> JsValue {
        serde_wasm_bindgen::to_value(self.inner.get_statistics()).unwrap_or(JsValue::NULL)
    }

    /// Export violations in specified format
    pub fn export_violations(&self, format: &str) -> String {
        self.inner
            .export_violations(format, None)
            .unwrap_or_default()
    }

    /// Clear violation history
    pub fn clear_history(&mut self) {
        self.inner.clear_history();
    }

    /// Get trend analysis
    pub fn get_trend_analysis(&self, time_window_ms: u64) -> JsValue {
        let trends = self.inner.get_trend_analysis(time_window_ms);
        serde_wasm_bindgen::to_value(&trends).unwrap_or(JsValue::NULL)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn create_test_violation() -> ViolationInstance {
        ViolationInstance {
            tick: 100,
            state_hash: 12345,
            details: ViolationDetails {
                description: "Test safety violation".to_string(),
                context: HashMap::new(),
                causality_trace: vec![1, 2, 3],
                debugging_hints: vec!["Check logs".to_string()],
            },
            resolved: false,
        }
    }

    #[test]
    fn test_violation_analyzer_creation() {
        let analyzer = ViolationAnalyzer::new();
        assert_eq!(analyzer.stats.total_violations_analyzed, 0);
    }

    #[test]
    fn test_violation_classification() {
        let mut analyzer = ViolationAnalyzer::new();
        let property_id = PropertyId::new_v4();
        let violation = create_test_violation();

        let analysis = analyzer.analyze_violation(property_id, violation, None);

        assert_eq!(analysis.property_id, property_id);
        assert_eq!(
            analysis.classification.violation_type,
            ViolationType::SafetyViolation
        );
        assert!(analysis.metadata.computation_time_ms >= 0);
    }

    #[test]
    fn test_violation_export() {
        let analyzer = ViolationAnalyzer::new();

        let json_export = analyzer.export_violations("json", None);
        assert!(json_export.is_ok());

        let csv_export = analyzer.export_violations("csv", None);
        assert!(csv_export.is_ok());
    }

    #[test]
    fn test_similarity_analysis() {
        let mut analyzer = ViolationAnalyzer::new();
        let property_id = PropertyId::new_v4();
        let violation1 = create_test_violation();
        let violation2 = create_test_violation();

        let analysis1 = analyzer.analyze_violation(property_id, violation1, None);
        let analysis2 = analyzer.analyze_violation(property_id, violation2, None);

        let comparison = analyzer.compare_violations(&analysis1, &analysis2);
        assert!(comparison.similarity_score >= 0.0);
    }

    #[test]
    fn test_violation_classification_types() {
        let mut analyzer = ViolationAnalyzer::new();
        let property_id = PropertyId::new_v4();

        // Test different violation types based on description
        let test_cases = vec![
            ("Test safety violation", ViolationType::SafetyViolation),
            ("Test liveness violation", ViolationType::LivenessViolation),
            (
                "Test consistency violation",
                ViolationType::ConsistencyViolation,
            ),
            (
                "Test threshold violation",
                ViolationType::ThresholdViolation,
            ),
            ("Test protocol violation", ViolationType::ProtocolViolation),
            (
                "Test byzantine violation",
                ViolationType::ByzantineViolation,
            ),
        ];

        for (description, expected_type) in test_cases {
            let violation = ViolationInstance {
                tick: 100,
                state_hash: 12345,
                details: ViolationDetails {
                    description: description.to_string(),
                    context: HashMap::new(),
                    causality_trace: vec![1, 2, 3],
                    debugging_hints: vec!["Check logs".to_string()],
                },
                resolved: false,
            };

            let analysis = analyzer.analyze_violation(property_id, violation, None);
            assert_eq!(analysis.classification.violation_type, expected_type);
        }
    }

    #[test]
    fn test_severity_assessment() {
        let mut analyzer = ViolationAnalyzer::new();
        let property_id = PropertyId::new_v4();

        let test_cases = vec![
            ("critical system failure", SeverityLevel::Critical),
            ("high impact error", SeverityLevel::High),
            ("medium priority issue", SeverityLevel::Medium),
            ("low impact warning", SeverityLevel::Low),
        ];

        for (description, expected_severity) in test_cases {
            let violation = ViolationInstance {
                tick: 100,
                state_hash: 12345,
                details: ViolationDetails {
                    description: description.to_string(),
                    context: HashMap::new(),
                    causality_trace: vec![1, 2, 3],
                    debugging_hints: vec!["Check logs".to_string()],
                },
                resolved: false,
            };

            let analysis = analyzer.analyze_violation(property_id, violation, None);
            assert_eq!(analysis.classification.severity, expected_severity);
        }
    }

    #[test]
    fn test_wasm_integration() {
        let wasm_analyzer = WasmViolationAnalyzer::new();
        let stats = wasm_analyzer.get_statistics();

        // Should return valid JSON
        assert!(!stats.is_null());
    }
}
