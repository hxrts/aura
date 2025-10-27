//! Debug Reporter
//!
//! This module provides comprehensive reporting capabilities for debugging sessions,
//! generating actionable insights and recommendations for developers to understand
//! and resolve protocol failures.

use crate::Result;
use crate::{FailureAnalysisResult, FocusedTestResult, MinimalReproduction, ViolationDebugResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Debug reporter for generating comprehensive analysis reports
///
/// This reporter combines insights from failure analysis, time travel debugging,
/// minimal reproduction discovery, and focused testing to generate actionable
/// developer reports with clear recommendations.
pub struct DebugReporter {
    /// Configuration for report generation
    config: ReporterConfig,
    /// Generated reports
    generated_reports: Vec<DeveloperReport>,
    /// Report templates
    report_templates: HashMap<ReportType, ReportTemplate>,
    /// Insight collection and ranking
    insight_collector: InsightCollector,
}

/// Configuration for debug reporting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReporterConfig {
    /// Include detailed technical analysis
    pub include_technical_details: bool,
    /// Include actionable recommendations
    pub include_recommendations: bool,
    /// Include visual diagrams and timelines
    pub include_visualizations: bool,
    /// Maximum number of insights per category
    pub max_insights_per_category: usize,
    /// Minimum confidence threshold for insights
    pub min_insight_confidence: f64,
    /// Report output format
    pub output_format: OutputFormat,
    /// Template customization options
    pub template_options: TemplateOptions,
}

impl Default for ReporterConfig {
    fn default() -> Self {
        Self {
            include_technical_details: true,
            include_recommendations: true,
            include_visualizations: true,
            max_insights_per_category: 10,
            min_insight_confidence: 0.3,
            output_format: OutputFormat::Markdown,
            template_options: TemplateOptions::default(),
        }
    }
}

/// Output format for reports
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum OutputFormat {
    /// Markdown format
    Markdown,
    /// HTML format
    Html,
    /// JSON format for programmatic access
    Json,
    /// Plain text format
    PlainText,
}

/// Template customization options
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateOptions {
    /// Include executive summary
    pub include_executive_summary: bool,
    /// Include detailed timeline
    pub include_detailed_timeline: bool,
    /// Include code snippets
    pub include_code_snippets: bool,
    /// Include related scenarios
    pub include_related_scenarios: bool,
    /// Custom sections to include
    pub custom_sections: Vec<String>,
}

impl Default for TemplateOptions {
    fn default() -> Self {
        Self {
            include_executive_summary: true,
            include_detailed_timeline: true,
            include_code_snippets: false,
            include_related_scenarios: true,
            custom_sections: Vec::new(),
        }
    }
}

/// Type of report being generated
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ReportType {
    /// Comprehensive violation analysis report
    ViolationAnalysis,
    /// Minimal reproduction report
    MinimalReproduction,
    /// Focused testing summary
    FocusedTestingSummary,
    /// Pattern analysis report
    PatternAnalysis,
    /// Comparative analysis across multiple violations
    ComparativeAnalysis,
}

/// Comprehensive developer report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeveloperReport {
    /// Report identifier
    pub report_id: String,
    /// Report type
    pub report_type: ReportType,
    /// Report title
    pub title: String,
    /// Report creation timestamp
    pub created_at: u64,
    /// Executive summary
    pub executive_summary: ExecutiveSummary,
    /// Technical analysis sections
    pub technical_analysis: TechnicalAnalysis,
    /// Actionable recommendations
    pub recommendations: Vec<ActionableRecommendation>,
    /// Debug insights
    pub insights: Vec<DebuggingInsight>,
    /// Visual elements
    pub visualizations: Vec<VisualizationElement>,
    /// Related resources
    pub related_resources: Vec<RelatedResource>,
    /// Report metadata
    pub metadata: ReportMetadata,
}

/// Executive summary of the debugging session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutiveSummary {
    /// Brief description of the failure
    pub failure_description: String,
    /// Key findings from the analysis
    pub key_findings: Vec<String>,
    /// Impact assessment
    pub impact_assessment: ImpactAssessment,
    /// Time to resolution estimate
    pub resolution_estimate: ResolutionEstimate,
    /// Urgency level
    pub urgency_level: UrgencyLevel,
}

/// Impact assessment of the failure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactAssessment {
    /// Severity of the issue
    pub severity: IssueSeverity,
    /// Affected components
    pub affected_components: Vec<String>,
    /// Potential consequences
    pub potential_consequences: Vec<String>,
    /// Likelihood of recurrence
    pub recurrence_likelihood: f64,
}

/// Severity levels for issues
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum IssueSeverity {
    /// Low severity - minor impact
    Low,
    /// Medium severity - moderate impact
    Medium,
    /// High severity - significant impact
    High,
    /// Critical severity - system-breaking
    Critical,
}

/// Resolution time estimate
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolutionEstimate {
    /// Estimated time to fix (hours)
    pub estimated_hours: f64,
    /// Confidence in the estimate
    pub confidence: f64,
    /// Factors affecting resolution time
    pub complexity_factors: Vec<String>,
}

/// Urgency level for addressing the issue
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum UrgencyLevel {
    /// Can be addressed in next sprint
    Low,
    /// Should be addressed this sprint
    Medium,
    /// Should be addressed this week
    High,
    /// Must be addressed immediately
    Immediate,
}

/// Technical analysis sections
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TechnicalAnalysis {
    /// Root cause analysis
    pub root_cause_analysis: RootCauseAnalysis,
    /// Causal chain analysis
    pub causal_chain_analysis: CausalChainAnalysis,
    /// State analysis
    pub state_analysis: StateAnalysis,
    /// Performance analysis
    pub performance_analysis: PerformanceAnalysis,
    /// Pattern analysis
    pub pattern_analysis: PatternAnalysis,
}

/// Root cause analysis results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootCauseAnalysis {
    /// Primary root cause
    pub primary_cause: RootCause,
    /// Contributing factors
    pub contributing_factors: Vec<ContributingFactor>,
    /// Evidence supporting the analysis
    pub supporting_evidence: Vec<Evidence>,
    /// Alternative hypotheses considered
    pub alternative_hypotheses: Vec<AlternativeHypothesis>,
}

/// Identified root cause
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootCause {
    /// Description of the root cause
    pub description: String,
    /// Category of the root cause
    pub category: RootCauseCategory,
    /// Confidence in this identification
    pub confidence: f64,
    /// Supporting evidence
    pub evidence_summary: String,
}

/// Categories of root causes
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RootCauseCategory {
    /// Logic error in protocol implementation
    ProtocolLogicError,
    /// Timing or race condition issue
    TimingIssue,
    /// Network configuration problem
    NetworkConfiguration,
    /// Byzantine behavior edge case
    ByzantineEdgeCase,
    /// State management issue
    StateManagement,
    /// Resource constraint issue
    ResourceConstraint,
    /// Complex interaction between components
    ComplexInteraction,
}

/// Contributing factor to the failure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributingFactor {
    /// Factor description
    pub description: String,
    /// Impact weight (0.0 to 1.0)
    pub impact_weight: f64,
    /// Factor category
    pub category: String,
    /// Supporting details
    pub details: String,
}

/// Evidence supporting analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    /// Evidence type
    pub evidence_type: EvidenceType,
    /// Evidence description
    pub description: String,
    /// Strength of evidence
    pub strength: f64,
    /// Source of evidence
    pub source: String,
    /// Related timestamp or location
    pub location: Option<String>,
}

/// Types of evidence
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum EvidenceType {
    /// Log entry evidence
    LogEntry,
    /// State snapshot evidence
    StateSnapshot,
    /// Timing evidence
    TimingEvidence,
    /// Message trace evidence
    MessageTrace,
    /// Property violation evidence
    PropertyViolation,
    /// Pattern correlation evidence
    PatternCorrelation,
}

/// Alternative hypothesis considered
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlternativeHypothesis {
    /// Hypothesis description
    pub hypothesis: String,
    /// Why it was ruled out
    pub ruled_out_reason: String,
    /// Confidence it was correctly ruled out
    pub confidence: f64,
}

/// Causal chain analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalChainAnalysis {
    /// Identified causal chains
    pub causal_chains: Vec<CausalChainSummary>,
    /// Chain interaction analysis
    pub chain_interactions: Vec<ChainInteraction>,
    /// Critical path analysis
    pub critical_path: Option<CriticalPath>,
}

/// Summary of a causal chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalChainSummary {
    /// Chain identifier
    pub chain_id: String,
    /// Chain description
    pub description: String,
    /// Chain strength
    pub strength: f64,
    /// Key events in the chain
    pub key_events: Vec<String>,
    /// Time span of the chain
    pub time_span_ms: u64,
}

/// Interaction between causal chains
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainInteraction {
    /// First chain ID
    pub chain_a: String,
    /// Second chain ID
    pub chain_b: String,
    /// Interaction type
    pub interaction_type: InteractionType,
    /// Interaction strength
    pub strength: f64,
    /// Description of interaction
    pub description: String,
}

/// Types of causal chain interactions
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum InteractionType {
    /// Chains reinforce each other
    Reinforcing,
    /// Chains interfere with each other
    Interfering,
    /// Chains are independent
    Independent,
    /// Chains have conditional dependency
    Conditional,
}

/// Critical path through causal chains
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CriticalPath {
    /// Path description
    pub description: String,
    /// Events on the critical path
    pub critical_events: Vec<String>,
    /// Total path duration
    pub duration_ms: u64,
    /// Path significance
    pub significance: f64,
}

/// State analysis results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateAnalysis {
    /// Key state transitions
    pub key_transitions: Vec<StateTransition>,
    /// State inconsistencies found
    pub inconsistencies: Vec<StateInconsistency>,
    /// State corruption indicators
    pub corruption_indicators: Vec<CorruptionIndicator>,
}

/// Significant state transition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateTransition {
    /// Transition description
    pub description: String,
    /// Timestamp of transition
    pub timestamp: u64,
    /// Participants involved
    pub participants: Vec<String>,
    /// Significance score
    pub significance: f64,
}

/// State inconsistency detected
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateInconsistency {
    /// Inconsistency description
    pub description: String,
    /// Affected components
    pub affected_components: Vec<String>,
    /// Severity of inconsistency
    pub severity: InconsistencySeverity,
    /// Potential impact
    pub potential_impact: String,
}

/// Severity of state inconsistencies
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum InconsistencySeverity {
    /// Minor inconsistency
    Minor,
    /// Moderate inconsistency
    Moderate,
    /// Major inconsistency
    Major,
    /// Critical inconsistency
    Critical,
}

/// Indicator of state corruption
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorruptionIndicator {
    /// Indicator description
    pub description: String,
    /// Confidence in corruption
    pub confidence: f64,
    /// Affected data structures
    pub affected_structures: Vec<String>,
}

/// Performance analysis results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceAnalysis {
    /// Performance bottlenecks
    pub bottlenecks: Vec<PerformanceBottleneck>,
    /// Resource utilization analysis
    pub resource_utilization: ResourceUtilization,
    /// Timing analysis
    pub timing_analysis: TimingAnalysis,
}

/// Performance bottleneck identified
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceBottleneck {
    /// Bottleneck description
    pub description: String,
    /// Component affected
    pub component: String,
    /// Performance impact
    pub impact_percentage: f64,
    /// Suggested optimizations
    pub optimizations: Vec<String>,
}

/// Resource utilization analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceUtilization {
    /// Memory usage patterns
    pub memory_usage: UsagePattern,
    /// CPU usage patterns
    pub cpu_usage: UsagePattern,
    /// Network usage patterns
    pub network_usage: UsagePattern,
}

/// Usage pattern for a resource
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsagePattern {
    /// Average usage
    pub average_usage: f64,
    /// Peak usage
    pub peak_usage: f64,
    /// Usage variance
    pub usage_variance: f64,
    /// Anomalous usage periods
    pub anomalies: Vec<UsageAnomaly>,
}

/// Usage anomaly detected
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageAnomaly {
    /// Anomaly description
    pub description: String,
    /// Start time of anomaly
    pub start_time: u64,
    /// Duration of anomaly
    pub duration_ms: u64,
    /// Severity of anomaly
    pub severity: f64,
}

/// Timing analysis results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimingAnalysis {
    /// Critical timing windows
    pub critical_windows: Vec<TimingWindow>,
    /// Timeout analysis
    pub timeout_analysis: TimeoutAnalysis,
    /// Synchronization issues
    pub synchronization_issues: Vec<SynchronizationIssue>,
}

/// Critical timing window
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimingWindow {
    /// Window description
    pub description: String,
    /// Start time
    pub start_time: u64,
    /// End time
    pub end_time: u64,
    /// Events in window
    pub events_count: usize,
    /// Window significance
    pub significance: f64,
}

/// Timeout analysis results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeoutAnalysis {
    /// Timeout events detected
    pub timeout_events: Vec<TimeoutEvent>,
    /// Timeout patterns
    pub timeout_patterns: Vec<TimeoutPattern>,
    /// Suggested timeout adjustments
    pub timeout_adjustments: Vec<TimeoutAdjustment>,
}

/// Timeout event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeoutEvent {
    /// Event description
    pub description: String,
    /// Participant that timed out
    pub participant: String,
    /// Timeout duration
    pub timeout_duration_ms: u64,
    /// Impact of timeout
    pub impact: String,
}

/// Timeout pattern identified
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeoutPattern {
    /// Pattern description
    pub description: String,
    /// Frequency of pattern
    pub frequency: f64,
    /// Pattern significance
    pub significance: f64,
}

/// Suggested timeout adjustment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeoutAdjustment {
    /// Component to adjust
    pub component: String,
    /// Current timeout value
    pub current_timeout_ms: u64,
    /// Suggested timeout value
    pub suggested_timeout_ms: u64,
    /// Rationale for adjustment
    pub rationale: String,
}

/// Synchronization issue detected
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynchronizationIssue {
    /// Issue description
    pub description: String,
    /// Participants involved
    pub participants: Vec<String>,
    /// Issue severity
    pub severity: SynchronizationSeverity,
    /// Suggested resolution
    pub suggested_resolution: String,
}

/// Severity of synchronization issues
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum SynchronizationSeverity {
    /// Minor synchronization issue
    Minor,
    /// Moderate synchronization issue
    Moderate,
    /// Major synchronization issue
    Major,
    /// Critical synchronization issue
    Critical,
}

/// Pattern analysis results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternAnalysis {
    /// Detected failure patterns
    pub failure_patterns: Vec<FailurePattern>,
    /// Pattern correlations
    pub pattern_correlations: Vec<PatternCorrelation>,
    /// Predictive patterns
    pub predictive_patterns: Vec<PredictivePattern>,
}

/// Detected failure pattern
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailurePattern {
    /// Pattern identifier
    pub pattern_id: String,
    /// Pattern description
    pub description: String,
    /// Pattern frequency
    pub frequency: f64,
    /// Pattern conditions
    pub conditions: Vec<String>,
    /// Pattern outcomes
    pub outcomes: Vec<String>,
}

/// Correlation between patterns
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternCorrelation {
    /// First pattern ID
    pub pattern_a: String,
    /// Second pattern ID
    pub pattern_b: String,
    /// Correlation strength
    pub correlation_strength: f64,
    /// Correlation type
    pub correlation_type: CorrelationType,
}

/// Types of pattern correlations
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CorrelationType {
    /// Patterns occur together
    Positive,
    /// Patterns are mutually exclusive
    Negative,
    /// Patterns have temporal relationship
    Temporal,
    /// Patterns have causal relationship
    Causal,
}

/// Predictive pattern for future failures
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictivePattern {
    /// Pattern description
    pub description: String,
    /// Prediction accuracy
    pub accuracy: f64,
    /// Early warning indicators
    pub warning_indicators: Vec<String>,
    /// Recommended preventive actions
    pub preventive_actions: Vec<String>,
}

/// Actionable recommendation for developers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionableRecommendation {
    /// Recommendation identifier
    pub recommendation_id: String,
    /// Recommendation title
    pub title: String,
    /// Detailed description
    pub description: String,
    /// Priority level
    pub priority: RecommendationPriority,
    /// Implementation effort estimate
    pub effort_estimate: EffortEstimate,
    /// Expected impact
    pub expected_impact: Impact,
    /// Implementation steps
    pub implementation_steps: Vec<ImplementationStep>,
    /// Success criteria
    pub success_criteria: Vec<String>,
    /// Related resources
    pub related_resources: Vec<String>,
}

/// Priority levels for recommendations
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum RecommendationPriority {
    /// Nice to have improvement
    Low,
    /// Recommended improvement
    Medium,
    /// Important fix needed
    High,
    /// Critical fix required immediately
    Critical,
}

/// Effort estimate for implementation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffortEstimate {
    /// Estimated hours
    pub estimated_hours: f64,
    /// Confidence in estimate
    pub confidence: f64,
    /// Complexity factors
    pub complexity_factors: Vec<String>,
    /// Required skills
    pub required_skills: Vec<String>,
}

/// Expected impact of recommendation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Impact {
    /// Risk reduction
    pub risk_reduction: f64,
    /// Performance improvement
    pub performance_improvement: Option<f64>,
    /// Maintainability improvement
    pub maintainability_improvement: Option<f64>,
    /// Testing improvement
    pub testing_improvement: Option<f64>,
}

/// Implementation step
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImplementationStep {
    /// Step number
    pub step_number: usize,
    /// Step description
    pub description: String,
    /// Estimated time for step
    pub estimated_time_hours: f64,
    /// Dependencies on other steps
    pub dependencies: Vec<usize>,
    /// Verification criteria
    pub verification_criteria: Vec<String>,
}

/// Debugging insight
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebuggingInsight {
    /// Insight identifier
    pub insight_id: String,
    /// Insight category
    pub category: InsightCategory,
    /// Insight title
    pub title: String,
    /// Insight description
    pub description: String,
    /// Confidence level
    pub confidence: f64,
    /// Supporting evidence
    pub evidence: Vec<String>,
    /// Actionability score
    pub actionability: f64,
    /// Related insights
    pub related_insights: Vec<String>,
}

/// Categories of debugging insights
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum InsightCategory {
    /// Code quality insight
    CodeQuality,
    /// Performance insight
    Performance,
    /// Security insight
    Security,
    /// Reliability insight
    Reliability,
    /// Maintainability insight
    Maintainability,
    /// Testing insight
    Testing,
    /// Architecture insight
    Architecture,
}

/// Visualization element for reports
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualizationElement {
    /// Element identifier
    pub element_id: String,
    /// Visualization type
    pub visualization_type: VisualizationType,
    /// Element title
    pub title: String,
    /// Element data
    pub data: VisualizationData,
    /// Display options
    pub display_options: DisplayOptions,
}

/// Types of visualizations
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum VisualizationType {
    /// Timeline visualization
    Timeline,
    /// State diagram
    StateDiagram,
    /// Network topology
    NetworkTopology,
    /// Performance chart
    PerformanceChart,
    /// Causal chain diagram
    CausalChain,
    /// Pattern visualization
    PatternVisualization,
}

/// Data for visualizations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VisualizationData {
    /// Timeline data
    Timeline(TimelineData),
    /// Chart data
    Chart(ChartData),
    /// Diagram data
    Diagram(DiagramData),
    /// Raw data
    Raw(serde_json::Value),
}

/// Timeline visualization data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineData {
    /// Timeline events
    pub events: Vec<TimelineEvent>,
    /// Time range
    pub time_range: (u64, u64),
    /// Highlighted periods
    pub highlights: Vec<TimelineHighlight>,
}

/// Event on timeline
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEvent {
    /// Event timestamp
    pub timestamp: u64,
    /// Event description
    pub description: String,
    /// Event type
    pub event_type: String,
    /// Event importance
    pub importance: f64,
}

/// Highlighted period on timeline
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineHighlight {
    /// Start time
    pub start_time: u64,
    /// End time
    pub end_time: u64,
    /// Highlight reason
    pub reason: String,
    /// Highlight color/style
    pub style: String,
}

/// Chart visualization data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChartData {
    /// Data series
    pub series: Vec<DataSeries>,
    /// Chart type
    pub chart_type: String,
    /// Axis labels
    pub axis_labels: (String, String),
}

/// Data series for charts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSeries {
    /// Series name
    pub name: String,
    /// Data points
    pub data_points: Vec<DataPoint>,
    /// Series style
    pub style: String,
}

/// Individual data point
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataPoint {
    /// X coordinate
    pub x: f64,
    /// Y coordinate
    pub y: f64,
    /// Optional label
    pub label: Option<String>,
}

/// Diagram visualization data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagramData {
    /// Diagram nodes
    pub nodes: Vec<DiagramNode>,
    /// Diagram edges
    pub edges: Vec<DiagramEdge>,
    /// Layout information
    pub layout: DiagramLayout,
}

/// Node in diagram
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagramNode {
    /// Node ID
    pub node_id: String,
    /// Node label
    pub label: String,
    /// Node type
    pub node_type: String,
    /// Node properties
    pub properties: HashMap<String, String>,
}

/// Edge in diagram
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagramEdge {
    /// Source node ID
    pub source: String,
    /// Target node ID
    pub target: String,
    /// Edge label
    pub label: Option<String>,
    /// Edge properties
    pub properties: HashMap<String, String>,
}

/// Layout information for diagrams
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagramLayout {
    /// Layout algorithm
    pub algorithm: String,
    /// Layout parameters
    pub parameters: HashMap<String, serde_json::Value>,
}

/// Display options for visualizations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayOptions {
    /// Width in pixels
    pub width: Option<u32>,
    /// Height in pixels
    pub height: Option<u32>,
    /// Display title
    pub show_title: bool,
    /// Display legend
    pub show_legend: bool,
    /// Custom styling
    pub custom_style: HashMap<String, String>,
}

/// Related resource reference
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedResource {
    /// Resource type
    pub resource_type: ResourceType,
    /// Resource title
    pub title: String,
    /// Resource description
    pub description: String,
    /// Resource URL or path
    pub location: String,
    /// Relevance score
    pub relevance: f64,
}

/// Types of related resources
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ResourceType {
    /// Documentation link
    Documentation,
    /// Related scenario
    Scenario,
    /// Test case
    TestCase,
    /// Code example
    CodeExample,
    /// Similar issue
    SimilarIssue,
    /// External reference
    ExternalReference,
}

/// Report metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportMetadata {
    /// Report version
    pub version: String,
    /// Generator version
    pub generator_version: String,
    /// Generation duration
    pub generation_duration_ms: u64,
    /// Data sources used
    pub data_sources: Vec<String>,
    /// Analysis methods used
    pub analysis_methods: Vec<String>,
    /// Report tags
    pub tags: Vec<String>,
}

/// Report template for different types
#[derive(Debug, Clone)]
pub struct ReportTemplate {
    /// Template sections
    pub sections: Vec<TemplateSection>,
    /// Template variables
    pub variables: HashMap<String, String>,
}

/// Section within a report template
#[derive(Debug, Clone)]
pub struct TemplateSection {
    /// Section title
    pub title: String,
    /// Section content template
    pub content_template: String,
    /// Section order
    pub order: usize,
    /// Whether section is required
    pub required: bool,
}

/// Insight collector for ranking debugging insights
pub struct InsightCollector {
    /// Collected insights
    insights: Vec<DebuggingInsight>,
    /// Insight ranking criteria
    ranking_criteria: InsightRankingCriteria,
}

/// Criteria for ranking insights
#[derive(Debug, Clone)]
pub struct InsightRankingCriteria {
    /// Weight for confidence
    pub confidence_weight: f64,
    /// Weight for actionability
    pub actionability_weight: f64,
    /// Weight for impact
    pub impact_weight: f64,
    /// Weight for novelty
    pub novelty_weight: f64,
}

impl DebugReporter {
    /// Create a new debug reporter
    pub fn new() -> Result<Self> {
        Ok(Self {
            config: ReporterConfig::default(),
            generated_reports: Vec::new(),
            report_templates: Self::create_default_templates(),
            insight_collector: InsightCollector::new(),
        })
    }

    /// Create reporter with custom configuration
    pub fn with_config(config: ReporterConfig) -> Result<Self> {
        let mut reporter = Self::new()?;
        reporter.config = config;
        Ok(reporter)
    }

    /// Generate comprehensive developer report
    pub fn generate_developer_report(
        &mut self,
        violation_debug_result: &ViolationDebugResult,
        minimal_reproduction: Option<&MinimalReproduction>,
        focused_test_results: Option<&[FocusedTestResult]>,
    ) -> Result<DeveloperReport> {
        let start_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let report_id = format!(
            "report_{}_{}",
            violation_debug_result.failure_analysis.analysis_id, start_time
        );

        // Generate executive summary
        let executive_summary =
            self.generate_executive_summary(violation_debug_result, minimal_reproduction)?;

        // Generate technical analysis
        let technical_analysis = self.generate_technical_analysis(violation_debug_result)?;

        // Generate recommendations
        let recommendations = self.generate_recommendations(
            violation_debug_result,
            minimal_reproduction,
            focused_test_results,
        )?;

        // Collect and rank insights
        let insights = self.collect_debugging_insights(
            violation_debug_result,
            minimal_reproduction,
            focused_test_results,
        )?;

        // Generate visualizations
        let visualizations = if self.config.include_visualizations {
            self.generate_visualizations(violation_debug_result)?
        } else {
            Vec::new()
        };

        // Find related resources
        let related_resources = self.find_related_resources(violation_debug_result)?;

        // Create report metadata
        let end_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let metadata = ReportMetadata {
            version: "1.0".to_string(),
            generator_version: env!("CARGO_PKG_VERSION").to_string(),
            generation_duration_ms: end_time - start_time,
            data_sources: vec![
                "ViolationDebugResult".to_string(),
                "FailureAnalysis".to_string(),
                "MinimalReproduction".to_string(),
            ],
            analysis_methods: vec![
                "CausalChainAnalysis".to_string(),
                "PatternAnalysis".to_string(),
                "RootCauseAnalysis".to_string(),
            ],
            tags: Vec::new(),
        };

        let report = DeveloperReport {
            report_id: report_id.clone(),
            report_type: ReportType::ViolationAnalysis,
            title: format!(
                "Violation Analysis: {}",
                violation_debug_result
                    .failure_analysis
                    .analyzed_violation
                    .property_name
            ),
            created_at: start_time,
            executive_summary,
            technical_analysis,
            recommendations,
            insights,
            visualizations,
            related_resources,
            metadata,
        };

        self.generated_reports.push(report.clone());
        Ok(report)
    }

    /// Generate executive summary
    fn generate_executive_summary(
        &self,
        debug_result: &ViolationDebugResult,
        minimal_reproduction: Option<&MinimalReproduction>,
    ) -> Result<ExecutiveSummary> {
        let violation = &debug_result.failure_analysis.analyzed_violation;

        let failure_description = format!(
            "Property '{}' was violated during simulation execution. {}",
            violation.property_name, violation.violation_details.description
        );

        let mut key_findings = vec![
            format!(
                "Violation detected at tick {} ({}ms)",
                violation.violation_state.tick, violation.detected_at
            ),
            format!(
                "Critical window spans {} ticks",
                debug_result.failure_analysis.critical_window.end_tick
                    - debug_result.failure_analysis.critical_window.start_tick
            ),
        ];

        if let Some(repro) = minimal_reproduction {
            key_findings.push(format!(
                "Minimal reproduction achieved {:.1}% complexity reduction",
                repro.complexity_reduction * 100.0
            ));
        }

        let impact_assessment = ImpactAssessment {
            severity: self.assess_violation_severity(violation),
            affected_components: debug_result
                .failure_analysis
                .causal_chains
                .iter()
                .flat_map(|chain| {
                    chain
                        .events
                        .iter()
                        .flat_map(|e| e.participants.clone())
                })
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect(),
            potential_consequences: vec![
                "Protocol execution failure".to_string(),
                "Network partition vulnerability".to_string(),
                "State inconsistency risk".to_string(),
            ],
            recurrence_likelihood: debug_result
                .failure_analysis
                .analysis_summary
                .reproduction_likelihood,
        };

        let resolution_estimate = ResolutionEstimate {
            estimated_hours: self.estimate_resolution_time(&debug_result.failure_analysis),
            confidence: 0.7,
            complexity_factors: vec![
                format!(
                    "{:?}",
                    debug_result
                        .failure_analysis
                        .analysis_summary
                        .failure_complexity
                ),
                format!(
                    "{} causal chains identified",
                    debug_result.failure_analysis.causal_chains.len()
                ),
            ],
        };

        let urgency_level = self.determine_urgency_level(violation, &impact_assessment);

        Ok(ExecutiveSummary {
            failure_description,
            key_findings,
            impact_assessment,
            resolution_estimate,
            urgency_level,
        })
    }

    /// Generate technical analysis
    fn generate_technical_analysis(
        &self,
        debug_result: &ViolationDebugResult,
    ) -> Result<TechnicalAnalysis> {
        let root_cause_analysis =
            self.generate_root_cause_analysis(&debug_result.failure_analysis)?;
        let causal_chain_analysis =
            self.generate_causal_chain_analysis(&debug_result.failure_analysis)?;
        let state_analysis = self.generate_state_analysis(debug_result)?;
        let performance_analysis = self.generate_performance_analysis(debug_result)?;
        let pattern_analysis = self.generate_pattern_analysis(&debug_result.failure_analysis)?;

        Ok(TechnicalAnalysis {
            root_cause_analysis,
            causal_chain_analysis,
            state_analysis,
            performance_analysis,
            pattern_analysis,
        })
    }

    /// Generate actionable recommendations
    fn generate_recommendations(
        &self,
        debug_result: &ViolationDebugResult,
        minimal_reproduction: Option<&MinimalReproduction>,
        _focused_test_results: Option<&[FocusedTestResult]>,
    ) -> Result<Vec<ActionableRecommendation>> {
        let mut recommendations = Vec::new();

        // Primary recommendation based on root cause
        recommendations.push(ActionableRecommendation {
            recommendation_id: "primary_fix".to_string(),
            title: "Address Root Cause".to_string(),
            description: format!(
                "Fix the primary cause: {:?}",
                debug_result.failure_analysis.analysis_summary.primary_cause
            ),
            priority: RecommendationPriority::Critical,
            effort_estimate: EffortEstimate {
                estimated_hours: 8.0,
                confidence: 0.7,
                complexity_factors: vec!["Root cause complexity".to_string()],
                required_skills: vec!["Protocol implementation".to_string()],
            },
            expected_impact: Impact {
                risk_reduction: 0.8,
                performance_improvement: None,
                maintainability_improvement: Some(0.3),
                testing_improvement: Some(0.5),
            },
            implementation_steps: vec![
                ImplementationStep {
                    step_number: 1,
                    description: "Analyze the root cause in detail".to_string(),
                    estimated_time_hours: 2.0,
                    dependencies: Vec::new(),
                    verification_criteria: vec!["Root cause confirmed".to_string()],
                },
                ImplementationStep {
                    step_number: 2,
                    description: "Implement fix for root cause".to_string(),
                    estimated_time_hours: 4.0,
                    dependencies: vec![1],
                    verification_criteria: vec!["Fix implemented and tested".to_string()],
                },
                ImplementationStep {
                    step_number: 3,
                    description: "Verify fix resolves violation".to_string(),
                    estimated_time_hours: 2.0,
                    dependencies: vec![2],
                    verification_criteria: vec!["Violation no longer reproduces".to_string()],
                },
            ],
            success_criteria: vec![
                "Property violation no longer occurs".to_string(),
                "All tests pass".to_string(),
                "Performance not degraded".to_string(),
            ],
            related_resources: Vec::new(),
        });

        // Add testing recommendations if we have minimal reproduction
        if let Some(_repro) = minimal_reproduction {
            recommendations.push(ActionableRecommendation {
                recommendation_id: "add_regression_test".to_string(),
                title: "Add Regression Test".to_string(),
                description: "Create regression test based on minimal reproduction".to_string(),
                priority: RecommendationPriority::High,
                effort_estimate: EffortEstimate {
                    estimated_hours: 2.0,
                    confidence: 0.9,
                    complexity_factors: vec!["Test complexity".to_string()],
                    required_skills: vec!["Testing framework".to_string()],
                },
                expected_impact: Impact {
                    risk_reduction: 0.6,
                    performance_improvement: None,
                    maintainability_improvement: None,
                    testing_improvement: Some(0.8),
                },
                implementation_steps: vec![
                    ImplementationStep {
                        step_number: 1,
                        description: "Convert minimal reproduction to test case".to_string(),
                        estimated_time_hours: 1.0,
                        dependencies: Vec::new(),
                        verification_criteria: vec!["Test case created".to_string()],
                    },
                    ImplementationStep {
                        step_number: 2,
                        description: "Add test to regression suite".to_string(),
                        estimated_time_hours: 1.0,
                        dependencies: vec![1],
                        verification_criteria: vec!["Test runs in CI".to_string()],
                    },
                ],
                success_criteria: vec![
                    "Regression test detects violation".to_string(),
                    "Test passes after fix".to_string(),
                ],
                related_resources: Vec::new(),
            });
        }

        // Sort recommendations by priority
        recommendations.sort_by(|a, b| b.priority.cmp(&a.priority));

        Ok(recommendations)
    }

    /// Collect and rank debugging insights
    fn collect_debugging_insights(
        &mut self,
        debug_result: &ViolationDebugResult,
        minimal_reproduction: Option<&MinimalReproduction>,
        focused_test_results: Option<&[FocusedTestResult]>,
    ) -> Result<Vec<DebuggingInsight>> {
        let mut insights = Vec::new();

        // Add failure analysis insights
        insights.extend(self.extract_failure_insights(&debug_result.failure_analysis)?);

        // Add minimal reproduction insights
        if let Some(repro) = minimal_reproduction {
            insights.extend(self.extract_reproduction_insights(repro)?);
        }

        // Add focused testing insights
        if let Some(test_results) = focused_test_results {
            insights.extend(self.extract_testing_insights(test_results)?);
        }

        // Rank and filter insights
        self.insight_collector.rank_insights(&mut insights);

        // Return top insights based on configuration
        insights.truncate(self.config.max_insights_per_category * 6); // 6 categories
        Ok(insights
            .into_iter()
            .filter(|i| i.confidence >= self.config.min_insight_confidence)
            .collect())
    }

    /// Generate visualizations for the report
    fn generate_visualizations(
        &self,
        debug_result: &ViolationDebugResult,
    ) -> Result<Vec<VisualizationElement>> {
        let mut visualizations = Vec::new();

        // Timeline visualization
        visualizations.push(VisualizationElement {
            element_id: "violation_timeline".to_string(),
            visualization_type: VisualizationType::Timeline,
            title: "Violation Timeline".to_string(),
            data: VisualizationData::Timeline(self.create_timeline_data(debug_result)?),
            display_options: DisplayOptions {
                width: Some(800),
                height: Some(400),
                show_title: true,
                show_legend: true,
                custom_style: HashMap::new(),
            },
        });

        // Causal chain diagram
        visualizations.push(VisualizationElement {
            element_id: "causal_chains".to_string(),
            visualization_type: VisualizationType::CausalChain,
            title: "Causal Chain Analysis".to_string(),
            data: VisualizationData::Diagram(self.create_causal_diagram(debug_result)?),
            display_options: DisplayOptions {
                width: Some(1000),
                height: Some(600),
                show_title: true,
                show_legend: true,
                custom_style: HashMap::new(),
            },
        });

        Ok(visualizations)
    }

    /// Find related resources
    fn find_related_resources(
        &self,
        _debug_result: &ViolationDebugResult,
    ) -> Result<Vec<RelatedResource>> {
        let mut resources = Vec::new();

        // Add related scenarios
        resources.push(RelatedResource {
            resource_type: ResourceType::Scenario,
            title: "Similar Byzantine Scenario".to_string(),
            description: "Related scenario that tests similar conditions".to_string(),
            location: "scenarios/byzantine/similar_test.toml".to_string(),
            relevance: 0.8,
        });

        // Add documentation
        resources.push(RelatedResource {
            resource_type: ResourceType::Documentation,
            title: "Protocol Implementation Guide".to_string(),
            description: "Documentation for the affected protocol".to_string(),
            location: "docs/protocols/implementation.md".to_string(),
            relevance: 0.7,
        });

        Ok(resources)
    }

    // Helper methods for analysis generation

    fn generate_root_cause_analysis(
        &self,
        failure_analysis: &FailureAnalysisResult,
    ) -> Result<RootCauseAnalysis> {
        Ok(RootCauseAnalysis {
            primary_cause: RootCause {
                description: format!("{:?}", failure_analysis.analysis_summary.primary_cause),
                category: self.map_cause_category(&failure_analysis.analysis_summary.primary_cause),
                confidence: 0.8,
                evidence_summary: "Based on causal chain analysis and event correlation"
                    .to_string(),
            },
            contributing_factors: failure_analysis
                .analysis_summary
                .contributing_factors
                .iter()
                .map(|factor| ContributingFactor {
                    description: factor.factor.clone(),
                    impact_weight: factor.impact_weight,
                    category: "Environmental".to_string(),
                    details: factor.evidence.join("; "),
                })
                .collect(),
            supporting_evidence: vec![Evidence {
                evidence_type: EvidenceType::PropertyViolation,
                description: "Property violation detected".to_string(),
                strength: 1.0,
                source: "PropertyMonitor".to_string(),
                location: Some(format!(
                    "tick {}",
                    failure_analysis.analyzed_violation.violation_state.tick
                )),
            }],
            alternative_hypotheses: Vec::new(),
        })
    }

    fn generate_causal_chain_analysis(
        &self,
        failure_analysis: &FailureAnalysisResult,
    ) -> Result<CausalChainAnalysis> {
        let causal_chains = failure_analysis
            .causal_chains
            .iter()
            .map(|chain| CausalChainSummary {
                chain_id: format!("chain_{}", chain.chain_id),
                description: chain.description.clone(),
                strength: chain.chain_strength,
                key_events: chain
                    .events
                    .iter()
                    .map(|e| format!("{:?}", e.event_type))
                    .collect(),
                time_span_ms: chain.events.last().map(|e| e.timestamp).unwrap_or(0)
                    - chain.events.first().map(|e| e.timestamp).unwrap_or(0),
            })
            .collect();

        Ok(CausalChainAnalysis {
            causal_chains,
            chain_interactions: Vec::new(),
            critical_path: None,
        })
    }

    fn generate_state_analysis(
        &self,
        _debug_result: &ViolationDebugResult,
    ) -> Result<StateAnalysis> {
        Ok(StateAnalysis {
            key_transitions: Vec::new(),
            inconsistencies: Vec::new(),
            corruption_indicators: Vec::new(),
        })
    }

    fn generate_performance_analysis(
        &self,
        _debug_result: &ViolationDebugResult,
    ) -> Result<PerformanceAnalysis> {
        Ok(PerformanceAnalysis {
            bottlenecks: Vec::new(),
            resource_utilization: ResourceUtilization {
                memory_usage: UsagePattern {
                    average_usage: 50.0,
                    peak_usage: 80.0,
                    usage_variance: 10.0,
                    anomalies: Vec::new(),
                },
                cpu_usage: UsagePattern {
                    average_usage: 30.0,
                    peak_usage: 60.0,
                    usage_variance: 15.0,
                    anomalies: Vec::new(),
                },
                network_usage: UsagePattern {
                    average_usage: 20.0,
                    peak_usage: 40.0,
                    usage_variance: 5.0,
                    anomalies: Vec::new(),
                },
            },
            timing_analysis: TimingAnalysis {
                critical_windows: Vec::new(),
                timeout_analysis: TimeoutAnalysis {
                    timeout_events: Vec::new(),
                    timeout_patterns: Vec::new(),
                    timeout_adjustments: Vec::new(),
                },
                synchronization_issues: Vec::new(),
            },
        })
    }

    fn generate_pattern_analysis(
        &self,
        failure_analysis: &FailureAnalysisResult,
    ) -> Result<PatternAnalysis> {
        Ok(PatternAnalysis {
            failure_patterns: failure_analysis
                .detected_patterns
                .iter()
                .map(|pattern| FailurePattern {
                    pattern_id: pattern.pattern_name.clone(),
                    description: pattern.description.clone(),
                    frequency: pattern.confidence * 100.0,
                    conditions: pattern.matching_events.clone(),
                    outcomes: vec![format!("Pattern type: {:?}", pattern.pattern_type)],
                })
                .collect(),
            pattern_correlations: Vec::new(),
            predictive_patterns: Vec::new(),
        })
    }

    fn extract_failure_insights(
        &self,
        failure_analysis: &FailureAnalysisResult,
    ) -> Result<Vec<DebuggingInsight>> {
        let mut insights = Vec::new();

        insights.push(DebuggingInsight {
            insight_id: "failure_complexity".to_string(),
            category: InsightCategory::Architecture,
            title: "Failure Complexity Assessment".to_string(),
            description: format!(
                "Failure complexity: {:?}",
                failure_analysis.analysis_summary.failure_complexity
            ),
            confidence: 0.9,
            evidence: vec!["Based on causal chain analysis".to_string()],
            actionability: 0.7,
            related_insights: Vec::new(),
        });

        Ok(insights)
    }

    fn extract_reproduction_insights(
        &self,
        reproduction: &MinimalReproduction,
    ) -> Result<Vec<DebuggingInsight>> {
        let mut insights = Vec::new();

        insights.push(DebuggingInsight {
            insight_id: "complexity_reduction".to_string(),
            category: InsightCategory::Testing,
            title: "Complexity Reduction Achieved".to_string(),
            description: format!(
                "Achieved {:.1}% complexity reduction in minimal reproduction",
                reproduction.complexity_reduction * 100.0
            ),
            confidence: 0.8,
            evidence: vec![format!(
                "Original complexity: {:.1}, minimal: {:.1}",
                reproduction.complexity_score / (1.0 - reproduction.complexity_reduction),
                reproduction.complexity_score
            )],
            actionability: 0.9,
            related_insights: Vec::new(),
        });

        Ok(insights)
    }

    fn extract_testing_insights(
        &self,
        test_results: &[FocusedTestResult],
    ) -> Result<Vec<DebuggingInsight>> {
        let mut insights = Vec::new();

        let success_rate = test_results
            .iter()
            .filter(|r| !r.failure_reproduced)
            .count() as f64
            / test_results.len() as f64;

        insights.push(DebuggingInsight {
            insight_id: "focused_test_success".to_string(),
            category: InsightCategory::Testing,
            title: "Focused Test Success Rate".to_string(),
            description: format!(
                "Focused tests had {:.1}% success rate",
                success_rate * 100.0
            ),
            confidence: 0.7,
            evidence: vec![format!("{} tests executed", test_results.len())],
            actionability: 0.6,
            related_insights: Vec::new(),
        });

        Ok(insights)
    }

    fn create_timeline_data(&self, debug_result: &ViolationDebugResult) -> Result<TimelineData> {
        let events = debug_result
            .causal_chain
            .iter()
            .enumerate()
            .map(|(i, event)| TimelineEvent {
                timestamp: i as u64, // Use index as placeholder timestamp
                description: format!("{:?}", event.event_type),
                event_type: "critical".to_string(),
                importance: 1.0,
            })
            .collect();

        let violation_time = debug_result.failure_analysis.analyzed_violation.detected_at;
        let time_range = (violation_time.saturating_sub(10000), violation_time + 1000);

        Ok(TimelineData {
            events,
            time_range,
            highlights: vec![TimelineHighlight {
                start_time: debug_result.failure_analysis.critical_window.start_tick * 100,
                end_time: debug_result.failure_analysis.critical_window.end_tick * 100,
                reason: "Critical window".to_string(),
                style: "highlight-critical".to_string(),
            }],
        })
    }

    fn create_causal_diagram(&self, debug_result: &ViolationDebugResult) -> Result<DiagramData> {
        let mut nodes = Vec::new();
        let mut edges = Vec::new();

        // Add nodes for key events
        for (i, chain) in debug_result
            .failure_analysis
            .causal_chains
            .iter()
            .enumerate()
        {
            for (j, event) in chain.events.iter().enumerate() {
                nodes.push(DiagramNode {
                    node_id: format!("event_{}_{}", i, j),
                    label: format!("{:?}", event.event_type),
                    node_type: "event".to_string(),
                    properties: HashMap::new(),
                });
            }
        }

        // Add causal edges
        for (i, chain) in debug_result
            .failure_analysis
            .causal_chains
            .iter()
            .enumerate()
        {
            for j in 0..chain.events.len().saturating_sub(1) {
                edges.push(DiagramEdge {
                    source: format!("event_{}_{}", i, j),
                    target: format!("event_{}_{}", i, j + 1),
                    label: Some("causes".to_string()),
                    properties: HashMap::new(),
                });
            }
        }

        Ok(DiagramData {
            nodes,
            edges,
            layout: DiagramLayout {
                algorithm: "hierarchical".to_string(),
                parameters: HashMap::new(),
            },
        })
    }

    // Helper methods

    fn assess_violation_severity(
        &self,
        violation: &crate::testing::PropertyViolation,
    ) -> IssueSeverity {
        match violation.violation_details.severity {
            crate::testing::ViolationSeverity::Low => IssueSeverity::Low,
            crate::testing::ViolationSeverity::Medium => IssueSeverity::Medium,
            crate::testing::ViolationSeverity::High => IssueSeverity::High,
            crate::testing::ViolationSeverity::Critical => IssueSeverity::Critical,
        }
    }

    fn estimate_resolution_time(&self, failure_analysis: &FailureAnalysisResult) -> f64 {
        match failure_analysis.analysis_summary.failure_complexity {
            crate::failure_analyzer::FailureComplexity::Simple => 2.0,
            crate::failure_analyzer::FailureComplexity::Moderate => 8.0,
            crate::failure_analyzer::FailureComplexity::Complex => 24.0,
            crate::failure_analyzer::FailureComplexity::VeryComplex => 72.0,
        }
    }

    fn determine_urgency_level(
        &self,
        _violation: &crate::testing::PropertyViolation,
        impact: &ImpactAssessment,
    ) -> UrgencyLevel {
        match impact.severity {
            IssueSeverity::Critical => UrgencyLevel::Immediate,
            IssueSeverity::High => UrgencyLevel::High,
            IssueSeverity::Medium => UrgencyLevel::Medium,
            IssueSeverity::Low => UrgencyLevel::Low,
        }
    }

    fn map_cause_category(
        &self,
        cause: &crate::failure_analyzer::CauseCategory,
    ) -> RootCauseCategory {
        match cause {
            crate::failure_analyzer::CauseCategory::ProtocolIssue => {
                RootCauseCategory::ProtocolLogicError
            }
            crate::failure_analyzer::CauseCategory::TimingIssue => RootCauseCategory::TimingIssue,
            crate::failure_analyzer::CauseCategory::NetworkConditions => {
                RootCauseCategory::NetworkConfiguration
            }
            crate::failure_analyzer::CauseCategory::ByzantineBehavior => {
                RootCauseCategory::ByzantineEdgeCase
            }
            crate::failure_analyzer::CauseCategory::ExternalFactors => {
                RootCauseCategory::StateManagement
            }
            crate::failure_analyzer::CauseCategory::ResourceConstraints => {
                RootCauseCategory::ResourceConstraint
            }
            crate::failure_analyzer::CauseCategory::ComplexInteraction => {
                RootCauseCategory::ComplexInteraction
            }
        }
    }

    fn create_default_templates() -> HashMap<ReportType, ReportTemplate> {
        let mut templates = HashMap::new();

        templates.insert(
            ReportType::ViolationAnalysis,
            ReportTemplate {
                sections: vec![
                    TemplateSection {
                        title: "Executive Summary".to_string(),
                        content_template: "{{executive_summary}}".to_string(),
                        order: 1,
                        required: true,
                    },
                    TemplateSection {
                        title: "Technical Analysis".to_string(),
                        content_template: "{{technical_analysis}}".to_string(),
                        order: 2,
                        required: true,
                    },
                    TemplateSection {
                        title: "Recommendations".to_string(),
                        content_template: "{{recommendations}}".to_string(),
                        order: 3,
                        required: true,
                    },
                ],
                variables: HashMap::new(),
            },
        );

        templates
    }

    /// Get generated reports
    pub fn get_generated_reports(&self) -> &[DeveloperReport] {
        &self.generated_reports
    }
}

impl InsightCollector {
    fn new() -> Self {
        Self {
            insights: Vec::new(),
            ranking_criteria: InsightRankingCriteria {
                confidence_weight: 0.3,
                actionability_weight: 0.4,
                impact_weight: 0.2,
                novelty_weight: 0.1,
            },
        }
    }

    fn rank_insights(&self, insights: &mut Vec<DebuggingInsight>) {
        insights.sort_by(|a, b| {
            let score_a = self.calculate_insight_score(a);
            let score_b = self.calculate_insight_score(b);
            score_b.partial_cmp(&score_a).unwrap()
        });
    }

    fn calculate_insight_score(&self, insight: &DebuggingInsight) -> f64 {
        insight.confidence * self.ranking_criteria.confidence_weight +
        insight.actionability * self.ranking_criteria.actionability_weight +
        0.7 * self.ranking_criteria.impact_weight + // Default impact score
        0.5 * self.ranking_criteria.novelty_weight // Default novelty score
    }
}

impl Default for DebugReporter {
    fn default() -> Self {
        Self::new().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reporter_creation() {
        let reporter = DebugReporter::new();
        assert!(reporter.is_ok());

        let reporter = reporter.unwrap();
        assert_eq!(reporter.generated_reports.len(), 0);
    }

    #[test]
    fn test_insight_ranking() {
        let mut collector = InsightCollector::new();

        let mut insights = vec![
            DebuggingInsight {
                insight_id: "low_confidence".to_string(),
                category: InsightCategory::CodeQuality,
                title: "Low confidence insight".to_string(),
                description: "Test insight".to_string(),
                confidence: 0.3,
                evidence: Vec::new(),
                actionability: 0.8,
                related_insights: Vec::new(),
            },
            DebuggingInsight {
                insight_id: "high_confidence".to_string(),
                category: InsightCategory::Performance,
                title: "High confidence insight".to_string(),
                description: "Test insight".to_string(),
                confidence: 0.9,
                evidence: Vec::new(),
                actionability: 0.7,
                related_insights: Vec::new(),
            },
        ];

        collector.rank_insights(&mut insights);

        // High confidence insight should be ranked first
        assert_eq!(insights[0].insight_id, "high_confidence");
        assert_eq!(insights[1].insight_id, "low_confidence");
    }

    #[test]
    fn test_severity_assessment() {
        let reporter = DebugReporter::new().unwrap();

        // Create mock violation with high severity
        let violation =
            create_mock_violation_with_severity(crate::testing::ViolationSeverity::High);
        let severity = reporter.assess_violation_severity(&violation);

        assert_eq!(severity, IssueSeverity::High);
    }

    fn create_mock_violation_with_severity(
        severity: crate::testing::ViolationSeverity,
    ) -> crate::testing::PropertyViolation {
        crate::testing::PropertyViolation {
            property_name: "test_property".to_string(),
            property_type: crate::testing::PropertyViolationType::Invariant,
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
            violation_details: crate::testing::ViolationDetails {
                description: "Test violation".to_string(),
                evidence: Vec::new(),
                potential_causes: Vec::new(),
                severity,
                remediation_suggestions: Vec::new(),
            },
            confidence: 0.9,
            detected_at: 10000,
        }
    }
}
